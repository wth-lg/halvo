mod common;

use common::*;
use halvo_coord_store::{coordinator, CommitRefused};
use halvo_schema::{ErrorName, RowRead, SummaryRead, TypedRecord, WriterStep};

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
