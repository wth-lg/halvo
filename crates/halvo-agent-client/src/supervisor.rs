use std::collections::HashMap;
use std::io;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use halvo_schema::executable::body::{
    CoordinatorCall, EndRun as EndRunBody, RunnerCall, RunnerReply,
};
use halvo_schema::executable::frame::{
    Browser, BrowserReply, Cancel, FromRunner, HandbackResult, ReplyError, RunError, SecretReply,
    StoreReply, StoreReplyFrame, StoreRequest, ToRunner,
};
use halvo_schema::executable::names::{Outcome, Unreadable};
use halvo_schema::executable::record::AgentRun;
use halvo_schema::executable::text::{Name, RunId};
use halvo_schema::request::{CoordinatorRole, ErrorName};

use crate::acts::Acts;
use crate::api::{
    absolute_paths, CallError, Config, EndRun, ForcedPath, NotStarted, Parts, ReapAnswer, Reaped,
    RunEnd, RunSpec, RunnerExit, StartError,
};
use crate::marks::{self, proc_line, Created, MarkFile};
use crate::owner;
use crate::proc::{self, ReadValue, RunProc, SpawnFailed};
use crate::wire::{read_frame, write_frame};

const TICK: Duration = Duration::from_millis(100);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) enum Event {
    Frame(Box<FromRunner>),
    Broken,
    Eof,
    Handback(HandbackResult, Sender<Result<(), CallError>>),
}

/// One run that this life started.
pub(crate) struct Slot {
    pub(crate) spec: RunSpec,
    pub(crate) proc: Mutex<Option<RunProc>>,
    writer: Mutex<Option<ChildStdin>>,
    events: Mutex<Option<Sender<Event>>>,
    ended: AtomicBool,
}

impl Slot {
    fn new(spec: RunSpec) -> Slot {
        Slot {
            spec,
            proc: Mutex::new(None),
            writer: Mutex::new(None),
            events: Mutex::new(None),
            ended: AtomicBool::new(false),
        }
    }

    pub(crate) fn ended(&self) -> bool {
        self.ended.load(Ordering::SeqCst)
    }

    /// Writes one frame to the runner; false when the link is closed or the write fails.
    pub(crate) fn write(&self, frame: &ToRunner) -> bool {
        let Ok(json) = frame.to_json() else {
            return false;
        };
        match lock(&self.writer).as_mut() {
            Some(w) => write_frame(w, &json).is_ok(),
            None => false,
        }
    }

    fn close_link(&self) {
        lock(&self.writer).take();
        lock(&self.events).take();
    }

    pub(crate) fn with_proc<R>(&self, f: impl FnOnce(&mut RunProc) -> R) -> Option<R> {
        lock(&self.proc).as_mut().map(f)
    }
}

pub(crate) struct Inner {
    pub(crate) config: Config,
    pub(crate) parts: Parts,
    pub(crate) role: CoordinatorRole,
    runs: Mutex<HashMap<RunId, Arc<Slot>>>,
    spawn_lock: Mutex<()>,
    read_value: ReadValue,
}

/// The supervisor library: it starts runners, relays their store requests, answers their frames through the
/// caller's traits, and ends their processes.
#[derive(Clone)]
pub struct Supervisor {
    inner: Arc<Inner>,
}

impl Supervisor {
    /// A supervisor over the caller's traits, with the coordinator role under which it sends `add_origin` and
    /// `end_run`.
    pub fn new(role: CoordinatorRole, config: Config, parts: Parts) -> Supervisor {
        Supervisor::reading(role, config, parts, proc::system_value)
    }

    fn reading(
        role: CoordinatorRole,
        config: Config,
        parts: Parts,
        read_value: ReadValue,
    ) -> Supervisor {
        Supervisor {
            inner: Arc::new(Inner {
                config,
                parts,
                role,
                runs: Mutex::new(HashMap::new()),
                spawn_lock: Mutex::new(()),
                read_value,
            }),
        }
    }

    fn slot(&self, run: &RunId) -> Result<Arc<Slot>, CallError> {
        lock(&self.inner.runs)
            .get(run)
            .cloned()
            .ok_or(CallError::UnknownRun)
    }

    /// Starts the runner of `spec`, all or nothing: `Ok` only when the runner runs, has said `hello` for its run
    /// and agent, and its mark is complete; `not_started` when it could not be made or was gone before the start
    /// answered, and then no process of the run that the library can see exists; `started` for a run id that this
    /// life started or whose mark of an earlier life stands. A start whose runner path, `--browser`, `--model` or
    /// data directory is not absolute, or whose environment list lacks a value that the system did not give, answers
    /// `not_started` with that path or value before any process is made, before a mark is written and before
    /// `MarkSink::start`; a start whose runner was made and exited before it said `hello` answers `not_started` with
    /// the runner's exit, the status 2 being a refusal. The runner's standard error is the null device: nobody
    /// reads it.
    pub fn start(&self, spec: RunSpec) -> Result<(), StartError> {
        absolute_paths(&self.inner.config, &spec)
            .map_err(|path| StartError::NotStarted(NotStarted::Relative(path)))?;
        let env = proc::environment(self.inner.read_value)
            .map_err(|value| StartError::NotStarted(NotStarted::Missing(value)))?;
        let run = spec.run.clone();
        let slot = Arc::new(Slot::new(spec));
        let mut held = lock(&slot.proc);
        {
            let mut runs = lock(&self.inner.runs);
            if runs.contains_key(&run) {
                return Err(StartError::Started);
            }
            runs.insert(run.clone(), Arc::clone(&slot));
        }
        let data_dir = &self.inner.config.data_dir;
        let mut mark = match MarkFile::create(data_dir, &run) {
            Ok(Created::Made(mark)) => mark,
            Ok(Created::Exists) => {
                lock(&self.inner.runs).remove(&run);
                return Err(StartError::Started);
            }
            Err(_) => {
                slot.ended.store(true, Ordering::SeqCst);
                return Err(not_started(None));
            }
        };
        self.inner.parts.marks.start(&run);
        let spawned = {
            let _one = lock(&self.inner.spawn_lock);
            proc::spawn(&mut self.command(&slot.spec), &env)
        };
        let mut runner = match spawned {
            Ok(p) => p,
            Err(SpawnFailed::NotMade) => {
                drop(held);
                let _ = marks::remove(data_dir, &run);
                slot.ended.store(true, Ordering::SeqCst);
                self.inner.parts.marks.end(
                    &run,
                    &RunEnd {
                        outcome: None,
                        finish: None,
                        end_run: EndRun::NoRunner,
                        exit: None,
                        forced: None,
                    },
                );
                return Err(not_started(None));
            }
            Err(SpawnFailed::Made(p)) => return self.fail_made(&slot, held, p),
        };
        let written = runner.line().map(|l| mark.append(&proc_line("runner", &l)));
        if !matches!(written, Some(Ok(()))) {
            return self.fail_made(&slot, held, runner);
        }
        let Some((stdin, stdout)) = runner.take_link() else {
            return self.fail_made(&slot, held, runner);
        };
        *held = Some(runner);
        drop(held);
        *lock(&slot.writer) = Some(stdin);
        let (tx, rx) = mpsc::channel();
        *lock(&slot.events) = Some(tx.clone());
        thread::spawn(move || {
            let mut stdout = stdout;
            loop {
                let event = match read_frame(&mut stdout) {
                    Ok(Some(bytes)) => match FromRunner::parse(&bytes) {
                        Ok(frame) => Event::Frame(Box::new(frame)),
                        Err(_) => Event::Broken,
                    },
                    Ok(None) => Event::Eof,
                    Err(_) => Event::Broken,
                };
                let last = !matches!(event, Event::Frame(_));
                if tx.send(event).is_err() || last {
                    return;
                }
            }
        });
        let (started_tx, started_rx) = mpsc::channel();
        let inner = Arc::clone(&self.inner);
        let driver_slot = Arc::clone(&slot);
        thread::spawn(move || drive(inner, driver_slot, rx, mark, started_tx));
        started_rx.recv().unwrap_or(Err(not_started(None)))
    }

    fn fail_made(
        &self,
        slot: &Arc<Slot>,
        mut held: MutexGuard<'_, Option<RunProc>>,
        mut runner: RunProc,
    ) -> Result<(), StartError> {
        runner.end_all();
        let gone = runner.all_gone();
        let exit = runner.exit();
        *held = Some(runner);
        drop(held);
        let inner = Arc::clone(&self.inner);
        let s = Arc::clone(slot);
        thread::spawn(move || finish_run(&inner, &s, Ending::default()));
        if gone {
            Err(not_started(exit))
        } else {
            Ok(())
        }
    }

    fn command(&self, spec: &RunSpec) -> Command {
        let mut cmd = Command::new(&self.inner.config.runner);
        cmd.arg("--run")
            .arg(spec.run.as_str())
            .arg("--agent")
            .arg(spec.agent.as_str())
            .arg("--workflow")
            .arg(spec.workflow.as_str())
            .arg("--mode")
            .arg(spec.mode.as_str())
            .arg("--browser")
            .arg(&spec.browser);
        if let Some(model) = &spec.model {
            cmd.arg("--model").arg(model);
        }
        if spec.headed {
            cmd.arg("--headed");
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        cmd
    }

    /// Writes the frame `cancel` to the runner of `run`.
    pub fn cancel(&self, run: &RunId) -> Result<(), CallError> {
        let slot = self.slot(run)?;
        if slot.ended() || !slot.write(&ToRunner::Cancel(Cancel {})) {
            return Err(CallError::Ended);
        }
        Ok(())
    }

    /// Ends every process of `run` that the library may end and waits for the runner to be gone; success also for a
    /// run whose runner has ended.
    pub fn kill(&self, run: &RunId) -> Result<(), CallError> {
        let slot = self.slot(run)?;
        match slot.with_proc(RunProc::kill) {
            Some(()) => Ok(()),
            None if slot.ended() => Ok(()),
            None => Err(CallError::UnknownRun),
        }
    }

    /// Answers for every mark under the data directory, one answer per run: a run of this life whose runner has
    /// not exited is `alive` and is left alone; for any other the library ends the processes of its mark that it
    /// may end, answers `gone` when its runner is gone, or its mark not complete, and then removes the mark, and
    /// `alive` when its runner lives and could not be ended.
    pub fn reap(&self) -> io::Result<Vec<ReapAnswer>> {
        let data_dir = &self.inner.config.data_dir;
        let mut answers = Vec::new();
        for mark in marks::read_all(data_dir)? {
            let slot = lock(&self.inner.runs).get(&mark.run).cloned();
            let live = slot.is_some_and(|slot| {
                !slot.ended() && slot.with_proc(RunProc::exited) == Some(false)
            });
            if live {
                answers.push(ReapAnswer {
                    run: mark.run,
                    answer: Reaped::Alive,
                    left: Vec::new(),
                    forced: None,
                });
                continue;
            }
            let (answer, left, forced) = proc::reap_mark(&mark);
            if answer == Reaped::Gone {
                marks::remove(data_dir, &mark.run)?;
            }
            answers.push(ReapAnswer {
                run: mark.run,
                answer,
                left,
                forced,
            });
        }
        Ok(answers)
    }

    /// Gives the person's result to the open handback of `run`.
    pub fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), CallError> {
        let slot = self.slot(run)?;
        if slot.ended() {
            return Err(CallError::Ended);
        }
        let (tx, rx) = mpsc::channel();
        let sent = lock(&slot.events)
            .as_ref()
            .is_some_and(|events| events.send(Event::Handback(result, tx)).is_ok());
        if !sent {
            return Err(CallError::Ended);
        }
        rx.recv().unwrap_or(Err(CallError::Ended))
    }
}

/// What the end of a run reports.
#[derive(Default)]
pub(crate) struct Ending {
    pub(crate) outcome: Option<Outcome>,
    pub(crate) finish: Option<AgentRun>,
}

pub(crate) enum Flow {
    Go,
    Lost,
}

/// Runs one started run until the end of its runner's output, the runner's exit, a lost flow, or `link_end` after
/// its stored `finish` while its runner has neither exited nor ended its output; then `finish_run` closes the link
/// and ends the run.
fn drive(
    inner: Arc<Inner>,
    slot: Arc<Slot>,
    rx: Receiver<Event>,
    mark: MarkFile,
    started: Sender<Result<(), StartError>>,
) {
    let mut run = Running {
        inner: &inner,
        slot: &slot,
        mark,
        ending: Ending::default(),
        acts: Acts::default(),
    };
    if !run.handshake(&rx, inner.config.start_wait) {
        let (gone, exit) = slot
            .with_proc(|p| {
                p.end_all();
                (p.all_gone(), p.exit())
            })
            .unwrap_or((true, None));
        let _ = started.send(if gone { Err(not_started(exit)) } else { Ok(()) });
        finish_run(&inner, &slot, Ending::default());
        return;
    }
    let _ = started.send(Ok(()));
    let mut finished: Option<Instant> = None;
    loop {
        let flow = match rx.recv_timeout(TICK) {
            Ok(Event::Frame(frame)) => run.on_frame(*frame),
            Ok(Event::Handback(result, reply)) => {
                let _ = reply.send(run.answer(result));
                Flow::Go
            }
            Ok(Event::Broken) => Flow::Lost,
            Ok(Event::Eof) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if slot.with_proc(RunProc::exited).unwrap_or(true) {
                    break;
                }
                Flow::Go
            }
        };
        let flow = match flow {
            Flow::Go => run.retry_adds(),
            Flow::Lost => Flow::Lost,
        };
        if let Flow::Lost = flow {
            let path = if run.ending.finish.is_some() {
                ForcedPath::AfterFinish
            } else {
                ForcedPath::LinkEnd
            };
            slot.with_proc(|p| {
                p.set_path(path);
                p.end_all();
            });
            break;
        }
        if finished.is_none() && run.ending.finish.is_some() {
            finished = Some(Instant::now());
        }
        if finish_over(finished, Instant::now(), inner.config.link_end) {
            slot.with_proc(|p| p.set_path(ForcedPath::AfterFinish));
            break;
        }
    }
    let ending = run.ending;
    finish_run(&inner, &slot, ending);
}

/// Whether a run whose `finish` was stored at `finished`, as the loop of `drive` first saw it, is over at `now`:
/// `link_end` after its stored `finish` a runner that has neither exited nor ended its output gets its link closed,
/// as at the end of its output. A run with no stored `finish` is never over by this clock.
fn finish_over(finished: Option<Instant>, now: Instant, link_end: Duration) -> bool {
    finished.is_some_and(|at| now.saturating_duration_since(at) >= link_end)
}

pub(crate) struct Running<'a> {
    pub(crate) inner: &'a Inner,
    pub(crate) slot: &'a Slot,
    mark: MarkFile,
    ending: Ending,
    pub(crate) acts: Acts,
}

impl Running<'_> {
    /// Waits for the runner's `hello` for its run and agent, answering a `browser` frame that comes first so that
    /// the mark names the browser before anything can end the start.
    fn handshake(&mut self, rx: &Receiver<Event>, wait: Duration) -> bool {
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let frame = match rx.recv_timeout(left) {
                Ok(Event::Frame(frame)) => frame,
                Ok(Event::Handback(_, reply)) => {
                    let _ = reply.send(Err(CallError::Ended));
                    continue;
                }
                _ => return false,
            };
            match *frame {
                FromRunner::Hello(h) => {
                    return h.run == self.slot.spec.run && h.agent == self.slot.spec.agent;
                }
                FromRunner::Browser(b) => {
                    if let Flow::Lost = self.browser(&b) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
    }

    /// Answers one frame of the running run. After a stored `finish` a `confirm`, `handback`, `bind`, `secret` or
    /// `browser` frame is a frame out of turn: no trait is called, no reply is written and the run is lost, by the
    /// path `after_finish` (ruling CP-193 item 1). After the library wrote `bound=no` it takes progress, done and the
    /// store of the run's `finish` alone, and any other frame is lost (ruling CP-193 item 2). A request while the
    /// runner waits for a reply is lost too.
    fn on_frame(&mut self, frame: FromRunner) -> Flow {
        let after_finish = matches!(
            frame,
            FromRunner::Confirm(_)
                | FromRunner::Handback(_)
                | FromRunner::Bind(_)
                | FromRunner::Secret(_)
                | FromRunner::Browser(_)
        );
        if after_finish && self.ending.finish.is_some() {
            return Flow::Lost;
        }
        let end_of_declined = match &frame {
            FromRunner::Progress(_) | FromRunner::Done(_) => true,
            FromRunner::Store(s) => matches!(s.request.body(), RunnerCall::Finish(_)),
            _ => false,
        };
        if self.acts.declined() && !end_of_declined {
            return Flow::Lost;
        }
        let request = !matches!(frame, FromRunner::Progress(_) | FromRunner::Done(_));
        if request && self.acts.waiting() {
            return Flow::Lost;
        }
        match frame {
            FromRunner::Progress(p) => {
                self.inner
                    .parts
                    .human
                    .progress(&self.slot.spec.run, p.step, p.of);
                Flow::Go
            }
            FromRunner::Done(d) => {
                self.ending.outcome = Some(d.outcome);
                Flow::Go
            }
            FromRunner::Browser(b) => self.browser(&b),
            FromRunner::Store(s) => self.store(&s.request),
            FromRunner::Confirm(c) => self.confirm(&c.request),
            FromRunner::Handback(h) => self.handback(h.request),
            FromRunner::Bind(b) => self.bind(b.origin),
            FromRunner::Secret(s) => self.secret(&s.secret),
            FromRunner::Hello(_) => Flow::Lost,
        }
    }

    /// Writes the answer of `Secrets::read` as the answer of the same name, but one that no frame can carry as
    /// `unreadable` with `fault`.
    fn secret(&mut self, name: &Name) -> Flow {
        let fault = ToRunner::SecretReply(SecretReply::Unreadable(Unreadable::Fault));
        let reply = ToRunner::SecretReply(self.inner.parts.secrets.read(name));
        let reply = if reply.to_json().is_ok() {
            reply
        } else {
            fault
        };
        if self.slot.write(&reply) {
            Flow::Go
        } else {
            Flow::Lost
        }
    }

    fn store(&mut self, request: &StoreRequest) -> Flow {
        let id = request.id();
        let body = request.body();
        let spec = &self.slot.spec;
        let refuse = |error| StoreReply {
            id: Some(id.clone()),
            outcome: Err(error),
        };
        let reply = if body.agent() != &spec.agent || body.run() != &spec.run {
            refuse(binding())
        } else if self.ending.finish.is_some() {
            refuse(ReplyError::Workflow(ErrorName::State))
        } else {
            self.inner
                .parts
                .link
                .request(request.envelope().as_bytes())
                .ok()
                .and_then(|bytes| relayed(&bytes, request))
                .unwrap_or_else(|| refuse(ReplyError::Workflow(ErrorName::Internal)))
        };
        match (body, &reply.outcome) {
            (RunnerCall::Finish(f), Ok(_)) => {
                let _ = marks::remove(&self.inner.config.data_dir, &spec.run);
                self.ending.finish = Some(f.record.clone());
            }
            (RunnerCall::Load(_), Ok(RunnerReply::Load(load))) => {
                if let Some(bindings) = &load.bindings {
                    self.acts.bound.extend(bindings.origins.iter().cloned());
                }
            }
            _ => {}
        }
        if self
            .slot
            .write(&ToRunner::StoreReply(StoreReplyFrame { reply }))
        {
            Flow::Go
        } else {
            Flow::Lost
        }
    }

    fn browser(&mut self, frame: &Browser) -> Flow {
        let line = self.slot.with_proc(|p| {
            if p.has_browser() {
                return None;
            }
            p.browser_line(frame)
        });
        let Some(Some(line)) = line else {
            return Flow::Lost;
        };
        if self.mark.append(&proc_line("browser", &line)).is_err() {
            return Flow::Lost;
        }
        self.slot.with_proc(|p| p.set_browser(line));
        if self.slot.write(&ToRunner::BrowserReply(BrowserReply {})) {
            Flow::Go
        } else {
            Flow::Lost
        }
    }
}

/// Closes the link, gives the runner `link_end` (`LINK_END` by default) to end by itself and then ends what is
/// left, and reports the end. For a runner that outlived its stored `finish` this close comes `link_end` after the
/// `finish`, so the library ends such a runner once twice `link_end` has passed since its `finish`.
fn finish_run(inner: &Inner, slot: &Slot, ending: Ending) {
    slot.close_link();
    let deadline = Instant::now() + inner.config.link_end;
    loop {
        match slot.with_proc(RunProc::exited) {
            Some(false) if Instant::now() < deadline => thread::sleep(TICK),
            Some(false) => {
                slot.with_proc(RunProc::end_all);
                break;
            }
            _ => {
                slot.with_proc(RunProc::end_leftovers);
                break;
            }
        }
    }
    let end_run = if ending.finish.is_some() {
        EndRun::NotOwed
    } else if send_end_run(inner, &slot.spec) {
        let _ = marks::remove(&inner.config.data_dir, &slot.spec.run);
        EndRun::Acknowledged
    } else {
        EndRun::NotAcknowledged
    };
    let (exit, forced) = slot
        .with_proc(|p| (p.exit(), p.forced()))
        .unwrap_or((None, None));
    let end = RunEnd {
        outcome: ending.outcome,
        finish: ending.finish,
        end_run,
        exit,
        forced,
    };
    slot.ended.store(true, Ordering::SeqCst);
    inner.parts.marks.end(&slot.spec.run, &end);
}

fn not_started(exit: Option<RunnerExit>) -> StartError {
    StartError::NotStarted(NotStarted::Runner { exit })
}

fn binding() -> ReplyError {
    RunError::parse("binding").map_or(ReplyError::Workflow(ErrorName::Internal), ReplyError::Run)
}

/// The owner's reply to a runner request as the frame `store_reply` carries it, when it is a reply that the frame
/// can carry, for that request's id, and of the form that the request's call answers with.
fn relayed(bytes: &[u8], request: &StoreRequest) -> Option<StoreReply> {
    let text = std::str::from_utf8(bytes).ok()?;
    let frame = format!(r#"{{"t":"store_reply","reply":{text}}}"#);
    let ToRunner::StoreReply(StoreReplyFrame { reply }) = ToRunner::parse(frame.as_bytes()).ok()?
    else {
        return None;
    };
    if reply.id.as_ref() != Some(request.id()) {
        return None;
    }
    match &reply.outcome {
        Ok(r) if !r.answers(request.body()) => None,
        _ => Some(reply),
    }
}

/// Sends `end_run` for a run whose runner ended without a stored `finish` (A7.2) once; true when acknowledged.
fn send_end_run(inner: &Inner, spec: &RunSpec) -> bool {
    let Some(id) = owner::request_id() else {
        return false;
    };
    let call = CoordinatorCall::EndRun(EndRunBody {
        workflow: spec.workflow.clone(),
        agent: spec.agent.clone(),
        run: spec.run.clone(),
    });
    owner::acknowledged(inner, &id, call)
}

#[cfg(test)]
mod tests {
    use super::*;

    mod missing {
        use std::num::NonZeroU32;
        use std::path::PathBuf;

        use halvo_schema::executable::frame::{
            ConfirmDecision, ConfirmRequest, HandbackRequest, SecretReply,
        };
        use halvo_schema::executable::names::{Answer, Mode};
        use halvo_schema::executable::text::WebOrigin;
        use halvo_schema::{AgentId, WorkflowId};

        use super::*;
        use crate::api::{
            Confirmer, Human, IssueRefused, LinkError, MarkSink, OriginAnswer, OwnerLink, Secrets,
            SystemValue,
        };

        #[derive(Default)]
        struct Calls(Mutex<Vec<&'static str>>);

        impl Calls {
            fn note(&self, call: &'static str) {
                lock(&self.0).push(call);
            }
        }

        impl OwnerLink for Calls {
            fn request(&self, _: &[u8]) -> Result<Vec<u8>, LinkError> {
                self.note("link");
                Err(LinkError)
            }
        }

        impl Confirmer for Calls {
            fn answers(&self, _: &RunId, _: &ConfirmRequest) -> Vec<Answer> {
                self.note("answers");
                Vec::new()
            }
            fn issued(&self, _: &RunId, _: &ConfirmDecision) -> Result<(), IssueRefused> {
                self.note("issued");
                Err(IssueRefused)
            }
        }

        impl Human for Calls {
            fn handback(&self, _: &RunId, _: &HandbackRequest) {
                self.note("handback");
            }
            fn allow_origin(&self, _: &RunId, _: &WebOrigin) -> OriginAnswer {
                self.note("allow_origin");
                OriginAnswer::Decline
            }
            fn progress(&self, _: &RunId, _: NonZeroU32, _: NonZeroU32) {
                self.note("progress");
            }
        }

        impl Secrets for Calls {
            fn read(&self, _: &Name) -> SecretReply {
                self.note("secret");
                SecretReply::None
            }
        }

        impl MarkSink for Calls {
            fn start(&self, _: &RunId) {
                self.note("mark_start");
            }
            fn end(&self, _: &RunId, _: &RunEnd) {
                self.note("mark_end");
            }
        }

        #[cfg(target_os = "macos")]
        const READS: &[SystemValue] = &[SystemValue::TmpDir];
        #[cfg(windows)]
        const READS: &[SystemValue] = &[SystemValue::SystemRoot, SystemValue::Temp];

        fn none(_: SystemValue) -> Option<std::ffi::OsString> {
            None
        }

        fn only_system_root(value: SystemValue) -> Option<std::ffi::OsString> {
            (value == SystemValue::SystemRoot).then(|| "C:\\Windows".into())
        }

        fn unique(tag: &str) -> PathBuf {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            std::env::temp_dir().join(format!(
                "agent-client-missing-{tag}-{}-{nanos}",
                std::process::id()
            ))
        }

        fn refused_with(read: ReadValue, tag: &str) -> (Result<(), StartError>, Vec<&'static str>) {
            let root = unique(tag);
            let calls = Arc::new(Calls::default());
            let parts = Parts {
                link: calls.clone(),
                confirmer: calls.clone(),
                human: calls.clone(),
                secrets: calls.clone(),
                marks: calls.clone(),
            };
            let config = Config::new(root.join("data"), root.join("no-such-runner"));
            let sup = Supervisor::reading(CoordinatorRole::claim(), config, parts, read);
            let spec = RunSpec {
                run: RunId::from_random([7; 16]),
                agent: AgentId::from_random([8; 16]),
                workflow: WorkflowId::from_random([9; 16]),
                mode: Mode::Replay,
                browser: root.join("script"),
                model: None,
                headed: false,
            };
            let answer = sup.start(spec);
            assert!(
                !root.exists(),
                "the refused start wrote under its data directory"
            );
            let calls = lock(&calls.0).clone();
            (answer, calls)
        }

        #[test]
        fn a_start_whose_list_lacks_a_value_of_the_system_answers_not_started_naming_it_before_any_mark_or_process(
        ) {
            let (answer, calls) = refused_with(none, "none");
            assert_eq!(
                answer,
                Err(StartError::NotStarted(NotStarted::Missing(READS[0]))),
                "the first value of the list that the system did not give is named"
            );
            assert!(
                calls.is_empty(),
                "a refused start made these calls: {calls:?}"
            );
            if READS.len() > 1 {
                let (answer, calls) = refused_with(only_system_root, "temp");
                assert_eq!(
                    answer,
                    Err(StartError::NotStarted(NotStarted::Missing(
                        SystemValue::Temp
                    )))
                );
                assert!(calls.is_empty(), "{calls:?}");
            }
        }

        #[test]
        fn the_names_of_the_values_are_those_of_the_list() {
            assert_eq!(SystemValue::TmpDir.as_str(), "TMPDIR");
            assert_eq!(SystemValue::SystemRoot.as_str(), "SystemRoot");
            assert_eq!(SystemValue::Temp.as_str(), "TEMP");
        }
    }

    #[test]
    fn a_run_with_no_finish_is_never_over_and_one_with_a_finish_is_over_at_link_end_and_after_it() {
        let link_end = Duration::from_secs(4);
        let t0 = Instant::now();
        let later = |d: Duration| t0 + d;
        assert!(!finish_over(
            None,
            later(Duration::from_secs(3600)),
            link_end
        ));
        assert!(!finish_over(None, t0, Duration::ZERO));
        assert!(!finish_over(Some(t0), t0, link_end), "at the finish");
        assert!(
            !finish_over(Some(t0), later(link_end - TICK), link_end),
            "one tick before link_end"
        );
        assert!(
            finish_over(Some(t0), later(link_end), link_end),
            "at link_end"
        );
        assert!(
            finish_over(Some(t0), later(link_end + TICK), link_end),
            "a tick after link_end"
        );
        assert!(
            finish_over(Some(t0), later(Duration::from_secs(3600)), link_end),
            "long after link_end"
        );
    }
}
