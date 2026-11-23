//! The count of failed starts at login on a Mac. The login item starts Halvo with `--background`
//! at each login and again behind every end that is not 0. A background start adds one to the
//! count before the rest of its start, a start that reaches its tray or its window sets it to
//! nothing, and an opening by hand clears it. Where the count reads [`GIVE_UP_AT`] at a
//! background start, that start sets the count to nothing and ends with 0 at once, so that
//! launchd leaves it until the next login, which has its three tries again. A count written more
//! than [`STANDS_FOR`] before a start reads as nothing.

use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use halvo_control::paths::DataDir;

/// The count at which a background start gives up.
pub const GIVE_UP_AT: u32 = 3;

/// How long a count stands after its last write. A count written longer before a start reads as
/// nothing, so that failed starts add up only where each follows the one before within this
/// time, and a login that comes later has its three tries; a loop of failing starts, which
/// launchd.plist(5) spaces 10 s apart by default, makes its three well within it.
pub const STANDS_FOR: Duration = Duration::from_secs(10 * 60);

/// The name of the count's file in the data folder's run/, beside the app's lock.
pub const FILE: &str = "failed-starts";

/// The name of the file that a write fills whole before it renames it over [`FILE`].
const NEW: &str = "failed-starts.new";

/// What a start does after its step of the count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// The start goes on as it does without the count.
    GoOn,
    /// The start ends with 0 at once and does nothing else.
    GiveUp,
}

/// The decision of a background start that reads `count`: it gives up where the count reads
/// [`GIVE_UP_AT`] or more.
pub fn decide(count: u32) -> Decision {
    if count >= GIVE_UP_AT {
        Decision::GiveUp
    } else {
        Decision::GoOn
    }
}

/// The count of one data folder, in its file run/failed-starts: the count in ASCII digits and
/// one LF, and no file for nothing.
#[derive(Debug, Clone)]
pub struct Count {
    run: PathBuf,
}

impl Count {
    /// The count of the data folder `dir`.
    pub fn of(dir: &DataDir) -> Self {
        Self { run: dir.run_dir() }
    }

    fn file(&self) -> PathBuf {
        self.run.join(FILE)
    }

    /// The count that the file holds, read against the clock of this call: nothing where no file
    /// stands, where it cannot be read, where it holds anything else than ASCII digits and one LF,
    /// and where the time of its last write lies more than [`STANDS_FOR`] before the clock, lies
    /// after it, or cannot be read.
    pub fn read(&self) -> u32 {
        use std::io::Read;
        let mut text = Vec::new();
        let stands = std::fs::File::open(self.file()).and_then(|mut file| {
            let written = file.metadata()?.modified()?;
            file.read_to_end(&mut text)?;
            Ok(stands_at(written, SystemTime::now()))
        });
        match stands {
            Ok(true) => count_in(&text).unwrap_or(0),
            Ok(false) | Err(_) => 0,
        }
    }

    /// Writes `count` whole: a new file of mode 0600, filled and then renamed over the count's
    /// file, so that a reader finds the old count or the new one.
    fn write(&self, count: u32) -> io::Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        let new = self.run.join(NEW);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&new)?;
        file.write_all(format!("{count}\n").as_bytes())?;
        drop(file);
        std::fs::rename(&new, self.file())
    }

    /// Sets the count to nothing: its file goes, and no file is nothing already.
    pub fn clear(&self) -> io::Result<()> {
        match std::fs::remove_file(self.file()) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    /// The step of a start of the app, after the lock of its data folder and before the rest of
    /// the start. A start by hand clears the count and goes on. A background start that reads
    /// [`GIVE_UP_AT`] or more sets the count to nothing and gives up, and goes on where the count
    /// cannot be set to nothing; any other background start adds one and goes on, whether the
    /// count could be written or not.
    pub fn at_the_start(&self, background: bool) -> Decision {
        if !background {
            let _ = self.clear();
            return Decision::GoOn;
        }
        let count = self.read();
        if decide(count) == Decision::GiveUp {
            return match self.clear() {
                Ok(()) => Decision::GiveUp,
                Err(_) => Decision::GoOn,
            };
        }
        let _ = self.write(count + 1);
        Decision::GoOn
    }
}

/// Whether a count written at `written` still stands at `now`: written at most [`STANDS_FOR`]
/// before it, and not after it.
fn stands_at(written: SystemTime, now: SystemTime) -> bool {
    now.duration_since(written)
        .is_ok_and(|age| age <= STANDS_FOR)
}

/// The count that `text` holds where it is ASCII digits and one LF, as [`Count::write`] writes
/// it.
fn count_in(text: &[u8]) -> Option<u32> {
    let digits = text.strip_suffix(b"\n")?;
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use halvo_control::paths::DataDir;

    use super::{Count, Decision, FILE};

    /// A data folder of the test's own under the scratch folder of `name`, with its run/.
    fn folder(name: &str) -> DataDir {
        let dir = DataDir::from_flag(crate::tests::scratch_dir(name).join("Halvo"))
            .expect("the scratch folder is a data folder");
        std::fs::create_dir_all(dir.run_dir()).expect("run/ is made");
        dir
    }

    fn file(dir: &DataDir) -> PathBuf {
        dir.run_dir().join(FILE)
    }

    fn held(path: &Path) -> Option<String> {
        std::fs::read_to_string(path).ok()
    }

    #[test]
    fn a_background_start_adds_one_and_a_start_that_reaches_its_tray_or_window_sets_it_to_nothing()
    {
        let dir = folder("failed-starts-add");
        let count = Count::of(&dir);
        assert_eq!(count.at_the_start(true), Decision::GoOn);
        assert_eq!(held(&file(&dir)).as_deref(), Some("1\n"));
        assert_eq!(count.at_the_start(true), Decision::GoOn);
        assert_eq!(held(&file(&dir)).as_deref(), Some("2\n"));
        assert_eq!(count.read(), 2);
        count.clear().expect("the count is set to nothing");
        assert_eq!(held(&file(&dir)), None);
        assert_eq!(count.read(), 0);
        count.clear().expect("a count of nothing stays nothing");
    }

    #[test]
    fn an_opening_by_hand_clears_the_count_and_never_gives_up() {
        let dir = folder("failed-starts-hand");
        let count = Count::of(&dir);
        for held_before in ["2\n", "3\n"] {
            std::fs::write(file(&dir), held_before).expect("the count is written");
            assert_eq!(count.at_the_start(false), Decision::GoOn, "{held_before:?}");
            assert_eq!(held(&file(&dir)), None, "{held_before:?}");
        }
        assert_eq!(count.at_the_start(false), Decision::GoOn);
        assert_eq!(held(&file(&dir)), None);
    }

    #[test]
    fn at_three_a_background_start_gives_up_and_the_next_login_has_its_three_tries_again() {
        let dir = folder("failed-starts-give-up");
        let count = Count::of(&dir);
        for _login in 0..2 {
            for tried in 1..=3 {
                assert_eq!(count.at_the_start(true), Decision::GoOn, "try {tried}");
                assert_eq!(count.read(), tried);
            }
            assert_eq!(count.at_the_start(true), Decision::GiveUp);
            assert_eq!(held(&file(&dir)), None, "the give-up leaves no count");
        }
    }

    #[test]
    fn a_count_that_cannot_be_read_or_holds_no_count_reads_as_nothing() {
        let dir = folder("failed-starts-doubt");
        let count = Count::of(&dir);
        for text in [
            "",
            "3",
            "3\n\n",
            " 3\n",
            "3 \n",
            "+3\n",
            "-1\n",
            "three\n",
            "4294967296\n",
            "3\r\n",
        ] {
            std::fs::write(file(&dir), text).expect("the file is written");
            assert_eq!(count.read(), 0, "{text:?}");
            assert_eq!(count.at_the_start(true), Decision::GoOn, "{text:?}");
            assert_eq!(held(&file(&dir)).as_deref(), Some("1\n"), "{text:?}");
        }
        std::fs::remove_file(file(&dir)).expect("the file goes");
        std::fs::create_dir(file(&dir)).expect("a folder stands at the file's name");
        assert_eq!(count.read(), 0);
        assert_eq!(count.at_the_start(true), Decision::GoOn);
    }

    #[test]
    fn a_count_written_more_than_ten_minutes_before_the_start_reads_as_nothing() {
        use std::time::{Duration, SystemTime};
        let dir = folder("failed-starts-age");
        let count = Count::of(&dir);
        std::fs::write(file(&dir), "3\n").expect("the count is written");
        let eleven_minutes_ago = SystemTime::now() - Duration::from_secs(11 * 60);
        std::fs::File::options()
            .write(true)
            .open(file(&dir))
            .and_then(|written| written.set_modified(eleven_minutes_ago))
            .expect("the count's time is set back");
        assert_eq!(count.at_the_start(true), Decision::GoOn);
        assert_eq!(held(&file(&dir)).as_deref(), Some("1\n"));
        std::fs::write(file(&dir), "3\n").expect("the count is written again");
        assert_eq!(count.at_the_start(true), Decision::GiveUp);
        assert_eq!(held(&file(&dir)), None);
    }

    #[test]
    fn a_count_that_cannot_be_written_lets_the_start_go_on() {
        use std::os::unix::fs::PermissionsExt;
        let missing =
            DataDir::from_flag(crate::tests::scratch_dir("failed-starts-no-run").join("Halvo"))
                .expect("the scratch folder is a data folder");
        let count = Count::of(&missing);
        for tried in 1..=4 {
            assert_eq!(count.at_the_start(true), Decision::GoOn, "try {tried}");
        }
        let dir = folder("failed-starts-read-only");
        std::fs::write(file(&dir), "3\n").expect("the count is written");
        std::fs::set_permissions(dir.run_dir(), std::fs::Permissions::from_mode(0o500))
            .expect("run/ is made read-only");
        let count = Count::of(&dir);
        let decisions = [count.at_the_start(true), count.at_the_start(true)];
        let after = held(&file(&dir));
        std::fs::set_permissions(dir.run_dir(), std::fs::Permissions::from_mode(0o700))
            .expect("run/ is made writable again");
        assert_eq!(decisions, [Decision::GoOn, Decision::GoOn]);
        assert_eq!(after.as_deref(), Some("3\n"));
    }

    /// The count is made in `run` of main.rs alone, the one start of the app: every other mode
    /// of the program is answered in `main` before `run` and reads and writes no count, and no
    /// other file of the app names this module.
    #[test]
    fn no_mode_of_the_program_but_a_start_of_the_app_reads_or_writes_the_count() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let main = std::fs::read_to_string(src.join("main.rs"))
            .expect("main.rs reads")
            .replace("\r\n", "\n");
        let start = main.find("\nfn run(").expect("main.rs has run");
        let end = start + main[start..].find("\n}\n").expect("run ends");
        for name in ["failed_starts::", "app::Opened("] {
            let at: Vec<usize> = main.match_indices(name).map(|(at, _)| at).collect();
            assert!(!at.is_empty(), "main.rs names {name}");
            assert!(
                at.iter().all(|at| (start..end).contains(at)),
                "{name} stands outside run"
            );
        }
        let mut others = Vec::new();
        for entry in walk(&src) {
            let name = entry
                .strip_prefix(&src)
                .expect("under src")
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(&entry).expect("a source file reads");
            if name != "main.rs" && name != "failed_starts.rs" && text.contains("failed_starts") {
                others.push(name);
            }
        }
        assert_eq!(others, Vec::<String>::new());
    }

    /// Every Rust file under `dir`.
    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(dir).expect("a source folder lists") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                files.extend(walk(&path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
        files
    }
}
