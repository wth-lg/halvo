mod common;

use common::*;
use halvo_coord_store::{coordinator, CommitRefused, OPS};
use halvo_schema::coordinator::body::{CoordHistory, CoordLog};
use halvo_schema::{ErrorName, RowRead, SummaryRead, TypedRecord, WriterStep};

#[test]
fn ops_names_the_two_bodies() {
    assert_eq!(OPS, ["coord_log", "coord_history"]);
    assert!(OPS.contains(&CoordLog::OP));
    assert!(OPS.contains(&CoordHistory::OP));
}

#[test]
fn a_further_role_with_a_history_body_is_role() {
    let mut store = store();
    assert_eq!(
        coordinator(
            &mut store,
            &request("probe_role", &history_body(1, 1, None))
        ),
        Err(ErrorName::Role)
    );
}

struct Returns(Vec<TypedRecord>);

impl WriterStep for Returns {
    fn after_commit(&self, _: &dyn SummaryRead, _: &dyn RowRead) -> Vec<TypedRecord> {
        self.0.clone()
    }
}

#[test]
fn coord_roles() {
    let mut analysis = store().with_step(Box::new(Returns(vec![typed(row(1, TODAY))])));
    assert_eq!(analysis.after_batch(), Err(CommitRefused::OwnFromStep));
    assert!(rows(&analysis).is_empty());

    let body = log_body(1, 1, &[row(1, TODAY)]);
    let mut store = store();
    for name in ["runner", "probe_role"] {
        assert_eq!(
            coordinator(&mut store, &request(name, &body)),
            Err(ErrorName::Role),
            "{name}"
        );
        assert!(rows(&store).is_empty(), "{name}");
    }
    assert_eq!(
        coordinator(&mut store, &request("coordinator_write", &body)),
        stored(1)
    );
    assert_eq!(rows(&store), [row(1, TODAY)]);
}
