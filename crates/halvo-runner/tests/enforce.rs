mod support;

use halvo_schema::executable::frame::{StoreReply, StoreReplyFrame};
use halvo_schema::executable::names::LogEvent;
use halvo_schema::executable::{Cause, FromRunner, Outcome, RunnerReply, ToRunner};
use halvo_schema::Category;

use support::{is_op, Act, FillFiles, Supervisor, LONG};

fn fill(n: u8, files: &FillFiles) -> Supervisor {
    Supervisor::new(n, Category::SpreadsheetFill, files.slots())
}

/// CP-135 (5), A7.4: the handler of `start_run` stores the row 1 of the run's log, so the runner's lines count
/// from 2.
#[test]
fn the_runners_log_lines_count_from_2() {
    let files = FillFiles::new("log-from-2", 1);
    let sup = fill(140, &files);
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    let seqs: Vec<u32> = seen.logs().iter().map(|l| l.seq.get()).collect();
    assert_eq!(seqs, [2, 3], "the read's line, then the write's line");
}

/// The load reply that the supervisor gives with `change` applied to its configuration and slots.
fn load_with(
    sup: &Supervisor,
    frame: &FromRunner,
    change: impl FnOnce(&mut halvo_schema::executable::body::LoadReply),
) -> Act {
    let FromRunner::Store(s) = frame else {
        unreachable!("a load is a store frame")
    };
    let mut reply = sup.store_answer(s.request.body());
    if let RunnerReply::Load(l) = &mut reply {
        change(l);
    }
    Act::With(ToRunner::StoreReply(StoreReplyFrame {
        reply: StoreReply {
            id: Some(s.request.id().clone()),
            outcome: Ok(reply),
        },
    }))
}

/// A6.5, CP-135 (3) (iv): a run whose time is at the `wall_s` of its configuration ends with `limit` and `wall`;
/// whole seconds are not rounded up, so a limit of 0 ends the run at its first read.
#[test]
fn a_run_at_exactly_its_wall_s_ends_with_limit_wall() {
    let files = FillFiles::new("wall-exact", 1);
    let before = files.workbook_bytes();
    let mut sup = fill(141, &files);
    sup.config.limits.wall_s = 0;
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(seen.done(), Some(Outcome::Limit), "{}", seen.stderr);
    assert_eq!(
        seen.logs().last().map(|l| (l.event, l.cause)),
        Some((LogEvent::Stopped, Some(Cause::Wall)))
    );
    assert!(!seen.tags().contains(&"progress"), "no step began");
    assert_eq!(files.workbook_bytes(), before);
}

/// A6.5: the limit `wall` is read against the `wall_s` of the newest load; a load before the edit that lowers it
/// ends the run with `limit` and `wall` at the next read, before any effect row.
#[test]
fn a_wall_s_lowered_at_the_edits_load_ends_the_run_at_the_next_read() {
    let files = FillFiles::new("wall-lowered", 2);
    let before = files.workbook_bytes();
    let sup = fill(142, &files);
    let mut loads = 0;
    let seen = sup.serve(
        |frame, _| {
            if !is_op(frame, "load") {
                return Act::Answer;
            }
            loads += 1;
            if loads < 2 {
                return Act::Answer;
            }
            load_with(&sup, frame, |l| l.config.limits.wall_s = 0)
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Limit), "{}", seen.stderr);
    assert_eq!(
        seen.logs().last().map(|l| (l.event, l.cause)),
        Some((LogEvent::Stopped, Some(Cause::Wall)))
    );
    assert!(seen.effects().is_empty(), "no effect row");
    assert_eq!(files.workbook_bytes(), before);
}
