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
                None => StoreError::Io(e.kind()),
            })
    }
}

#[cfg(windows)]
pub(crate) const WRITER_LOCK: &str = "writer.lock";

impl Drop for DirLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}
