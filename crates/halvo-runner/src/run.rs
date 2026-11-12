use std::num::NonZeroU32;
use std::time::{Duration, Instant as Clock};

use halvo_schema::executable::body::{Finish, Load, LoadReply, PutLog, Stored};
use halvo_schema::executable::frame::{Done, Hello, ReplyError, Store, StoreReplyFrame};
use halvo_schema::executable::names::{LogEvent, Runtime};
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{
    AgentLog, AgentRun, Cause, FromRunner, Outcome, RunnerCall, RunnerReply, StoreRequest, ToRunner,
};
use halvo_schema::{RequestId, RunnerRequest, RunnerRole};

use crate::args::Args;
use crate::clock::{day, now, random16};
use crate::link::{Broken, Link};

/// Why the run's work stopped before its end: the link broke, or the run ends with an outcome and a cause.
#[derive(Debug)]
pub(crate) enum Stop {
    Link(Broken),
    End(Outcome, Option<Cause>),
}

impl From<Broken> for Stop {
    fn from(broken: Broken) -> Stop {
        Stop::Link(broken)
    }
}

/// The cause that a store's refusal of a body gives a run's end.
fn cause_of(error: ReplyError) -> Cause {
    match error.as_str() {
        "effect" => Cause::Effect,
        "class" => Cause::Class,
        _ => Cause::StepFailed,
    }
}

pub(crate) struct Runner {
    pub(crate) args: Args,
    pub(crate) link: Link,
    role: RunnerRole,
    started: Clock,
    pub(crate) waited: Duration,
    log_seq: u32,
    pub(crate) steps: u32,
    pub(crate) handbacks: u32,
    pub(crate) confirmations: u32,
    pub(crate) playbook: Option<NonZeroU32>,
}

/// Carries out one run on the link and returns the exit status: 0 once `done` is written after a stored
/// `finish`, 1 when the link closed or the `finish` was not stored, 2 for a frame that breaks the form of A6.2.
pub(crate) fn run(args: Args, link: Link) -> i32 {
    let mut runner = Runner {
        args,
        link,
        role: RunnerRole::claim(),
        started: Clock::now(),
        waited: Duration::ZERO,
        log_seq: 0,
        steps: 0,
        handbacks: 0,
        confirmations: 0,
        playbook: None,
    };
    let end = runner.carry_out();
    runner.close(end)
}

impl Runner {
    fn carry_out(&mut self) -> Result<Outcome, Stop> {
        self.link.send(&FromRunner::Hello(Hello {
            run: self.args.run.clone(),
            agent: self.args.agent.clone(),
            v: V1,
        }))?;
        let loaded = self.load()?;
        self.check()?;
        match loaded.config.runtime {
            Runtime::None => Ok(Outcome::NothingToDo),
            _ => Err(Stop::End(Outcome::Failed, Some(Cause::StepFailed))),
        }
    }

    /// Takes what came on the link while no request waited, and ends the run after a `cancel`.
    pub(crate) fn check(&mut self) -> Result<(), Stop> {
        self.link.poll()?;
        if self.link.cancelled() {
            return Err(Stop::End(Outcome::Declined, Some(Cause::Declined)));
        }
        Ok(())
    }

    /// Writes one `store` frame and returns the store's reply, which must answer this request.
    pub(crate) fn store(
        &mut self,
        call: RunnerCall,
    ) -> Result<Result<RunnerReply, ReplyError>, Broken> {
        let id = RequestId::from_random(random16());
        let request = RunnerRequest::new(&self.role, id.clone(), call);
        let envelope = StoreRequest::new(&request).map_err(|_| Broken::Frame("own_body"))?;
        let reply = self
            .link
            .ask(&FromRunner::Store(Store { request: envelope }))?;
        let ToRunner::StoreReply(StoreReplyFrame { reply }) = reply else {
            return Err(Broken::Frame("not_a_store_reply"));
        };
        match reply.outcome {
            Ok(answer) if reply.id.as_ref() == Some(&id) && answer.answers(request.body()) => {
                Ok(Ok(answer))
            }
            Err(error) if reply.id.as_ref().is_none_or(|got| *got == id) => Ok(Err(error)),
            _ => Err(Broken::Frame("not_the_answer")),
        }
    }

    /// The agent's configuration, playbook, bindings and sessions as they stand now.
    pub(crate) fn load(&mut self) -> Result<LoadReply, Stop> {
        let call = RunnerCall::Load(Load {
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
        });
        match self.store(call)? {
            Ok(RunnerReply::Load(reply)) => {
                if reply.config.agent != self.args.agent
                    || reply.config.workflow != self.args.workflow
                {
                    return Err(Broken::Frame("load_of_another_agent").into());
                }
                Ok(reply)
            }
            Ok(_) => Err(Broken::Frame("not_the_answer").into()),
            Err(error) => Err(Stop::End(Outcome::Failed, Some(cause_of(error)))),
        }
    }

    /// Stores one line of the run's log.
    pub(crate) fn log(
        &mut self,
        event: LogEvent,
        step: Option<NonZeroU32>,
        cause: Option<Cause>,
    ) -> Result<(), Stop> {
        self.log_seq += 1;
        let at = now();
        let record = AgentLog {
            v: V1,
            workflow: self.args.workflow.clone(),
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            seq: NonZeroU32::new(self.log_seq).unwrap_or(NonZeroU32::MAX),
            day: day(at),
            at,
            event,
            step,
            cause,
            model: None,
            ms: None,
            tokens_in: None,
            tokens_out: None,
        };
        let call = RunnerCall::Log(PutLog {
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            records: vec![record],
        });
        match self.store(call)? {
            Ok(_) => Ok(()),
            Err(error) => Err(Stop::End(Outcome::Failed, Some(cause_of(error)))),
        }
    }

    /// The runner's seconds without the waits for a confirmation or a handback reply.
    pub(crate) fn wall_s(&self) -> u32 {
        let spent = self.started.elapsed().saturating_sub(self.waited);
        u32::try_from(spent.as_secs()).unwrap_or(u32::MAX)
    }

    fn close(mut self, end: Result<Outcome, Stop>) -> i32 {
        let (outcome, cause) = match end {
            Ok(outcome) => (outcome, None),
            Err(Stop::End(outcome, cause)) => (outcome, cause),
            Err(Stop::Link(broken)) => return ended(broken),
        };
        match self.finish(outcome, cause) {
            Ok(true) => 0,
            Ok(false) => 1,
            Err(broken) => ended(broken),
        }
    }

    /// Stores the run's record and writes `done`; whether the store took the record.
    fn finish(&mut self, outcome: Outcome, cause: Option<Cause>) -> Result<bool, Broken> {
        if cause.is_some() {
            match self.log(LogEvent::Stopped, None, cause) {
                Err(Stop::Link(broken)) => return Err(broken),
                Err(Stop::End(..)) | Ok(()) => {}
            }
        }
        let record = AgentRun {
            v: V1,
            workflow: self.args.workflow.clone(),
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            day: day(now()),
            mode: Some(self.args.mode),
            outcome,
            playbook: self.playbook,
            steps: self.steps,
            prompts: self.handbacks + self.confirmations,
            handbacks: self.handbacks,
            confirmations: self.confirmations,
            effects: 0,
            unknown: 0,
            model_calls: 0,
            tokens_in: 0,
            tokens_out: 0,
            wall_s: self.wall_s(),
            model: None,
        };
        let call = RunnerCall::Finish(Finish {
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            record,
        });
        let stored = matches!(
            self.store(call)?,
            Ok(RunnerReply::Stored(Stored { stored: 1 }))
        );
        self.link.send(&FromRunner::Done(Done { outcome }))?;
        eprintln!(
            "run outcome={} steps={} finish={}",
            outcome.as_str(),
            self.steps,
            if stored { "yes" } else { "no" }
        );
        Ok(stored)
    }
}

/// The status of a run whose link broke: 1 after a close, which writes nothing more; 2 with one line `REFUSED
/// frame: <rule>` for a frame that breaks the form.
fn ended(broken: Broken) -> i32 {
    match broken {
        Broken::Closed => {
            eprintln!("run link=closed");
            1
        }
        Broken::Frame(rule) => {
            eprintln!("REFUSED frame: {rule}");
            2
        }
    }
}
