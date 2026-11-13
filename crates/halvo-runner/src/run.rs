use std::collections::HashMap;
use std::num::NonZeroU32;
use std::time::{Duration, Instant as Clock};

use halvo_schema::executable::body::{Finish, Load, LoadReply, PutLog, Stored};
use halvo_schema::executable::frame::{
    Done, Handback, HandbackReply, Hello, Progress, ReplyError, Store, StoreReplyFrame,
};
use halvo_schema::executable::names::{HandbackReason, LogEvent};
use halvo_schema::executable::step::Target;
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{
    ActionClass, AgentLog, AgentRun, Cause, EffectState, ExecutableStep, FromRunner,
    HandbackRequest, HandbackResult, Limits, Outcome, RunnerCall, RunnerReply, StoreRequest,
    ToRunner, WebOrigin,
};
use halvo_schema::{RequestId, RunnerRequest, RunnerRole};
use halvo_sheet::Value;

use crate::args::Args;
use crate::clock::{day, now, random16};
use crate::effect::Tracked;
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

/// Why a step did not end well: it failed with a cause, or the run stops.
#[derive(Debug)]
pub(crate) enum Failure {
    Step(Cause),
    Stop(Stop),
}

impl From<Stop> for Failure {
    fn from(stop: Stop) -> Failure {
        Failure::Stop(stop)
    }
}

impl From<Broken> for Failure {
    fn from(broken: Broken) -> Failure {
        Failure::Stop(Stop::Link(broken))
    }
}

/// The cause that a store's refusal of a body gives a run's end.
pub(crate) fn cause_of(error: ReplyError) -> Cause {
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
    pub(crate) effects: Vec<Tracked>,
    pub(crate) kept: HashMap<String, Vec<Vec<Value>>>,
}

/// Carries out one run on the link and returns the exit status: 0 once `done` is written after a stored
/// `finish`, 1 when the link closed or the `finish` was not stored, 2 for a frame that breaks the form of A6.2.
pub(crate) fn run(args: Args, link: Link) -> i32 {
    let mut runner = Runner::new(args, link);
    let end = runner.carry_out();
    runner.close(end)
}

impl Runner {
    /// A runner of `args` on `link`, before its run.
    fn new(args: Args, link: Link) -> Runner {
        Runner {
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
            effects: Vec::new(),
            kept: HashMap::new(),
        }
    }

    fn carry_out(&mut self) -> Result<Outcome, Stop> {
        self.link.send(&FromRunner::Hello(Hello {
            run: self.args.run.clone(),
            agent: self.args.agent.clone(),
            v: V1,
        }))?;
        let loaded = self.load()?;
        self.check()?;
        let steps = self.playbook(&loaded)?;
        let of = NonZeroU32::new(steps.len() as u32).unwrap_or(NonZeroU32::MIN);
        for step in &steps {
            self.within(&loaded.config.limits)?;
            self.check()?;
            self.link
                .send(&FromRunner::Progress(Progress { step: step.n, of }))?;
            self.steps += 1;
            match self.step(step, &loaded) {
                Ok(()) => self.log(LogEvent::StepOk, Some(step.n), None)?,
                Err(Failure::Stop(stop)) => return Err(stop),
                Err(Failure::Step(cause)) => return Err(self.failed_step(step, cause)),
            }
        }
        Ok(Outcome::Completed)
    }

    /// Ends the run at a limit of its configuration that it has reached: `steps`, then `wall`.
    fn within(&self, limits: &Limits) -> Result<(), Stop> {
        if self.steps >= limits.steps {
            return Err(Stop::End(Outcome::Limit, Some(Cause::Steps)));
        }
        self.within_wall(limits)
    }

    /// Ends the run when its `wall` limit has passed.
    pub(crate) fn within_wall(&self, limits: &Limits) -> Result<(), Stop> {
        if self.wall_s() > limits.wall_s {
            return Err(Stop::End(Outcome::Limit, Some(Cause::Wall)));
        }
        Ok(())
    }

    /// Carries out one step. This build has no browser, so a `web` step fails, and with it every step that holds a
    /// condition, since only the stored playbook of a `web` agent can hold one.
    fn step(&mut self, step: &ExecutableStep, loaded: &LoadReply) -> Result<(), Failure> {
        match &step.target {
            Some(Target::File(target)) if step.class == ActionClass::Read => {
                self.file_read(step, target, loaded)
            }
            Some(Target::File(target)) => self.file_write(step, target, loaded),
            Some(Target::Api(target)) => self.api_call(target, loaded),
            _ => Err(Failure::Step(Cause::StepFailed)),
        }
    }

    /// The end of a run whose step failed (A5.1): a step of the class `read` or `prepare` is handed back with the
    /// reason `step_failed`, and the run ends `handed_back`; a step of an effect class ends the run `failed`.
    fn failed_step(&mut self, step: &ExecutableStep, cause: Cause) -> Stop {
        if let Err(Stop::Link(broken)) = self.log(LogEvent::StepFailed, Some(step.n), Some(cause)) {
            return Stop::Link(broken);
        }
        if !matches!(step.class, ActionClass::Read | ActionClass::Prepare) {
            return Stop::End(Outcome::Failed, Some(cause));
        }
        match self.handback(HandbackReason::StepFailed, None) {
            Ok(_) => Stop::End(Outcome::HandedBack, Some(cause)),
            Err(stop) => stop,
        }
    }

    /// Opens a handback and waits for its result; the wait is no time of the run's `wall_s`. A log line that the
    /// store refuses does not keep the handback from opening.
    pub(crate) fn handback(
        &mut self,
        reason: HandbackReason,
        origin: Option<WebOrigin>,
    ) -> Result<HandbackResult, Stop> {
        if let Err(Stop::Link(broken)) = self.log(LogEvent::Handback, None, None) {
            return Err(Stop::Link(broken));
        }
        self.handbacks += 1;
        let frame = FromRunner::Handback(Handback {
            request: HandbackRequest {
                v: V1,
                run: self.args.run.clone(),
                agent: self.args.agent.clone(),
                reason,
                origin,
                endpoint: None,
            },
        });
        let asked = Clock::now();
        let reply = self.link.ask(&frame);
        self.waited += asked.elapsed();
        match reply? {
            ToRunner::HandbackReply(HandbackReply { result }) => Ok(result),
            _ => Err(Broken::Frame("not_a_handback_reply").into()),
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
        let request = RunnerRequest::new(&self.role, RequestId::from_random(random16()), call);
        self.send_store(&request)
    }

    /// The role that builds the runner's requests.
    pub(crate) fn role(&self) -> &RunnerRole {
        &self.role
    }

    /// Writes the `store` frame of a request and returns the store's reply, which must answer it. The frame of a
    /// row that proposes or begins an effect (the call `effect` with a state other than `committed`) goes as
    /// `Link::ask_effect` lets it; every other frame goes as `Link::ask` lets it.
    pub(crate) fn send_store(
        &mut self,
        request: &RunnerRequest<RunnerCall>,
    ) -> Result<Result<RunnerReply, ReplyError>, Broken> {
        let id = request.id();
        let effect = matches!(
            request.body(),
            RunnerCall::Effect(put) if put.record.state != EffectState::Committed
        );
        let envelope = StoreRequest::new(request).map_err(|_| Broken::Frame("own_body"))?;
        let frame = FromRunner::Store(Store { request: envelope });
        let reply = if effect {
            self.link.ask_effect(&frame)?
        } else {
            self.link.ask(&frame)?
        };
        let ToRunner::StoreReply(StoreReplyFrame { reply }) = reply else {
            return Err(Broken::Frame("not_a_store_reply"));
        };
        match reply.outcome {
            Ok(answer) if reply.id.as_ref() == Some(id) && answer.answers(request.body()) => {
                Ok(Ok(answer))
            }
            Err(error) if reply.id.as_ref().is_none_or(|got| got == id) => Ok(Err(error)),
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
        let (mut outcome, mut cause) = match end {
            Ok(outcome) => (outcome, None),
            Err(Stop::End(outcome, cause)) => (outcome, cause),
            Err(Stop::Link(broken)) => return ended(broken),
        };
        if self.unknown() > 0 {
            if let Err(Stop::Link(broken)) = self.handback(HandbackReason::EffectUnknown, None) {
                return ended(broken);
            }
            outcome = Outcome::EffectUnknown;
            cause = Some(Cause::Unknown);
        }
        match self.finish(outcome, cause) {
            Ok(true) => 0,
            Ok(false) => 1,
            Err(broken) => ended(broken),
        }
    }

    /// The count of effects that are `releasing`, which the store's handler of the run's end makes `unknown`.
    fn unknown(&self) -> u32 {
        self.effects
            .iter()
            .filter(|e| e.state == EffectState::Releasing)
            .count() as u32
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
            effects: self.effects.len() as u32,
            unknown: self.unknown(),
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
        crate::line(&format!(
            "run outcome={} steps={} finish={}",
            outcome.as_str(),
            self.steps,
            if stored { "yes" } else { "no" }
        ));
        Ok(stored)
    }
}

/// The status of a run whose link broke: 1 after a close, which writes nothing more; 2 with one line `REFUSED
/// frame: <rule>` for a frame that breaks the form.
fn ended(broken: Broken) -> i32 {
    match broken {
        Broken::Closed => {
            crate::line("run link=closed");
            1
        }
        Broken::Frame(rule) => {
            crate::line(&format!("REFUSED frame: {rule}"));
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::EndLater;
    use halvo_schema::executable::body::PutEffect;
    use halvo_schema::executable::names::{FieldName, Source};
    use halvo_schema::executable::text::OwnValue;
    use halvo_schema::executable::{AgentEffect, Bound, BoundField, EffectId, KeptField};
    use std::ffi::OsString;
    use std::io::Write;
    use std::sync::{mpsc, Arc, Mutex};

    struct Out(Arc<Mutex<Vec<u8>>>);

    impl Write for Out {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Whether `seen` holds within `bound`, asked every 5 ms.
    fn within(bound: Duration, seen: impl Fn() -> bool) -> bool {
        let until = Clock::now() + bound;
        while Clock::now() < until {
            if seen() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        seen()
    }

    /// A runner whose link reads an end of file alone, with its reader held between the return of that read and
    /// its note until the test lets it go; the runner's output, the receiver that hears that the reader got the
    /// end of file, and the sender that lets it go.
    struct HeldRunner {
        runner: Runner,
        out: Arc<Mutex<Vec<u8>>>,
        got: mpsc::Receiver<()>,
        go: mpsc::Sender<()>,
    }

    fn held_runner() -> HeldRunner {
        let (got, heard) = mpsc::channel();
        let (go, gone) = mpsc::channel::<()>();
        let end: EndLater = Box::new(move || {
            let _ = got.send(());
            let _ = gone.recv();
        });
        let out = Arc::new(Mutex::new(Vec::new()));
        let input = std::io::Cursor::new(Vec::new());
        let link = Link::start(Box::new(input), Box::new(Out(Arc::clone(&out))), end);
        let run = "0a".repeat(16);
        let agent = "08".repeat(16);
        let workflow = "09".repeat(16);
        let words = [
            "--run",
            &run,
            "--agent",
            &agent,
            "--workflow",
            &workflow,
            "--mode",
            "replay",
            "--browser",
            "none",
        ];
        let words: Vec<OsString> = words.iter().map(|w| OsString::from(*w)).collect();
        let args = crate::args::parse(&words).expect("the arguments");
        HeldRunner {
            runner: Runner::new(args, link),
            out,
            got: heard,
            go,
        }
    }

    /// The request of a row in `state` of a file edit's effect, as `store_row` builds it.
    fn row(runner: &Runner, state: EffectState) -> RunnerRequest<RunnerCall> {
        let field = |name: FieldName, value: &str| BoundField {
            name,
            from: Source::Slot,
            origin: None,
            value: OwnValue::new(value),
        };
        let bound = Bound::new(
            ActionClass::LocalEdit,
            vec![
                field(FieldName::File, "/f.xlsx"),
                field(FieldName::Range, "B2:D6"),
            ],
        )
        .expect("a bound object");
        let names = bound.class().kept_fields();
        let kept = bound
            .fields()
            .iter()
            .filter(|f| names.iter().any(|(name, _)| *name == f.name))
            .map(|f| KeptField {
                name: f.name,
                value: f.value.clone(),
            })
            .collect();
        let at = now();
        let record = AgentEffect {
            v: V1,
            workflow: runner.args.workflow.clone(),
            agent: runner.args.agent.clone(),
            run: runner.args.run.clone(),
            effect: EffectId::from_random(random16()),
            seq: NonZeroU32::MIN,
            day: day(at),
            at,
            state,
            class: bound.class(),
            digest: halvo_policy::digest(&bound).expect("a digest"),
            kept,
            fields: bound.fields().len() as u32,
            approval: None,
            cause: None,
        };
        let call = RunnerCall::Effect(PutEffect {
            agent: runner.args.agent.clone(),
            run: runner.args.run.clone(),
            record,
        });
        RunnerRequest::new(runner.role(), RequestId::from_random(random16()), call)
    }

    #[test]
    fn a_cleared_row_waits_for_a_held_reader_and_a_committed_row_does_not() {
        let HeldRunner {
            mut runner,
            out,
            got,
            go,
        } = held_runner();
        got.recv().expect("the reader got the end of file");
        let request = row(&runner, EffectState::Cleared);
        std::thread::scope(|s| {
            let sent = s.spawn(|| runner.send_store(&request).map(|_| ()));
            let written = within(Duration::from_millis(500), || {
                !out.lock().expect("lock").is_empty()
            });
            go.send(()).expect("the reader goes on");
            assert_eq!(sent.join().expect("the row's thread"), Err(Broken::Closed));
            assert!(
                !written,
                "no cleared row while the reader holds the end of file"
            );
        });
        assert!(out.lock().expect("lock").is_empty(), "no cleared row");

        let HeldRunner {
            mut runner,
            out,
            got,
            go,
        } = held_runner();
        got.recv().expect("the reader got the end of file");
        let request = row(&runner, EffectState::Committed);
        std::thread::scope(|s| {
            let sent = s.spawn(|| runner.send_store(&request).map(|_| ()));
            let written = within(Duration::from_secs(5), || {
                !out.lock().expect("lock").is_empty()
            });
            go.send(()).expect("the reader goes on");
            assert_eq!(sent.join().expect("the row's thread"), Err(Broken::Closed));
            assert!(
                written,
                "the committed row is written while the reader holds the end of file"
            );
        });
    }
}
