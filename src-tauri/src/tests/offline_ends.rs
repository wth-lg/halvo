use halvo_control::exit;
use serde_json::json;

use crate::view_state::{offline_view, ActionError, DeleteView, OfflineEnd, OutcomeError};

const LOOKED_IN: &str = "/Volumes/Data/Halvo";

const FAILED_WALK: &str = "deleted segments_shredded=unknown keys_destroyed=unknown root_destroyed=unknown root_id=- files_left=3 database_left=yes not_followed=0 entries_removed=4 entries_left=2 unknown_entries=0 intent=written";

fn exited(code: i32, lines: &[&str]) -> OfflineEnd {
    OfflineEnd::Exited {
        code,
        out: lines.iter().map(|line| format!("{line}\n")).collect(),
    }
}

fn data_dir_line() -> String {
    format!("data_dir={LOOKED_IN}")
}

#[test]
fn the_failed_walk_reads_not_finished_and_an_unusable_folder_removed_nothing() {
    let walk = offline_view(&exited(exit::ERROR, &[&data_dir_line(), FAILED_WALK]), None);
    match &walk {
        Ok(
            view @ DeleteView::Deleted {
                nothing_left: false,
                entries_left: 2,
                files_left: 3,
                database_left: true,
                looked_in: Some(place),
                ..
            },
        ) => {
            assert_eq!(place, LOOKED_IN);
            assert!(!view.finished(), "the failed walk read as finished");
        }
        other => panic!("the failed walk: {other:?}"),
    }

    let unusable = offline_view(&exited(exit::DATA_DIR_UNUSABLE, &[&data_dir_line()]), None);
    assert_eq!(
        unusable,
        Err(OutcomeError {
            error: ActionError::DeleteNotStarted,
            outcome_not_known: false,
        }),
        "an end with 66 and data_dir= alone removed nothing"
    );
    assert_eq!(
        serde_json::to_value(unusable.unwrap_err()).unwrap(),
        json!({ "error": "delete_not_started", "outcome_not_known": false })
    );
}

#[test]
fn an_end_that_tells_nothing_of_what_was_removed_stays_not_known() {
    let data_dir = data_dir_line();
    let removed_nothing: Result<DeleteView, OutcomeError> = Err(OutcomeError {
        error: ActionError::DeleteNotStarted,
        outcome_not_known: false,
    });
    let not_known = |looked_in: Option<&str>| -> Result<DeleteView, OutcomeError> {
        Err(OutcomeError {
            error: ActionError::DeleteNotKnown {
                looked_in: looked_in.map(str::to_owned),
            },
            outcome_not_known: true,
        })
    };
    let cases = [
        (
            "66 and data_dir= alone",
            exited(exit::DATA_DIR_UNUSABLE, &[&data_dir]),
            removed_nothing,
        ),
        (
            "1 and data_dir= alone",
            exited(exit::ERROR, &[&data_dir]),
            not_known(Some(LOOKED_IN)),
        ),
        (
            "66 and a deleted line",
            exited(exit::DATA_DIR_UNUSABLE, &[&data_dir, FAILED_WALK]),
            not_known(Some(LOOKED_IN)),
        ),
        (
            "66 and no_store",
            exited(exit::DATA_DIR_UNUSABLE, &[&data_dir, "no_store"]),
            not_known(Some(LOOKED_IN)),
        ),
        (
            "66 and nothing printed",
            exited(exit::DATA_DIR_UNUSABLE, &[]),
            not_known(None),
        ),
        (
            "66 and a line that does not read",
            exited(
                exit::DATA_DIR_UNUSABLE,
                &[&data_dir, "data_dir is not usable"],
            ),
            not_known(None),
        ),
        (
            "a signal after data_dir=",
            OfflineEnd::Signal {
                out: format!("{data_dir}\n"),
            },
            not_known(Some(LOOKED_IN)),
        ),
        ("no end", OfflineEnd::NoEnd, not_known(None)),
    ];
    for (name, end, reading) in cases {
        assert_eq!(offline_view(&end, None), reading, "{name}");
    }
}
