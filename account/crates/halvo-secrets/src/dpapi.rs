use crate::answer::{Fault, Locked};
use crate::place::{fault_with, Codec, EntryKind, Fs, Last, Meta, Opened, Published, Sealed, Walk};
use crate::secret::wipe_raw;
use crate::security::{make_store_dir, open_dir_following, owner_of, user_sid};
use crate::windows_place::{is_absent, owner_is_trusted, store_dir_gone, Gone, PATH_NOT_FOUND};
use crate::{Secret, MAX_SECRET_LEN};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::ptr;
use windows_sys::Win32::Foundation::{
    GetLastError, LocalFree, ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS,
};
use windows_sys::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, MOVEFILE_WRITE_THROUGH,
};

const BLOB_OVERHEAD: usize = 4096;

pub(crate) struct WindowsFs;

fn meta_of(metadata: &fs::Metadata) -> Meta {
    let file_type = metadata.file_type();
    let kind = if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || file_type.is_symlink()
    {
        EntryKind::Link
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_file() {
        EntryKind::File
    } else {
        EntryKind::Other
    };
    Meta {
        kind,
        owner: 0,
        mode: 0,
        len: metadata.len(),
        acl: false,
        acl_write: false,
        links: 1,
    }
}

fn as_fault(error: io::Error) -> io::Error {
    fault_with(error.raw_os_error().unwrap_or(-1))
}

fn reachable_root(path: &Path) -> io::Result<()> {
    let root: PathBuf = path
        .components()
        .take_while(|step| matches!(step, Component::Prefix(_) | Component::RootDir))
        .collect();
    fs::metadata(&root).map(|_| ()).map_err(as_fault)
}

fn wide(path: &Path) -> Vec<u16> {
    OsStr::new(path).encode_wide().chain([0]).collect()
}

impl Fs for WindowsFs {
    type Dir = PathBuf;
    type Item = File;
    type Temp = File;

    fn user(&self) -> u32 {
        0
    }

    fn walk(&self, path: &Path) -> io::Result<Walk<PathBuf>> {
        reachable_root(path)?;
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let metadata = match fs::metadata(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                let gone = store_dir_gone(
                    error.raw_os_error(),
                    || {
                        fs::symlink_metadata(path)
                            .map(|_| ())
                            .map_err(|e| e.raw_os_error())
                    },
                    || {
                        fs::metadata(parent)
                            .map(|meta| meta.is_dir())
                            .map_err(|e| e.raw_os_error())
                    },
                );
                return match gone {
                    Gone::Absent => Ok(Walk {
                        ancestors: Vec::new(),
                        last: Last::Absent(parent.to_path_buf()),
                    }),
                    Gone::Missing => Ok(Walk {
                        ancestors: Vec::new(),
                        last: Last::Missing(PATH_NOT_FOUND),
                    }),
                    Gone::Fault(code) => Err(fault_with(code)),
                    Gone::AsIs => Err(error),
                };
            }
        };
        let mut meta = meta_of(&metadata);
        if meta.kind != EntryKind::Directory {
            return Ok(Walk {
                ancestors: Vec::new(),
                last: Last::NotADirectory,
            });
        }
        let handle = open_dir_following(path)?;
        let owner = owner_of(&handle).map_err(as_fault)?;
        let user = user_sid().map_err(as_fault)?;
        let trusted = owner.is_some_and(|owner| owner_is_trusted(&owner, &user));
        meta.owner = u32::from(!trusted);
        Ok(Walk {
            ancestors: Vec::new(),
            last: Last::Dir(meta, path.to_path_buf()),
        })
    }

    fn make_dir(&self, parent: &PathBuf, name: &OsStr) -> io::Result<()> {
        make_store_dir(&parent.join(name), &user_sid()?)
    }

    fn is_absent(&self, error: &io::Error) -> bool {
        is_absent(error.raw_os_error())
    }

    fn exists(&self, dir: &PathBuf, name: &str) -> io::Result<bool> {
        match fs::symlink_metadata(dir.join(name)) {
            Ok(_) => Ok(true),
            Err(error) if is_absent(error.raw_os_error()) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn open_item(&self, dir: &PathBuf, name: &str) -> io::Result<Opened<File>> {
        let path = dir.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if meta_of(&metadata).kind != EntryKind::File => {
                return Ok(Opened::NotAFile)
            }
            Ok(_) => {}
            Err(error) if is_absent(error.raw_os_error()) => return Ok(Opened::Absent),
            Err(error) => return Err(error),
        }
        match OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&path)
        {
            Ok(file) => Ok(Opened::Item(file)),
            Err(error) if is_absent(error.raw_os_error()) => Ok(Opened::Absent),
            Err(error) => Err(error),
        }
    }

    fn item_meta(&self, item: &File) -> io::Result<Meta> {
        Ok(meta_of(&item.metadata()?))
    }

    fn read_item(&self, item: &mut File, buf: &mut [u8]) -> io::Result<usize> {
        item.read(buf)
    }

    fn create_temp(&self, dir: &PathBuf, name: &str) -> io::Result<Option<File>> {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(name))
        {
            Ok(file) => Ok(Some(file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn write_temp(&self, temp: &mut File, bytes: &[u8]) -> io::Result<()> {
        temp.write_all(bytes)
    }

    fn sync_temp(&self, temp: &File) -> io::Result<()> {
        temp.sync_all()
    }

    fn publish(&self, dir: &PathBuf, from: &str, to: &str) -> io::Result<Published> {
        let from = wide(&dir.join(from));
        let to = wide(&dir.join(to));
        // SAFETY: both names are NUL-terminated wide strings in vectors that live to the end of
        // this function, and the call keeps no pointer to them.
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } != 0 {
            return Ok(Published::Done);
        }
        // SAFETY: reads the last error of this thread, which the failed call above set.
        let code = unsafe { GetLastError() };
        if code == ERROR_ALREADY_EXISTS || code == ERROR_FILE_EXISTS {
            Ok(Published::Exists)
        } else {
            Err(io::Error::from_raw_os_error(code as i32))
        }
    }

    fn remove(&self, dir: &PathBuf, name: &str) -> io::Result<()> {
        fs::remove_file(dir.join(name))
    }

    fn sync_dir(&self, _dir: &PathBuf) -> io::Result<()> {
        Ok(())
    }

    fn names(&self, dir: &PathBuf) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for entry in fs::read_dir(dir)? {
            if let Ok(name) = entry?.file_name().into_string() {
                names.push(name);
            }
        }
        Ok(names)
    }
}

pub(crate) struct Dpapi;

fn blob_of(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    }
}

fn empty_blob() -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    }
}

impl Codec for Dpapi {
    fn seal<'a>(&self, secret: &'a Secret) -> Result<Sealed<'a>, Fault> {
        let input = blob_of(secret.expose());
        let mut output = empty_blob();
        // SAFETY: `input` points at the secret's bytes, which `secret` keeps alive and which the
        // call only reads; `output` is a local that receives a buffer that LocalFree releases
        // below; the optional arguments are null.
        let ok = unsafe {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            // SAFETY: reads the last error of this thread, which the failed call above set.
            let code = unsafe { GetLastError() };
            return Err(Fault {
                code: i64::from(code),
            });
        }
        // SAFETY: on success DPAPI hands over `cbData` initialized bytes at `pbData`, which this
        // function owns until the LocalFree below and copies before freeing.
        let sealed =
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
        // SAFETY: the buffer came from CryptProtectData, which documents LocalFree as its
        // release; no pointer to it is used after this line.
        unsafe { LocalFree(output.pbData.cast()) };
        Ok(Sealed::Blob(sealed))
    }

    fn open(&self, bytes: Vec<u8>) -> Result<Secret, Locked> {
        let input = blob_of(&bytes);
        let mut output = empty_blob();
        // SAFETY: `input` points at `bytes`, which lives to the end of this function and which
        // the call only reads; `output` is a local that receives a buffer that LocalFree
        // releases below; the optional arguments are null.
        let ok = unsafe {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if ok == 0 {
            // SAFETY: reads the last error of this thread, which the failed call above set.
            let code = unsafe { GetLastError() };
            return Err(Locked::Refused {
                code: i64::from(code),
            });
        }
        let len = output.cbData as usize;
        let opened = if len == 0 || len > MAX_SECRET_LEN {
            None
        } else {
            let mut plain = Vec::with_capacity(len);
            // SAFETY: on success DPAPI hands over `len` initialized bytes at `pbData`, which stay
            // valid until the LocalFree below.
            let opened = unsafe { std::slice::from_raw_parts(output.pbData, len) };
            plain.extend_from_slice(opened);
            Secret::new(plain)
        };
        // SAFETY: `pbData` holds the `len` bytes that this function owns and writes no more
        // through another pointer; the buffer came from CryptUnprotectData, whose release is
        // LocalFree, and it is freed once after the wipe.
        unsafe {
            wipe_raw(output.pbData, len);
            LocalFree(output.pbData.cast());
        }
        opened.ok_or(Locked::Size)
    }

    fn limit(&self) -> usize {
        MAX_SECRET_LEN + BLOB_OVERHEAD
    }
}
