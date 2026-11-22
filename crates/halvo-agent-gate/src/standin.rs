//! The stand-in of the gate (contracts.md A8.5): the `Human` and the `Confirmer` that the gate gives the supervisor
//! library in place of a person. It answers what the policy asks and nothing else, and it writes no row.
//!
//! A confirmation: each effect of the request is answered `allow_once` when the gate host's store holds that effect
//! as the run proposed it, its newest row `held` with the request's class and digest, and `reject_once` otherwise, so
//! it never allows an effect that the run did not propose. `allow_origin` is answered `allow` (A8.5). A handback
//! goes to the command, which answers it with `unscripted`, `declined`: this pull request scripts no handback (the
//! sign-in script of A8.5 is the runner's feature `standin`, and `account_link` comes with the second pull request
//! of U4.11). `progress` is not shown.
//!
//! The gate host is no `Send` value, so it stays on the command's thread: the stand-in hands each request to that
//! thread and waits for its answer there (`crate::task::Event`).

use std::num::NonZeroU32;
use std::sync::mpsc::{self, Sender};

use halvo_agent_client::{Confirmer, Human, IssueRefused, OriginAnswer};
use halvo_schema::executable::frame::{ConfirmDecision, ConfirmEffect, ConfirmRequest};
use halvo_schema::executable::names::{Answer, HandbackOutcome};
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{
    AgentEffect, EffectId, EffectState, HandbackRequest, HandbackResult, RunId, WebOrigin,
};
use halvo_schema::RowRead;

use crate::task::Event;

/// The stand-in, with the channel to the command's thread, which holds the gate host.
pub(crate) struct Standin {
    events: Sender<Event>,
}

impl Standin {
    pub(crate) fn new(events: Sender<Event>) -> Standin {
        Standin { events }
    }
}

/// The answers to a confirmation of `run`, read from the gate host's store: `allow_once` for an effect that the run
/// proposed and that waits for the answer, `reject_once` for every other.
pub(crate) fn answers(
    rows: &dyn RowRead,
    run: &RunId,
    request: &ConfirmRequest,
) -> Vec<(EffectId, Answer)> {
    request
        .effects
        .iter()
        .map(|effect| {
            let answer = if proposed(rows, run, effect) {
                Answer::AllowOnce
            } else {
                Answer::RejectOnce
            };
            (effect.effect.clone(), answer)
        })
        .collect()
}

/// The answer to a handback that the stand-in has no script for: `declined`, with no secret.
pub(crate) fn unscripted() -> HandbackResult {
    HandbackResult {
        v: V1,
        outcome: HandbackOutcome::Declined,
        secret: None,
    }
}

impl Confirmer for Standin {
    /// The answers of the command's thread; none when that thread is gone, which makes the library reject every
    /// effect of the request.
    fn answers(&self, run: &RunId, request: &ConfirmRequest) -> Vec<(EffectId, Answer)> {
        let (tx, rx) = mpsc::channel();
        if self
            .events
            .send(Event::Confirm(run.clone(), request.clone(), tx))
            .is_err()
        {
            return Vec::new();
        }
        rx.recv().unwrap_or_default()
    }

    fn issued(&self, _run: &RunId, _decision: &ConfirmDecision) -> Result<(), IssueRefused> {
        Ok(())
    }
}

impl Human for Standin {
    fn handback(&self, run: &RunId, _request: &HandbackRequest) {
        let _ = self.events.send(Event::Handback(run.clone()));
    }

    fn allow_origin(&self, _run: &RunId, _origin: &WebOrigin) -> OriginAnswer {
        OriginAnswer::Allow
    }

    fn progress(&self, _run: &RunId, _step: NonZeroU32, _of: NonZeroU32) {}
}

/// Whether the store holds `asked` as the run proposed it: the newest row of its effect id in the run is `held`, of
/// the request's class and digest.
fn proposed(rows: &dyn RowRead, run: &RunId, asked: &ConfirmEffect) -> bool {
    let mut newest: Option<AgentEffect> = None;
    rows.for_each("agent_effect", &mut |row| {
        let Ok(effect) = AgentEffect::parse(row.as_bytes()) else {
            return;
        };
        let later = newest.as_ref().is_none_or(|n| effect.seq > n.seq);
        if &effect.run == run && effect.effect == asked.effect && later {
            newest = Some(effect);
        }
    });
    newest.is_some_and(|e| {
        e.state == EffectState::Held && e.class == asked.class && e.digest == asked.digest
    })
}
