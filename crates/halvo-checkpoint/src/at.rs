//! A directory that [`crate::remove`] opened part by part from the folder without following a
//! link, and the calls that act on an entry of it by its name alone. On macOS each part is a
//! descriptor opened relative to the one before, and every call acts relative to the last, so
//! a directory on the way that becomes a link after the walk is never followed. On Windows the
//! call holds a handle on each directory of the way, opened on the directory itself and not on
//! what a reparse point names, and shared for reading and writing but not for deletion, so no
//! directory of the way can be moved, removed or replaced by a link until the call ends.

use std::ffi::{OsStr, OsString};
use std::fs::{File, Metadata};
use std::io;
use std::path::Path;

#[cfg(target_os = "macos")]
pub(crate) use macos::Dir;

#[cfg(not(target_os = "macos"))]
pub(crate) use by_path::Dir;

impl Dir {
    /// The directory that holds the last name of `names`, walked from `folder` part by part.
    pub(crate) fn walk(folder: &Path, names: &[&OsStr]) -> io::Result<Dir> {
        let mut dir = Dir::folder(folder)?;
        for name in names {
            dir = dir.child(name)?;
        }
        Ok(dir)
    }

    /// The names of the directory's entries.
    pub(crate) fn names(&self) -> io::Result<Vec<OsString>> {
        self.entries()
    }

    /// The entry `name` opened for reading, also a link itself where the system opens one, and
    /// its metadata as the open file reports it.
    pub(crate) fn open(&self, name: &OsStr) -> io::Result<(File, Metadata)> {
        let file = self.file(name)?;
        let meta = file.metadata()?;
        Ok((file, meta))
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::c_void;
    use std::ffi::{c_char, CString, OsStr, OsString};
    use std::fs::{File, Metadata, OpenOptions};
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::Path;

    use crate::identity::FileId;

    const O_RDONLY: i32 = 0;
    const O_NONBLOCK: i32 = 0x0004;
    const O_NOFOLLOW: i32 = 0x0100;
    const O_DIRECTORY: i32 = 0x0010_0000;
    const O_CLOEXEC: i32 = 0x0100_0000;
    const RENAME_EXCL: u32 = 0x0000_0004;

    extern "C" {
        fn openat(fd: i32, path: *const c_char, flags: i32, ...) -> i32;
        fn renameatx_np(
            from_fd: i32,
            from: *const c_char,
            to_fd: i32,
            to: *const c_char,
            flags: u32,
        ) -> i32;
        fn unlinkat(fd: i32, path: *const c_char, flags: i32) -> i32;
        #[cfg_attr(target_arch = "x86_64", link_name = "fdopendir$INODE64")]
        fn fdopendir(fd: i32) -> *mut c_void;
        #[cfg_attr(target_arch = "x86_64", link_name = "readdir$INODE64")]
        fn readdir(stream: *mut c_void) -> *mut Entry;
        fn closedir(stream: *mut c_void) -> i32;
        fn __error() -> *mut i32;
        fn linkat(
            from_fd: i32,
            from: *const c_char,
            to_fd: i32,
            to: *const c_char,
            flags: i32,
        ) -> i32;
    }

    pub(crate) struct Dir(File);

    /// `struct dirent` of macOS with 64-bit inode numbers.
    #[repr(C)]
    struct Entry {
        ino: u64,
        seekoff: u64,
        reclen: u16,
        namlen: u16,
        kind: u8,
        name: [c_char; 1024],
    }

    fn c_name(name: &OsStr) -> io::Result<CString> {
        CString::new(name.as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
    }

    fn answer(code: i32) -> io::Result<()> {
        if code < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }

    impl Dir {
        pub(super) fn folder(folder: &Path) -> io::Result<Dir> {
            let file = OpenOptions::new()
                .read(true)
                .custom_flags(O_DIRECTORY | O_CLOEXEC)
                .open(folder)?;
            Ok(Dir(file))
        }

        fn at(&self, name: &OsStr, flags: i32) -> io::Result<File> {
            let name = c_name(name)?;
            // SAFETY: `name` is a NUL-terminated string that lives across the call, the
            // descriptor is open for as long as `self` lives, and the flags take no mode, so
            // the variadic part is empty.
            let fd = unsafe { openat(self.0.as_raw_fd(), name.as_ptr(), flags) };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: `fd` is a descriptor that `openat` just opened and nothing else owns.
            Ok(unsafe { File::from_raw_fd(fd) })
        }

        pub(super) fn child(&self, name: &OsStr) -> io::Result<Dir> {
            let flags = O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC;
            Ok(Dir(self.at(name, flags)?))
        }

        /// Opens the entry `name` without following a link: a link answers the error of a
        /// loop (ELOOP), and a pipe does not block the open.
        pub(crate) fn file(&self, name: &OsStr) -> io::Result<File> {
            self.at(name, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
        }

        /// The identity of the directory that the descriptor has open.
        pub(crate) fn id(&self) -> io::Result<FileId> {
            FileId::of(&self.0)
        }

        /// Renames the entry `from` to `to` in this directory, and fails with
        /// `AlreadyExists` when an entry stands at `to`.
        pub(crate) fn rename_new(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            let (from, to) = (c_name(from)?, c_name(to)?);
            let fd = self.0.as_raw_fd();
            // SAFETY: both names are NUL-terminated strings that live across the call, and the
            // descriptor is open for as long as `self` lives.
            answer(unsafe { renameatx_np(fd, from.as_ptr(), fd, to.as_ptr(), RENAME_EXCL) })
        }

        /// Removes the entry `name`, a link itself when it is one.
        pub(crate) fn unlink(&self, name: &OsStr) -> io::Result<()> {
            let name = c_name(name)?;
            // SAFETY: `name` is a NUL-terminated string that lives across the call, and the
            // descriptor is open for as long as `self` lives.
            answer(unsafe { unlinkat(self.0.as_raw_fd(), name.as_ptr(), 0) })
        }

        /// Gives the file of the entry `from` the second name `to`, following no link.
        pub(crate) fn link(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            let (from, to) = (c_name(from)?, c_name(to)?);
            let fd = self.0.as_raw_fd();
            // SAFETY: both names are NUL-terminated strings that live across the call, and the
            // descriptor is open for as long as `self` lives.
            answer(unsafe { linkat(fd, from.as_ptr(), fd, to.as_ptr(), 0) })
        }

        /// What the entry `name` is, read without opening it, after an open failed: nothing
        /// on macOS, where the open itself tells a link (ELOOP) and opens any other entry.
        pub(crate) fn look(&self, _: &OsStr) -> Option<Metadata> {
            None
        }

        /// The names of the entries, read through a new descriptor of this directory, which
        /// `openat` opens as `.` relative to it, so the listing starts at its first entry.
        pub(super) fn entries(&self) -> io::Result<Vec<OsString>> {
            let fresh = self.at(OsStr::new("."), O_RDONLY | O_DIRECTORY | O_CLOEXEC)?;
            let fd = fresh.into_raw_fd();
            // SAFETY: `fd` is a directory descriptor that nothing else owns; on success the
            // stream owns it and closedir closes it.
            let stream = unsafe { fdopendir(fd) };
            if stream.is_null() {
                let error = io::Error::last_os_error();
                // SAFETY: fdopendir failed, so `fd` is still ours alone and is closed once here.
                drop(unsafe { File::from_raw_fd(fd) });
                return Err(error);
            }
            let mut names = Vec::new();
            let read = loop {
                // SAFETY: __error returns the address of this thread's errno, which is valid
                // for the thread's life; it is cleared so that an end tells from an error.
                unsafe { *__error() = 0 };
                // SAFETY: `stream` is the open stream that fdopendir returned.
                let entry = unsafe { readdir(stream) };
                if entry.is_null() {
                    // SAFETY: as above, the address of this thread's errno.
                    let code = unsafe { *__error() };
                    break if code == 0 {
                        Ok(())
                    } else {
                        Err(io::Error::from_raw_os_error(code))
                    };
                }
                // SAFETY: readdir returned an entry that stays valid until the next call on
                // `stream`; its name holds `namlen` bytes, at most the 1024 of the field.
                let entry = unsafe { &*entry };
                let length = usize::from(entry.namlen).min(entry.name.len());
                let bytes: Vec<u8> = entry.name[..length].iter().map(|&b| b as u8).collect();
                if bytes != b"." && bytes != b".." {
                    names.push(OsStr::from_bytes(&bytes).to_os_string());
                }
            };
            // SAFETY: `stream` came from fdopendir, is closed once here and not used after.
            unsafe { closedir(stream) };
            read.map(|()| names)
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod by_path {
    use std::ffi::{OsStr, OsString};
    use std::fs::{self, File, Metadata};
    use std::io;
    use std::path::{Path, PathBuf};

    use crate::identity::FileId;

    pub(crate) struct Dir {
        path: PathBuf,
        #[cfg(windows)]
        _pins: Vec<File>,
    }

    #[cfg(windows)]
    mod system {
        use std::fs::{File, OpenOptions};
        use std::io;
        use std::os::windows::ffi::OsStrExt;
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use std::path::Path;

        const FILE_READ_ATTRIBUTES: u32 = 0x0080;
        const FILE_SHARE_READ: u32 = 0x0001;
        const FILE_SHARE_WRITE: u32 = 0x0002;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;

        #[link(name = "kernel32")]
        extern "system" {
            fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
        }

        /// Opens the directory at `path` itself, never what a reparse point there names, shared
        /// for reading and writing but not for deletion: while the handle is open, no one can
        /// move, remove or replace the directory. A reparse point or a file answers an error
        /// whose kind is `NotADirectory`.
        pub(super) fn pin(path: &Path) -> io::Result<File> {
            let file = OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?;
            let meta = file.metadata()?;
            if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 || !meta.is_dir() {
                return Err(io::Error::from(io::ErrorKind::NotADirectory));
            }
            Ok(file)
        }

        pub(super) fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
            let wide = |path: &Path| -> Vec<u16> {
                path.as_os_str().encode_wide().chain(Some(0)).collect()
            };
            let (from, to) = (wide(from), wide(to));
            // SAFETY: both arguments are NUL-terminated UTF-16 strings that live across the
            // call; the flags 0 ask for no replacement of an entry at `to`.
            let done = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) };
            if done == 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    impl Dir {
        #[cfg(windows)]
        pub(super) fn folder(folder: &Path) -> io::Result<Dir> {
            let pin = system::pin(folder)?;
            Ok(Dir {
                path: folder.to_path_buf(),
                _pins: vec![pin],
            })
        }

        #[cfg(not(windows))]
        pub(super) fn folder(folder: &Path) -> io::Result<Dir> {
            Ok(Dir {
                path: folder.to_path_buf(),
            })
        }

        #[cfg(windows)]
        pub(super) fn child(mut self, name: &OsStr) -> io::Result<Dir> {
            let path = self.path.join(name);
            let pin = system::pin(&path)?;
            self._pins.push(pin);
            self.path = path;
            Ok(self)
        }

        #[cfg(not(windows))]
        pub(super) fn child(self, name: &OsStr) -> io::Result<Dir> {
            let path = self.path.join(name);
            let meta = fs::symlink_metadata(&path)?;
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(io::Error::from(io::ErrorKind::NotADirectory));
            }
            Ok(Dir { path })
        }

        pub(crate) fn file(&self, name: &OsStr) -> io::Result<File> {
            crate::walk::no_follow(&self.path.join(name))
        }

        /// The identity of the last directory of the way, read through the handle that pins it.
        #[cfg(windows)]
        pub(crate) fn id(&self) -> io::Result<FileId> {
            match self._pins.last() {
                Some(pin) => FileId::of(pin),
                None => Err(io::Error::from(io::ErrorKind::NotFound)),
            }
        }

        #[cfg(not(windows))]
        pub(crate) fn id(&self) -> io::Result<FileId> {
            FileId::at(&self.path)
        }

        #[cfg(windows)]
        pub(crate) fn rename_new(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            system::rename_new(&self.path.join(from), &self.path.join(to))
        }

        #[cfg(not(windows))]
        pub(crate) fn rename_new(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            if fs::symlink_metadata(self.path.join(to)).is_ok() {
                return Err(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            fs::rename(self.path.join(from), self.path.join(to))
        }

        pub(crate) fn unlink(&self, name: &OsStr) -> io::Result<()> {
            fs::remove_file(self.path.join(name))
        }

        pub(crate) fn link(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            fs::hard_link(self.path.join(from), self.path.join(to))
        }

        /// What the entry `name` is, read without following a link, after an open failed.
        pub(crate) fn look(&self, name: &OsStr) -> Option<Metadata> {
            fs::symlink_metadata(self.path.join(name)).ok()
        }

        pub(super) fn entries(&self) -> io::Result<Vec<OsString>> {
            let listed = fs::read_dir(&self.path)?;
            listed
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect()
        }
    }
}
