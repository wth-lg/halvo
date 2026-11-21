//! The removal of checkpoints, the copies that an edit leaves beside a person's workbook (contracts.md A6.6, A11.3;
//! the coordinator's rulings CP-96 (c), (d) and CP-98 (3), (4)). The library is the one party that removes one: it
//! reads the list, removes each copy that may go by the one call of halvo-checkpoint, which builds the copy's name
//! and works inside the row's folder alone, and writes the answer with `mark_checkpoint`. It builds no name and makes
//! no other file call in a person's folder.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use halvo_checkpoint::{Expect, Left, Locks, Removal, Row, Sha256};
use halvo_schema::executable::body::{CoordinatorCall, ListedCheckpoint, MarkCheckpoint};
use halvo_schema::executable::names::{EffectState, LeftReason, ListedState, MarkState, YesNo};
use halvo_schema::executable::text::RunId;

use crate::api::ListFailed;
use crate::owner;
use crate::supervisor::Inner;

/// The age by its row's `at` at which a checkpoint goes whatever its run's effect, 30 days (A11.3).
const AGED_NANOS: i128 = 30 * 24 * 60 * 60 * 1_000_000_000;

/// How long a removal waits for the lock of a workbook's directory, the wait of the runner's sheet executor.
const LOCK_WAIT: Duration = Duration::from_secs(10);

/// One pass over every page of the list: each row that may go is asked, and each answer that decides is written
/// with `mark_checkpoint`. A `ListFailed` ends the pass after the rows of the pages before it; so does a page whose
/// `next` does not come after the `after` that it was asked with. While the lock directory of an absolute data
/// directory cannot be opened, no row is asked and none is written, as for the lock of an undecided answer.
pub(crate) fn pass(inner: &Inner) -> Result<(), ListFailed> {
    let mut locks = None;
    let mut after = None;
    loop {
        let page = owner::checkpoints(inner, after.clone())?;
        for row in &page.checkpoints {
            let Some(expect) = asked(row, SystemTime::now()) else {
                continue;
            };
            if locks.is_none() && inner.config.data_dir.is_absolute() {
                locks = Locks::open(&inner.config.data_dir, LOCK_WAIT).ok();
            }
            let Some(locks) = &locks else {
                continue;
            };
            if let Some((state, left)) = removed(locks, row, expect) {
                mark(inner, row, state, left);
            }
        }
        match page.next {
            Some(next) if after.as_ref().is_none_or(|before: &RunId| next > *before) => {
                after = Some(next);
            }
            _ => return Ok(()),
        }
    }
}

/// What the removal of `row` is asked to check, or none when the row is not asked: only a row whose run's end is
/// stored and whose effect is not `unknown`, and then when it is `superseded`, or 30 days old by its `at` and the
/// system clock, or its run's effect on that workbook is null or none of `committed`, `releasing` and `unknown`. A
/// `made` row is asked with its SHA-256 and length; a `planned` row against the workbook as it stands.
fn asked(row: &ListedCheckpoint, now: SystemTime) -> Option<Expect> {
    if row.ended != YesNo::Yes || row.effect == Some(EffectState::Unknown) {
        return None;
    }
    let replaced = matches!(
        row.effect,
        Some(EffectState::Committed | EffectState::Releasing | EffectState::Unknown)
    );
    if row.superseded != YesNo::Yes && !aged(row.at.0, now) && replaced {
        return None;
    }
    match row.state {
        ListedState::Made => Some(Expect::Made {
            sha256: Sha256::parse(row.sha256.as_ref()?.as_str())?,
            bytes: u64::from(row.bytes?),
        }),
        ListedState::Planned => Some(Expect::Planned),
    }
}

/// Whether the instant `at`, in nanoseconds since 1970, is 30 days old or more at `now`.
fn aged(at: i64, now: SystemTime) -> bool {
    let Ok(since) = now.duration_since(UNIX_EPOCH) else {
        return false;
    };
    i128::try_from(since.as_nanos()).is_ok_and(|now| now - i128::from(at) >= AGED_NANOS)
}

/// The removal of the row's copy, as the state and the reason that `mark_checkpoint` writes; none for an undecided
/// answer or a run id that halvo-checkpoint does not take, which writes no row.
fn removed(
    locks: &Locks,
    row: &ListedCheckpoint,
    expect: Expect,
) -> Option<(MarkState, Option<LeftReason>)> {
    let run = halvo_checkpoint::RunId::parse(row.run.as_str())?;
    let folder = PathBuf::from(row.folder.joined());
    let workbook = PathBuf::from(row.workbook.joined());
    let asked = Row {
        folder: &folder,
        workbook: &workbook,
        run: &run,
        expect,
    };
    match halvo_checkpoint::remove(locks, &asked) {
        Ok(Removal::Removed | Removal::Absent) => Some((MarkState::Gone, None)),
        Ok(Removal::Left(why) | Removal::LeftAside(why)) => {
            Some((MarkState::Left, Some(reason(why))))
        }
        Err(_) => None,
    }
}

fn reason(why: Left) -> LeftReason {
    match why {
        Left::Changed => LeftReason::Changed,
        Left::Link => LeftReason::Link,
        Left::NotFile => LeftReason::NotFile,
        Left::Name => LeftReason::Name,
        Left::Unreadable => LeftReason::Unreadable,
    }
}

/// Sends the row's `mark_checkpoint` once; a mark that gets no acknowledgement is written by a later pass, which
/// finds a removed copy absent.
fn mark(inner: &Inner, row: &ListedCheckpoint, state: MarkState, left: Option<LeftReason>) {
    let Some(id) = owner::request_id() else {
        return;
    };
    let call = CoordinatorCall::MarkCheckpoint(MarkCheckpoint {
        workflow: row.workflow.clone(),
        agent: row.agent.clone(),
        run: row.run.clone(),
        workbook: row.workbook.clone(),
        state,
        left,
    });
    let _ = owner::acknowledged(inner, &id, call);
}
