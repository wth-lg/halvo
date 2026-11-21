use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;
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
    absolute_paths, CallError, Config, EndRun, ForcedPath, NotStarted, Parts, ReapAnswer, ReapHook,
    ReapStep, Reaped, RelativePath, RunEnd, RunSpec, RunnerExit, StartError, SystemValue,
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
    cancelled: AtomicBool,
}

impl Slot {
    fn new(spec: RunSpec) -> Slot {
        Slot {
            spec,
            proc: Mutex::new(None),
            writer: Mutex::new(None),
            events: Mutex::new(None),
            ended: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
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

    /// Writes the frame `cancel` and records it under the lock of the writer, so that a frame that
    /// `write_unless_cancelled` writes comes before the cancel or not at all.
    fn write_cancel(&self) -> bool {
        let Ok(json) = ToRunner::Cancel(Cancel {}).to_json() else {
            return false;
        };
        let mut writer = lock(&self.writer);
        let written = writer
            .as_mut()
            .is_some_and(|w| write_frame(w, &json).is_ok());
        if written {
            self.cancelled.store(true, Ordering::SeqCst);
        }
        written
    }

    /// Whether the library wrote the frame `cancel` to the runner.
    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Writes one frame unless the library has written `cancel`: `None` when it had, else whether the write went.
    pub(crate) fn write_unless_cancelled(&self, frame: &ToRunner) -> Option<bool> {
        let Ok(json) = frame.to_json() else {
            return Some(false);
        };
        let mut writer = lock(&self.writer);
        if self.cancelled() {
            return None;
        }
        Some(
            writer
                .as_mut()
                .is_some_and(|w| write_frame(w, &json).is_ok()),
        )
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
    reap_hook: Mutex<Option<ReapHook>>,
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

    /// A supervisor whose start reads the values of the runner's environment list through `read` in place of the
    /// system: a seam of the crate's tests, which the app does not call.
    #[doc(hidden)]
    pub fn with_system_reader(
        role: CoordinatorRole,
        config: Config,
        parts: Parts,
        read: fn(SystemValue) -> Option<std::ffi::OsString>,
    ) -> Supervisor {
        Supervisor::reading(role, config, parts, read)
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
                reap_hook: Mutex::new(None),
            }),
        }
    }

    /// Sets the hook that `reap` calls at each `ReapStep`: a seam of the crate's tests, which the app does not call.
    #[doc(hidden)]
    pub fn set_reap_hook(&self, hook: Option<ReapHook>) {
        *lock(&self.inner.reap_hook) = hook;
    }

    fn reap_step(&self, step: ReapStep) {
        let hook = lock(&self.inner.reap_hook).clone();
        if let Some(hook) = hook {
            hook(step);
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
    /// life started or whose mark of an earlier life stands. The order of the answers: `started` for a run id in
    /// the run table, before any path or value is read; then `not_started` for a data directory that is not
    /// absolute; then `started` for a run id whose mark stands, read without writing it; then `not_started` for a
    /// runner path, `--browser` or `--model` that is not absolute, or for an environment list that lacks a value
    /// that the system did not give, with that path or value. Each comes before any process is made, before a mark
    /// is written and before `MarkSink::start`, and a start refused for a path or a value leaves nothing of the run
    /// id in the table. The insert into the table checks the table again under the same lock and answers `started`
    /// for a start of the same run id that came between. A start whose runner was made and exited before it said
    /// `hello` answers `not_started` with the runner's exit, the status 2 being a refusal, but for the case of
    /// ruling CP-29 (b): a start that cannot say whether what it made is gone answers ok. A start made while `reap`
    /// removes profiles does not wait for it. The runner's standard error is the null device: nobody reads it.
    pub fn start(&self, spec: RunSpec) -> Result<(), StartError> {
        if lock(&self.inner.runs).contains_key(&spec.run) {
            return Err(StartError::Started);
        }
        let data_dir = &self.inner.config.data_dir;
        if !data_dir.is_absolute() {
            return Err(StartError::NotStarted(NotStarted::Relative(
                RelativePath::DataDir,
            )));
        }
        if fs::symlink_metadata(marks::marks_dir(data_dir).join(spec.run.as_str())).is_ok() {
            return Err(StartError::Started);
        }
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
            .arg(&spec.browser)
            .arg("--utc-offset")
            .arg(spec.utc_offset_s.to_string());
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
        if slot.ended() || !slot.write_cancel() {
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
    /// `alive` when its runner lives and could not be ended. Then it removes each profile directory that a run left
    /// under `<data directory>/agents/profiles/` (contracts.md A8.3): just before it removes each entry, it reads
    /// under the lock of the run table whether a run of this life that has not ended, or a run that this reap
    /// answered `alive`, names that entry, and keeps the entry if so; the removal runs outside the lock, so a start,
    /// a kill, an answer and the runs never wait on a reap. That holds because a start puts its run id into the
    /// table before it writes the mark or makes any process, and only the runner makes a profile. The one case left:
    /// a run id that this reap answered `gone` and that is started again while its old directory is still being
    /// removed; the runner then refuses the existing path (A8.3). A mark that cannot be read makes `reap` answer
    /// `Err`, with no profile removed.
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
        let alive: HashSet<RunId> = answers
            .iter()
            .filter(|a| a.answer == Reaped::Alive)
            .map(|a| a.run.clone())
            .collect();
        self.reap_step(ReapStep::Answered);
        remove_profiles(
            data_dir,
            |run| {
                alive.contains(run)
                    || lock(&self.inner.runs)
                        .get(run)
                        .is_some_and(|slot| !slot.ended())
            },
            |step| self.reap_step(step),
        );
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

/// How a start's wait for the runner's `hello` ended.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Handshake {
    Hello,
    Eof,
    Failed,
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
    let shaken = run.handshake(&rx, inner.config.start_wait);
    if shaken != Handshake::Hello {
        if shaken == Handshake::Eof {
            let deadline = Instant::now() + proc::SETTLE;
            while !slot.with_proc(RunProc::exited).unwrap_or(true) && Instant::now() < deadline {
                thread::sleep(proc::POLL);
            }
        }
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
    fn handshake(&mut self, rx: &Receiver<Event>, wait: Duration) -> Handshake {
        let deadline = Instant::now() + wait;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let frame = match rx.recv_timeout(left) {
                Ok(Event::Frame(frame)) => frame,
                Ok(Event::Handback(_, reply)) => {
                    let _ = reply.send(Err(CallError::Ended));
                    continue;
                }
                Ok(Event::Eof) => return Handshake::Eof,
                _ => return Handshake::Failed,
            };
            match *frame {
                FromRunner::Hello(h) => {
                    return if h.run == self.slot.spec.run && h.agent == self.slot.spec.agent {
                        Handshake::Hello
                    } else {
                        Handshake::Failed
                    };
                }
                FromRunner::Browser(b) => {
                    if let Flow::Lost = self.browser(&b) {
                        return Handshake::Failed;
                    }
                }
                _ => return Handshake::Failed,
            }
        }
    }

    /// Answers one frame of the running run. After a stored `finish` a `confirm`, `handback`, `bind`, `secret` or
    /// `browser` frame is a frame out of turn: no trait is called, no reply is written and the run is lost, by the
    /// path `after_finish` (ruling CP-193 item 1). After the library wrote `bound=no` it takes progress, done and the
    /// store of the run's `finish` alone, and any other frame is lost (ruling CP-193 item 2). After it wrote
    /// `cancel` to a run whose bind is open, it drops that bind and takes the same three alone (ruling CP-193 item
    /// 3). A request while the runner waits for a reply is lost too.
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
        self.drop_cancelled_bind();
        if self.acts.cancelled_bind() {
            return if end_of_declined {
                self.on_turn(frame)
            } else {
                Flow::Lost
            };
        }
        let request = !matches!(frame, FromRunner::Progress(_) | FromRunner::Done(_));
        if request && self.acts.waiting() {
            return Flow::Lost;
        }
        self.on_turn(frame)
    }

    fn on_turn(&mut self, frame: FromRunner) -> Flow {
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

    /// Writes the answer of `Secrets::read` for the run's agent as the answer of the same name, but one that no frame
    /// can carry as `unreadable` with `fault`.
    fn secret(&mut self, name: &Name) -> Flow {
        let fault = ToRunner::SecretReply(SecretReply::Unreadable(Unreadable::Fault));
        let secrets = &self.inner.parts.secrets;
        let reply = ToRunner::SecretReply(secrets.read(&self.slot.spec.agent, name));
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
            // A link error is a stream that ended before a whole reply, never a completed write (A6.6): `store`, on
            // which the runner repeats the request.
            match self.inner.parts.link.request(request.envelope().as_bytes()) {
                Err(_) => refuse(ReplyError::Workflow(ErrorName::Store)),
                Ok(bytes) => relayed(&bytes, request)
                    .unwrap_or_else(|| refuse(ReplyError::Workflow(ErrorName::Internal))),
            }
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

/// Removes each directory under `<data_dir>/agents/profiles/` whose name is a run id that `keep` does not keep,
/// asked just before each removal; an entry that is a link, that is no directory or whose name is no run id stays,
/// and nothing is removed when `agents` or `profiles` is a link or no directory. `step` runs at the seam's points.
/// On macOS the folder `profiles` is opened once from the data directory, through `agents`, each without following
/// a link, and every entry is listed and removed relative to that descriptor: a directory below it is opened
/// without following a link and emptied through its own descriptor, and a link is removed as itself. On Windows
/// `agents` and then `profiles` are opened as handles that follow no reparse point, refused when either is one or is
/// no directory, and held for the whole walk with a share mode that leaves out deletion, so that neither can be
/// renamed, removed or replaced while the walk holds it; each entry is then removed by its path under them with
/// `fs::remove_dir_all`, which removes a reparse point that it meets as itself and follows none. What the Windows form
/// rests on is that share mode; an entry below `profiles` that is replaced by a link between the listing and its
/// removal is removed as a link, and what the link names stays. A directory that cannot be removed stays until the
/// next reap.
fn remove_profiles(data_dir: &Path, keep: impl Fn(&RunId) -> bool, step: impl Fn(ReapStep)) {
    let Some(held) = HeldProfiles::open(data_dir) else {
        return;
    };
    step(ReapStep::ProfilesHeld);
    let Some(entries) = held.entries() else {
        return;
    };
    step(ReapStep::Listed);
    for (name, is_dir) in entries {
        if !is_dir {
            continue;
        }
        let Some(run) = name.to_str().and_then(|n| RunId::parse(n).ok()) else {
            continue;
        };
        if !keep(&run) {
            held.remove(&name);
        }
    }
}

/// The folder `profiles` as `remove_profiles` holds it.
#[cfg(target_os = "macos")]
struct HeldProfiles(crate::sys::Dir);

#[cfg(target_os = "macos")]
impl HeldProfiles {
    fn open(data_dir: &Path) -> Option<HeldProfiles> {
        let root = crate::sys::Dir::open(data_dir).ok()?;
        let agents = root.child("agents".as_ref()).ok()?;
        agents.child("profiles".as_ref()).ok().map(HeldProfiles)
    }

    fn entries(&self) -> Option<Vec<(std::ffi::OsString, bool)>> {
        self.0.entries().ok()
    }

    fn remove(&self, name: &std::ffi::OsStr) {
        let _ = self.0.remove_tree(name);
    }
}

/// The folder `profiles` as `remove_profiles` holds it: its path, and the handles of `agents` and `profiles`.
#[cfg(windows)]
struct HeldProfiles {
    path: std::path::PathBuf,
    _held: [fs::File; 2],
}

#[cfg(windows)]
impl HeldProfiles {
    fn open(data_dir: &Path) -> Option<HeldProfiles> {
        let agents = data_dir.join("agents");
        let path = agents.join("profiles");
        let first = hold_folder(&agents)?;
        let second = hold_folder(&path)?;
        Some(HeldProfiles {
            path,
            _held: [first, second],
        })
    }

    fn entries(&self) -> Option<Vec<(std::ffi::OsString, bool)>> {
        let found = fs::read_dir(&self.path).ok()?;
        Some(
            found
                .flatten()
                .map(|e| (e.file_name(), e.file_type().is_ok_and(|t| t.is_dir())))
                .collect(),
        )
    }

    fn remove(&self, name: &std::ffi::OsStr) {
        let _ = fs::remove_dir_all(self.path.join(name));
    }
}

/// Opens the folder `path` without following a reparse point, with a share mode that lets others read and write in
/// it but not delete or rename it; none when it is a reparse point or no directory.
#[cfg(windows)]
fn hold_folder(path: &Path) -> Option<fs::File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    const FILE_LIST_DIRECTORY: u32 = 0x0001;
    const FILE_READ_ATTRIBUTES: u32 = 0x0080;
    const SYNCHRONIZE: u32 = 0x0010_0000;
    const FILE_SHARE_READ: u32 = 0x0001;
    const FILE_SHARE_WRITE: u32 = 0x0002;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x0010;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
    let file = fs::OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .ok()?;
    let attributes = file.metadata().ok()?.file_attributes();
    let folder = attributes & FILE_ATTRIBUTE_DIRECTORY != 0
        && attributes & FILE_ATTRIBUTE_REPARSE_POINT == 0;
    folder.then_some(file)
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
    owner::acknowledged(inner, &id, call) == owner::Ack::Acknowledged
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
            fn read(&self, _: &AgentId, _: &Name) -> SecretReply {
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
                utc_offset_s: 0,
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
