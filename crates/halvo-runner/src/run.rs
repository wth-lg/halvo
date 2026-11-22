use std::collections::HashMap;
use std::num::NonZeroU32;
use std::time::{Duration, Instant as Clock};

use halvo_schema::executable::body::{Finish, Load, LoadReply, PutLog, Stored};
use halvo_schema::executable::day::day_of;
use halvo_schema::executable::frame::{
    Done, Handback, HandbackReply, Hello, Progress, ReplyError, Store, StoreReplyFrame,
};
use halvo_schema::executable::names::{HandbackReason, LogEvent, StepKind};
use halvo_schema::executable::step::Target;
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{
    ActionClass, AgentLog, AgentRun, ApprovalId, Cause, EffectState, ExecutableStep, FromRunner,
    HandbackRequest, HandbackResult, Limits, Outcome, RunnerCall, RunnerReply, StoreRequest,
    ToRunner, WebOrigin,
};
use halvo_schema::{RequestId, RunnerRequest, RunnerRole};
use halvo_sheet::Value;

use crate::args::Args;
use crate::clock::{now, random16};
use crate::connect::Lists;
use crate::effect::Tracked;
use crate::file::{ReadUnder, Taken};
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

/// The `seq` of the log row `start_accepted`, which the store's handler of `start_run` stores, so that the
/// runner's own lines count from the next (A7.4, CP-135 (5)).
const START_ROW: u32 = 1;

pub(crate) struct Runner {
    pub(crate) args: Args,
    pub(crate) link: Link,
    role: RunnerRole,
    started: Clock,
    pub(crate) waited: Duration,
    /// Milliseconds that a test adds to the run's time: the clock of the test's own.
    #[cfg(test)]
    pub(crate) test_clock: std::sync::Arc<std::sync::atomic::AtomicU64>,
    log_seq: u32,
    /// The `wall_s` of the newest load of the configuration; 0 before the first load.
    wall_limit: u32,
    pub(crate) steps: u32,
    pub(crate) handbacks: u32,
    pub(crate) confirmations: u32,
    pub(crate) playbook: Option<NonZeroU32>,
    pub(crate) effects: Vec<Tracked>,
    /// The approval ids that a row of this run held.
    #[cfg_attr(
        not(any(test, feature = "loopback")),
        expect(
            dead_code,
            reason = "the confirm path has no request maker in a build without the feature loopback until the adapters and the browser are linked (CP-142)"
        )
    )]
    pub(crate) approvals: Vec<ApprovalId>,
    pub(crate) kept: HashMap<String, Vec<Vec<Value>>>,
    pub(crate) read_under: Option<ReadUnder>,
    /// The newest row of each checkpoint of the run that the store took, by the workbook's path as the rows name it.
    pub(crate) checkpoints: HashMap<String, Taken>,
    /// What the run's list and draft steps gave, for its record.
    pub(crate) lists: Lists,
    /// The run's browser, once a web step started it (crate::web); stopped at every end of the run.
    pub(crate) web: Option<crate::web::Web>,
    /// Every origin that a `bind_reply` with `bound=yes` bound in this run (crate::handback), in order.
    pub(crate) bound: Vec<WebOrigin>,
    /// What the run knows of the kept sign-ins of its origins (crate::session).
    pub(crate) sessions: crate::session::Sessions,
    /// The stand-in that a unit test gives in the place of the browser's start.
    #[cfg(test)]
    pub(crate) stand_in: Option<crate::web::Web>,
    /// The roots under which a run given no `--browser` finds the person's browser (crate::find): this system's, and
    /// none in a unit test unless the test gives some.
    pub(crate) roots: crate::find::Roots,
}

/// Carries out one run on the link and returns the exit status: 0 once `done` is written after a stored
/// `finish`, 1 when the link closed or the `finish` was not stored, 2 for a frame that breaks the form of A6.2. The
/// run's browser is stopped and its profile removed before the run's end is written, on every way out; before that,
/// where the link holds, the sign-ins of its origins are stored (crate::session).
pub(crate) fn run(args: Args, link: Link) -> i32 {
    Runner::new(args, link).run()
}

impl Runner {
    /// Carries out the run of this runner and returns the exit status, as `run` does.
    pub(crate) fn run(mut self) -> i32 {
        let end = self.carry_out();
        self.end(end)
    }

    /// Ends the run after `end`: the sign-ins of its origins stored where the link holds, the browser stopped, then
    /// the run's end written.
    pub(crate) fn end(mut self, end: Result<Outcome, Stop>) -> i32 {
        if !matches!(end, Err(Stop::Link(_))) {
            self.keep_sessions();
        }
        self.stop_browser();
        self.close(end)
    }

    /// A runner of `args` on `link`, before its run.
    pub(crate) fn new(args: Args, link: Link) -> Runner {
        Runner {
            args,
            link,
            role: RunnerRole::claim(),
            started: Clock::now(),
            waited: Duration::ZERO,
            #[cfg(test)]
            test_clock: std::sync::Arc::default(),
            log_seq: START_ROW,
            wall_limit: 0,
            steps: 0,
            handbacks: 0,
            confirmations: 0,
            playbook: None,
            effects: Vec::new(),
            approvals: Vec::new(),
            kept: HashMap::new(),
            read_under: None,
            checkpoints: HashMap::new(),
            lists: Lists::default(),
            web: None,
            bound: Vec::new(),
            sessions: crate::session::Sessions::default(),
            #[cfg(test)]
            stand_in: None,
            #[cfg(not(test))]
            roots: crate::find::Roots::of_this_system(),
            #[cfg(test)]
            roots: crate::find::Roots::default(),
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
        self.within_wall()
    }

    /// Ends the run when its time is at or above the `wall_s` of the newest load (A6.5, CP-135 (3) (iv)).
    pub(crate) fn within_wall(&self) -> Result<(), Stop> {
        if self.wall_s() >= self.wall_limit {
            return Err(Stop::End(Outcome::Limit, Some(Cause::Wall)));
        }
        Ok(())
    }

    /// Carries out one step. A web step goes to `web_step` (crate::web), which carries a step of the class `read` or
    /// `prepare` with its conditions and fails every other with `step_failed`.
    fn step(&mut self, step: &ExecutableStep, loaded: &LoadReply) -> Result<(), Failure> {
        match &step.target {
            Some(Target::File(target)) if step.class == ActionClass::Read => {
                self.file_read(step, target, loaded)
            }
            Some(Target::File(target)) => self.file_write(step, target),
            Some(Target::Api(target)) => self.api_call(step, target, loaded),
            _ if step.kind == StepKind::Web => self.web_step(step, loaded),
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
    /// store refuses does not keep the handback from opening. A `cancel` is read when the result has come.
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
        let asked = self.elapsed();
        let reply = self.link.ask(&frame);
        self.waited += self.elapsed().saturating_sub(asked);
        let result = match reply? {
            ToRunner::HandbackReply(HandbackReply { result }) => result,
            _ => return Err(Broken::Frame("not_a_handback_reply").into()),
        };
        self.check()?;
        Ok(result)
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
                self.wall_limit = reply.config.limits.wall_s;
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
            day: day_of(at, self.args.utc_offset_s),
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

    /// How many times a body of the run's end, its `stopped` row or its `finish`, is sent when the store answers it
    /// with a fault (`store` or `internal`), each time as the same request, as a row of an effect is sent (A7.5,
    /// CP-178 (1)): the store's handler of `log` keeps a record only when no held row equals it, and it answers a
    /// `finish` whose record equals the stored run record as the first (CP-132 (e)).
    const END_SENDS: u32 = 3;

    /// Sends a body of the run's end as one request, built once: again after a fault of the store, up to
    /// `END_SENDS` sends in all, and never again after any other answer. Returns the store's last answer.
    fn log_end(&mut self, call: RunnerCall) -> Result<Result<RunnerReply, ReplyError>, Broken> {
        let request = RunnerRequest::new(&self.role, RequestId::from_random(random16()), call);
        let mut sends = 1;
        loop {
            match self.send_store(&request)? {
                Err(error)
                    if matches!(error.as_str(), "store" | "internal")
                        && sends < Self::END_SENDS =>
                {
                    sends += 1;
                }
                answer => return Ok(answer),
            }
        }
    }

    /// The time since the runner started, on a clock that never goes back.
    pub(crate) fn elapsed(&self) -> Duration {
        let elapsed = self.started.elapsed();
        #[cfg(test)]
        let elapsed = elapsed
            + Duration::from_millis(self.test_clock.load(std::sync::atomic::Ordering::SeqCst));
        elapsed
    }

    /// The runner's seconds without the waits for a confirmation or a handback reply.
    pub(crate) fn wall_s(&self) -> u32 {
        let spent = self.elapsed().saturating_sub(self.waited);
        u32::try_from(spent.as_secs()).unwrap_or(u32::MAX)
    }

    pub(crate) fn close(mut self, end: Result<Outcome, Stop>) -> i32 {
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
            self.log_seq += 1;
            let at = now();
            let row = AgentLog {
                v: V1,
                workflow: self.args.workflow.clone(),
                agent: self.args.agent.clone(),
                run: self.args.run.clone(),
                seq: NonZeroU32::new(self.log_seq).unwrap_or(NonZeroU32::MAX),
                day: day_of(at, self.args.utc_offset_s),
                at,
                event: LogEvent::Stopped,
                step: None,
                cause,
                model: None,
                ms: None,
                tokens_in: None,
                tokens_out: None,
            };
            let call = RunnerCall::Log(PutLog {
                agent: self.args.agent.clone(),
                run: self.args.run.clone(),
                records: vec![row],
            });
            // The run goes on to its `finish` whatever the store answered: after the third fault the cause of the
            // run's end is not handed out, a named limit (A7.5).
            let _ = self.log_end(call)?;
        }
        let mut record = AgentRun {
            v: V1,
            workflow: self.args.workflow.clone(),
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            day: day_of(now(), self.args.utc_offset_s),
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
            before_start: None,
            cut: None,
            no_key: None,
            not_to_account: None,
            incomplete: None,
            reply_to_other: None,
            more_pages: None,
            held_recipient: None,
            organized: None,
            other_task: None,
            held_earlier: None,
            earlier_page: None,
        };
        self.lists.fill(&mut record);
        let call = RunnerCall::Finish(Finish {
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            record,
        });
        let stored = matches!(
            self.log_end(call)?,
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
            "--utc-offset",
            "0",
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
            day: day_of(at, runner.args.utc_offset_s),
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
