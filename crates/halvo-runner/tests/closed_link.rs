mod support;

use halvo_runner::LINK_END;
use halvo_schema::executable::FromRunner;
use halvo_schema::Category;

use support::{is_op, Act, FillFiles, Seen, Supervisor, LONG};

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
