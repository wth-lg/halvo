//! The run marker of a service run, run/running of the data directory
//! (contracts.md C3.2 step 5 and its stop, D-05): read at the start to set
//! `last_stop`, written anew for the life that starts, and written once more
//! by a life that ends in order, with the line `ended=` and the trigger of
//! that end in the words of contracts.md C2.3 (C14 item 4), and at the
//! decision of a stop by a Stop request or a signal, with that line beside
//! its process line, which an end with 1, the watchdog's end and a kill
//! leave. It holds the process id of its life, and the hold of ruling
//! PL-3 part (5) while that stands: a record of a life that a source which
//! did not stop ended, or the line `hold=yes` that carries such a record's
//! hold on through the lives after it until a Resume lifts it.

use std::io::{self, Read, Write};
#[cfg(target_os = "macos")]
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;

use halvo_control::paths::DataDir;
use halvo_control::proto::{LastStop, StopKind};

const MARKER_READ_MAX_BYTES: u64 = 4_096;

const HOLD_LINE: &str = "hold=yes";

const ENDED: &str = "ended=";

const PID: &str = "pid=";

const RECORD: &str = "last_stop=";

/// What a start found in the marker and made of it: its reading, the result
/// of its write, and the marker that it found, which [`give_back`] puts back.
pub struct Begun {
    pub last_stop: LastStop,
    pub written: io::Result<()>,
    found: Option<String>,
}

/// Why the last life of the data directory ended, as its run marker tells a
/// start that no person's press stands behind (`--no-press`, contracts.md
/// C14 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastEnd {
    /// A life left the line of the trigger of its end: at an end with 0
    /// alone or after the hold, and at the decision of a stop by a Stop
    /// request or a signal, beside its process line, where the end did not
    /// write the clean form over it.
    Ended(StopKind),
    /// A life left its own process line alone, or the record of ruling PL-3:
    /// a kill or a crash before any decision of a stop, an end with 4 or 5,
    /// an end with 1 that no Stop request and no signal began, or a decision
    /// whose line could not be written.
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
    /// The one table of what a start with `--no-press` does behind each end
    /// (the coordinator's rulings of log lines 17942, 17965, 18046 and
    /// 18137): behind a Stop request or a signal, with 0 or with 1, and where
    /// the marker does not tell, it waits for a press, erring toward no
    /// capture; behind the lifeline's end, a session's end, a delete's end,
    /// and a process line alone or a record, it serves as any start does. The
    /// other kinds of C2.3 end no life with 0 or 1 and so stand in no line; a
    /// line that names one serves.
    pub fn no_press(self) -> NoPress {
        match self {
            LastEnd::Ended(kind) => match kind {
                StopKind::Stop => NoPress::WaitsForAPress,
                StopKind::Signal => NoPress::WaitsForAPress,
                StopKind::Lifeline => NoPress::Serves,
                StopKind::SessionEnd => NoPress::Serves,
                StopKind::DeleteAll => NoPress::Serves,
                StopKind::Pause => NoPress::Serves,
                StopKind::Lapse => NoPress::Serves,
                StopKind::GrantLost => NoPress::Serves,
                StopKind::GrantAppeared => NoPress::Serves,
            },
            LastEnd::NotInOrder => NoPress::Serves,
            LastEnd::Untold => NoPress::WaitsForAPress,
        }
    }
}

/// Reads the marker that the last life left and writes the marker of this
/// one, with the hold when the marker that stood held it. No marker and no
/// run/ before it is a first life (`none`); no marker in a run/ that stands,
/// or a marker whose lines are the hold and the line of an end in order,
/// either or both, is a life that ended in order (`clean`); any other marker
/// that stands is a life that did not end in order (`unclean`). A marker that
/// cannot be read, or one that cannot be written, is an error; the reading is
/// then `unclean`.
pub fn begin(dir: &DataDir) -> Begun {
    let run = dir.run_dir();
    let run_stood = std::fs::symlink_metadata(&run).is_ok_and(|meta| meta.is_dir());
    let read = read(dir);
    let last_stop = match &read {
        Ok(None) if run_stood => LastStop::Clean,
        Ok(None) => LastStop::None,
        Ok(Some(text)) if ended_in_order(text) => LastStop::Clean,
        Ok(Some(_)) | Err(_) => LastStop::Unclean,
    };
    let found = read.as_ref().ok().cloned().flatten();
    let written = read.and_then(|text| write(dir, text.as_deref().is_some_and(holds_the_hold)));
    Begun {
        last_stop,
        written,
        found,
    }
}

/// Reads why the last life ended, for a start that no person's press stands
/// behind, and writes nothing: the line of an end names its trigger, also
/// beside a process line, a word that is no kind of C2.3 tells nothing, and
/// with no such line the process line of a life or a record is a life that
/// did not end in order.
pub fn last_end(dir: &DataDir) -> LastEnd {
    let Ok(Some(text)) = read(dir) else {
        return LastEnd::Untold;
    };
    if let Some(word) = text.lines().find_map(|line| line.strip_prefix(ENDED)) {
        return word.parse().map_or(LastEnd::Untold, LastEnd::Ended);
    }
    if text
        .lines()
        .any(|line| line.starts_with(PID) || line.starts_with(RECORD))
    {
        LastEnd::NotInOrder
    } else {
        LastEnd::Untold
    }
}

/// At a start that cannot bind its channel: where the start read the marker
/// clean or found none, it puts back the marker that it found (no marker
/// stays none; the hold and the line of an end in order stay), so that the
/// next start reads what this one read; where it found the marker of a life
/// that did not end in order, the marker that `begin` wrote in its place
/// stays, so that the next start reads unclean too: a start that cannot bind
/// hides no lost batch.
pub fn give_back(dir: &DataDir, begun: &Begun) -> io::Result<()> {
    if begun.last_stop == LastStop::Unclean {
        return Ok(());
    }
    match &begun.found {
        Some(text) => replace(dir, text.as_bytes()),
        None => remove(dir),
    }
}

/// Whether the marker that stands holds the hold of ruling PL-3 part (5).
pub fn held(dir: &DataDir) -> bool {
    read(dir).is_ok_and(|text| text.as_deref().is_some_and(holds_the_hold))
}

/// What the end of a life leaves in its marker, by its exit status and the
/// trigger of its end (contracts.md C3.2 Stop, C14 item 4): at 0 [`end`]; at
/// 1 where a Stop request or a signal began the end, the form of
/// [`decided`] once more, so that the next start reads it unclean, as before,
/// and a start that no press stands behind reads the trigger; at every other
/// end the marker stays as the life left it: its process line alone, the
/// form of the decision, or the record of ruling PL-3 at an end with 5.
/// At 1 a marker that cannot be read, or a write that fails, gives what
/// [`decided`] gives.
pub fn at_the_end(dir: &DataDir, status: i32, ended_by: Option<StopKind>) -> io::Result<()> {
    match (status, ended_by) {
        (halvo_control::exit::CLEAN, _) => end(dir, ended_by),
        (halvo_control::exit::ERROR, Some(kind @ (StopKind::Stop | StopKind::Signal))) => {
            decided(dir, kind)
        }
        _ => Ok(()),
    }
}

/// The decision of a stop (contracts.md C3.2 Stop, C14 item 4), written where
/// a Stop request is accepted or the run reads that a signal set its stop
/// flag, before anything of the end that can hang or fail: the process line
/// of the life, the hold while it stands and the line of `kind`, by the
/// durable write. An end with 0 writes its clean form over it ([`end`]); an
/// end with 1, the watchdog's end and a kill leave it, which the next start
/// reads unclean and a start that no press stands behind reads as `kind`.
/// Where the marker cannot be read, or the write fails before its rename,
/// the marker stays as it stands, the hold kept; where only the sync of run/
/// behind the rename fails, on a Mac, the decision stands and the call
/// answers that error.
pub fn decided(dir: &DataDir, kind: StopKind) -> io::Result<()> {
    let found = read(dir)?;
    let mut text = format!("{PID}{}\n", std::process::id());
    if found.as_deref().is_some_and(holds_the_hold) {
        text.push_str(HOLD_LINE);
        text.push('\n');
    }
    text.push_str(ENDED);
    text.push_str(kind.as_str());
    text.push('\n');
    replace(dir, text.as_bytes())
}

/// Ends the marker of a life that ended in order: in its place the hold,
/// while it stands, and the line of `ended_by`, the trigger of the end, by
/// the durable write. With no trigger, or where that write fails while no
/// hold stands, a removal of the marker and a sync of run/ are tried, as an
/// end in order did before the line, which a start that no press stands
/// behind reads, where the removal was made, as an end that does not tell.
pub fn end(dir: &DataDir, ended_by: Option<StopKind>) -> io::Result<()> {
    let hold = held(dir);
    let mut text = String::new();
    if hold {
        text.push_str(HOLD_LINE);
        text.push('\n');
    }
    if let Some(kind) = ended_by {
        text.push_str(ENDED);
        text.push_str(kind.as_str());
        text.push('\n');
    }
    if text.is_empty() {
        return remove(dir);
    }
    let written = replace(dir, text.as_bytes());
    if written.is_err() && !hold {
        let _ = remove(dir);
    }
    written
}

fn ended_in_order(text: &str) -> bool {
    let text = text.trim_end();
    !text.is_empty()
        && text.lines().all(|line| {
            line == HOLD_LINE
                || line
                    .strip_prefix(ENDED)
                    .is_some_and(|word| word.parse::<StopKind>().is_ok())
        })
}

fn holds_the_hold(text: &str) -> bool {
    text.lines()
        .any(|line| line == HOLD_LINE || line.starts_with(RECORD))
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
    let mut text = format!("{PID}{}\n", std::process::id());
    if hold {
        text.push_str(HOLD_LINE);
        text.push('\n');
    }
    replace(dir, text.as_bytes())
}

fn remove(dir: &DataDir) -> io::Result<()> {
    match std::fs::remove_file(dir.run_marker()) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    sync_folder(&dir.run_dir())
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
    fn a_stop_decided_whose_end_is_cut_off_leaves_its_line_beside_the_process_line_and_a_start_with_no_press_waits(
    ) {
        let pid = format!("pid={}\n", std::process::id());
        let record =
            "last_stop=source_not_stopped kind=pause final=yes not_stopped=input:timeout\n";
        for kind in [StopKind::Stop, StopKind::Signal] {
            for hold in [false, true] {
                let (_root, dir) = scratch();
                if hold {
                    stand(&dir, record);
                }
                assert!(begin(&dir).written.is_ok(), "{kind:?}");
                decided(&dir, kind).unwrap();
                let hold_line = if hold { "hold=yes\n" } else { "" };
                assert_eq!(
                    marker(&dir),
                    Some(format!("{pid}{hold_line}ended={}\n", kind.as_str())),
                    "{kind:?}: the end is cut off behind the decision"
                );
                assert_eq!(
                    last_end(&dir).no_press(),
                    NoPress::WaitsForAPress,
                    "{kind:?}: a start with --no-press ends with 7"
                );
                assert_eq!(held(&dir), hold, "{kind:?}: the hold as before");
                assert_eq!(
                    begin(&dir).last_stop,
                    LastStop::Unclean,
                    "{kind:?}: a start without the argument reads unclean, as behind a kill"
                );
            }
        }
        let (_root, dir) = scratch();
        assert!(begin(&dir).written.is_ok());
        decided(&dir, StopKind::Stop).unwrap();
        at_the_end(
            &dir,
            halvo_control::exit::SOURCE_NOT_STOPPED,
            Some(StopKind::Stop),
        )
        .unwrap();
        assert_eq!(
            marker(&dir),
            Some(format!("{pid}ended=stop\n")),
            "the shutdown entry's own end with 5 leaves the decision"
        );
        assert_eq!(last_end(&dir).no_press(), NoPress::WaitsForAPress);
        let (_root, dir) = scratch();
        assert!(begin(&dir).written.is_ok());
        decided(&dir, StopKind::Stop).unwrap();
        at_the_end(&dir, halvo_control::exit::CLEAN, Some(StopKind::Stop)).unwrap();
        assert_eq!(
            marker(&dir).as_deref(),
            Some("ended=stop\n"),
            "an end with 0 behind the decision writes the clean form"
        );
        assert_eq!(begin(&dir).last_stop, LastStop::Clean);
    }

    #[test]
    fn a_stop_or_a_signal_that_ends_with_1_leaves_its_line_beside_the_process_line_and_a_start_with_no_press_waits(
    ) {
        use halvo_control::exit::{CLEAN, ERROR, RESTART, SOURCE_NOT_STOPPED};
        let pid = format!("pid={}\n", std::process::id());
        let record =
            "last_stop=source_not_stopped kind=pause final=yes not_stopped=input:timeout\n";
        for (end, kind) in [
            ("no answer to the last settle", StopKind::Stop),
            ("the drain still fails at the last settle", StopKind::Signal),
            ("events lost between the two settles", StopKind::Stop),
            (
                "the store's owner silent for 1 s at its close",
                StopKind::Signal,
            ),
        ] {
            for hold in [false, true] {
                let (_root, dir) = scratch();
                if hold {
                    stand(&dir, record);
                }
                assert!(begin(&dir).written.is_ok(), "{end}");
                at_the_end(&dir, ERROR, Some(kind)).unwrap();
                let hold_line = if hold { "hold=yes\n" } else { "" };
                assert_eq!(
                    marker(&dir),
                    Some(format!("{pid}{hold_line}ended={}\n", kind.as_str())),
                    "{end}: the line beside the process line"
                );
                assert_eq!(last_end(&dir), LastEnd::Ended(kind), "{end}");
                assert_eq!(last_end(&dir).no_press(), NoPress::WaitsForAPress, "{end}");
                assert_eq!(held(&dir), hold, "{end}: the hold as before");
                assert_eq!(
                    begin(&dir).last_stop,
                    LastStop::Unclean,
                    "{end}: a start without the argument reads unclean, as before"
                );
            }
        }
        for (status, ended_by, stood, left) in [
            (
                CLEAN,
                Some(StopKind::Stop),
                None,
                Some("ended=stop\n".to_owned()),
            ),
            (ERROR, Some(StopKind::Lifeline), None, Some(pid.clone())),
            (ERROR, None, None, Some(pid.clone())),
            (RESTART, None, None, Some(pid.clone())),
            (
                SOURCE_NOT_STOPPED,
                Some(StopKind::Stop),
                Some(record),
                Some(record.to_owned()),
            ),
        ] {
            let (_root, dir) = scratch();
            assert!(begin(&dir).written.is_ok());
            if let Some(stood) = stood {
                stand(&dir, stood);
            }
            at_the_end(&dir, status, ended_by).unwrap();
            assert_eq!(marker(&dir), left, "{status} {ended_by:?}");
        }
    }

    #[test]
    fn a_start_that_no_press_stands_behind_waits_behind_a_stop_a_signal_or_an_untold_end_and_serves_behind_every_other(
    ) {
        use NoPress::{Serves, WaitsForAPress};
        let rows: [(Option<&str>, LastEnd, NoPress); 14] = [
            (Some("ended=stop\n"), LastEnd::Ended(StopKind::Stop), WaitsForAPress),
            (Some("hold=yes\nended=stop\n"), LastEnd::Ended(StopKind::Stop), WaitsForAPress),
            (Some("ended=signal\n"), LastEnd::Ended(StopKind::Signal), WaitsForAPress),
            (Some("ended=lifeline\n"), LastEnd::Ended(StopKind::Lifeline), Serves),
            (Some("ended=session_end\n"), LastEnd::Ended(StopKind::SessionEnd), Serves),
            (Some("ended=delete_all\n"), LastEnd::Ended(StopKind::DeleteAll), Serves),
            (Some("hold=yes\nended=lifeline\n"), LastEnd::Ended(StopKind::Lifeline), Serves),
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
            assert_eq!(
                marker(&dir).as_deref(),
                text,
                "{text:?}: the read writes nothing"
            );
        }
        for kind in StopKind::ALL {
            let does = if matches!(*kind, StopKind::Stop | StopKind::Signal) {
                WaitsForAPress
            } else {
                Serves
            };
            assert_eq!(LastEnd::Ended(*kind).no_press(), does, "{kind:?}");
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
