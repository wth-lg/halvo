mod support;

use std::time::Duration;

use halvo_schema::executable::frame::Cancel;
use halvo_schema::executable::names::{HandbackReason, LogEvent};
use halvo_schema::executable::{EffectState, FromRunner, Outcome, ToRunner};
use halvo_schema::Category;

use support::{is_effect, is_op, store_error, Act, FillFiles, Supervisor, LONG};

fn fill(n: u8, files: &FillFiles) -> Supervisor {
    Supervisor::new(n, Category::SpreadsheetFill, files.slots())
}

/// A releasing effect must be handed back as effect_unknown before finish (A6.4). The commit row is refused,
/// so the effect stays releasing after the replace; then the store refuses the log line of the handback.
/// `close` ignores a `Stop::End` from `handback`, so the handback frame is never written and finish still goes.
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
    if seen.finish().is_some() {
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
}

/// A6.4: a releasing effect is handed back with effect_unknown before finish, even when the store refuses the log
/// line of the handback. close() ignores a Stop::End from handback(), whose log() runs before the Handback frame.
#[test]
fn the_effect_unknown_handback_opens_when_its_log_line_is_refused() {
    let files = FillFiles::new("acting-1-handback-log", 1);
    let before = files.workbook_bytes();
    let sup = fill(101, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Releasing) {
                return Act::Then(ToRunner::Cancel(Cancel {}), Box::new(Act::Answer));
            }
            if let FromRunner::Store(s) = frame {
                if let halvo_schema::executable::RunnerCall::Log(b) = s.request.body() {
                    if b.records.iter().any(|r| r.event == LogEvent::Handback) {
                        return Act::With(store_error(frame, "store"));
                    }
                }
            }
            Act::Answer
        },
        LONG,
    );
    assert_eq!(
        files.workbook_bytes(),
        before,
        "no replace after the cancel"
    );
    assert_eq!(
        seen.handbacks(),
        [HandbackReason::EffectUnknown],
        "the releasing effect is handed back before finish: {}",
        seen.stderr
    );
    let tags = seen.tags();
    let hb = tags.iter().position(|t| *t == "handback");
    let fin = seen.frames.iter().position(|f| is_op(f, "finish"));
    assert!(
        hb.is_some() && fin.is_some() && hb < fin,
        "handback before finish: {tags:?}"
    );
}

/// Limits reached between the prepare and the replace: the wall limit passes while the edit waits for its second
/// load, and the runner still proposes, begins and replaces, and the run ends completed past its wall limit;
/// within() runs only at the top of each step.
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
/// ROW_SENDS says so). The repeat is then refused with `effect`. Only a begin whose every send faults counts as
/// releasing, so here the runner drops the "may be stored" state: no effect_unknown handback, finish unknown 0.
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
    if seen.finish().is_some() {
        assert_eq!(
            seen.handbacks(),
            [HandbackReason::EffectUnknown],
            "a begin that met a fault may be stored, so it must be handed back effect_unknown before finish"
        );
        assert_eq!(seen.finish().map(|f| f.unknown), Some(1));
        assert_eq!(seen.done(), Some(Outcome::EffectUnknown));
    }
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

/// Serves the runner from the in-memory store; `reply` may answer a frame itself and say that the supervisor closes
/// the runner's standard input right after that answer, while it goes on reading. Returns the frames that came after
/// the close and the status.
fn close_input_after(
    sup: &Supervisor,
    mut reply: impl FnMut(&FromRunner) -> Option<(ToRunner, bool)>,
) -> (Vec<FromRunner>, Option<i32>) {
    let mut runner = support::Runner::start(&sup.args());
    let mut closed = false;
    let mut after_close = Vec::new();
    while let Some(frame) = runner.recv() {
        if closed {
            after_close.push(frame);
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
            closed = true;
        }
    }
    assert!(closed, "the run reached the frame of the close");
    let status = runner.wait_within(halvo_runner::LINK_END);
    if status.is_none() {
        runner.kill();
    }
    (after_close, status.and_then(|s| s.code()))
}

/// What the runner keeps after a close of its standard input alone (A6.1): it reads the link before every frame,
/// but the end of file can still be on its way to its reader when it writes, so the one frame that it was about to
/// write may come; then no other frame, no replace, and the status 1 within LINK_END.
fn kept_after_a_close(
    after_close: &[FromRunner],
    code: Option<i32>,
    files: &FillFiles,
    before: &[u8],
) {
    let tags: Vec<&'static str> = after_close.iter().map(support::tag).collect();
    assert!(
        after_close.len() <= 1,
        "at most the frame on its way: {tags:?}"
    );
    assert_eq!(files.workbook_bytes(), before, "no replace after the close");
    assert_eq!(code, Some(1), "status 1 within LINK_END");
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

#[test]
fn a_close_of_input_after_the_edits_load_lets_at_most_its_propose_out() {
    let files = FillFiles::new("review-close-1", 1);
    let before = files.workbook_bytes();
    let sup = fill(101, &files);
    let mut count = load_count();
    let (after, code) = close_input_after(&sup, |frame| {
        (count(frame) == 2 && is_op(frame, "load"))
            .then(|| (sup.answer(frame).expect("a reply"), true))
    });
    kept_after_a_close(&after, code, &files, &before);
    assert!(
        after.iter().all(|f| is_effect(f, EffectState::Cleared)),
        "only the propose can be on its way"
    );
}

#[test]
fn a_close_of_input_after_a_fault_of_the_begin_lets_at_most_its_repeat_out() {
    let files = FillFiles::new("review-close-2", 2);
    let before = files.workbook_bytes();
    let sup = fill(102, &files);
    let (after, code) = close_input_after(&sup, |frame| {
        is_effect(frame, EffectState::Releasing).then(|| (store_error(frame, "store"), true))
    });
    kept_after_a_close(&after, code, &files, &before);
}

#[test]
fn a_close_of_input_after_the_last_fault_of_the_begin_lets_at_most_one_frame_out() {
    let files = FillFiles::new("review-close-4", 1);
    let before = files.workbook_bytes();
    let sup = fill(104, &files);
    let mut begins = 0;
    let (after, code) = close_input_after(&sup, |frame| {
        if !is_effect(frame, EffectState::Releasing) {
            return None;
        }
        begins += 1;
        Some((store_error(frame, "store"), begins == 3))
    });
    kept_after_a_close(&after, code, &files, &before);
    assert!(
        !after
            .iter()
            .any(|f| matches!(f, FromRunner::Handback(_) | FromRunner::Done(_))),
        "no handback and no done after the close"
    );
}

#[test]
fn a_close_of_input_after_the_reads_log_lets_at_most_the_next_progress_out() {
    let files = FillFiles::new("review-close-5", 3);
    let before = files.workbook_bytes();
    let sup = fill(105, &files);
    let (after, code) = close_input_after(&sup, |frame| {
        is_op(frame, "log").then(|| (sup.answer(frame).expect("a reply"), true))
    });
    kept_after_a_close(&after, code, &files, &before);
    assert!(
        after.iter().all(|f| matches!(f, FromRunner::Progress(_))),
        "no store request after the close"
    );
}
