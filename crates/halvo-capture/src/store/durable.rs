use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

pub(crate) trait DurableFs: Send + Sync {
    fn create_new(&self, dir: &Dir, name: &OsStr) -> io::Result<File>;
    fn sync_file(&self, file: &File, dir: &Dir, name: &OsStr) -> io::Result<()>;
    fn rename(&self, dir: &Dir, from: &OsStr, to: &OsStr) -> io::Result<()>;
    fn remove_file(&self, dir: &Dir, name: &OsStr) -> io::Result<()>;
    fn sync_dir(&self, dir: &Dir) -> io::Result<()>;
    fn entries(&self, dir: &Dir) -> io::Result<Vec<Entry>>;
    fn open_dir(&self, dir: &Dir, name: &OsStr) -> io::Result<Dir>;
}

pub(crate) struct OsFs;

impl DurableFs for OsFs {
    fn create_new(&self, dir: &Dir, name: &OsStr) -> io::Result<File> {
        dir.create_new(name)
    }

    fn sync_file(&self, file: &File, _dir: &Dir, _name: &OsStr) -> io::Result<()> {
        file.sync_all()
    }

    fn rename(&self, dir: &Dir, from: &OsStr, to: &OsStr) -> io::Result<()> {
        dir.rename(from, to)
    }

    fn remove_file(&self, dir: &Dir, name: &OsStr) -> io::Result<()> {
        dir.remove_file(name)
    }

    fn sync_dir(&self, dir: &Dir) -> io::Result<()> {
        dir.sync()
    }

    fn entries(&self, dir: &Dir) -> io::Result<Vec<Entry>> {
        dir.entries()
    }

    fn open_dir(&self, dir: &Dir, name: &OsStr) -> io::Result<Dir> {
        dir.open_dir(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum EntryKind {
    Dir,
    File,
    Link,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) name: OsString,
    pub(crate) kind: EntryKind,
    pub(crate) len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    Link,
    NotADirectory,
    NotARegularFile,
    #[cfg(windows)]
    Moved,
}

impl fmt::Display for Refused {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Refused::Link => "the name is a link",
            Refused::NotADirectory => "the name is no directory",
            Refused::NotARegularFile => "the name is no regular file",
            #[cfg(windows)]
            Refused::Moved => "the name no longer names the directory that was opened",
        })
    }
}

impl std::error::Error for Refused {}

fn refused(why: Refused) -> io::Error {
    io::Error::other(why)
}

pub(crate) fn refusal(e: &io::Error) -> Option<Refused> {
    e.get_ref()
        .and_then(|inner| inner.downcast_ref::<Refused>())
        .copied()
}

#[cfg(target_os = "macos")]
pub(crate) struct Dir {
    fd: std::os::fd::OwnedFd,
    path: PathBuf,
}

#[cfg(windows)]
pub(crate) struct Dir {
    _held: File,
    path: PathBuf,
}

impl Dir {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn remove_entry(&self, name: &OsStr) -> io::Result<()> {
        match self.entry(name)?.kind {
            EntryKind::Dir => self.remove_dir(name),
            EntryKind::Link => match self.remove_file(name) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => self.remove_dir(name),
                other => other,
            },
            EntryKind::File | EntryKind::Other => self.remove_file(name),
        }
    }

    pub(crate) fn remove_tree(&self, name: &OsStr) -> io::Result<()> {
        let gone = |removed: io::Result<()>| match removed {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
        let entry = match self.entry(name) {
            Ok(entry) => entry,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e),
        };
        if entry.kind == EntryKind::Dir {
            match self.open_dir(name) {
                Ok(below) => {
                    for child in below.entries()? {
                        below.remove_tree(&child.name)?;
                    }
                    drop(below);
                    return gone(self.remove_dir(name));
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(e) if refusal(&e) == Some(Refused::Link) => {}
                Err(e) => return Err(e),
            }
        }
        match self.remove_file(name) {
            Err(e) if e.kind() != io::ErrorKind::NotFound && entry.kind == EntryKind::Link => {
                gone(self.remove_dir(name))
            }
            other => gone(other),
        }
    }
}

#[cfg(target_os = "macos")]
pub(crate) use unix_dir::{volume_of, EntryIdentity};

#[cfg(target_os = "macos")]
mod unix_dir {
    use std::ffi::{CStr, CString, OsStr, OsString};
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    use super::{refusal, refused, Dir, Entry, EntryKind, Refused};

    fn c_name(name: &OsStr) -> io::Result<CString> {
        CString::new(name.as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
    }

    fn opened(fd: libc::c_int, at: RawFd, name: &CStr) -> io::Result<OwnedFd> {
        if fd >= 0 {
            // SAFETY: an open call answered this descriptor and nothing else owns
            // it, so the OwnedFd becomes its one owner and closes it once.
            return Ok(unsafe { OwnedFd::from_raw_fd(fd) });
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::ELOOP | libc::ENOTDIR) => {}
            _ => return Err(e),
        }
        // SAFETY: libc::stat is plain data, for which all-zero bytes are a valid value.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: name is NUL-terminated and st is a stat; both outlive the call.
        let linked =
            unsafe { libc::fstatat(at, name.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) } == 0
                && kind(&st) == EntryKind::Link;
        Err(match (linked, e.raw_os_error()) {
            (true, _) | (false, Some(libc::ELOOP)) => refused(Refused::Link),
            _ => refused(Refused::NotADirectory),
        })
    }

    fn checked(result: libc::c_int) -> io::Result<()> {
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn path_of(fd: RawFd) -> io::Result<PathBuf> {
        let mut buf = vec![0u8; libc::PATH_MAX as usize];
        // SAFETY: F_GETPATH writes at most PATH_MAX bytes, which buf holds, and fd
        // is a descriptor that the caller keeps open across the call.
        checked(unsafe { libc::fcntl(fd, libc::F_GETPATH, buf.as_mut_ptr()) })?;
        let len = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        buf.truncate(len);
        Ok(PathBuf::from(OsString::from_vec(buf)))
    }

    fn stat_at(fd: RawFd, name: &OsStr) -> io::Result<libc::stat> {
        let c = c_name(name)?;
        // SAFETY: libc::stat is plain data, for which all-zero bytes are a valid value.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: c is NUL-terminated and st is a stat; both outlive the call.
        checked(unsafe { libc::fstatat(fd, c.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) })?;
        Ok(st)
    }

    fn stat_of(fd: RawFd) -> io::Result<libc::stat> {
        // SAFETY: libc::stat is plain data, for which all-zero bytes are a valid value.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: st is a stat that outlives the call, which writes only into it.
        checked(unsafe { libc::fstat(fd, &mut st) })?;
        Ok(st)
    }

    pub(crate) struct EntryIdentity {
        pub(crate) kind: EntryKind,
        pub(crate) device: u64,
        pub(crate) file_number: u64,
    }

    pub(crate) struct Volume {
        pub(crate) flags: u32,
        pub(crate) mount_point: PathBuf,
        pub(crate) fsid: [i32; 2],
    }

    pub(crate) fn volume_of(fd: RawFd) -> io::Result<Volume> {
        // SAFETY: libc::statfs is plain data, for which all-zero bytes are a valid value.
        let mut st: libc::statfs = unsafe { std::mem::zeroed() };
        // SAFETY: st is a statfs that outlives the call, which writes only into it,
        // and fd is a descriptor that the caller keeps open across the call.
        checked(unsafe { libc::fstatfs(fd, &mut st) })?;
        // SAFETY: f_mntonname is a NUL-terminated array that the call filled.
        let mount = unsafe { CStr::from_ptr(st.f_mntonname.as_ptr()) };
        Ok(Volume {
            flags: st.f_flags,
            mount_point: PathBuf::from(OsStr::from_bytes(mount.to_bytes())),
            // SAFETY: fsid_t is a C struct of two i32 values, the size of [i32; 2],
            // and every bit pattern of either is a valid value.
            fsid: unsafe { std::mem::transmute::<libc::fsid_t, [i32; 2]>(st.f_fsid) },
        })
    }

    fn kind(st: &libc::stat) -> EntryKind {
        match st.st_mode & libc::S_IFMT {
            libc::S_IFDIR => EntryKind::Dir,
            libc::S_IFREG => EntryKind::File,
            libc::S_IFLNK => EntryKind::Link,
            _ => EntryKind::Other,
        }
    }

    const DIR_FLAGS: libc::c_int =
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;

    const LINK_THE_ENTRY_NOT_ITS_TARGET: libc::c_int = 0;

    impl Dir {
        pub(crate) fn open(path: &Path) -> io::Result<Dir> {
            let c = c_name(path.as_os_str())?;
            // SAFETY: c is NUL-terminated and outlives the call.
            let fd = opened(
                unsafe { libc::open(c.as_ptr(), DIR_FLAGS) },
                libc::AT_FDCWD,
                &c,
            )?;
            let path = path_of(fd.as_raw_fd())?;
            Ok(Dir { fd, path })
        }

        pub(crate) fn try_clone(&self) -> io::Result<Dir> {
            Ok(Dir {
                fd: self.fd.try_clone()?,
                path: self.path.clone(),
            })
        }

        pub(crate) fn open_dir(&self, name: &OsStr) -> io::Result<Dir> {
            let c = c_name(name)?;
            let at = self.fd.as_raw_fd();
            // SAFETY: c is NUL-terminated, and at is this handle's descriptor, open
            // while self lives.
            let fd = opened(unsafe { libc::openat(at, c.as_ptr(), DIR_FLAGS) }, at, &c)?;
            Ok(Dir {
                fd,
                path: self.path.join(name),
            })
        }

        pub(crate) fn create_dir(&self, name: &OsStr) -> io::Result<()> {
            let c = c_name(name)?;
            // SAFETY: c is NUL-terminated, and the descriptor is this handle's, open
            // while self lives.
            if unsafe { libc::mkdirat(self.fd.as_raw_fd(), c.as_ptr(), 0o700) } < 0 {
                let e = io::Error::last_os_error();
                if e.kind() != io::ErrorKind::AlreadyExists {
                    return Err(e);
                }
            }
            Ok(())
        }

        pub(crate) fn make_private(&self) -> io::Result<()> {
            // SAFETY: the descriptor is this handle's, open while self lives.
            checked(unsafe { libc::fchmod(self.fd.as_raw_fd(), 0o700) })
        }

        pub(crate) fn entry(&self, name: &OsStr) -> io::Result<Entry> {
            stat_at(self.fd.as_raw_fd(), name).map(|st| Entry {
                name: name.to_os_string(),
                kind: kind(&st),
                len: st.st_size as u64,
            })
        }

        pub(crate) fn entries(&self) -> io::Result<Vec<Entry>> {
            let at = self.fd.as_raw_fd();
            // SAFETY: "." is a NUL-terminated literal, and at is this handle's
            // descriptor, open while self lives.
            let fd = opened(
                unsafe { libc::openat(at, c".".as_ptr(), DIR_FLAGS) },
                at,
                c".",
            )?;
            let raw = std::os::fd::IntoRawFd::into_raw_fd(fd);
            // SAFETY: raw came from into_raw_fd, so nothing else owns it, and a
            // stream that fdopendir answers takes that one ownership.
            let stream = unsafe { libc::fdopendir(raw) };
            if stream.is_null() {
                let e = io::Error::last_os_error();
                // SAFETY: fdopendir failed, so raw is still ours alone, and the
                // OwnedFd that takes it back closes it once when dropped.
                drop(unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) });
                return Err(e);
            }
            let mut names = Vec::new();
            let listed = loop {
                // SAFETY: __error gives this thread's errno, a valid int for the
                // thread's life.
                unsafe { *libc::__error() = 0 };
                // SAFETY: stream is open until the one closedir below.
                let entry = unsafe { libc::readdir(stream) };
                if entry.is_null() {
                    // SAFETY: as above, the errno of this thread.
                    let errno = unsafe { *libc::__error() };
                    break if errno == 0 {
                        Ok(())
                    } else {
                        Err(io::Error::from_raw_os_error(errno))
                    };
                }
                // SAFETY: readdir answered an entry, whose d_name is NUL-terminated
                // and stays valid until the next call on the stream; the bytes are
                // copied before that.
                let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
                if name != b"." && name != b".." {
                    names.push(OsString::from_vec(name.to_vec()));
                }
            };
            // SAFETY: stream came from fdopendir and is closed here, once.
            let closed = match unsafe { libc::closedir(stream) } {
                0 => Ok(()),
                _ => Err(io::Error::last_os_error()),
            };
            listed?;
            closed?;
            let mut entries = Vec::new();
            for name in names {
                match self.entry(&name) {
                    Ok(entry) => entries.push(entry),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e),
                }
            }
            Ok(entries)
        }

        pub(crate) fn open_file(&self, name: &OsStr) -> io::Result<File> {
            let c = c_name(name)?;
            let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
            let at = self.fd.as_raw_fd();
            // SAFETY: c is NUL-terminated, and at is this handle's descriptor, open
            // while self lives.
            let fd = opened(unsafe { libc::openat(at, c.as_ptr(), flags) }, at, &c)
                .map_err(|e| self.refuse_other_kind_or(name, e))?;
            match kind(&stat_of(fd.as_raw_fd())?) {
                EntryKind::File => Ok(File::from(fd)),
                EntryKind::Dir => Err(io::Error::from_raw_os_error(libc::EISDIR)),
                _ => Err(refused(Refused::NotARegularFile)),
            }
        }

        pub(crate) fn create_new(&self, name: &OsStr) -> io::Result<File> {
            let c = c_name(name)?;
            let flags =
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
            // SAFETY: c is NUL-terminated, the descriptor is this handle's, and the
            // mode is the one variadic argument that O_CREAT reads.
            let fd = unsafe {
                libc::openat(
                    self.fd.as_raw_fd(),
                    c.as_ptr(),
                    flags,
                    0o600 as libc::c_uint,
                )
            };
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: openat answered this descriptor and nothing else owns it, so
            // the OwnedFd becomes its one owner.
            Ok(File::from(unsafe { OwnedFd::from_raw_fd(fd) }))
        }

        pub(crate) fn rename(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            let (from, to) = (c_name(from)?, c_name(to)?);
            let fd = self.fd.as_raw_fd();
            // SAFETY: from and to are NUL-terminated, and fd is this handle's
            // descriptor, open while self lives.
            checked(unsafe { libc::renameat(fd, from.as_ptr(), fd, to.as_ptr()) })
        }

        pub(crate) fn remove_file(&self, name: &OsStr) -> io::Result<()> {
            let c = c_name(name)?;
            // SAFETY: c is NUL-terminated, and the descriptor is this handle's.
            checked(unsafe { libc::unlinkat(self.fd.as_raw_fd(), c.as_ptr(), 0) })
        }

        pub(crate) fn remove_dir(&self, name: &OsStr) -> io::Result<()> {
            let c = c_name(name)?;
            // SAFETY: c is NUL-terminated, and the descriptor is this handle's.
            checked(unsafe { libc::unlinkat(self.fd.as_raw_fd(), c.as_ptr(), libc::AT_REMOVEDIR) })
        }

        /// Sets the mode of the entry `name` by its name through this handle,
        /// following no link.
        pub(crate) fn set_file_mode(&self, name: &OsStr, mode: libc::mode_t) -> io::Result<()> {
            let c = c_name(name)?;
            // SAFETY: c is NUL-terminated, and the descriptor is this handle's,
            // open while self lives.
            checked(unsafe {
                libc::fchmodat(
                    self.fd.as_raw_fd(),
                    c.as_ptr(),
                    mode,
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            })
        }

        /// Opens the regular file `name` for reading and writing, following
        /// no link.
        pub(crate) fn open_file_for_update(&self, name: &OsStr) -> io::Result<File> {
            let c = c_name(name)?;
            let flags = libc::O_RDWR | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
            let at = self.fd.as_raw_fd();
            // SAFETY: c is NUL-terminated, and at is this handle's descriptor, open
            // while self lives.
            let fd = opened(unsafe { libc::openat(at, c.as_ptr(), flags) }, at, &c)
                .map_err(|e| self.refuse_other_kind_or(name, e))?;
            match kind(&stat_of(fd.as_raw_fd())?) {
                EntryKind::File => Ok(File::from(fd)),
                _ => Err(refused(Refused::NotARegularFile)),
            }
        }

        pub(crate) fn open_file_for_write(&self, name: &OsStr) -> io::Result<File> {
            let c = c_name(name)?;
            let flags = libc::O_WRONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
            let at = self.fd.as_raw_fd();
            // SAFETY: c is NUL-terminated, and at is this handle's descriptor, open
            // while self lives.
            let fd = opened(unsafe { libc::openat(at, c.as_ptr(), flags) }, at, &c)
                .map_err(|e| self.refuse_other_kind_or(name, e))?;
            match kind(&stat_of(fd.as_raw_fd())?) {
                EntryKind::File => Ok(File::from(fd)),
                _ => Err(refused(Refused::NotARegularFile)),
            }
        }

        fn refuse_other_kind_or(&self, name: &OsStr, e: io::Error) -> io::Error {
            if e.kind() == io::ErrorKind::NotFound || refusal(&e).is_some() {
                return e;
            }
            match stat_at(self.fd.as_raw_fd(), name).map(|st| kind(&st)) {
                Ok(EntryKind::Other) => refused(Refused::NotARegularFile),
                _ => e,
            }
        }

        pub(crate) fn link(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            let (from, to) = (c_name(from)?, c_name(to)?);
            let fd = self.fd.as_raw_fd();
            // SAFETY: from and to are NUL-terminated, and fd is this handle's
            // descriptor, open while self lives.
            checked(unsafe {
                libc::linkat(
                    fd,
                    from.as_ptr(),
                    fd,
                    to.as_ptr(),
                    LINK_THE_ENTRY_NOT_ITS_TARGET,
                )
            })
        }

        pub(crate) fn identity(&self) -> io::Result<(u64, u64)> {
            let st = stat_of(self.fd.as_raw_fd())?;
            Ok((st.st_dev as u64, st.st_ino))
        }

        pub(crate) fn raw_descriptor(&self) -> RawFd {
            self.fd.as_raw_fd()
        }

        pub(crate) fn link_count(&self) -> io::Result<u64> {
            Ok(u64::from(stat_of(self.fd.as_raw_fd())?.st_nlink))
        }

        pub(crate) fn volume(&self) -> io::Result<Volume> {
            volume_of(self.fd.as_raw_fd())
        }

        pub(crate) fn entry_identity(&self, name: &OsStr) -> io::Result<EntryIdentity> {
            let st = stat_at(self.fd.as_raw_fd(), name)?;
            Ok(EntryIdentity {
                kind: kind(&st),
                device: st.st_dev as u64,
                file_number: st.st_ino,
            })
        }

        pub(crate) fn into_file(self) -> File {
            File::from(self.fd)
        }

        pub(crate) fn still_named(&self) -> io::Result<bool> {
            let c = c_name(self.path.as_os_str())?;
            // SAFETY: libc::stat is plain data, for which all-zero bytes are a valid value.
            let mut st: libc::stat = unsafe { std::mem::zeroed() };
            // SAFETY: c is NUL-terminated and st is a stat; both outlive the call.
            if unsafe { libc::lstat(c.as_ptr(), &mut st) } < 0 {
                let e = io::Error::last_os_error();
                return match e.kind() {
                    io::ErrorKind::NotFound => Ok(false),
                    _ => Err(e),
                };
            }
            let held = stat_of(self.fd.as_raw_fd())?;
            Ok(kind(&st) == EntryKind::Dir && st.st_dev == held.st_dev && st.st_ino == held.st_ino)
        }

        pub(crate) fn sync(&self) -> io::Result<()> {
            File::from(self.fd.try_clone()?).sync_all()
        }

        pub(crate) fn names_file(&self, name: &OsStr, file: &File) -> io::Result<bool> {
            let named = match stat_at(self.fd.as_raw_fd(), name) {
                Ok(st) => st,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(e) => return Err(e),
            };
            let held = stat_of(file.as_raw_fd())?;
            Ok(kind(&named) == EntryKind::File
                && named.st_dev == held.st_dev
                && named.st_ino == held.st_ino)
        }

        pub(crate) fn names(&self, name: &OsStr, child: &Dir) -> io::Result<bool> {
            let named = match stat_at(self.fd.as_raw_fd(), name) {
                Ok(st) => st,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(e) => return Err(e),
            };
            let held = stat_of(child.fd.as_raw_fd())?;
            Ok(kind(&named) == EntryKind::Dir
                && named.st_dev == held.st_dev
                && named.st_ino == held.st_ino)
        }
    }
}

#[cfg(windows)]
mod windows_dir {
    use std::ffi::OsStr;
    use std::fs::{File, Metadata};
    use std::io;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};

    use super::{refusal, refused, Dir, Entry, EntryKind, Refused, FILE_FLAG_OPEN_REPARSE_POINT};

    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    const FILE_SHARE_READ: u32 = 0x1;
    const FILE_SHARE_WRITE: u32 = 0x2;
    const FILE_SHARE_DELETE: u32 = 0x4;

    fn kind_of(meta: &Metadata) -> EntryKind {
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || meta.file_type().is_symlink()
        {
            EntryKind::Link
        } else if meta.is_dir() {
            EntryKind::Dir
        } else if meta.is_file() {
            EntryKind::File
        } else {
            EntryKind::Other
        }
    }

    #[repr(C)]
    struct FileInformation {
        attributes: u32,
        creation_time: [u32; 2],
        last_access_time: [u32; 2],
        last_write_time: [u32; 2],
        volume_serial_number: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetFileInformationByHandle(
            file: std::os::windows::io::RawHandle,
            information: *mut FileInformation,
        ) -> i32;
    }

    pub(crate) fn file_link_count(file: &File) -> io::Result<u64> {
        use std::os::windows::io::AsRawHandle;
        let mut information = std::mem::MaybeUninit::<FileInformation>::zeroed();
        // SAFETY: the handle is open for as long as `file` is borrowed, and the
        // pointer is to a zeroed structure of the layout that the call writes.
        let done =
            unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) };
        if done == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call answered success, so it wrote every field, and the
        // zeroed start was already a valid value of the structure.
        let information = unsafe { information.assume_init() };
        Ok(u64::from(information.links))
    }

    fn file_id(file: &File) -> io::Result<(u32, u64)> {
        use std::os::windows::io::AsRawHandle;
        let mut information = std::mem::MaybeUninit::<FileInformation>::zeroed();
        // SAFETY: the handle is open for as long as `file` is borrowed, and the
        // pointer is to a zeroed structure of the layout that the call writes.
        let done =
            unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) };
        if done == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: the call answered success, so it wrote every field, and the
        // zeroed start was already a valid value of the structure.
        let information = unsafe { information.assume_init() };
        Ok((
            information.volume_serial_number,
            (u64::from(information.index_high) << 32) | u64::from(information.index_low),
        ))
    }

    fn held_dir(path: &Path) -> io::Result<File> {
        let handle = File::options()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        match kind_of(&handle.metadata()?) {
            EntryKind::Dir => Ok(handle),
            EntryKind::Link => Err(refused(Refused::Link)),
            _ => Err(refused(Refused::NotADirectory)),
        }
    }

    fn held_file(path: &Path, share: u32) -> io::Result<File> {
        let file = File::options()
            .read(true)
            .share_mode(share)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        match kind_of(&file.metadata()?) {
            EntryKind::File => Ok(file),
            EntryKind::Link => Err(refused(Refused::Link)),
            EntryKind::Dir => Err(io::Error::from(io::ErrorKind::IsADirectory)),
            EntryKind::Other => Err(refused(Refused::NotARegularFile)),
        }
    }

    /// The canonical path without its verbatim prefix, so that it reads as
    /// the path that the caller gave, except where a part ends in a dot or a
    /// space: Win32 trims that end on a path that is not verbatim, so such a
    /// path keeps its prefix and names the entry that the handle holds.
    fn without_verbatim_prefix(full: PathBuf) -> PathBuf {
        if super::has_a_part_that_windows_trims(&full) {
            return full;
        }
        let Some(text) = full.to_str() else {
            return full;
        };
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{rest}"));
        }
        match text.strip_prefix(r"\\?\") {
            Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
            _ => full,
        }
    }

    impl Dir {
        pub(crate) fn open(path: &Path) -> io::Result<Dir> {
            let given = held_dir(&std::path::absolute(path)?)?;
            let path = without_verbatim_prefix(std::fs::canonicalize(path)?);
            let held = held_dir(&path)?;
            if file_id(&given)? != file_id(&held)? {
                return Err(refused(Refused::Moved));
            }
            drop(given);
            Ok(Dir { _held: held, path })
        }

        pub(crate) fn try_clone(&self) -> io::Result<Dir> {
            Ok(Dir {
                _held: self._held.try_clone()?,
                path: self.path.clone(),
            })
        }

        pub(crate) fn open_dir(&self, name: &OsStr) -> io::Result<Dir> {
            let path = self.path.join(name);
            Ok(Dir {
                _held: held_dir(&path)?,
                path,
            })
        }

        pub(crate) fn create_dir(&self, name: &OsStr) -> io::Result<()> {
            match std::fs::create_dir(self.path.join(name)) {
                Err(e) if e.kind() != io::ErrorKind::AlreadyExists => Err(e),
                _ => Ok(()),
            }
        }

        pub(crate) fn make_private(&self) -> io::Result<()> {
            Ok(())
        }

        pub(crate) fn entry(&self, name: &OsStr) -> io::Result<Entry> {
            let meta = std::fs::symlink_metadata(self.path.join(name))?;
            Ok(Entry {
                name: name.to_os_string(),
                kind: kind_of(&meta),
                len: meta.len(),
            })
        }

        pub(crate) fn entries(&self) -> io::Result<Vec<Entry>> {
            let mut entries = Vec::new();
            for entry in std::fs::read_dir(&self.path)? {
                let entry = entry?;
                let meta = match std::fs::symlink_metadata(entry.path()) {
                    Ok(meta) => meta,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(_) => match entry.metadata() {
                        Ok(meta) => meta,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                        Err(e) => return Err(e),
                    },
                };
                entries.push(Entry {
                    name: entry.file_name(),
                    kind: kind_of(&meta),
                    len: meta.len(),
                });
            }
            Ok(entries)
        }

        pub(crate) fn open_file(&self, name: &OsStr) -> io::Result<File> {
            held_file(
                &self.path.join(name),
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            )
        }

        pub(crate) fn hold_file(&self, name: &OsStr) -> io::Result<File> {
            held_file(&self.path.join(name), FILE_SHARE_READ | FILE_SHARE_WRITE)
        }

        pub(crate) fn create_new(&self, name: &OsStr) -> io::Result<File> {
            File::options()
                .write(true)
                .create_new(true)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(self.path.join(name))
        }

        pub(crate) fn rename(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            std::fs::rename(self.path.join(from), self.path.join(to))
        }

        pub(crate) fn remove_file(&self, name: &OsStr) -> io::Result<()> {
            std::fs::remove_file(self.path.join(name))
        }

        pub(crate) fn remove_dir(&self, name: &OsStr) -> io::Result<()> {
            std::fs::remove_dir(self.path.join(name))
        }

        /// Opens the regular file `name` for reading and writing, following
        /// no link.
        pub(crate) fn open_file_for_update(&self, name: &OsStr) -> io::Result<File> {
            let file = File::options()
                .read(true)
                .write(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(self.path.join(name))?;
            match kind_of(&file.metadata()?) {
                EntryKind::File => Ok(file),
                _ => Err(refused(Refused::NotARegularFile)),
            }
        }

        pub(crate) fn open_file_for_write(&self, name: &OsStr) -> io::Result<File> {
            let file = File::options()
                .write(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(self.path.join(name))?;
            match kind_of(&file.metadata()?) {
                EntryKind::File => Ok(file),
                _ => Err(refused(Refused::NotARegularFile)),
            }
        }

        pub(crate) fn link(&self, from: &OsStr, to: &OsStr) -> io::Result<()> {
            std::fs::hard_link(self.path.join(from), self.path.join(to))
        }

        pub(crate) fn open_lock_file(&self, name: &OsStr) -> io::Result<File> {
            let file = File::options()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
                .open(self.path.join(name))?;
            match kind_of(&file.metadata()?) {
                EntryKind::File => Ok(file),
                EntryKind::Link => Err(refused(Refused::Link)),
                EntryKind::Dir => Err(io::Error::from(io::ErrorKind::IsADirectory)),
                EntryKind::Other => Err(refused(Refused::NotARegularFile)),
            }
        }

        pub(crate) fn still_named(&self) -> io::Result<bool> {
            Ok(true)
        }

        pub(crate) fn sync(&self) -> io::Result<()> {
            Ok(())
        }

        pub(crate) fn names_file(&self, name: &OsStr, file: &File) -> io::Result<bool> {
            match held_file(
                &self.path.join(name),
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ) {
                Ok(named) => Ok(file_id(&named)? == file_id(file)?),
                Err(e) if e.kind() == io::ErrorKind::NotFound || refusal(&e).is_some() => Ok(false),
                Err(e) => Err(e),
            }
        }

        pub(crate) fn names(&self, name: &OsStr, _child: &Dir) -> io::Result<bool> {
            match self.entry(name) {
                Ok(entry) => Ok(entry.kind == EntryKind::Dir),
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(e) => Err(e),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_durably<T, E>(
    fs: &dyn DurableFs,
    dir: &Dir,
    temp: &OsStr,
    target: &OsStr,
    io_error: fn(io::Error) -> E,
    before_publish: impl FnOnce(&File) -> Result<(), E>,
    discard: impl FnOnce(&Dir, &OsStr) -> Result<(), E>,
    fill: impl FnOnce(&mut dyn Write) -> Result<T, E>,
) -> Result<T, E> {
    let file = fs.create_new(dir, temp).map_err(io_error)?;
    match fill_and_publish(fs, file, dir, temp, target, io_error, before_publish, fill) {
        Ok(value) => Ok(value),
        Err(failed) => match discard(dir, temp) {
            Ok(()) => Err(failed),
            Err(left_behind) => Err(left_behind),
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn fill_and_publish<T, E>(
    fs: &dyn DurableFs,
    file: File,
    dir: &Dir,
    temp: &OsStr,
    target: &OsStr,
    io_error: fn(io::Error) -> E,
    before_publish: impl FnOnce(&File) -> Result<(), E>,
    fill: impl FnOnce(&mut dyn Write) -> Result<T, E>,
) -> Result<T, E> {
    let mut writer = BufWriter::new(file);
    let value = fill(&mut writer)?;
    writer.flush().map_err(io_error)?;
    fs.sync_file(writer.get_ref(), dir, temp)
        .map_err(io_error)?;
    before_publish(writer.get_ref())?;
    drop(writer);
    fs.rename(dir, temp, target).map_err(io_error)?;
    fs.sync_dir(dir).map_err(io_error)?;
    Ok(value)
}

#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

#[cfg(windows)]
pub(crate) const ERROR_SHARING_VIOLATION: i32 = 32;

/// A part of the path ends in a dot or a space, which Win32 trims when it
/// opens a name that is not verbatim, so that an open could reach the sibling
/// without that end.
#[cfg(windows)]
pub(crate) fn has_a_part_that_windows_trims(path: &Path) -> bool {
    path.components().any(|part| match part {
        std::path::Component::Normal(name) => {
            let name = name.to_string_lossy();
            name.ends_with('.') || name.ends_with(' ')
        }
        _ => false,
    })
}

/// The verbatim form of a path, which Win32 opens without trimming: the path
/// itself when it is verbatim, `\\?\` before an absolute path on a drive that
/// holds no `.` or `..` part, and nothing for any other path.
#[cfg(windows)]
pub(crate) fn verbatim_form(path: &Path) -> Option<std::path::PathBuf> {
    use std::path::{Component, Prefix};
    let mut parts = path.components();
    let Some(Component::Prefix(prefix)) = parts.next() else {
        return None;
    };
    match prefix.kind() {
        Prefix::Verbatim(_) | Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(..) => {
            Some(path.to_path_buf())
        }
        Prefix::Disk(_)
            if parts.next() == Some(Component::RootDir)
                && parts.all(|part| matches!(part, Component::Normal(_))) =>
        {
            let mut verbatim = std::ffi::OsString::from(r"\\?\");
            verbatim.push(path.as_os_str());
            Some(std::path::PathBuf::from(verbatim))
        }
        _ => None,
    }
}

#[cfg(windows)]
pub(crate) use windows_dir::file_link_count;

#[cfg(unix)]
pub(crate) fn file_link_count(file: &File) -> io::Result<u64> {
    use std::os::unix::fs::MetadataExt;
    Ok(file.metadata()?.nlink())
}

pub(crate) fn create_private_dir_all(dir: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(dir)
}

pub(crate) fn remove_durably(fs: &dyn DurableFs, dir: &Dir, name: &OsStr) -> io::Result<()> {
    match fs.remove_file(dir, name) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    fs.sync_dir(dir)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::ffi::OsStr;
    use std::fs::File;
    use std::io;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::{Dir, DurableFs, Entry, OsFs};

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Call {
        CreateNew(PathBuf),
        SyncFile(PathBuf),
        Rename(PathBuf, PathBuf),
        RemoveFile(PathBuf),
        SyncDir(PathBuf),
        List(PathBuf),
        OpenDir(PathBuf),
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Kind {
        CreateNew,
        SyncFile,
        Rename,
        RemoveFile,
        SyncDir,
        List,
        OpenDir,
    }

    impl Call {
        fn kind(&self) -> Kind {
            match self {
                Call::CreateNew(_) => Kind::CreateNew,
                Call::SyncFile(_) => Kind::SyncFile,
                Call::Rename(..) => Kind::Rename,
                Call::RemoveFile(_) => Kind::RemoveFile,
                Call::SyncDir(_) => Kind::SyncDir,
                Call::List(_) => Kind::List,
                Call::OpenDir(_) => Kind::OpenDir,
            }
        }
    }

    type Observer = Box<dyn Fn(&Call) + Send + Sync>;

    #[derive(Default)]
    pub(crate) struct RecordingFs {
        calls: Mutex<Vec<Call>>,
        failing: Mutex<Vec<Kind>>,
        observer: Option<Observer>,
    }

    impl RecordingFs {
        pub(crate) fn new() -> Self {
            Self::default()
        }

        pub(crate) fn observed_by(observer: impl Fn(&Call) + Send + Sync + 'static) -> Self {
            Self {
                observer: Some(Box::new(observer)),
                ..Self::default()
            }
        }

        pub(crate) fn fail(&self, kind: Kind) {
            self.failing.lock().unwrap().push(kind);
        }

        pub(crate) fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }

        fn record(&self, call: Call) -> io::Result<()> {
            if let Some(observer) = &self.observer {
                observer(&call);
            }
            let kind = call.kind();
            self.calls.lock().unwrap().push(call);
            if self.failing.lock().unwrap().contains(&kind) {
                return Err(io::Error::from(io::ErrorKind::StorageFull));
            }
            Ok(())
        }
    }

    impl DurableFs for RecordingFs {
        fn create_new(&self, dir: &Dir, name: &OsStr) -> io::Result<File> {
            self.record(Call::CreateNew(dir.path().join(name)))?;
            OsFs.create_new(dir, name)
        }

        fn sync_file(&self, file: &File, dir: &Dir, name: &OsStr) -> io::Result<()> {
            self.record(Call::SyncFile(dir.path().join(name)))?;
            OsFs.sync_file(file, dir, name)
        }

        fn rename(&self, dir: &Dir, from: &OsStr, to: &OsStr) -> io::Result<()> {
            self.record(Call::Rename(dir.path().join(from), dir.path().join(to)))?;
            OsFs.rename(dir, from, to)
        }

        fn remove_file(&self, dir: &Dir, name: &OsStr) -> io::Result<()> {
            self.record(Call::RemoveFile(dir.path().join(name)))?;
            OsFs.remove_file(dir, name)
        }

        fn sync_dir(&self, dir: &Dir) -> io::Result<()> {
            self.record(Call::SyncDir(dir.path().to_path_buf()))?;
            OsFs.sync_dir(dir)
        }

        fn entries(&self, dir: &Dir) -> io::Result<Vec<Entry>> {
            self.record(Call::List(dir.path().to_path_buf()))?;
            OsFs.entries(dir)
        }

        fn open_dir(&self, dir: &Dir, name: &OsStr) -> io::Result<Dir> {
            self.record(Call::OpenDir(dir.path().join(name)))?;
            OsFs.open_dir(dir, name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{Call, Kind, RecordingFs};
    use super::*;

    fn held(dir: &tempfile::TempDir) -> Dir {
        Dir::open(dir.path()).unwrap()
    }

    fn name(text: &str) -> &OsStr {
        OsStr::new(text)
    }

    #[cfg(unix)]
    #[test]
    fn remove_entry_refuses_a_directory_with_entries_and_unlinks_a_link_to_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("full")).unwrap();
        std::fs::write(dir.path().join("full").join("kept"), b"kept").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("kept"), b"kept").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        let handle = held(&dir);
        assert!(handle.remove_entry(name("full")).is_err());
        assert_eq!(
            std::fs::read(dir.path().join("full").join("kept")).unwrap(),
            b"kept"
        );
        assert!(handle.remove_entry(name("link")).is_ok());
        assert!(std::fs::symlink_metadata(dir.path().join("link")).is_err());
        assert_eq!(std::fs::read(outside.path().join("kept")).unwrap(), b"kept");
    }

    #[cfg(unix)]
    #[test]
    fn remove_tree_unlinks_a_link_inside_the_tree_and_keeps_its_target() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("tree").join("below")).unwrap();
        std::fs::write(dir.path().join("tree").join("below").join("file"), b"x").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("kept"), b"kept").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("tree").join("link")).unwrap();
        held(&dir).remove_tree(name("tree")).unwrap();
        assert!(std::fs::symlink_metadata(dir.path().join("tree")).is_err());
        assert_eq!(std::fs::read(outside.path().join("kept")).unwrap(), b"kept");
    }

    #[cfg(windows)]
    #[test]
    fn a_held_store_directory_cannot_be_renamed_and_stays_named() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("store");
        std::fs::create_dir(&store).unwrap();
        let handle = Dir::open(&store).unwrap();
        let renamed = std::fs::rename(&store, dir.path().join("moved"));
        println!(
            "WINDOWS-RENAME renamed={} code={:?}",
            renamed.is_ok(),
            renamed.as_ref().err().and_then(|e| e.raw_os_error())
        );
        assert!(renamed.is_err(), "a held store directory was renamed");
        assert!(handle.still_named().unwrap());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_identity_of_an_entry_that_is_a_link_reads_link_and_names_the_link_itself() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("target"), b"target").unwrap();
        std::os::unix::fs::symlink(dir.path().join("target"), dir.path().join("link")).unwrap();
        let handle = held(&dir);
        let link = handle.entry_identity(name("link")).unwrap();
        let target = handle.entry_identity(name("target")).unwrap();
        assert_eq!(link.kind, EntryKind::Link);
        assert_eq!(target.kind, EntryKind::File);
        assert_ne!(link.file_number, target.file_number);
        assert_eq!(link.device, target.device);
    }

    #[test]
    fn a_durable_write_syncs_the_file_then_publishes_it_then_syncs_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let handle = held(&dir);
        let (temp, target) = (handle.path().join("file.tmp"), handle.path().join("file"));
        let fs = RecordingFs::new();
        let written = write_durably(
            &fs,
            &handle,
            name("file.tmp"),
            name("file"),
            |e| e,
            |_| Ok(()),
            |_, _| Ok(()),
            |out| out.write_all(b"durable bytes").map(|()| 13),
        );
        assert_eq!(written.unwrap(), 13);
        assert_eq!(
            fs.calls(),
            vec![
                Call::CreateNew(temp.clone()),
                Call::SyncFile(temp.clone()),
                Call::Rename(temp.clone(), target.clone()),
                Call::SyncDir(handle.path().to_path_buf()),
            ]
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"durable bytes");
        assert!(!temp.exists());
    }

    #[test]
    fn a_durable_write_on_a_full_volume_publishes_nothing() {
        for kind in [Kind::CreateNew, Kind::SyncFile, Kind::Rename] {
            let dir = tempfile::tempdir().unwrap();
            let handle = held(&dir);
            let fs = RecordingFs::new();
            fs.fail(kind);
            let written = write_durably(
                &fs,
                &handle,
                name("file.tmp"),
                name("file"),
                |e| e,
                |_| Ok(()),
                |_, _| Ok(()),
                |out| out.write_all(b"bytes"),
            );
            assert_eq!(
                written.map_err(|e| e.kind()),
                Err(std::io::ErrorKind::StorageFull),
                "{kind:?}"
            );
            assert!(
                !dir.path().join("file").exists(),
                "a write that failed at {kind:?} published the file"
            );
        }
    }

    #[test]
    fn a_durable_write_never_writes_through_an_existing_temporary_name() {
        let dir = tempfile::tempdir().unwrap();
        let (temp, target) = (dir.path().join("file.tmp"), dir.path().join("file"));
        std::fs::write(&temp, b"left by another writer").unwrap();
        let written = write_durably(
            &OsFs,
            &held(&dir),
            name("file.tmp"),
            name("file"),
            |e| e,
            |_| Ok(()),
            |_, _| Ok(()),
            |out| out.write_all(b"new"),
        );
        assert_eq!(
            written.map_err(|e| e.kind()),
            Err(std::io::ErrorKind::AlreadyExists)
        );
        assert_eq!(std::fs::read(&temp).unwrap(), b"left by another writer");
        assert!(!target.exists());
    }

    #[test]
    fn a_durable_write_refused_before_its_publish_publishes_nothing_and_discards_the_partial() {
        let dir = tempfile::tempdir().unwrap();
        let handle = held(&dir);
        let fs = RecordingFs::new();
        let written: io::Result<()> = write_durably(
            &fs,
            &handle,
            name("file.tmp"),
            name("file"),
            |e| e,
            |_| Err(io::Error::from(io::ErrorKind::PermissionDenied)),
            |dir, partial| dir.remove_file(partial),
            |out| out.write_all(b"bytes"),
        );
        assert_eq!(
            written.map_err(|e| e.kind()),
            Err(io::ErrorKind::PermissionDenied)
        );
        assert!(!fs
            .calls()
            .iter()
            .any(|call| matches!(call, Call::Rename(..))));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_durable_removal_removes_the_file_then_syncs_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let handle = held(&dir);
        let path = handle.path().join("file");
        std::fs::write(&path, b"bytes").unwrap();
        let fs = RecordingFs::new();
        remove_durably(&fs, &handle, name("file")).unwrap();
        remove_durably(&fs, &handle, name("file")).unwrap();
        assert_eq!(
            fs.calls(),
            vec![
                Call::RemoveFile(path.clone()),
                Call::SyncDir(handle.path().to_path_buf()),
                Call::RemoveFile(path.clone()),
                Call::SyncDir(handle.path().to_path_buf()),
            ]
        );
        assert!(!path.exists());
    }

    #[test]
    fn a_handle_names_its_directory_with_no_link_in_the_name() {
        let dir = tempfile::tempdir().unwrap();
        let handle = held(&dir);
        assert!(handle.path().is_absolute());
        assert_eq!(
            handle.path().canonicalize().unwrap(),
            dir.path().canonicalize().unwrap()
        );
        #[cfg(unix)]
        assert_eq!(handle.path(), dir.path().canonicalize().unwrap().as_path());
    }

    #[cfg(unix)]
    #[test]
    fn each_open_through_a_handle_refuses_a_link_and_a_file_open_refuses_a_name_that_is_no_regular_file(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("file"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("dir-link")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("file"), dir.path().join("file-link"))
            .unwrap();
        std::fs::write(dir.path().join("file"), b"inside").unwrap();
        std::fs::create_dir(dir.path().join("dir")).unwrap();
        let fifo = std::ffi::CString::new(
            dir.path()
                .join("fifo")
                .into_os_string()
                .into_encoded_bytes(),
        )
        .unwrap();
        // SAFETY: fifo is a NUL-terminated name that outlives the call.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let handle = held(&dir);

        let started = std::time::Instant::now();
        let fifo_open = handle.open_file(name("fifo"));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "the open of a FIFO took {:?}",
            started.elapsed()
        );
        let refusals = [
            handle.open_dir(name("dir-link")).map(drop),
            handle.open_dir(name("file")).map(drop),
            handle.open_file(name("file-link")).map(drop),
            fifo_open.map(drop),
            Dir::open(&dir.path().join("dir-link")).map(drop),
        ]
        .map(|opened| opened.map_err(|e| refusal(&e)));
        assert_eq!(
            refusals,
            [
                Err(Some(Refused::Link)),
                Err(Some(Refused::NotADirectory)),
                Err(Some(Refused::Link)),
                Err(Some(Refused::NotARegularFile)),
                Err(Some(Refused::Link)),
            ]
        );
        assert_eq!(
            handle
                .open_file(name("dir"))
                .map(drop)
                .map_err(|e| e.kind()),
            Err(io::ErrorKind::IsADirectory)
        );
        let mut read = String::new();
        std::io::Read::read_to_string(&mut handle.open_file(name("file")).unwrap(), &mut read)
            .unwrap();
        assert_eq!(read, "inside");
        let mut kinds: Vec<(String, EntryKind)> = handle
            .entries()
            .unwrap()
            .into_iter()
            .map(|entry| (entry.name.to_string_lossy().into_owned(), entry.kind))
            .collect();
        kinds.sort();
        assert_eq!(
            kinds,
            [
                ("dir".to_string(), EntryKind::Dir),
                ("dir-link".to_string(), EntryKind::Link),
                ("fifo".to_string(), EntryKind::Other),
                ("file".to_string(), EntryKind::File),
                ("file-link".to_string(), EntryKind::Link),
            ]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_handle_keeps_its_directory_when_the_name_moves_and_tells_that_the_name_no_longer_names_it()
    {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("shard")).unwrap();
        let parent = held(&dir);
        let shard = parent.open_dir(name("shard")).unwrap();
        assert!(parent.names(name("shard"), &shard).unwrap());
        std::fs::rename(dir.path().join("shard"), dir.path().join("moved")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("moved"), dir.path().join("shard")).unwrap();
        assert!(!parent.names(name("shard"), &shard).unwrap());
        shard.create_new(name("file")).unwrap();
        assert!(dir.path().join("moved").join("file").exists());
    }
}
