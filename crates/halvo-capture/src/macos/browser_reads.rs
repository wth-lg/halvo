//! What the title rules read of the system about browsers: the places of the
//! apps that the system lists as handlers of https links, the place of a
//! running app, a process's start in the machine's ticks, and which language
//! file of its own bundle a browser's process holds open. For that last read
//! alone a bundle is read from the disk: the folders of its frameworks and
//! the device and inode of each language file there, for a bundle under an
//! Applications folder alone. Of a process the read asks the system for the
//! list of what it holds open, which gives a number and a kind for each
//! entry, and then about each open file, which gives a block of numbers and
//! no name; of that block it takes the device and the inode alone. Each read
//! runs on the title thread (`halvo-window-title`) alone, never on the tap
//! thread, through the seams of the title rules in
//! [`crate::macos::foreground`], which a test replaces.

#![deny(clippy::let_underscore_must_use)]

use std::ffi::c_void;
use std::path::{Path, PathBuf};

use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSRunningApplication, NSWorkspace};
use objc2_foundation::{NSString, NSURL};

extern "C" {
    fn proc_pid_rusage(
        pid: libc::c_int,
        flavor: libc::c_int,
        buffer: *mut *mut c_void,
    ) -> libc::c_int;
}

/// `vnode_fdinfo` of the system's `sys/proc_info.h`, which the libc crate does
/// not hold: a `proc_fileinfo` of 24 bytes, then the file's `vnode_info`,
/// whose `vi_stat` holds the device and the inode of the file. It holds no
/// path.
#[repr(C)]
struct OpenFileInfo {
    _file: [u64; 3],
    info: libc::vnode_info,
}

/// The flavor `PROC_PIDFDVNODEINFO` of that header: the info of an open file
/// with no path.
const FD_VNODE_INFO: libc::c_int = 1;

const _: () = assert!(std::mem::size_of::<OpenFileInfo>() == 176);

/// The most entries of the list of what a process holds open, files and
/// every other kind together, and the most language files of a bundle, that
/// one read takes: beyond either it gives no answer. A bundle holds the
/// language files of each of its version folders.
const OPEN_FILES_READ: usize = 4_096;
const LANGUAGE_FILES_READ: usize = 4_096;

/// The most entries that the list of a process's open files may take room
/// for; a process whose list asks for more gives no answer.
const OPEN_FILES_ROOM: usize = 65_536;

/// The reads of a browser's language and of the app in front: one seam,
/// which a test replaces.
pub(crate) trait LanguageReads: Send {
    /// The start of the process `pid` in the machine's ticks, which no change
    /// of the clock moves; `None` when it cannot be read.
    fn started(&mut self, pid: libc::pid_t) -> Option<u64>;

    /// The place of the bundle of the running app `pid` whose bundle id is
    /// `bundle_id`, as the system gives it, with no read of a file; `None`
    /// when it cannot be read or the id differs.
    fn place(&mut self, pid: libc::pid_t, bundle_id: &str) -> Option<String>;

    /// The code of the one language file of the bundle at `place` that the
    /// process `pid` holds open: the `<code>` of its `<code>.lproj`. `None`
    /// when the read gives no answer: the bundle or the process cannot be
    /// read, or the process holds none of the bundle's language files open,
    /// or more than one.
    fn held_language(&mut self, pid: libc::pid_t, place: &str) -> Option<String>;
}

/// The reads of the system itself.
pub(crate) struct SystemLanguages;

impl LanguageReads for SystemLanguages {
    fn started(&mut self, pid: libc::pid_t) -> Option<u64> {
        let mut info = std::mem::MaybeUninit::<libc::rusage_info_v0>::zeroed();
        // SAFETY: `info` is writable for a whole rusage_info_v0, the struct of
        // the flavor RUSAGE_INFO_V0, which is all that the call writes.
        let answer = unsafe {
            proc_pid_rusage(
                pid,
                libc::RUSAGE_INFO_V0,
                info.as_mut_ptr().cast::<*mut c_void>(),
            )
        };
        if answer != 0 {
            return None;
        }
        // SAFETY: the call answered 0, so it wrote the whole value; the
        // buffer was zeroed before.
        let start = unsafe { info.assume_init() }.ri_proc_start_abstime;
        (start != 0).then_some(start)
    }

    fn place(&mut self, pid: libc::pid_t, bundle_id: &str) -> Option<String> {
        autoreleasepool(|_| -> Option<String> {
            let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
            let id = app.bundleIdentifier()?;
            if crate::macos::foreground::bundle_identifier(&id) != bundle_id {
                return None;
            }
            let bundle = app.bundleURL()?;
            place_of(&bundle)
        })
    }

    fn held_language(&mut self, pid: libc::pid_t, place: &str) -> Option<String> {
        let files = language_files(place)?;
        let open = open_file_numbers(pid)?;
        let mut held = files
            .iter()
            .filter(|file| open.contains(&(file.device, file.inode)));
        let one = held.next()?;
        held.next().is_none().then(|| one.code.clone())
    }
}

/// The places of the apps that the system lists as handlers of https links,
/// as it gives them, with no read of a file; `None` when the system names
/// none: an https link always has a handler, so an empty list is a failed
/// read.
pub(crate) fn https_handlers() -> Option<Vec<String>> {
    autoreleasepool(|_| {
        let link = NSURL::URLWithString(&NSString::from_str("https://localhost/"))?;
        let apps = NSWorkspace::sharedWorkspace().URLsForApplicationsToOpenURL(&link);
        let places: Vec<String> = apps.iter().filter_map(|app| place_of(&app)).collect();
        (!places.is_empty()).then_some(places)
    })
}

/// The path of a file URL, as NSURL gives it, with no read of a file.
fn place_of(url: &NSURL) -> Option<String> {
    url.path().map(|path| path.to_string())
}

/// A language file of a bundle as the disk holds it: the code of its folder
/// `<code>.lproj`, and the device and the inode of its `locale.pak`.
struct LanguageFile {
    code: String,
    device: u64,
    inode: u64,
}

/// The folders and files that the folder `folder` holds, or none where it
/// cannot be read.
fn entries_of(folder: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(folder)
        .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
        .unwrap_or_default()
}

/// The language files inside the bundle at `place`, read from the disk: for
/// each folder under `Contents/Frameworks`, each folder under its `Versions`
/// and each folder named `<code>.lproj` under that version's `Resources`, the
/// `locale.pak` there, each file once for its code, also where two version
/// folders name it. `None` where the bundle holds more than
/// [`LANGUAGE_FILES_READ`] of them.
fn language_files(place: &str) -> Option<Vec<LanguageFile>> {
    use std::os::unix::fs::MetadataExt;
    let mut files: Vec<LanguageFile> = Vec::new();
    for framework in entries_of(&Path::new(place).join("Contents/Frameworks")) {
        for version in entries_of(&framework.join("Versions")) {
            for folder in entries_of(&version.join("Resources")) {
                let Some(code) = folder
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.strip_suffix(".lproj"))
                else {
                    continue;
                };
                let Ok(file) = std::fs::metadata(folder.join("locale.pak")) else {
                    continue;
                };
                let known = files.iter().any(|known| {
                    known.code == code && (known.device, known.inode) == (file.dev(), file.ino())
                });
                if !file.is_file() || known {
                    continue;
                }
                if files.len() == LANGUAGE_FILES_READ {
                    return None;
                }
                files.push(LanguageFile {
                    code: code.to_string(),
                    device: file.dev(),
                    inode: file.ino(),
                });
            }
        }
    }
    Some(files)
}

/// The device and the inode of each file of a disk that the process `pid`
/// holds open: the list of what it holds open, then, for each entry that is a
/// file of a disk, the call that gives the info of an open file with no path.
/// No call asks for the name of a file. The list is asked twice: first for
/// the bytes that it takes, then with room for that many entries and 64
/// more. `None` where the list does not answer, fills that room, so that it
/// may be cut, or names more than [`OPEN_FILES_READ`] entries, or where the
/// info of one file does not answer.
fn open_file_numbers(pid: libc::pid_t) -> Option<Vec<(u64, u64)>> {
    let entry = std::mem::size_of::<libc::proc_fdinfo>();
    // SAFETY: with a null buffer of no bytes the call writes nothing and
    // answers the bytes that the list takes.
    let need =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    let room = usize::try_from(need).ok().filter(|bytes| *bytes > 0)? / entry + 64;
    if room > OPEN_FILES_ROOM {
        return None;
    }
    let mut list = vec![
        libc::proc_fdinfo {
            proc_fd: 0,
            proc_fdtype: 0,
        };
        room
    ];
    let bytes = libc::c_int::try_from(room * entry).ok()?;
    // SAFETY: `list` is writable for `bytes` bytes, and the call writes at
    // most that many.
    let wrote = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDLISTFDS,
            0,
            list.as_mut_ptr().cast(),
            bytes,
        )
    };
    let listed = usize::try_from(wrote).ok().filter(|bytes| *bytes > 0)? / entry;
    if listed >= room || listed > OPEN_FILES_READ {
        return None;
    }
    list.truncate(listed);
    let size = libc::c_int::try_from(std::mem::size_of::<OpenFileInfo>()).ok()?;
    let mut numbers = Vec::new();
    for open in &list {
        if i64::from(open.proc_fdtype) != i64::from(libc::PROX_FDTYPE_VNODE) {
            continue;
        }
        let mut info = std::mem::MaybeUninit::<OpenFileInfo>::zeroed();
        // SAFETY: `info` is writable for a whole vnode_fdinfo, the struct of
        // this flavor, which is all that the call writes.
        let answer = unsafe {
            libc::proc_pidfdinfo(
                pid,
                open.proc_fd,
                FD_VNODE_INFO,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        if answer != size {
            return None;
        }
        // SAFETY: the buffer was zeroed before the call, and every bit
        // pattern is a value of this struct of plain numbers.
        let info = unsafe { info.assume_init() };
        numbers.push((
            u64::from(info.info.vi_stat.vst_dev),
            info.info.vi_stat.vst_ino,
        ));
    }
    Some(numbers)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The read of `pid` at `place`, made again while it gives no answer, at
    /// most 25 times: another test of this process can close a file between
    /// the read's two calls.
    fn held(pid: libc::pid_t, place: &str) -> Option<String> {
        (0..25).find_map(|_| SystemLanguages.held_language(pid, place))
    }

    #[test]
    fn the_read_finds_the_one_language_file_that_a_process_holds_open() {
        let me = libc::pid_t::try_from(std::process::id()).expect("a pid");
        let root = tempfile::tempdir().expect("a temporary directory");
        let app = root.path().join("Made Browser.app");
        let versions = app.join("Contents/Frameworks/Made Browser Framework.framework/Versions");
        let resources = versions.join("1.2.3/Resources");
        for version in ["1.2.3", "1.2.4"] {
            for code in ["de", "en"] {
                let folder = versions.join(format!("{version}/Resources/{code}.lproj"));
                std::fs::create_dir_all(&folder).expect("the folder");
                std::fs::write(folder.join("locale.pak"), code).expect("the file");
            }
        }
        std::os::unix::fs::symlink("1.2.3", versions.join("Current")).ok();
        let place = app.to_str().expect("a path of plain text");
        let none = SystemLanguages.held_language(me, place);
        let german =
            std::fs::File::open(resources.join("de.lproj/locale.pak")).expect("the German file");
        let one = held(me, place);
        let english =
            std::fs::File::open(resources.join("en.lproj/locale.pak")).expect("the English file");
        let two = SystemLanguages.held_language(me, place);
        drop(german);
        let other = held(me, place);
        drop(english);
        assert_eq!(
            (none, one, two, other),
            (None, Some("de".to_string()), None, Some("en".to_string()))
        );
    }
}
