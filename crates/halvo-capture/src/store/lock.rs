use std::fs::{File, TryLockError};
#[cfg(windows)]
use std::io::ErrorKind;

use super::durable::Dir;
#[cfg(windows)]
use super::durable::{self, Refused};
#[cfg(windows)]
use super::{Met, StoreName};
use super::{StoreError, StoreResult};

pub(crate) struct DirLock {
    pub(crate) file: File,
}

impl DirLock {
    pub(crate) fn take(dir: &Dir) -> StoreResult<Self> {
        let file = Self::lock_file(dir)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { file }),
            Err(TryLockError::WouldBlock) => Err(StoreError::Locked),
            Err(TryLockError::Error(e)) => Err(StoreError::Io(e.kind())),
        }
    }

    #[cfg(target_os = "macos")]
    fn lock_file(dir: &Dir) -> StoreResult<File> {
        dir.try_clone()
            .map(Dir::into_file)
            .map_err(|e| StoreError::Io(e.kind()))
    }

    #[cfg(windows)]
    fn lock_file(dir: &Dir) -> StoreResult<File> {
        dir.open_lock_file(std::ffi::OsStr::new(WRITER_LOCK))
            .map_err(|e| match durable::refusal(&e) {
                Some(Refused::Link) => StoreError::Obstructed {
                    name: StoreName::WriterLock,
                    met: Met::Link,
                },
                Some(_) => StoreError::Obstructed {
                    name: StoreName::WriterLock,
                    met: Met::OtherKind,
                },
                None if e.kind() == ErrorKind::IsADirectory => StoreError::Obstructed {
                    name: StoreName::WriterLock,
                    met: Met::OtherKind,
                },
                None if e.raw_os_error() == Some(durable::ERROR_SHARING_VIOLATION) => {
                    StoreError::Locked
                }
                None => StoreError::Io(e.kind()),
            })
    }
}

#[cfg(windows)]
impl DirLock {
    /// Marks this lock's own file for removal while the lock still holds it,
    /// at the end of a delete: the file goes when this handle closes, and
    /// until then a take in this directory is refused, since Windows refuses
    /// every open of the marked name, so that no other process takes the
    /// lock between the mark and the close.
    pub(crate) fn remove_its_file_at_close(&self) -> std::io::Result<()> {
        durable::remove_when_closed(&self.file)
    }

    /// Whether the lock file stands in `dir`. Only an answer of no such entry
    /// reads as gone; a marked name that another handle still keeps open, or
    /// a name that cannot be read, reads as standing.
    pub(crate) fn file_stands_in(dir: &Dir) -> bool {
        !matches!(
            dir.entry(std::ffi::OsStr::new(WRITER_LOCK)),
            Err(e) if e.kind() == ErrorKind::NotFound
        )
    }

    /// Whether the lock file stands in `dir` marked for removal while a
    /// handle keeps it open. Windows then refuses every open of the name, also
    /// one that asks no right, with STATUS_DELETE_PENDING, which the thread's
    /// last status holds right behind the refused open; every other answer, a
    /// refusal by the file's rights among them, reads as not so.
    pub(crate) fn file_marked_and_kept_in(dir: &Dir) -> bool {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        const ERROR_ACCESS_DENIED: i32 = 5;
        const STATUS_DELETE_PENDING: u32 = 0xC000_0056;
        #[link(name = "ntdll")]
        extern "system" {
            fn RtlGetLastNtStatus() -> u32;
        }
        let refused = std::fs::OpenOptions::new()
            .access_mode(0)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(dir.entry_path(std::ffi::OsStr::new(WRITER_LOCK)))
            .is_err_and(|e| e.raw_os_error() == Some(ERROR_ACCESS_DENIED));
        // SAFETY: the call takes no argument and reads the calling thread's own
        // last status, which the refused open above set.
        refused && unsafe { RtlGetLastNtStatus() } == STATUS_DELETE_PENDING
    }
}

#[cfg(windows)]
pub(crate) const WRITER_LOCK: &str = "writer.lock";

impl Drop for DirLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::io::ErrorKind;

    use super::{DirLock, WRITER_LOCK};
    use crate::store::durable::Dir;
    use crate::store::StoreError;

    const ERROR_ACCESS_DENIED: i32 = 5;

    /// Sentence 3 of win-delete-lock: from the mark until the lock's handle
    /// closes, the name stands and a second take is refused; behind the close
    /// the name is gone, and a take makes a new file and holds it.
    #[test]
    fn a_marked_lock_refuses_a_second_take_until_it_closes_and_its_file_goes_with_the_close() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(WRITER_LOCK);
        let first = DirLock::take(&Dir::open(dir.path()).unwrap()).unwrap();
        first.remove_its_file_at_close().unwrap();
        let in_the_gap = DirLock::take(&Dir::open(dir.path()).unwrap()).map(drop);
        let raw_open_in_the_gap = std::fs::OpenOptions::new()
            .read(true)
            .open(&path)
            .err()
            .and_then(|e| e.raw_os_error());
        let named_in_the_gap = DirLock::file_stands_in(&Dir::open(dir.path()).unwrap());
        drop(first);
        let named_after = std::fs::symlink_metadata(&path)
            .map(drop)
            .map_err(|e| e.kind());
        let taken_after = DirLock::take(&Dir::open(dir.path()).unwrap());
        let beside_it = DirLock::take(&Dir::open(dir.path()).unwrap()).map(drop);
        println!(
            "WIN-LOCK-MARK take_in_the_gap={in_the_gap:?} raw_open_in_the_gap={raw_open_in_the_gap:?} \
             named_in_the_gap={named_in_the_gap} named_after_the_close={named_after:?} \
             take_after={} take_beside_it={beside_it:?}",
            taken_after.is_ok()
        );
        assert!(
            matches!(
                in_the_gap,
                Err(StoreError::Io(ErrorKind::PermissionDenied) | StoreError::Locked)
            ),
            "a take between the mark and the close answered {in_the_gap:?}"
        );
        assert_eq!(raw_open_in_the_gap, Some(ERROR_ACCESS_DENIED));
        assert!(named_in_the_gap, "the marked name went before the close");
        assert_eq!(named_after, Err(ErrorKind::NotFound));
        assert!(
            taken_after.is_ok(),
            "a take behind the close answered {:?}",
            taken_after.err()
        );
        assert!(
            matches!(beside_it, Err(StoreError::Locked)),
            "a take beside the new lock answered {beside_it:?}"
        );
        assert!(path.is_file(), "the take behind the close made no file");
    }
}
