mod support;

use halvo_runner::LINK_END;
use halvo_schema::executable::{EffectState, FromRunner};
use halvo_schema::Category;

use support::{is_effect, is_op, Act, FillFiles, Seen, Supervisor, LONG};

fn ended_by_the_close(seen: &Seen, what: &str) {
    assert_eq!(
        seen.code(),
        Some(1),
        "{what}: status 1; stderr: {}",
        seen.stderr
    );
    let took = seen.close_to_end.expect("the runner ended after the close");
    assert!(
        took < LINK_END,
        "{what}: gone within LINK_END, took {took:?}"
    );
    assert_eq!(
        seen.group_gone,
        Some(true),
        "{what}: no process of its group is left"
    );
    assert!(seen.done().is_none(), "{what}: no done after the close");
    assert!(seen.finish().is_none(), "{what}: no finish after the close");
}

#[test]
fn link_end_is_twenty_seconds() {
    assert_eq!(LINK_END.as_secs(), 20);
}

#[test]
fn closed_while_the_runner_waits_for_a_store_reply() {
    let files = FillFiles::new("close-load", 1);
    let before = files.workbook_bytes();
    let sup = Supervisor::new(21, Category::SpreadsheetFill, files.slots());
    let seen = sup.serve(
        |frame, _| {
            if is_op(frame, "load") {
                Act::Close
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    ended_by_the_close(&seen, "store reply");
    assert!(matches!(seen.frames.last(), Some(FromRunner::Store(_))));
    assert_eq!(files.workbook_bytes(), before, "the workbook is untouched");
}

fn checkpoint_and_workbook(sup: &Supervisor) -> Vec<String> {
    vec![
        format!("formulas.halvo-checkpoint-{}.xlsx", sup.run.as_str()),
        "formulas.xlsx".into(),
        "source.csv".into(),
    ]
}

#[test]
fn closed_while_the_runner_holds_its_edit_paused() {
    for (what, state) in [
        ("propose", EffectState::Cleared),
        ("begin", EffectState::Releasing),
    ] {
        let files = FillFiles::new("close-paused", 2);
        let before = files.workbook_bytes();
        let sup = Supervisor::new(22, Category::SpreadsheetFill, files.slots());
        let seen = sup.serve(
            |frame, _| {
                if is_effect(frame, state) {
                    Act::Close
                } else {
                    Act::Answer
                }
            },
            LONG,
        );
        ended_by_the_close(&seen, what);
        assert_eq!(
            files.workbook_bytes(),
            before,
            "{what}: no byte of the paused edit reached the workbook"
        );
        assert_eq!(
            files.names(),
            checkpoint_and_workbook(&sup),
            "{what}: the prepared copy is gone and the checkpoint stays"
        );
    }
}

#[test]
fn closed_while_a_handback_is_open() {
    let files = FillFiles::new("close-handback", 3);
    std::fs::write(&files.source, "a,b\r\n").expect("a source that fails the read");
    let sup = Supervisor::new(23, Category::SpreadsheetFill, files.slots());
    let seen = sup.serve(
        |frame, _| {
            if matches!(frame, FromRunner::Handback(_)) {
                Act::Close
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    ended_by_the_close(&seen, "handback");
    assert!(matches!(seen.frames.last(), Some(FromRunner::Handback(_))));
}

#[test]
fn closed_after_the_begin_of_the_edit_was_acknowledged() {
    let files = FillFiles::new("close-after-begin", 1);
    let before = files.workbook_bytes();
    let sup = Supervisor::new(24, Category::SpreadsheetFill, files.slots());
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Releasing) {
                Act::CloseReadThenAnswer
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    ended_by_the_close(&seen, "after begin");
    assert!(
        !seen.states().contains(&EffectState::Committed),
        "no commit is written after the close"
    );
    let after = files.workbook_bytes();
    let whole = after == before || support::range_texts(&files.dir, &files.workbook) == files.rows;
    assert!(
        whole,
        "the workbook is the original or the whole edit, never a part"
    );
    assert_eq!(files.names(), checkpoint_and_workbook(&sup));
}

#[test]
fn closed_while_the_runner_waits_for_a_secret_reply() {
    let mut sup = Supervisor::new(25, Category::Email, Vec::new());
    sup.origins = vec![
        halvo_schema::executable::WebOrigin::parse("http://mail.agent.test:8080")
            .expect("an origin"),
    ];
    let seen = sup.serve(
        |frame, _| {
            if matches!(frame, FromRunner::Secret(_)) {
                Act::Close
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    ended_by_the_close(&seen, "secret reply");
    assert!(matches!(seen.frames.last(), Some(FromRunner::Secret(_))));
}

#[test]
fn closed_by_a_write_error_on_standard_output_alone() {
    let files = FillFiles::new("close-write", 1);
    let before = files.workbook_bytes();
    let sup = Supervisor::new(26, Category::SpreadsheetFill, files.slots());
    let seen = sup.serve(
        |frame, _| {
            if is_op(frame, "load") {
                Act::CloseReadKeepInput
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    ended_by_the_close(&seen, "write error");
    assert_eq!(files.workbook_bytes(), before);
}
