use std::collections::BTreeSet;

use halvo_schema::learning::Refusal;
use halvo_schema::{RowRead, WorkflowId};

use crate::rows;

pub(super) fn read<T>(
    rows: &dyn RowRead,
    kind: &str,
    parse: fn(&[u8]) -> Result<T, Refusal>,
    stopped: &mut BTreeSet<WorkflowId>,
) -> Option<Vec<T>> {
    let mut out = Vec::new();
    let mut every = false;
    rows.for_each(kind, &mut |row| match parse(row.as_bytes()) {
        Ok(value) => out.push(value),
        Err(_) => match rows::refused_workflow(kind, row) {
            Some(workflow) => {
                stopped.insert(workflow);
            }
            None => every = true,
        },
    });
    if every {
        return None;
    }
    Some(out)
}
