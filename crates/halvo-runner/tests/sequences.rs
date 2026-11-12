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

/// The supervisor answers the `load` of the edit step and closes its writer (the runner's standard input) at
/// once, while it still reads the runner's frames. From that close on the runner makes no propose (A6.1): no
/// `store` frame of an effect row may come after it, the workbook stays as it was, and the runner exits 1 within
/// LINK_END.
#[test]
fn a_close_of_input_after_the_edits_load_lets_no_propose_out() {
    let files = FillFiles::new("review-close-1", 1);
    let before = files.workbook_bytes();
    let sup = fill(101, &files);
    let mut runner = support::Runner::start(&sup.args());
    let mut loads = 0;
    let mut closed = false;
    let mut after_close: Vec<FromRunner> = Vec::new();
    while let Some(frame) = runner.recv() {
        if closed {
            after_close.push(frame);
            continue;
        }
        if is_op(&frame, "load") {
            loads += 1;
        }
        if let Some(reply) = sup.answer(&frame) {
            runner.send(&reply);
        }
        if loads == 2 {
            runner.close_input();
            closed = true;
        }
    }
    let status = runner.wait_within(halvo_runner::LINK_END);
    assert!(closed, "the runner reached the load of its edit step");
    let effect_rows: Vec<&'static str> = after_close
        .iter()
        .filter(|f| {
            matches!(f, FromRunner::Store(s)
                if matches!(s.request.body(), halvo_schema::executable::RunnerCall::Effect(_)))
        })
        .map(support::tag)
        .collect();
    assert!(
        effect_rows.is_empty(),
        "no effect row is proposed after the close; frames after the close: {:?}",
        after_close.iter().map(support::tag).collect::<Vec<_>>()
    );
    assert_eq!(files.workbook_bytes(), before, "the workbook is untouched");
    if status.is_none() {
        runner.kill();
    }
    let stderr = runner.stderr();
    assert_eq!(
        status.and_then(|s| s.code()),
        Some(1),
        "status 1 within LINK_END; stderr: {stderr}"
    );
}

/// A6.1: from the close on the runner makes no propose. file_write calls propose() right after the reply to its
/// second load with no check() between them, and Link::send never reads the reader's closed flag, so a close that
/// follows that reply still lets the 'cleared' row's store frame out.
#[test]
fn a_close_after_the_second_load_reply_lets_no_cleared_row_out() {
    let files = FillFiles::new("acting-2-close-after-load", 2);
    let before = files.workbook_bytes();
    let sup = fill(102, &files);
    let mut runner = support::Runner::start(&sup.args());
    let mut loads = 0;
    let mut closed = false;
    let mut after_close: Vec<FromRunner> = Vec::new();
    while let Some(frame) = runner.recv() {
        if closed {
            after_close.push(frame);
            continue;
        }
        let is_load = is_op(&frame, "load");
        if is_load {
            loads += 1;
        }
        if let Some(reply) = sup.answer(&frame) {
            runner.send(&reply);
        }
        if is_load && loads == 2 {
            runner.close_input();
            closed = true;
        }
    }
    let status = runner.wait_within(halvo_runner::LINK_END);
    if status.is_none() {
        runner.kill();
    }
    assert!(closed, "the edit reached its second load");
    let rows: Vec<&'static str> = after_close.iter().map(support::tag).collect();
    assert!(
        !after_close.iter().any(|f| is_op(f, "effect")),
        "no effect row after the close: {rows:?}"
    );
    assert_eq!(
        status.and_then(|s| s.code()),
        Some(1),
        "exit 1 within LINK_END"
    );
    assert_eq!(files.workbook_bytes(), before);
}

/// The supervisor answers the first `begin` of the edit with a store fault and closes its writer (the runner's
/// standard input) at once, while it still reads the runner's frames. A7.5 has the runner repeat a `begin` whose
/// reply it lost, but A6.1 says that from the close on it makes no begin: no `releasing` row may come after the
/// close, the workbook stays as it was, and the runner exits 1 within LINK_END.
#[test]
fn a_close_after_a_fault_of_the_begin_sends_no_begin_again() {
    let files = FillFiles::new("review-close-2", 2);
    let before = files.workbook_bytes();
    let sup = fill(102, &files);
    let mut runner = support::Runner::start(&sup.args());
    let mut closed = false;
    let mut after_close: Vec<FromRunner> = Vec::new();
    while let Some(frame) = runner.recv() {
        if closed {
            after_close.push(frame);
            continue;
        }
        if is_effect(&frame, EffectState::Releasing) {
            runner.send(&store_error(&frame, "store"));
            runner.close_input();
            closed = true;
            continue;
        }
        if let Some(reply) = sup.answer(&frame) {
            runner.send(&reply);
        }
    }
    let status = runner.wait_within(halvo_runner::LINK_END);
    assert!(closed, "the runner reached the begin of its edit");
    let begins = after_close
        .iter()
        .filter(|f| is_effect(f, EffectState::Releasing))
        .count();
    assert_eq!(
        begins,
        0,
        "no begin after the close; frames after the close: {:?}",
        after_close.iter().map(support::tag).collect::<Vec<_>>()
    );
    assert_eq!(files.workbook_bytes(), before, "the workbook is untouched");
    if status.is_none() {
        runner.kill();
    }
    let stderr = runner.stderr();
    assert_eq!(
        status.and_then(|s| s.code()),
        Some(1),
        "status 1 within LINK_END; stderr: {stderr}"
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

/// Every send of the edit's `begin` meets a store fault, so the effect counts as releasing and the run owes an
/// `effect_unknown` handback before its finish. The supervisor answers the third send with the fault and closes
/// its writer (the runner's standard input) at once, while it still reads. From the close on the runner writes
/// nothing more (A6.1): no log row of the handback, no handback, no finish, no done; the workbook stays as it
/// was, and the runner exits 1 within LINK_END.
#[test]
fn a_close_after_the_last_fault_of_the_begin_writes_no_frame() {
    let files = FillFiles::new("review-close-4", 1);
    let before = files.workbook_bytes();
    let sup = fill(104, &files);
    let mut runner = support::Runner::start(&sup.args());
    let mut begins = 0;
    let mut closed = false;
    let mut after_close: Vec<FromRunner> = Vec::new();
    while let Some(frame) = runner.recv() {
        if closed {
            after_close.push(frame);
            continue;
        }
        if is_effect(&frame, EffectState::Releasing) {
            begins += 1;
            runner.send(&store_error(&frame, "store"));
            if begins == 3 {
                runner.close_input();
                closed = true;
            }
            continue;
        }
        if let Some(reply) = sup.answer(&frame) {
            runner.send(&reply);
        }
    }
    let status = runner.wait_within(halvo_runner::LINK_END);
    assert!(closed, "the runner sent its begin three times");
    assert!(
        after_close.is_empty(),
        "no frame after the close; frames after the close: {:?}",
        after_close.iter().map(support::tag).collect::<Vec<_>>()
    );
    assert_eq!(files.workbook_bytes(), before, "the workbook is untouched");
    if status.is_none() {
        runner.kill();
    }
    let stderr = runner.stderr();
    assert_eq!(
        status.and_then(|s| s.code()),
        Some(1),
        "status 1 within LINK_END; stderr: {stderr}"
    );
}
