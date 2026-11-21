mod support;

use std::time::{Duration, Instant};

use halvo_schema::executable::frame::Cancel;
use halvo_schema::executable::names::{HandbackReason, LogEvent};
use halvo_schema::executable::{CheckpointState, EffectState, FromRunner, Outcome, ToRunner};
use halvo_schema::Category;

use support::{is_checkpoint, is_effect, is_op, store_error, Act, FillFiles, Supervisor, LONG};

fn fill(n: u8, files: &FillFiles) -> Supervisor {
    Supervisor::new(n, Category::SpreadsheetFill, files.slots())
}

/// A releasing effect must be handed back as effect_unknown before finish (A6.4). The commit row is refused,
/// so the effect stays releasing after the replace; then the store refuses the log line of the handback.
/// `handback` goes on past the refused line, so the handback frame is written, and finish follows it.
#[test]
fn a_refused_log_line_does_not_keep_the_effect_unknown_handback_from_opening() {
    let files = FillFiles::new("review-twice-1", 1);
    let sup = fill(141, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Committed) {
                return Act::With(store_error(frame, "effect"));
            }
            if let FromRunner::Store(s) = frame {
                if let halvo_schema::executable::RunnerCall::Log(b) = s.request.body() {
                    if b.records
                        .iter()
                        .any(|r| matches!(r.event, LogEvent::Handback))
                    {
                        return Act::With(store_error(frame, "store"));
                    }
                }
            }
            Act::Answer
        },
        LONG,
    );
    assert_eq!(
        seen.states(),
        [
            EffectState::Cleared,
            EffectState::Releasing,
            EffectState::Committed
        ],
        "precondition: the edit left with its releasing row and the commit row was refused: {}",
        seen.stderr
    );
    assert!(
        seen.finish().is_some(),
        "the run stores its finish: tags {:?} {}",
        seen.tags(),
        seen.stderr
    );
    assert_eq!(
        seen.handbacks(),
        [HandbackReason::EffectUnknown],
        "a finish went out while the effect was releasing and no effect_unknown handback came before it; tags {:?}",
        seen.tags()
    );
    let tags = seen.tags();
    let handback_at = tags.iter().position(|t| *t == "handback");
    let done_at = tags.iter().position(|t| *t == "done");
    assert!(
        handback_at.is_some() && handback_at < done_at,
        "the handback must come before done: {tags:?}"
    );
}

/// The wall limit passes while the edit waits for the load before its `prepare`: the run ends with `limit` and
/// `wall` at the read before the `propose`, with no effect row and no replace.
#[test]
fn a_wall_limit_passed_during_the_edit_ends_the_run_before_its_effect() {
    let files = FillFiles::new("acting-3-wall-mid-edit", 3);
    let before = files.workbook_bytes();
    let mut sup = fill(103, &files);
    sup.config.limits.wall_s = 1;
    let mut loads = 0;
    let seen = sup.serve(
        |frame, _| {
            if is_op(frame, "load") {
                loads += 1;
                if loads == 2 {
                    return Act::After(Duration::from_millis(2600), Box::new(Act::Answer));
                }
            }
            Act::Answer
        },
        LONG,
    );
    assert_ne!(
        seen.done(),
        Some(Outcome::Completed),
        "a run past its wall limit does not end completed: {}",
        seen.stderr
    );
    assert!(
        !seen.states().contains(&EffectState::Releasing),
        "no begin after the wall limit: {:?}",
        seen.states()
    );
    assert_eq!(
        files.workbook_bytes(),
        before,
        "no replace after the wall limit"
    );
    assert!(seen.effects().is_empty(), "no propose after the wall limit");
    assert_eq!(seen.done(), Some(Outcome::Limit));
    assert_eq!(
        seen.logs().last().map(|l| l.cause),
        Some(Some(halvo_schema::executable::Cause::Wall))
    );
}

/// The begin's first send meets a store fault, which may come after the releasing row was stored (the comment of
/// ROW_SENDS says so). The repeat is then refused with `effect`. A begin that met a fault counts as releasing, so
/// the runner lets nothing leave and hands the effect back as effect_unknown before its finish, with unknown 1.
#[test]
fn a_begin_that_met_a_fault_and_then_a_refusal_is_handed_back_unknown() {
    let files = FillFiles::new("review-twice-2", 2);
    let before = files.workbook_bytes();
    let sup = fill(142, &files);
    let sends = std::cell::Cell::new(0);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Releasing) {
                sends.set(sends.get() + 1);
                if sends.get() == 1 {
                    return Act::With(store_error(frame, "store"));
                }
                return Act::With(store_error(frame, "effect"));
            }
            Act::Answer
        },
        LONG,
    );
    assert_eq!(
        seen.states(),
        [
            EffectState::Cleared,
            EffectState::Releasing,
            EffectState::Releasing
        ],
        "precondition: one fault, then a refusal of the repeat: {}",
        seen.stderr
    );
    assert_eq!(files.workbook_bytes(), before, "nothing may leave");
    assert!(
        seen.finish().is_some(),
        "the run stores its finish: tags {:?} {}",
        seen.tags(),
        seen.stderr
    );
    assert_eq!(
        seen.handbacks(),
        [HandbackReason::EffectUnknown],
        "a begin that met a fault may be stored, so it must be handed back effect_unknown before finish"
    );
    assert_eq!(seen.finish().map(|f| f.unknown), Some(1));
    assert_eq!(seen.done(), Some(Outcome::EffectUnknown));
}

/// While the runner waits for the reply to the `load` of its edit step, the supervisor sends `cancel` and then the
/// load's answer. The cancel is pending before any effect of the run exists, so the run must end declined with no
/// effect row at all: no propose after the cancel, and the workbook stays as it was.
#[test]
fn a_cancel_during_the_edits_load_makes_no_effect() {
    let files = FillFiles::new("review-close-3", 3);
    let before = files.workbook_bytes();
    let sup = fill(103, &files);
    let seen = sup.serve(
        |frame, frames_before| {
            let loads_before = frames_before.iter().filter(|f| is_op(f, "load")).count();
            if is_op(frame, "load") && loads_before == 1 {
                Act::Then(ToRunner::Cancel(Cancel {}), Box::new(Act::Answer))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(
        seen.ops().iter().filter(|op| **op == "load").count(),
        2,
        "the run reached the load of its edit step; ops: {:?}",
        seen.ops()
    );
    assert_eq!(
        seen.states(),
        Vec::<EffectState>::new(),
        "no effect is proposed once the cancel is pending"
    );
    assert_eq!(seen.done(), Some(Outcome::Declined));
    assert_eq!(files.workbook_bytes(), before, "the workbook is untouched");
}

/// What the supervisor saw of a run whose standard input it closed: the frames that came after the close, the
/// runner's status, the time from the close to the runner's end and its standard error.
struct AfterClose {
    frames: Vec<FromRunner>,
    code: Option<i32>,
    close_to_end: Option<Duration>,
    stderr: String,
}

impl AfterClose {
    fn names(&self) -> Vec<String> {
        self.frames.iter().map(support::named).collect()
    }
}

/// Serves the runner from the in-memory store; `reply` may answer a frame itself and say that the supervisor closes
/// the runner's standard input right after that answer, while it goes on reading.
fn close_input_after(
    sup: &Supervisor,
    mut reply: impl FnMut(&FromRunner) -> Option<(ToRunner, bool)>,
) -> AfterClose {
    let mut runner = support::Runner::start(&sup.args());
    let mut closed_at = None;
    let mut frames = Vec::new();
    while let Some(frame) = runner.recv() {
        if closed_at.is_some() {
            frames.push(frame);
            continue;
        }
        let (answer, close) = match reply(&frame) {
            Some((answer, close)) => (Some(answer), close),
            None => (sup.answer(&frame), false),
        };
        if let Some(answer) = answer {
            runner.send(&answer);
        }
        if close {
            runner.close_input();
            closed_at = Some(Instant::now());
        }
    }
    let closed_at = closed_at.expect("the run reached the frame of the close");
    let status = runner.wait_within(halvo_runner::LINK_END);
    let close_to_end = status.map(|_| closed_at.elapsed());
    if status.is_none() {
        runner.kill();
    }
    AfterClose {
        frames,
        code: status.and_then(|s| s.code()),
        close_to_end,
        stderr: runner.stderr(),
    }
}

/// The bound within which a runner ends by itself after a close, well under the 15 s after which the reader's grace
/// exit ends the process with the status 1 (`LINK_GRACE` of src/link.rs).
const BY_ITSELF: Duration = Duration::from_secs(5);

/// The line with which the runner's run ends after a close (src/run.rs, `ended`).
const LINK_CLOSED_LINE: &str = "run link=closed";

/// What the runner keeps after a close of its standard input alone (A6.1): it reads the link before every frame,
/// but the end of file can still be on its way to its reader when it writes, so the frames that it writes before
/// it waits for a reply may come, at most `most`; then no other frame, no replace, and the status 1 of the run's
/// own end on the close, with its line, within `BY_ITSELF`, so that a runner that holds its request after the close
/// and leaves by the grace exit fails.
fn kept_after_a_close_of(after: &AfterClose, files: &FillFiles, before: &[u8], most: usize) {
    let names = after.names();
    assert!(
        after.frames.len() <= most,
        "at most the frames on their way: {names:?}"
    );
    assert_eq!(files.workbook_bytes(), before, "no replace after the close");
    assert_eq!(after.code, Some(1), "status 1: {names:?}");
    assert!(
        after.stderr.lines().any(|line| line == LINK_CLOSED_LINE),
        "the run ends on the close with its own line: {:?}",
        after.stderr
    );
    assert!(
        after.close_to_end.is_some_and(|took| took < BY_ITSELF),
        "the runner ends by itself within {BY_ITSELF:?} of the close, before the grace exit: {:?}",
        after.close_to_end
    );
}

/// What the runner keeps after a close of its standard input alone when it waits for a reply after the next frame.
fn kept_after_a_close(after: &AfterClose, files: &FillFiles, before: &[u8]) {
    kept_after_a_close_of(after, files, before, 1);
}

fn load_count() -> impl FnMut(&FromRunner) -> usize {
    let mut loads = 0;
    move |frame| {
        if is_op(frame, "load") {
            loads += 1;
        }
        loads
    }
}

/// Serves a fill run of seed `seed` with the link open and every frame answered, to its `done`, and names the frame
/// that the runner wrote right after the answer to the `nth` frame (counted from 1) that `is` matches: the frame that
/// a close on that answer can let out.
fn next_after_an_open_run(seed: u8, n: u8, is: impl Fn(&FromRunner) -> bool, nth: usize) -> String {
    let files = FillFiles::new("review-open", seed);
    let seen = fill(n, &files).serve(|_, _| Act::Answer, LONG);
    let names: Vec<String> = seen.frames.iter().map(support::named).collect();
    assert_eq!(
        seen.done(),
        Some(Outcome::Completed),
        "the open run ends completed: {names:?} {}",
        seen.stderr
    );
    let at = seen
        .frames
        .iter()
        .enumerate()
        .filter(|(_, f)| is(f))
        .nth(nth - 1)
        .map(|(at, _)| at)
        .unwrap_or_else(|| panic!("the open run wrote that frame: {names:?}"));
    names
        .get(at + 1)
        .cloned()
        .unwrap_or_else(|| panic!("a frame after it: {names:?}"))
}

/// The close after the answer to the checkpoint's `made` row, the last request between the edit's load and its
/// `propose` (A7.1, CP-98 (4)): a run with the link open shows that the frame after that answer is the propose, so
/// that a request added between the two fails on every run (CP-225 N1); after the close at most the propose is on
/// its way, and no replace follows.
#[test]
fn a_close_of_input_after_the_checkpoints_made_row_lets_at_most_the_propose_out() {
    assert_eq!(
        next_after_an_open_run(1, 101, |f| is_checkpoint(f, CheckpointState::Made), 1),
        "store effect Cleared",
        "the propose is the frame after the made row's answer"
    );
    let files = FillFiles::new("review-close-1", 1);
    let before = files.workbook_bytes();
    let sup = fill(101, &files);
    let mut count = load_count();
    let after = close_input_after(&sup, |frame| {
        (count(frame) == 2 && is_checkpoint(frame, CheckpointState::Made))
            .then(|| (sup.answer(frame).expect("a reply"), true))
    });
    kept_after_a_close(&after, &files, &before);
    let names = after.names();
    assert!(
        after
            .frames
            .iter()
            .all(|f| is_effect(f, EffectState::Cleared)),
        "only the propose can be on its way: {names:?}"
    );
}

/// The close after the answer to the edit's load, the run's second load (CP-135 (3)): a run with the link open shows
/// that the frame after that answer is the checkpoint's `planned` row (A7.1, CP-98 (4)); after the close at most that
/// row is on its way, no replace follows and nothing is made beside the workbook, neither a checkpoint nor a
/// prepared copy.
#[test]
fn a_close_of_input_after_the_edits_load_lets_at_most_the_planned_row_out() {
    assert_eq!(
        next_after_an_open_run(1, 107, |f| is_op(f, "load"), 2),
        "store checkpoint Planned",
        "the planned row is the frame after the answer to the edit's load"
    );
    let files = FillFiles::new("review-close-7", 1);
    let before = files.workbook_bytes();
    let names_before = files.names();
    let sup = fill(107, &files);
    let mut count = load_count();
    let after = close_input_after(&sup, |frame| {
        (count(frame) == 2 && is_op(frame, "load"))
            .then(|| (sup.answer(frame).expect("a reply"), true))
    });
    kept_after_a_close(&after, &files, &before);
    let names = after.names();
    assert!(
        after
            .frames
            .iter()
            .all(|f| is_checkpoint(f, CheckpointState::Planned)),
        "only the planned row can be on its way: {names:?}"
    );
    assert_eq!(
        files.names(),
        names_before,
        "nothing beside the workbook: no checkpoint and no prepared copy"
    );
}

/// The close after the load before the edit's `begin` (CP-135 (3)): at most the begin row is on its way, and no
/// replace follows.
#[test]
fn a_close_of_input_after_the_load_before_the_begin_lets_at_most_the_begin_out() {
    let files = FillFiles::new("review-close-6", 2);
    let before = files.workbook_bytes();
    let sup = fill(106, &files);
    let mut count = load_count();
    let after = close_input_after(&sup, |frame| {
        (count(frame) == 3 && is_op(frame, "load"))
            .then(|| (sup.answer(frame).expect("a reply"), true))
    });
    kept_after_a_close(&after, &files, &before);
    assert!(
        after
            .frames
            .iter()
            .all(|f| is_effect(f, EffectState::Releasing)),
        "only the begin can be on its way: {:?}",
        after.names()
    );
}

#[test]
fn a_close_of_input_after_a_fault_of_the_begin_lets_at_most_its_repeat_out() {
    let files = FillFiles::new("review-close-2", 2);
    let before = files.workbook_bytes();
    let sup = fill(102, &files);
    let after = close_input_after(&sup, |frame| {
        is_effect(frame, EffectState::Releasing).then(|| (store_error(frame, "store"), true))
    });
    kept_after_a_close(&after, &files, &before);
}

#[test]
fn a_close_of_input_after_the_last_fault_of_the_begin_lets_at_most_one_frame_out() {
    let files = FillFiles::new("review-close-4", 1);
    let before = files.workbook_bytes();
    let sup = fill(104, &files);
    let mut begins = 0;
    let after = close_input_after(&sup, |frame| {
        if !is_effect(frame, EffectState::Releasing) {
            return None;
        }
        begins += 1;
        Some((store_error(frame, "store"), begins == 3))
    });
    kept_after_a_close(&after, &files, &before);
    assert!(
        !after
            .frames
            .iter()
            .any(|f| matches!(f, FromRunner::Handback(_) | FromRunner::Done(_))),
        "no handback and no done after the close: {:?}",
        after.names()
    );
}

/// The close after the answer to the read step's line: the next `progress` and the edit's first `load`, which the
/// runner writes before it waits for a reply, may be on their way (CP-135 (3)); no effect row and no replace.
#[test]
fn a_close_of_input_after_the_reads_log_lets_at_most_the_next_progress_and_the_edits_load_out() {
    let files = FillFiles::new("review-close-5", 3);
    let before = files.workbook_bytes();
    let sup = fill(105, &files);
    let after = close_input_after(&sup, |frame| {
        is_op(frame, "log").then(|| (sup.answer(frame).expect("a reply"), true))
    });
    kept_after_a_close_of(&after, &files, &before, 2);
    let kinds: Vec<&str> = after
        .frames
        .iter()
        .map(|f| match f {
            FromRunner::Progress(_) => "progress",
            f if is_op(f, "load") => "load",
            _ => "other",
        })
        .collect();
    assert!(
        ["progress", "load"].starts_with(&kinds),
        "at most the progress and then the edit's load: {kinds:?}"
    );
}
