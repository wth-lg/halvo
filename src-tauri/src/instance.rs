use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use halvo_control::paths::DataDir;

/// How often the one Halvo reads whether a second launch asked it to show its window.
pub const SHOW_POLL: Duration = Duration::from_millis(250);

/// How far a second launch moves the lock's modification time forward at the least, so that a
/// file system of coarse times still reads a change.
pub const SHOW_STEP: Duration = Duration::from_secs(2);

/// What a launch found at the lock of its data directory.
#[derive(Debug)]
pub enum Taken {
    /// This launch holds the lock: it is the one Halvo of the directory.
    First(Instance),
    /// Another Halvo holds the lock: this launch makes no builder, no tray, no child and no
    /// poller.
    Second,
}

/// The held lock of run/app.lock, which ends with its process, however that ends, and the
/// modification time of the file as this Halvo read it last: a second launch that asks to show
/// the window moves that time forward, and changes nothing else.
#[derive(Debug)]
pub struct Instance {
    file: File,
    seen: Option<SystemTime>,
}

impl Instance {
    /// Whether a second launch asked this Halvo to show its window since the last reading: the
    /// lock file's modification time moved.
    pub fn asked_to_show(&mut self) -> bool {
        let now = modified(&self.file);
        let asked = now.is_some() && now != self.seen;
        self.seen = now;
        asked
    }

    /// Starts a thread of its own that reads every SHOW_POLL, until the process ends, whether a
    /// second launch asked to show the window, and calls `show` at each ask; it reads through a
    /// second handle of the lock file and holds no lock of its own, and the lock stays with
    /// this value.
    pub fn show_on_ask(&self, show: impl Fn() + Send + 'static) {
        let mut watcher = match self.file.try_clone() {
            Ok(file) => Instance {
                file,
                seen: self.seen,
            },
            Err(e) => {
                eprintln!("halvo: a second launch cannot show this window: {e}");
                return;
            }
        };
        let started = std::thread::Builder::new()
            .name("halvo-show-request".to_owned())
            .spawn(move || loop {
                std::thread::sleep(SHOW_POLL);
                if watcher.asked_to_show() {
                    show();
                }
            });
        if let Err(e) = started {
            eprintln!("halvo: a second launch cannot show this window: {e}");
        }
    }
}

/// Takes the one lock of `dir` (contracts.md C14 item 8), before the builder runs: makes the
/// data directory where it is missing (mode 0700), run/ (mode 0700) and run/app.lock (mode
/// 0600), and nothing else, then takes the lock of that file without waiting.
/// A launch that finds the lock held asks the holder to show its window, unless `background`.
pub fn take(dir: &DataDir, background: bool) -> io::Result<Taken> {
    if std::fs::symlink_metadata(dir.path())
        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
    {
        private_dirs(dir.path())?;
    }
    match private_dir(&dir.run_dir()) {
        Err(e) if e.kind() != io::ErrorKind::AlreadyExists => return Err(e),
        _ => {}
    }
    let file = lock_file(&dir.app_lock())?;
    match file.try_lock() {
        Ok(()) => Ok(Taken::First(Instance {
            seen: modified(&file),
            file,
        })),
        Err(TryLockError::WouldBlock) => {
            if !background {
                ask_to_show(&file);
            }
            Ok(Taken::Second)
        }
        Err(TryLockError::Error(e)) => Err(e),
    }
}

fn modified(file: &File) -> Option<SystemTime> {
    file.metadata().and_then(|meta| meta.modified()).ok()
}

fn ask_to_show(file: &File) {
    let at = modified(file).map_or(SystemTime::now(), |at| {
        SystemTime::now().max(at + SHOW_STEP)
    });
    if let Err(e) = file.set_modified(at) {
        eprintln!("halvo: the Halvo that runs was not asked to show its window: {e}");
    }
}

fn lock_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// Makes the directory `path` with mode 0700, its parent being there.
#[cfg(unix)]
pub fn private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

/// Makes the directory `path`, its parent being there.
#[cfg(not(unix))]
pub fn private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir(path)
}

#[cfg(unix)]
fn private_dirs(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
}

#[cfg(not(unix))]
fn private_dirs(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn a_second_launch_asks_the_first_to_show_its_window_and_with_background_asks_nothing() {
        let root = crate::tests::scratch_dir("instance");
        let dir = DataDir::from_flag(root.join("missing").join("Halvo")).unwrap();
        let Ok(Taken::First(mut first)) = take(&dir, false) else {
            panic!("the first take holds the lock");
        };
        assert_eq!(
            names(dir.path()),
            ["run"],
            "the data directory holds run alone"
        );
        assert_eq!(
            names(&dir.run_dir()),
            ["app.lock"],
            "run holds the lock alone"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(
                (
                    mode(dir.path()),
                    mode(&dir.run_dir()),
                    mode(&dir.app_lock())
                ),
                (0o700, 0o700, 0o600)
            );
        }
        assert!(!first.asked_to_show(), "no ask before a second launch");

        assert!(
            matches!(take(&dir, false), Ok(Taken::Second)),
            "a second take finds the lock held"
        );
        assert!(
            first.asked_to_show(),
            "a second launch without --background asks to show the window"
        );
        assert!(!first.asked_to_show(), "it asks once");

        assert!(matches!(take(&dir, true), Ok(Taken::Second)));
        assert!(!first.asked_to_show(), "with --background it asks nothing");

        drop(first);
        assert!(
            matches!(take(&dir, true), Ok(Taken::First(_))),
            "a new take after the first is dropped holds the lock"
        );
    }
}
