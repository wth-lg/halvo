use std::cmp::Ordering;

use crate::K_CLEAN;

tokens! {
    /// The state of an edit (L3.7). `candidate` is held in memory until its run ends and
    /// `refused` is final; neither is stored.
    pub enum EditState {
        None = "none",
        Candidate = "candidate",
        Provisional = "provisional",
        Active = "active",
        Reverted = "reverted",
        Withdrawn = "withdrawn",
        Refused = "refused",
    }
}

tokens! {
    /// A move of an edit (L3.7).
    pub enum EditMove {
        ProposeOk = "propose_ok",
        ProposeRefused = "propose_refused",
        Keep = "keep",
        Drop = "drop",
        UseOk = "use_ok",
        UseFailCounted = "use_fail_counted",
        UseFailOther = "use_fail_other",
        Recompile = "recompile",
    }
}

/// The answer of [`edit_step`]: the next state with the next `clean`, or `edit` when no row
/// of L3.7 holds, which leaves the state as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditAnswer {
    /// The next state and the next `clean`.
    Next {
        /// The next state.
        state: EditState,
        /// The next count of clean uses.
        clean: u32,
    },
    /// No row holds for the state, the move and `clean`.
    Edit,
}

/// The move of an edit that was served to a run (L3.7), the first of these that holds:
/// `use_fail_counted` when the run's log holds a counted `step_failed` line of the edit's
/// step, whatever the run's outcome; `use_ok` when the run is verified and its log holds
/// `step_ok` for the step; `use_fail_other` when the log holds any line of the step. `None`
/// when the log holds no line of the step. Each flag reads the lines of the edit's own step
/// alone.
pub fn use_move(
    counted_line: bool,
    verified: bool,
    step_ok: bool,
    any_line: bool,
) -> Option<EditMove> {
    if counted_line {
        Some(EditMove::UseFailCounted)
    } else if verified && step_ok {
        Some(EditMove::UseOk)
    } else if any_line {
        Some(EditMove::UseFailOther)
    } else {
        None
    }
}

/// One step of an edit by the 57 rows of L3.7: the next state and the next `clean` for a
/// state, a move and the `clean` passed in. `provisional` with `use_ok` takes the guard
/// `clean_below` when `clean` + 1 is below [`K_CLEAN`], and `clean_reaches` when it equals
/// it; above it, and for every pair that no row names, the answer is `edit`. For an edit in
/// a stored state, the caller stores a new revision exactly when the next state or the next
/// `clean` differs from what it passed in.
pub fn edit_step(state: EditState, edit_move: EditMove, clean: u32) -> EditAnswer {
    use EditMove as M;
    use EditState as S;
    let next = |state| EditAnswer::Next { state, clean };
    match (state, edit_move) {
        (S::None, M::ProposeOk) => next(S::Candidate),
        (S::None, M::ProposeRefused) => next(S::Refused),
        (S::Candidate, M::Keep) => EditAnswer::Next {
            state: S::Provisional,
            clean: 0,
        },
        (S::Candidate, M::Drop) => next(S::Refused),
        (S::Provisional, M::UseOk) => match clean.cmp(&(K_CLEAN - 1)) {
            Ordering::Less => EditAnswer::Next {
                state: S::Provisional,
                clean: clean + 1,
            },
            Ordering::Equal => EditAnswer::Next {
                state: S::Active,
                clean: K_CLEAN,
            },
            Ordering::Greater => EditAnswer::Edit,
        },
        (S::Provisional, M::UseFailCounted) => next(S::Reverted),
        (S::Provisional, M::UseFailOther) => next(S::Provisional),
        (S::Active, M::UseOk | M::UseFailCounted | M::UseFailOther) => next(S::Active),
        (S::Provisional | S::Active | S::Reverted, M::Recompile) => next(S::Withdrawn),
        _ => EditAnswer::Edit,
    }
}
