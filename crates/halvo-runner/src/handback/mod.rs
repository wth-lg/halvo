//! The handback at a credential field (A6.4, A8.1, A8.2): before a web step acts, and at a step of the verb
//! `handback`, the runner reads the page, and where the page shows a credential field it gives the browser to the
//! person and acts on nothing until the person is done.
//!
//! The reason of the handback follows the field: `captcha` for a place inside a challenge block, `second_factor`
//! for a field that takes a one-time code, else `sign_in`, and the request carries the origin of the top document,
//! scheme, host and port alone, which the runner reads once before the opening, null where the top document has no
//! origin of http or https (the coordinator's ruling of log line 13906). Before the handback opens the runner, in
//! this order,
//! fails every request that the rule holds paused, closes every other page of the run, clears for every origin that
//! the run has loaded what a page keeps for later but the cookies, and loads the top document's address again by a
//! GET navigation; then it arms the handback's mode, in which the rule lets every request of the person's pages
//! through and the relay reaches every host and port. While the handback is open the runner sends the browser no
//! command and reads nothing. After `done` it reads the page again and hands back again while a credential field is
//! still there, at most `IN_A_ROW` times in a row. After the `done` that leaves no credential field it reads once
//! the address of the page on which the handback opened and sends its origin in the frame `bind`; after
//! `bind_reply` with `bound=yes` it first arms `Mode::Fail` with the origins of the bindings and every origin bound
//! in this run, then closes the pages that were opened meanwhile, so that no page loads outside the rule (the
//! coordinator's ruling of log line 14127 (3), (4)), and the run goes on. Since the opening clears what the pages of
//! every site keep for later, the run then stores anew at its end only a site bound at a handback after it
//! (crate::session, the coordinator's ruling of log line 14366).
//!
//! Every other end binds nothing and makes no further step: the runner first lets no request through (the rule in
//! `Mode::Fail` with no origin, the relay with no host), fails every request that it holds paused and closes the
//! pages that were opened while the handback was open (`shut`). `declined` ends the run `declined` with the cause
//! `declined`, as `bound=no` does; `timeout` ends it `handed_back` with the cause `timeout`; a credential field after
//! the `IN_A_ROW`-th `done`, a reading that fails after a `done`, and an address that gives no origin end it
//! `handed_back`.

#[cfg(test)]
mod tests;

use std::num::NonZeroU32;

use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::frame::{Bind, BindReply};
use halvo_schema::executable::names::{HandbackOutcome, HandbackReason, YesNo};
use halvo_schema::executable::{Cause, FromRunner, Outcome, ToRunner, WebOrigin};
use halvo_web::cdp::Sign;

use crate::link::Broken;
use crate::run::{Failure, Runner, Stop};
use crate::web::Page;

/// How many handbacks the runner opens in a row while the page still shows a credential field after each `done`.
pub(crate) const IN_A_ROW: u32 = 3;

/// The reason of a handback at a field of the sign `sign` (A6.4).
pub(crate) fn reason_of(sign: Sign) -> HandbackReason {
    match sign {
        Sign::Challenge => HandbackReason::Captcha,
        Sign::Code => HandbackReason::SecondFactor,
        Sign::Secret => HandbackReason::SignIn,
    }
}

/// The end of a run whose handback ended with no bind: `outcome` with `cause`.
fn end(outcome: Outcome, cause: Option<Cause>) -> Stop {
    Stop::End(outcome, cause)
}

impl Runner {
    /// The reading of the page before a web step acts: false where the page shows no credential field; true after
    /// a handback that ended in a bind with `bound=yes`; every other end of a handback stops the run. A reading that
    /// fails, a read of the address before the opening that fails, or a step of the opening that fails, fails the
    /// step with `step_failed` before the handback opens. In a run with no window that imported a kept sign-in
    /// (crate::session), a credential field opens no handback: the step `step` ends the run (`Runner::no_window`).
    pub(crate) fn read_page(
        &mut self,
        page: &mut dyn Page,
        loaded: &LoadReply,
        step: NonZeroU32,
    ) -> Result<bool, Failure> {
        let failed = |()| Failure::Step(Cause::StepFailed);
        let Some(sign) = page.sign().map_err(failed)? else {
            return Ok(false);
        };
        let top = WebOrigin::of_address(&page.address().map_err(failed)?).ok();
        self.sessions.at_field = top.clone();
        if self.sessions.imported && !self.args.headed {
            return Err(self.no_window(step, top).into());
        }
        page.fail_held().map_err(failed)?;
        page.close_others().map_err(failed)?;
        self.sessions.cleared();
        page.clear_stores().map_err(failed)?;
        page.load_again().map_err(failed)?;
        if page.pass_all().is_err() {
            shut(page);
            return Err(end(Outcome::Failed, Some(Cause::StepFailed)).into());
        }
        match self
            .while_open(page, sign, top)
            .and_then(|origin| self.bind(origin))
        {
            Ok((origin, YesNo::Yes)) => {
                self.sessions.at_field = None;
                self.sessions.keep(&origin);
                if !self.bound.contains(&origin) {
                    self.bound.push(origin);
                }
                let mut origins = loaded
                    .bindings
                    .as_ref()
                    .map_or_else(Vec::new, |b| b.origins.clone());
                for bound in &self.bound {
                    if !origins.contains(bound) {
                        origins.push(bound.clone());
                    }
                }
                if page
                    .arm(&origins)
                    .and_then(|()| page.close_others())
                    .is_err()
                {
                    shut(page);
                    return Err(end(Outcome::Failed, Some(Cause::StepFailed)).into());
                }
                Ok(true)
            }
            Ok((_, YesNo::No)) => {
                shut(page);
                Err(end(Outcome::Declined, Some(Cause::Declined)).into())
            }
            Err(stop) => {
                shut(page);
                Err(stop.into())
            }
        }
    }

    /// The open handback: a handback request of the reason of `sign` that names the origin `top`, and after each
    /// `done` one reading of the page, again while a credential field is still there, at most `IN_A_ROW` times in a
    /// row; then one read of the address that the page holds, and its origin.
    fn while_open(
        &mut self,
        page: &mut dyn Page,
        mut sign: Sign,
        top: Option<WebOrigin>,
    ) -> Result<WebOrigin, Stop> {
        let mut done = 0;
        loop {
            match self.handback(reason_of(sign), top.clone())?.outcome {
                HandbackOutcome::Done => done += 1,
                HandbackOutcome::Declined => {
                    return Err(end(Outcome::Declined, Some(Cause::Declined)))
                }
                HandbackOutcome::Timeout => {
                    return Err(end(Outcome::HandedBack, Some(Cause::Timeout)))
                }
            }
            match page.sign() {
                Ok(None) => break,
                Ok(Some(next)) if done < IN_A_ROW => sign = next,
                Ok(Some(_)) | Err(()) => return Err(end(Outcome::HandedBack, None)),
            }
        }
        let address = page
            .address()
            .map_err(|()| end(Outcome::HandedBack, None))?;
        WebOrigin::of_address(&address).map_err(|_| end(Outcome::HandedBack, None))
    }

    /// Sends `origin` in the frame `bind` and reads its `bind_reply`; the wait is no time of the run's `wall_s`, and
    /// no `cancel` is read after it.
    fn bind(&mut self, origin: WebOrigin) -> Result<(WebOrigin, YesNo), Stop> {
        let frame = FromRunner::Bind(Bind {
            origin: origin.clone(),
        });
        let asked = self.elapsed();
        let reply = self.link.ask(&frame);
        self.waited += self.elapsed().saturating_sub(asked);
        match reply? {
            ToRunner::BindReply(BindReply { bound }) => Ok((origin, bound)),
            _ => Err(Broken::Frame("not_a_bind_reply").into()),
        }
    }
}

/// The close of a handback that binds nothing: the rule lets no request through and the relay reaches no host, every
/// request that the rule holds paused is failed, and the pages that were opened while it was open are closed. A
/// call that fails does not keep the next from being made; the browser is stopped at the run's end.
fn shut(page: &mut dyn Page) {
    let _ = page.arm(&[]);
    let _ = page.fail_held();
    let _ = page.close_others();
}
