use std::num::NonZeroU32;

use halvo_learn::{edit_step, EditAnswer, EditCause, EditMove};
use halvo_schema::executable::RunId;
use halvo_schema::learning::record::{Day, EditState, LearnEdit};
use halvo_schema::{AgentId, ErrorName};

use crate::convert;
use crate::rows::Edits;

/// The next revision of an edit for a move by the table of L3.7, or none where the table answers `edit` or gives
/// the same state and `clean`. A `use_ok` adds the run with its day to the source set; a revert stores the
/// counted cause.
pub(crate) fn moved(
    edit: &LearnEdit,
    edit_move: EditMove,
    run: Option<(&RunId, Day)>,
    cause: Option<EditCause>,
) -> Result<Option<LearnEdit>, ErrorName> {
    let from = convert::learn_state(edit.state);
    let EditAnswer::Next { state, clean } = edit_step(from, edit_move, edit.clean) else {
        return Ok(None);
    };
    if state == from && clean == edit.clean {
        return Ok(None);
    }
    let state = convert::stored_state(state, cause).ok_or(ErrorName::Internal)?;
    let mut next = edit.clone();
    next.rev = edit.rev.checked_add(1).ok_or(ErrorName::Internal)?;
    next.state = state;
    next.clean = clean;
    if edit_move == EditMove::UseOk {
        let (run, day) = run.ok_or(ErrorName::Internal)?;
        next.runs.push(run.clone());
        next.first = next.first.min(day);
        next.last = next.last.max(day);
    }
    Ok(Some(next))
}

/// The move `recompile` for every edit of the agent whose `base` is below the newest compile number and that is
/// not `withdrawn` yet (contracts.md L4.2, Q9), applied to `edits` so that the reads after it see the rows.
pub(crate) fn withdrawals(
    edits: &mut Edits,
    agent: &AgentId,
    newest: NonZeroU32,
) -> Result<Vec<LearnEdit>, ErrorName> {
    let lower: Vec<LearnEdit> = edits
        .of_agent(agent)
        .filter(|e| e.base < newest && e.state != EditState::Withdrawn)
        .cloned()
        .collect();
    let mut out = Vec::new();
    for edit in lower {
        if let Some(next) = moved(&edit, EditMove::Recompile, None, None)? {
            edits.apply(next.clone());
            out.push(next);
        }
    }
    Ok(out)
}
