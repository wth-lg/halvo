use crate::answer::{Fault, Locked};
use crate::place::{Codec, EntryKind, Fs, Meta, Opened, Published, Sealed};
use crate::secret::wipe_raw;
use crate::{Secret, MAX_SECRET_LEN};
use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
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
        links: 1,
    }
}

fn wide(path: &Path) -> Vec<u16> {
    OsStr::new(path).encode_wide().chain([0]).collect()
}

impl Fs for WindowsFs {
    type Item = File;
    type Temp = File;

    fn user(&self) -> u32 {
        0
    }

    fn lstat(&self, path: &Path) -> io::Result<Option<Meta>> {
        match fs::symlink_metadata(path) {
            Ok(metadata) => Ok(Some(meta_of(&metadata))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn create_dir(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::create_dir(path) {
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            other => other,
        }
    }

    fn open_item(&self, path: &Path) -> io::Result<Opened<File>> {
        match self.lstat(path)? {
            None => return Ok(Opened::Absent),
            Some(meta) if meta.kind != EntryKind::File => return Ok(Opened::NotAFile),
            Some(_) => {}
        }
        match OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
        {
            Ok(file) => Ok(Opened::Item(file)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Opened::Absent),
            Err(error) => Err(error),
        }
    }

    fn item_meta(&self, item: &File) -> io::Result<Meta> {
        Ok(meta_of(&item.metadata()?))
    }

    fn read_item(&self, item: &mut File, buf: &mut [u8]) -> io::Result<usize> {
        item.read(buf)
    }

    fn create_temp(&self, path: &Path) -> io::Result<Option<File>> {
        match OpenOptions::new().write(true).create_new(true).open(path) {
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

    fn publish(&self, from: &Path, to: &Path) -> io::Result<Published> {
        let from = wide(from);
        let to = wide(to);
        if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } != 0 {
            return Ok(Published::Done);
        }
        let code = unsafe { GetLastError() };
        if code == ERROR_ALREADY_EXISTS || code == ERROR_FILE_EXISTS {
            Ok(Published::Exists)
        } else {
            Err(io::Error::from_raw_os_error(code as i32))
        }
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn sync_dir(&self, _path: &Path) -> io::Result<()> {
        Ok(())
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
            return Err(Fault {
                code: i64::from(unsafe { GetLastError() }),
            });
        }
        let sealed =
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) }.to_vec();
        unsafe { LocalFree(output.pbData.cast()) };
        Ok(Sealed::Blob(sealed))
    }

    fn open(&self, bytes: Vec<u8>) -> Result<Secret, Locked> {
        let input = blob_of(&bytes);
        let mut output = empty_blob();
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
            return Err(Locked::Refused {
                code: i64::from(unsafe { GetLastError() }),
            });
        }
        let len = output.cbData as usize;
        let opened = if len == 0 || len > MAX_SECRET_LEN {
            None
        } else {
            let mut plain = Vec::with_capacity(len);
            plain.extend_from_slice(unsafe { std::slice::from_raw_parts(output.pbData, len) });
            Secret::new(plain)
        };
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
