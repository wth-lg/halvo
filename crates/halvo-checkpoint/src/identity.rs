//! The one compare of two files for identity (the coordinator's ruling CP-129). Every check of
//! this crate and of halvo-sheet that says "this is the file that I checked" compares two
//! [`FileId`]s with [`same_file`], and no other code of either crate reads what names a file on
//! the system. A test of the crate fails when a source file reads such a value outside this
//! module.

use std::fs::{File, Metadata};
use std::io;
use std::path::Path;
use std::time::SystemTime;

/// What names a file on the system while it exists, read from a handle open on it: on macOS the
/// device and inode numbers; on Windows the volume's serial number and the file's 128-bit id
/// (`FILE_ID_INFO` of `GetFileInformationByHandleEx`), and whether the volume keeps that id
/// stable. With it the length and the times of the file as the handle saw them, which
/// [`same_file`] compares too on a volume whose id is not stable. Its Debug form holds numbers
/// alone, no path.
#[derive(Clone, Copy, Debug)]
pub struct FileId {
    volume: u64,
    index: u128,
    stable: bool,
    len: u64,
    times: (Option<SystemTime>, Option<SystemTime>),
}

impl FileId {
    /// The identity of the file that `file` has open.
    pub fn of(file: &File) -> io::Result<FileId> {
        let meta = file.metadata()?;
        let (volume, index, stable) = system::id(file, &meta)?;
        Ok(FileId {
            volume,
            index,
            stable,
            len: meta.len(),
            times: (meta.created().ok(), meta.modified().ok()),
        })
    }

    /// The identity of the entry at `path` itself, a link itself when it is one: on macOS read
    /// without an open, on Windows through a handle opened on the entry itself (never on what a
    /// reparse point names) and closed again.
    pub fn at(path: &Path) -> io::Result<FileId> {
        system::at(path)
    }

    /// The device (macOS) or the volume's serial number (Windows), and the inode or the file's
    /// id: the numbers that name the lock file of a directory.
    pub fn numbers(&self) -> (u64, u128) {
        (self.volume, self.index)
    }
}

/// Whether `a` and `b` name one file: the same volume and the same inode or file id. On a
/// volume that keeps no stable file id, also the same length, creation time and time of the
/// last write, so that the check refuses a file that it cannot tell apart rather than take it:
/// on Windows a volume that does not report `FILE_SUPPORTS_OPEN_BY_FILE_ID`, such as FAT and
/// exFAT, whose file id comes from the place of the entry in its directory and can change when
/// the file is renamed.
pub fn same_file(a: &FileId, b: &FileId) -> bool {
    let named = (a.volume, a.index) == (b.volume, b.index);
    named && ((a.stable && b.stable) || (a.len, a.times) == (b.len, b.times))
}

/// Whether the file that two looks at one file saw has the same length and time of the last
/// write: a check that it was not written between them, never a check of identity.
pub(crate) fn unchanged(a: &Metadata, b: &Metadata) -> bool {
    a.len() == b.len() && a.modified().ok() == b.modified().ok()
}

#[cfg(unix)]
mod system {
    use std::fs::{self, File, Metadata};
    use std::io;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    use super::FileId;

    pub(super) fn id(_: &File, meta: &Metadata) -> io::Result<(u64, u128, bool)> {
        Ok((meta.dev(), u128::from(meta.ino()), true))
    }

    pub(super) fn at(path: &Path) -> io::Result<FileId> {
        let meta = fs::symlink_metadata(path)?;
        Ok(FileId {
            volume: meta.dev(),
            index: u128::from(meta.ino()),
            stable: true,
            len: meta.len(),
            times: (meta.created().ok(), meta.modified().ok()),
        })
    }
}

#[cfg(windows)]
mod system {
    use std::fs::{File, Metadata, OpenOptions};
    use std::io;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;

    use windows_sys::Win32::Storage::FileSystem::{
        FileIdInfo, GetFileInformationByHandleEx, GetVolumeInformationByHandleW,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO,
    };
    use windows_sys::Win32::System::SystemServices::FILE_SUPPORTS_OPEN_BY_FILE_ID;

    use super::FileId;

    const FILE_READ_ATTRIBUTES: u32 = 0x0080;

    pub(super) fn id(file: &File, _: &Metadata) -> io::Result<(u64, u128, bool)> {
        let handle = file.as_raw_handle();
        let mut info = FILE_ID_INFO::default();
        // SAFETY: the handle is open for the whole call, as the borrowed file lives across it,
        // and `info` is a FILE_ID_INFO of the size that the call is given.
        let done = unsafe {
            GetFileInformationByHandleEx(
                handle,
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        };
        if done == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut flags = 0u32;
        // SAFETY: the handle is open for the whole call; the one out pointer that the call is
        // asked to fill points at a local, and the null buffers have the size 0.
        let known = unsafe {
            GetVolumeInformationByHandleW(
                handle,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut flags,
                std::ptr::null_mut(),
                0,
            )
        };
        let stable = known != 0 && flags & FILE_SUPPORTS_OPEN_BY_FILE_ID != 0;
        let index = u128::from_le_bytes(info.FileId.Identifier);
        Ok((info.VolumeSerialNumber, index, stable))
    }

    pub(super) fn at(path: &Path) -> io::Result<FileId> {
        let entry = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        FileId::of(&entry)
    }
}

#[cfg(not(any(unix, windows)))]
mod system {
    use std::fs::{File, Metadata};
    use std::io;
    use std::path::Path;

    use super::FileId;

    pub(super) fn id(_: &File, _: &Metadata) -> io::Result<(u64, u128, bool)> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    pub(super) fn at(_: &Path) -> io::Result<FileId> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn id(stable: bool, index: u128, written: u64) -> FileId {
        FileId {
            volume: 1,
            index,
            stable,
            len: 10,
            times: (
                Some(UNIX_EPOCH),
                Some(UNIX_EPOCH + Duration::from_secs(written)),
            ),
        }
    }

    /// CP-134 H1, the review of the round: on a volume without stable ids each part of the old
    /// test decides alone (the length, the creation time, the time of the last write), the
    /// stable flag of either side decides, and another volume is never the same file.
    #[test]
    fn each_part_of_the_fallback_and_each_side_of_the_flag_decides() {
        let base = id(false, 7, 1);
        let mut longer = base;
        longer.len = 11;
        assert!(!same_file(&base, &longer), "the length alone");
        let mut created = base;
        created.times.0 = Some(UNIX_EPOCH + Duration::from_secs(5));
        assert!(!same_file(&base, &created), "the creation time alone");
        assert!(
            !same_file(&id(false, 7, 1), &id(true, 7, 2)),
            "the unstable side first"
        );
        let mut elsewhere = id(true, 7, 1);
        elsewhere.volume = 2;
        assert!(!same_file(&id(true, 7, 1), &elsewhere), "another volume");
    }

    /// CP-134 H1 (the verifier's L15): on a volume whose file ids are not stable, one id with
    /// other times is not the same file, so the check refuses rather than takes; on a volume
    /// whose ids are stable the id decides alone; another id is never the same file.
    #[test]
    fn on_a_volume_without_stable_ids_one_id_with_other_times_is_not_the_same_file() {
        assert!(
            same_file(&id(true, 7, 1), &id(true, 7, 2)),
            "a stable id decides alone"
        );
        assert!(
            !same_file(&id(false, 7, 1), &id(false, 7, 2)),
            "other times refuse"
        );
        assert!(
            !same_file(&id(true, 7, 1), &id(false, 7, 2)),
            "one unstable side refuses"
        );
        assert!(
            same_file(&id(false, 7, 1), &id(false, 7, 1)),
            "same id and times"
        );
        assert!(!same_file(&id(true, 7, 1), &id(true, 8, 1)), "another id");
        assert!(
            !same_file(&id(false, 7, 1), &id(false, 8, 1)),
            "another id, same times"
        );
    }
}
