//! The run marker of a service run, run/running of the data directory
//! (contracts.md C3.2 step 5 and its stop, D-05): read at the start to set
//! `last_stop`, written anew for the life that starts, and removed by a life
//! that ends in order. It holds the process id of its life, and the hold of
//! ruling PL-3 part (5) while that stands: a record of a life that a source
//! which did not stop ended, or the line `hold=yes` that carries such a
//! record's hold on through the lives after it until a Resume lifts it.

use std::io::{self, Read, Write};
#[cfg(target_os = "macos")]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;

use halvo_control::paths::DataDir;
use halvo_control::proto::{LastStop, StopKind};

const MARKER_READ_MAX_BYTES: u64 = 4_096;

const HOLD_LINE: &str = "hold=yes";

/// What a start found in the marker and made of it: its reading, the result
/// of its write, and the marker that it found, which [`give_back`] puts back.
pub struct Begun {
    pub last_stop: LastStop,
    pub written: io::Result<()>,
}

/// Why the last life of the data directory ended, as its run marker tells a
/// start that no person's press stands behind (`--no-press`, contracts.md
/// C14 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastEnd {
    /// A life that ended in order left the line of the trigger of its end.
    InOrder(StopKind),
    /// A life that did not end in order left its own marker or the record of
    /// ruling PL-3: a kill, a crash, or an end with a status other than 0.
    NotInOrder,
    /// No marker, a marker that cannot be read, or one with no line that
    /// tells why its life ended, as a service of an older build leaves at
    /// its end in order.
    Untold,
}

/// What a start that no person's press stands behind does behind the last
/// life's end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoPress {
    Serves,
    WaitsForAPress,
}

impl LastEnd {
    /// The one table of what a start with `--no-press` does behind each end.
    pub fn no_press(self) -> NoPress {
        NoPress::Serves
    }
}

/// Reads the marker that the last life left and writes the marker of this
/// one, with the hold when the marker that stood held it. No marker and no
/// run/ before it is a first life (`none`); no marker in a run/ that stands,
/// or a marker that holds the hold alone, is a life that ended in order
/// (`clean`); any other marker that stands is a life that did not remove it
/// (`unclean`). A marker that cannot be read, or one that cannot be
/// written, is an error; the reading is then `unclean`.
pub fn begin(dir: &DataDir) -> Begun {
    let run = dir.run_dir();
    let run_stood = std::fs::symlink_metadata(&run).is_ok_and(|meta| meta.is_dir());
    let read = read(dir);
    let last_stop = match &read {
        Ok(None) if run_stood => LastStop::Clean,
        Ok(None) => LastStop::None,
        Ok(Some(text)) if text.trim_end() == HOLD_LINE => LastStop::Clean,
        Ok(Some(_)) | Err(_) => LastStop::Unclean,
    };
    let written = read.and_then(|text| write(dir, text.as_deref().is_some_and(holds_the_hold)));
    Begun { last_stop, written }
}

/// Reads why the last life ended, for a start that no person's press stands
/// behind, and writes nothing.
pub fn last_end(_dir: &DataDir) -> LastEnd {
    LastEnd::NotInOrder
}

/// At a start that cannot bind its channel: where the start read the marker
/// clean or found none, the marker is removed, as a life that ended in order
/// does; where it found the marker of a life that did not end in order, the
/// marker that `begin` wrote in its place stays, so that the next start reads
/// unclean too: a start that cannot bind hides no lost batch.
pub fn give_back(dir: &DataDir, begun: &Begun) -> io::Result<()> {
    if begun.last_stop == LastStop::Unclean {
        return Ok(());
    }
    end(dir, None)
}

/// Whether the marker that stands holds the hold of ruling PL-3 part (5).
pub fn held(dir: &DataDir) -> bool {
    read(dir).is_ok_and(|text| text.as_deref().is_some_and(holds_the_hold))
}

/// Removes the marker of this life and syncs run/, as a life that ended in
/// order does; while the hold stands, the marker keeps the hold alone.
pub fn end(dir: &DataDir, _ended_by: Option<StopKind>) -> io::Result<()> {
    if held(dir) {
        return replace(dir, format!("{HOLD_LINE}\n").as_bytes());
    }
    match std::fs::remove_file(dir.run_marker()) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    sync_folder(&dir.run_dir())
}

fn holds_the_hold(text: &str) -> bool {
    text.lines()
        .any(|line| line == HOLD_LINE || line.starts_with("last_stop="))
}

fn read(dir: &DataDir) -> io::Result<Option<String>> {
    match std::fs::File::open(dir.run_marker()) {
        Ok(file) => {
            let mut held = Vec::new();
            file.take(MARKER_READ_MAX_BYTES).read_to_end(&mut held)?;
            Ok(Some(String::from_utf8_lossy(&held).into_owned()))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write(dir: &DataDir, hold: bool) -> io::Result<()> {
    let mut text = format!("pid={}\n", std::process::id());
    if hold {
        text.push_str(HOLD_LINE);
        text.push('\n');
    }
    replace(dir, text.as_bytes())
}

fn replace(dir: &DataDir, bytes: &[u8]) -> io::Result<()> {
    let run = dir.run_dir();
    make_own_folder(&run)?;
    let path = dir.run_marker();
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".new");
    let next = path.with_file_name(name);
    let mut file = std::fs::File::create(&next)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&next, &path)?;
    sync_folder(&run)
}

#[cfg(target_os = "macos")]
fn make_own_folder(run: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(run)?;
    std::fs::set_permissions(run, std::fs::Permissions::from_mode(0o700))
}

#[cfg(windows)]
fn make_own_folder(run: &Path) -> io::Result<()> {
    std::fs::create_dir_all(run)
}

#[cfg(target_os = "macos")]
fn sync_folder(folder: &Path) -> io::Result<()> {
    std::fs::File::open(folder)?.sync_all()
}

#[cfg(windows)]
fn sync_folder(_folder: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> (tempfile::TempDir, DataDir) {
        let root = tempfile::tempdir().unwrap();
        let dir = DataDir::from_flag(root.path().join("data").to_str().unwrap()).unwrap();
        (root, dir)
    }

    fn marker(dir: &DataDir) -> Option<String> {
        std::fs::read_to_string(dir.run_marker()).ok()
    }

    fn stand(dir: &DataDir, text: &str) {
        replace(dir, text.as_bytes()).unwrap();
    }

    #[test]
    fn an_end_in_order_leaves_the_line_of_its_trigger_which_a_start_reads_clean_with_the_hold_as_before(
    ) {
        for kind in [
            StopKind::Stop,
            StopKind::Signal,
            StopKind::Lifeline,
            StopKind::SessionEnd,
            StopKind::DeleteAll,
        ] {
            let (_root, dir) = scratch();
            assert_eq!(begin(&dir).last_stop, LastStop::None, "{kind:?}");
            end(&dir, Some(kind)).unwrap();
            assert_eq!(
                marker(&dir),
                Some(format!("ended={}\n", kind.as_str())),
                "{kind:?}: the end leaves its line in place of the marker"
            );
            assert!(!held(&dir), "{kind:?}: no hold behind an end with none");
            let next = begin(&dir);
            assert_eq!(next.last_stop, LastStop::Clean, "{kind:?}");
            assert!(next.written.is_ok(), "{kind:?}");
            assert!(!held(&dir), "{kind:?}: the next life holds no hold");
        }

        let (_root, dir) = scratch();
        stand(
            &dir,
            "last_stop=source_not_stopped kind=pause final=yes not_stopped=input:timeout\n",
        );
        assert_eq!(begin(&dir).last_stop, LastStop::Unclean);
        assert!(held(&dir), "a record's hold goes on into the next life");
        end(&dir, Some(StopKind::Lifeline)).unwrap();
        assert_eq!(
            marker(&dir).as_deref(),
            Some("hold=yes\nended=lifeline\n"),
            "the hold stands before the line"
        );
        assert!(held(&dir), "the end keeps the hold");
        assert_eq!(begin(&dir).last_stop, LastStop::Clean);
        assert!(held(&dir), "the next life holds the hold");
        stand(&dir, &format!("pid={}\n", std::process::id()));
        assert!(!held(&dir), "a Resume's marker lifts the hold");
        end(&dir, Some(StopKind::Signal)).unwrap();
        assert_eq!(marker(&dir).as_deref(), Some("ended=signal\n"));
        assert_eq!(begin(&dir).last_stop, LastStop::Clean);
        assert!(!held(&dir), "the hold stays lifted");

        for (text, read) in [
            ("hold=yes\n", LastStop::Clean),
            ("ended=bogus\n", LastStop::Unclean),
            ("", LastStop::Unclean),
            ("pid=1\n", LastStop::Unclean),
        ] {
            let (_root, dir) = scratch();
            stand(&dir, text);
            assert_eq!(begin(&dir).last_stop, read, "{text:?}");
        }
    }

    #[test]
    fn a_start_that_no_press_stands_behind_waits_behind_a_stop_a_signal_or_an_untold_end_and_serves_behind_every_other(
    ) {
        use NoPress::{Serves, WaitsForAPress};
        let rows: [(Option<&str>, LastEnd, NoPress); 14] = [
            (Some("ended=stop\n"), LastEnd::InOrder(StopKind::Stop), WaitsForAPress),
            (Some("hold=yes\nended=stop\n"), LastEnd::InOrder(StopKind::Stop), WaitsForAPress),
            (Some("ended=signal\n"), LastEnd::InOrder(StopKind::Signal), WaitsForAPress),
            (Some("ended=lifeline\n"), LastEnd::InOrder(StopKind::Lifeline), Serves),
            (Some("ended=session_end\n"), LastEnd::InOrder(StopKind::SessionEnd), Serves),
            (Some("ended=delete_all\n"), LastEnd::InOrder(StopKind::DeleteAll), Serves),
            (Some("hold=yes\nended=lifeline\n"), LastEnd::InOrder(StopKind::Lifeline), Serves),
            (Some("pid=4242\n"), LastEnd::NotInOrder, Serves),
            (Some("pid=4242\nhold=yes\n"), LastEnd::NotInOrder, Serves),
            (
                Some("last_stop=source_not_stopped kind=lapse final=yes not_stopped=screen:timeout\n"),
                LastEnd::NotInOrder,
                Serves,
            ),
            (None, LastEnd::Untold, WaitsForAPress),
            (Some("hold=yes\n"), LastEnd::Untold, WaitsForAPress),
            (Some("ended=bogus\n"), LastEnd::Untold, WaitsForAPress),
            (Some(""), LastEnd::Untold, WaitsForAPress),
        ];
        for (text, end, does) in rows {
            let (_root, dir) = scratch();
            if let Some(text) = text {
                stand(&dir, text);
            }
            assert_eq!(last_end(&dir), end, "{text:?}");
            assert_eq!(last_end(&dir).no_press(), does, "{text:?}");
            assert_eq!(marker(&dir).as_deref(), text, "{text:?}: the read writes nothing");
        }
        for kind in StopKind::ALL {
            let does = if matches!(*kind, StopKind::Stop | StopKind::Signal) {
                WaitsForAPress
            } else {
                Serves
            };
            assert_eq!(LastEnd::InOrder(*kind).no_press(), does, "{kind:?}");
        }

        let (_root, dir) = scratch();
        std::fs::create_dir_all(dir.run_marker()).unwrap();
        assert_eq!(
            last_end(&dir),
            LastEnd::Untold,
            "a marker that cannot be read"
        );
    }
}
