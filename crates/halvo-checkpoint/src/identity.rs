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
/// device and inode numbers, and whether the volume's kind keeps them stable; on Windows the
/// volume's serial number and the file's 128-bit id (`FILE_ID_INFO` of
/// `GetFileInformationByHandleEx`), and whether the volume keeps that id stable. With it the
/// length and the times of the file as the handle saw them, which
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
/// the file is renamed; on macOS a volume whose kind is not `apfs` or `hfs`, such as FAT
/// (`msdos`), exFAT (`exfat`) and a share (`smbfs`), or whose kind cannot be read.
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

    use super::{volume, FileId};

    pub(super) fn id(file: &File, meta: &Metadata) -> io::Result<(u64, u128, bool)> {
        let stable = volume::of_file(file, meta.dev());
        Ok((meta.dev(), u128::from(meta.ino()), stable))
    }

    pub(super) fn at(path: &Path) -> io::Result<FileId> {
        let meta = fs::symlink_metadata(path)?;
        Ok(FileId {
            volume: meta.dev(),
            index: u128::from(meta.ino()),
            stable: volume::of_path(path, meta.dev()),
            len: meta.len(),
            times: (meta.created().ok(), meta.modified().ok()),
        })
    }
}

/// Whether the ids of a volume are stable on macOS, read from the volume's kind: `f_fstypename`
/// of `fstatfs` for an open file and of `statfs` for a path. Only `apfs` and `hfs` keep an
/// inode number with a file while it exists. The system makes the inode of a FAT or exFAT
/// volume (`msdos`, `exfat`) from the place of the entry, and a share (`smbfs`) gives the
/// server's numbers; those, every other kind and a read that fails are not stable, and so is a
/// read whose volume is not the one whose device number the identity holds, as when `statfs`
/// follows a link to another volume.
#[cfg(target_os = "macos")]
mod volume {
    use std::ffi::{c_char, CString};
    use std::fs::File;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    /// The kinds of volume whose ids are stable.
    const STABLE: [&[u8]; 2] = [b"apfs", b"hfs"];

    /// `struct statfs` of macOS with 64-bit inode numbers.
    #[repr(C)]
    struct StatFs {
        bsize: u32,
        iosize: i32,
        blocks: u64,
        bfree: u64,
        bavail: u64,
        files: u64,
        ffree: u64,
        fsid: [i32; 2],
        owner: u32,
        kind: u32,
        flags: u32,
        subtype: u32,
        typename: [c_char; 16],
        mounted_on: [c_char; 1024],
        mounted_from: [c_char; 1024],
        flags_ext: u32,
        reserved: [u32; 7],
    }

    const _: () = assert!(std::mem::size_of::<StatFs>() == 2168);

    extern "C" {
        #[cfg_attr(target_arch = "x86_64", link_name = "fstatfs$INODE64")]
        fn fstatfs(fd: i32, buf: *mut StatFs) -> i32;
        #[cfg_attr(target_arch = "x86_64", link_name = "statfs$INODE64")]
        fn statfs(path: *const c_char, buf: *mut StatFs) -> i32;
    }

    /// A volume as the system reads it: the number that the device number of its files holds,
    /// and its kind.
    #[derive(Debug)]
    pub(super) struct Volume {
        pub(super) number: u64,
        pub(super) kind: Vec<u8>,
    }

    fn read(fill: impl FnOnce(*mut StatFs) -> i32) -> Option<Volume> {
        // SAFETY: every field of StatFs is an integer or an array of integers, for which all
        // bits zero is a valid value.
        let mut buf: StatFs = unsafe { std::mem::zeroed() };
        if fill(&mut buf as *mut StatFs) != 0 {
            return None;
        }
        let kind = buf.typename.iter().take_while(|&&byte| byte != 0);
        Some(Volume {
            number: buf.fsid[0] as u64,
            kind: kind.map(|&byte| byte as u8).collect(),
        })
    }

    /// The volume of the file that `file` has open, `None` when the read fails.
    pub(super) fn of_open(file: &File) -> Option<Volume> {
        // SAFETY: the descriptor is open for the whole call, as the borrowed file lives across
        // it, and the buffer is a StatFs, the size that the call writes.
        read(|buf| unsafe { fstatfs(file.as_raw_fd(), buf) })
    }

    /// The volume of what `path` names, a link followed, `None` when the read fails.
    pub(super) fn of_named(path: &Path) -> Option<Volume> {
        let path = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: `path` is a NUL-terminated string that lives across the call, and the
        // buffer is a StatFs, the size that the call writes.
        read(|buf| unsafe { statfs(path.as_ptr(), buf) })
    }

    /// Whether a volume of the kind `kind` keeps its ids stable.
    pub(super) fn stable_kind(kind: &[u8]) -> bool {
        STABLE.contains(&kind)
    }

    /// Whether the id of a file on the volume of the device number `device` is stable, by the
    /// volume that the system read: that volume, of a stable kind.
    pub(super) fn stable(read: Option<Volume>, device: u64) -> bool {
        read.is_some_and(|volume| volume.number == device && stable_kind(&volume.kind))
    }

    pub(super) fn of_file(file: &File, device: u64) -> bool {
        stable(of_open(file), device)
    }

    pub(super) fn of_path(path: &Path, device: u64) -> bool {
        stable(of_named(path), device)
    }
}

/// Other unix: every volume's ids count as stable, as before.
#[cfg(all(unix, not(target_os = "macos")))]
mod volume {
    use std::fs::File;
    use std::path::Path;

    pub(super) fn of_file(_: &File, _: u64) -> bool {
        true
    }

    pub(super) fn of_path(_: &Path, _: u64) -> bool {
        true
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

    /// U2.4b (3): on macOS the volume's kind decides whether an id alone names a file: `apfs`
    /// and `hfs` say stable; FAT (`msdos`), exFAT, a share and any other kind do not, nor does a
    /// failed read or a volume other than the id's own. The read finds the kind of the volume
    /// that holds this test's binary, with the device number of the binary's own identity, and
    /// `FileId::of` and `FileId::at` say stable as that kind does.
    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_the_volumes_kind_decides_whether_an_id_is_stable() {
        use std::os::unix::fs::MetadataExt;
        let kinds = [
            ("apfs", true),
            ("hfs", true),
            ("msdos", false),
            ("exfat", false),
            ("smbfs", false),
            ("nfs", false),
            ("webdav", false),
            ("APFS", false),
            ("", false),
        ];
        for (kind, stable) in kinds {
            assert_eq!(volume::stable_kind(kind.as_bytes()), stable, "{kind:?}");
        }
        let read = |number: u64, kind: &[u8]| {
            Some(volume::Volume {
                number,
                kind: kind.to_vec(),
            })
        };
        assert!(volume::stable(read(5, b"apfs"), 5), "apfs, the id's volume");
        assert!(!volume::stable(read(6, b"apfs"), 5), "another volume");
        assert!(!volume::stable(read(5, b"msdos"), 5), "a FAT volume");
        assert!(!volume::stable(None, 5), "a failed read");

        let path = std::env::current_exe().expect("the test binary");
        let file = File::open(&path).expect("open the test binary");
        let device = file.metadata().expect("its metadata").dev();
        let open = volume::of_open(&file).expect("the volume of the open file");
        let (kind, number) = (String::from_utf8_lossy(&open.kind), open.number);
        println!("the volume of the test binary: {kind}, number {number}");
        assert_eq!(open.number, device, "the number of the file's own volume");
        let named = volume::of_named(&path).expect("the volume of the path");
        assert_eq!((named.number, &named.kind), (open.number, &open.kind));
        let absent = path.with_file_name("halvo-checkpoint-no-such-entry");
        assert!(volume::of_named(&absent).is_none(), "a read that fails");
        let stable = volume::stable_kind(&open.kind);
        assert_eq!(FileId::of(&file).expect("of").stable, stable, "of");
        assert_eq!(FileId::at(&path).expect("at").stable, stable, "at");
    }
}
