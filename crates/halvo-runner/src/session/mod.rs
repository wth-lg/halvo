//! The sign-in that a run keeps for the agent's next run (A8.3, A7.4).
//!
//! When the run's browser has started, the frame `browser` has its reply and the rule is armed, and before a step
//! acts, the runner loads every part of the newest stored revision of each origin of the bindings that the load
//! lists, and of no other (`load_session`), and imports the state into the browser.
//!
//! A run stores a site's state anew ONLY where it imported that site's kept sign-in whole at its start, or bound the
//! site at a handback in this run, and no handback cleared it since (the coordinator's ruling of log line 14366). In
//! every other case the listed revision stays as it is and nothing new is stored for that site, so Halvo keeps no
//! cookies of a site at which nobody signed in for that agent. The opening of every handback clears what the pages
//! keep for later, so before it clears them it stores each such site but the one at whose sign-in field it opens, and
//! after it the run stores anew only a site bound at a handback after it (the coordinator's ruling of log line 14407):
//! of two sign-ins in one run both are kept. Before `finish`, also after a run that failed, a run whose browser lives
//! does the same for each such site but the one at whose sign-in field the run ended. Each store exports the browser's
//! cookies of that site's host and of its parent domains and the site's localStorage, and stores them by
//! `put_session` as the next revision of that site: the newer of the listed revision and the one that the run itself
//! took for that site, plus 1, else 1; where the store refuses its part 1 with `binding`, as it does at a number where
//! a revision that a fault cut between its parts left a part, the next number, at most 64 numbers in all (U5.11). The
//! store refuses a revision more than one above the newest stored one of its origin, so the number after every stored
//! part of the site is the highest that a run can take. The state is stored as its JSON form in base64 text, cut into
//! pieces of 1,024 bytes and parts of 48 pieces; a state of more than 64 parts is not stored, the run's log gets one
//! `stopped` line with the cause `session_size` (once a run), and the run's outcome stays.
//!
//! The state passes in the frames `store` and `store_reply` alone: the runner writes no byte of it in a line, an
//! error or another frame.

#[cfg(test)]
mod tests;

use std::num::NonZeroU32;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use halvo_schema::executable::body::{
    DropSession, LoadReply, LoadSession, LoadedSession, PutSession,
};
use halvo_schema::executable::names::LogEvent;
use halvo_schema::executable::record::SESSION_PARTS_MAX;
use halvo_schema::executable::text::{SessionData, OWN_TEXT_MAX, SESSION_PIECES_MAX};
use halvo_schema::executable::{Cause, Outcome, RunnerCall, RunnerReply, WebOrigin};
use halvo_web::cdp::SessionState;

use crate::link::Broken;
use crate::run::{cause_of, Failure, Runner, Stop};
use crate::web::Page;

/// What the run knows of the sign-ins of its origins.
#[derive(Debug, Default)]
pub(crate) struct Sessions {
    /// The sites whose state the run stores anew: each whose kept sign-in it imported whole and each that it bound at
    /// a handback, until the opening of a handback after it stores and clears them.
    keep: Vec<WebOrigin>,
    /// The newest revision of each origin that the load listed.
    listed: Vec<(WebOrigin, NonZeroU32)>,
    /// The newest revision of each origin that the run itself took by `put_session`: one whose part 1 the store took.
    taken: Vec<(WebOrigin, NonZeroU32)>,
    /// Whether the run's log has its `stopped` line with the cause `session_size`.
    sized: bool,
    /// Whether the run imported the state of an origin.
    pub(crate) imported: bool,
    /// The origin of the page on which the reading found a credential field, until a handback of it ended in a bind.
    pub(crate) at_field: Option<WebOrigin>,
}

impl Sessions {
    /// Notes that the run stores `origin` anew: at the next handback's opening, or where none opens, at the run's end.
    /// An opening at a field of `origin` itself does not store it (`Runner::store_kept`).
    pub(crate) fn keep(&mut self, origin: &WebOrigin) {
        if !self.keep.contains(origin) {
            self.keep.push(origin.clone());
        }
    }

    /// The revision that the run stores for `origin`: the newer of the listed one and the one that the run took, plus
    /// 1, else 1.
    fn next(&self, origin: &WebOrigin) -> Option<NonZeroU32> {
        let newest = self
            .listed
            .iter()
            .chain(&self.taken)
            .filter(|(o, _)| o == origin)
            .map(|(_, rev)| *rev)
            .max();
        match newest {
            Some(rev) => rev.checked_add(1),
            None => Some(NonZeroU32::MIN),
        }
    }

    /// Notes that the run took `rev` for `origin`.
    fn took(&mut self, origin: &WebOrigin, rev: NonZeroU32) {
        self.taken.retain(|(o, _)| o != origin);
        self.taken.push((origin.clone(), rev));
    }
}

/// A state whose parts would be more than 64.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct TooLarge;

/// The count of numbers that a run tries at most for one revision of a site: the first, and each next one after the
/// store refused part 1 at the number before with `binding`.
const NUMBERS_MAX: u32 = 64;

/// The parts of `state` as `put_session` stores them: its JSON form in base64 text, cut into pieces of 1,024 bytes,
/// 48 pieces a part; `TooLarge` for more than 64 parts.
pub(crate) fn parts_of(state: &SessionState) -> Result<Vec<SessionData>, TooLarge> {
    let text = STANDARD.encode(state.to_json());
    let pieces: Vec<String> = text
        .as_bytes()
        .chunks(OWN_TEXT_MAX)
        .map(|piece| piece.iter().map(|&b| char::from(b)).collect())
        .collect();
    if pieces.len().div_ceil(SESSION_PIECES_MAX) > SESSION_PARTS_MAX as usize {
        return Err(TooLarge);
    }
    pieces
        .chunks(SESSION_PIECES_MAX)
        .map(|part| SessionData::new(part.to_vec()).map_err(|_| TooLarge))
        .collect()
}

/// The state of the joined pieces of a revision, none where the text is not the form of `parts_of`.
fn state_of(text: &str) -> Option<SessionState> {
    let bytes = STANDARD.decode(text).ok()?;
    SessionState::from_json(std::str::from_utf8(&bytes).ok()?).ok()
}

impl Runner {
    /// Loads and imports, once the run's browser runs, the newest revision of each origin of the bindings of
    /// `loaded` that the load lists, and of no other, and notes each origin imported whole as one to store anew. A
    /// store's refusal of a part ends the run as a refused load does; a revision whose text does not read is imported
    /// nothing of; a fault of the executor fails the step, and no origin from it on is stored anew.
    pub(crate) fn import_sessions(&mut self, loaded: &LoadReply) -> Result<(), Failure> {
        let origins = loaded
            .bindings
            .as_ref()
            .map_or_else(Vec::new, |b| b.origins.clone());
        self.sessions.listed = loaded
            .sessions
            .iter()
            .map(|s| (s.origin.clone(), s.rev))
            .collect();
        for listed in loaded
            .sessions
            .iter()
            .filter(|s| origins.contains(&s.origin))
        {
            let Some(state) = self.load_revision(listed)?.as_deref().and_then(state_of) else {
                continue;
            };
            let web = self.web.as_mut().ok_or(Failure::Step(Cause::StepFailed))?;
            web.0
                .import(&state)
                .map_err(|()| Failure::Step(Cause::StepFailed))?;
            self.sessions.imported = true;
            self.sessions.keep(&listed.origin);
        }
        Ok(())
    }

    /// The joined pieces of every part of the revision `listed`, none where a reply names another part.
    fn load_revision(&mut self, listed: &LoadedSession) -> Result<Option<String>, Stop> {
        let mut text = String::new();
        for part in 1..=listed.parts.get() {
            let part = NonZeroU32::new(part).unwrap_or(NonZeroU32::MIN);
            let call = RunnerCall::LoadSession(LoadSession {
                agent: self.args.agent.clone(),
                run: self.args.run.clone(),
                origin: listed.origin.clone(),
                rev: listed.rev,
                part,
            });
            match self.store(call)? {
                Ok(RunnerReply::Session(reply)) => {
                    if reply.origin != listed.origin
                        || reply.rev != listed.rev
                        || reply.part != part
                        || reply.of != listed.parts
                    {
                        return Ok(None);
                    }
                    if reply.data.is_dropped() {
                        return Ok(None);
                    }
                    reply.data.pieces().iter().for_each(|p| text.push_str(p));
                }
                Ok(_) => return Err(Broken::Frame("not_the_answer").into()),
                Err(error) => return Err(Stop::End(Outcome::Failed, Some(cause_of(error)))),
            }
        }
        Ok(Some(text))
    }

    /// The end of a run with no window whose reading met a credential field after the run imported a kept sign-in:
    /// the step's `step_failed` line with the cause `unauthorized`, which the roster's `refused_run` reads, then
    /// `drop_session` for the origin of the page, where it has one, so that the store serves that origin's kept
    /// sign-in no more and the roster's `session` reads `no`; the run ends `failed` with the cause `step_failed`. No
    /// handback opens and no browser starts again. A store's refusal of the line or of the drop changes nothing of it.
    pub(crate) fn no_window(&mut self, step: NonZeroU32, origin: Option<WebOrigin>) -> Stop {
        if let Err(Stop::Link(broken)) =
            self.log(LogEvent::StepFailed, Some(step), Some(Cause::Unauthorized))
        {
            return Stop::Link(broken);
        }
        if let Some(origin) = origin {
            let call = RunnerCall::DropSession(DropSession {
                agent: self.args.agent.clone(),
                run: self.args.run.clone(),
                origin,
            });
            if let Err(broken) = self.store(call) {
                return Stop::Link(broken);
            }
        }
        Stop::End(Outcome::Failed, Some(Cause::StepFailed))
    }

    /// Stores, when the run's browser lives, the state of each site that the run stores anew (`Sessions::keep`) but the
    /// one at whose credential field the run ended, as `store_kept` does; every other site's listed revision stays as
    /// it is.
    pub(crate) fn keep_sessions(&mut self) {
        let Some(mut web) = self.web.take() else {
            return;
        };
        let _ = self.store_kept(&mut *web.0);
        self.web = Some(web);
    }

    /// The step of a handback's opening before it clears what the pages keep for later (crate::handback, the
    /// coordinator's ruling of log line 14407): stores through `page`, the run's browser, the state of each site that
    /// the run stores anew so far but the one at whose credential field the handback opens, as `store_kept` does, then
    /// notes that no site is stored anew unless a handback after it binds it. A link that broke stops it; no other
    /// fault keeps the handback from opening.
    pub(crate) fn keep_before_clear(&mut self, page: &mut dyn Page) -> Result<(), Broken> {
        let kept = self.store_kept(page);
        self.sessions.keep.clear();
        kept
    }

    /// Stores, through `page`, the state of each site of `Sessions::keep` but the one of `Sessions::at_field`, as the
    /// next revision of that site. An export that fails and a part that the store refuses store nothing more of that
    /// site; a state over the limit is not stored, and the run's log gets one `stopped` line with the cause
    /// `session_size`, once a run, whose refusal by the store changes nothing. A link that broke stops it.
    fn store_kept(&mut self, page: &mut dyn Page) -> Result<(), Broken> {
        let mut origins = self.sessions.keep.clone();
        origins.retain(|o| self.sessions.at_field.as_ref() != Some(o));
        let mut too_large = false;
        for origin in &origins {
            let Ok(state) = page.export(origin) else {
                continue;
            };
            match parts_of(&state) {
                Ok(parts) => self.put_revision(origin, parts)?,
                Err(TooLarge) => too_large = true,
            }
        }
        if too_large && !self.sessions.sized {
            self.sessions.sized = true;
            if let Err(Stop::Link(broken)) =
                self.log(LogEvent::Stopped, None, Some(Cause::SessionSize))
            {
                return Err(broken);
            }
        }
        Ok(())
    }

    /// Stores `parts` as the next revision of `origin` (`Sessions::next`), part by part, until the store refuses one.
    /// Where the store refuses part 1 with `binding`, as it does at a number where a revision that a fault cut between
    /// its parts left a part, the run takes the next number and stores from part 1 again, at most `NUMBERS_MAX`
    /// numbers in all.
    fn put_revision(&mut self, origin: &WebOrigin, parts: Vec<SessionData>) -> Result<(), Broken> {
        let Some(of) = u32::try_from(parts.len()).ok().and_then(NonZeroU32::new) else {
            return Ok(());
        };
        let mut rev = self.sessions.next(origin);
        for _ in 0..NUMBERS_MAX {
            let Some(at) = rev else {
                break;
            };
            if !self.put_parts(origin, at, of, &parts)? {
                break;
            }
            rev = at.checked_add(1);
        }
        Ok(())
    }

    /// Stores `parts` as revision `rev` of `origin`, part by part, until the store refuses one; the run takes `rev` for
    /// `origin` once the store took part 1. It answers whether the store refused part 1 with `binding`.
    fn put_parts(
        &mut self,
        origin: &WebOrigin,
        rev: NonZeroU32,
        of: NonZeroU32,
        parts: &[SessionData],
    ) -> Result<bool, Broken> {
        for (part, data) in (1..).filter_map(NonZeroU32::new).zip(parts) {
            let call = RunnerCall::PutSession(PutSession {
                agent: self.args.agent.clone(),
                run: self.args.run.clone(),
                origin: origin.clone(),
                rev,
                part,
                of,
                data: data.clone(),
            });
            match self.store(call)? {
                Ok(_) if part == NonZeroU32::MIN => self.sessions.took(origin, rev),
                Ok(_) => {}
                Err(error) => return Ok(part == NonZeroU32::MIN && error.as_str() == "binding"),
            }
        }
        Ok(false)
    }
}
