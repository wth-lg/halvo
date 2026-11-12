use std::collections::HashMap;
use std::io;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use halvo_schema::executable::body::{CoordinatorCall, EndRun as EndRunBody, RunnerCall};
use halvo_schema::executable::frame::{
    Browser, BrowserReply, Cancel, FromRunner, HandbackResult, ReplyError, RunError, StoreReply,
    StoreReplyFrame, StoreRequest, ToRunner,
};
use halvo_schema::executable::names::Outcome;
use halvo_schema::executable::record::AgentRun;
use halvo_schema::executable::text::RunId;
use halvo_schema::request::{CoordinatorRole, ErrorName};

use crate::api::{CallError, Config, EndRun, Parts, ReapAnswer, Reaped, RunEnd, RunSpec, LINK_END};
use crate::marks::{self, proc_line, Created, MarkFile};
use crate::owner;
use crate::proc::{self, RunProc, SpawnFailed};
use crate::wire::{read_frame, write_frame};

const TICK: Duration = Duration::from_millis(100);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) enum Event {
    Frame(Box<FromRunner>),
    Broken,
    Eof,
}

/// One run that this life started.
pub(crate) struct Slot {
    pub(crate) spec: RunSpec,
    pub(crate) proc: Mutex<Option<RunProc>>,
    writer: Mutex<Option<ChildStdin>>,
    ended: AtomicBool,
}

impl Slot {
    fn new(spec: RunSpec) -> Slot {
        Slot {
            spec,
            proc: Mutex::new(None),
            writer: Mutex::new(None),
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
        Supervisor {
            inner: Arc::new(Inner {
                config,
                parts,
                role,
                runs: Mutex::new(HashMap::new()),
                spawn_lock: Mutex::new(()),
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
    /// life started or whose mark of an earlier life stands.
    pub fn start(&self, spec: RunSpec) -> Result<(), CallError> {
        let run = spec.run.clone();
        let slot = {
            let mut runs = lock(&self.inner.runs);
            if runs.contains_key(&run) {
                return Err(CallError::Started);
            }
            let slot = Arc::new(Slot::new(spec));
            runs.insert(run.clone(), Arc::clone(&slot));
            slot
        };
        let data_dir = &self.inner.config.data_dir;
        let mut mark = match MarkFile::create(data_dir, &run) {
            Ok(Created::Made(mark)) => mark,
            Ok(Created::Exists) => {
                lock(&self.inner.runs).remove(&run);
                return Err(CallError::Started);
            }
            Err(_) => {
                slot.ended.store(true, Ordering::SeqCst);
                return Err(CallError::NotStarted);
            }
        };
        self.inner.parts.marks.start(&run);
        let spawned = {
            let _one = lock(&self.inner.spawn_lock);
            proc::spawn(&mut self.command(&slot.spec))
        };
        let mut runner = match spawned {
            Ok(p) => p,
            Err(SpawnFailed::NotMade) => {
                let _ = marks::remove(data_dir, &run);
                self.inner.parts.marks.end(
                    &run,
                    &RunEnd {
                        outcome: None,
                        finish: None,
                        end_run: EndRun::NoRunner,
                    },
                );
                slot.ended.store(true, Ordering::SeqCst);
                return Err(CallError::NotStarted);
            }
            Err(SpawnFailed::Made(p)) => return self.fail_made(&slot, p),
        };
        let written = runner.line().map(|l| mark.append(&proc_line("runner", &l)));
        if !matches!(written, Some(Ok(()))) {
            return self.fail_made(&slot, runner);
        }
        let Some((stdin, stdout)) = runner.take_link() else {
            return self.fail_made(&slot, runner);
        };
        *lock(&slot.proc) = Some(runner);
        *lock(&slot.writer) = Some(stdin);
        let (tx, rx) = mpsc::channel();
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
        started_rx.recv().unwrap_or(Err(CallError::NotStarted))
    }

    fn fail_made(&self, slot: &Arc<Slot>, mut runner: RunProc) -> Result<(), CallError> {
        runner.end_all();
        let gone = runner.all_gone();
        *lock(&slot.proc) = Some(runner);
        let inner = Arc::clone(&self.inner);
        let s = Arc::clone(slot);
        thread::spawn(move || finish_run(&inner, &s, Ending::default()));
        if gone {
            Err(CallError::NotStarted)
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
            .stderr(Stdio::inherit());
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
        slot.with_proc(RunProc::end_all);
        Ok(())
    }

    /// Answers for every mark under the data directory, one answer per run: a run that this life started is
    /// `alive` and is left alone; for any other the library ends the processes of its mark that it may end, answers
    /// `gone` when its runner is gone, or its mark not complete, and then removes the mark, and `alive` when its
    /// runner lives and could not be ended.
    pub fn reap(&self) -> io::Result<Vec<ReapAnswer>> {
        let data_dir = &self.inner.config.data_dir;
        let mut answers = Vec::new();
        for mark in marks::read_all(data_dir)? {
            if lock(&self.inner.runs).contains_key(&mark.run) {
                answers.push(ReapAnswer {
                    run: mark.run,
                    answer: Reaped::Alive,
                    left: Vec::new(),
                });
                continue;
            }
            let (answer, left) = proc::reap_mark(&mark);
            if answer == Reaped::Gone {
                marks::remove(data_dir, &mark.run)?;
            }
            answers.push(ReapAnswer {
                run: mark.run,
                answer,
                left,
            });
        }
        Ok(answers)
    }

    /// Gives the person's result to the open handback of `run`.
    pub fn answer_handback(&self, run: &RunId, result: HandbackResult) -> Result<(), CallError> {
        let _ = result;
        self.slot(run)?;
        Err(CallError::Ended)
    }
}

/// What the end of a run reports.
#[derive(Default)]
pub(crate) struct Ending {
    pub(crate) outcome: Option<Outcome>,
    pub(crate) finish: Option<AgentRun>,
}

enum Flow {
    Go,
    Lost,
}

fn handshake(slot: &Slot, rx: &Receiver<Event>, wait: Duration) -> bool {
    match rx.recv_timeout(wait) {
        Ok(Event::Frame(frame)) => match *frame {
            FromRunner::Hello(h) => h.run == slot.spec.run && h.agent == slot.spec.agent,
            _ => false,
        },
        _ => false,
    }
}

fn drive(
    inner: Arc<Inner>,
    slot: Arc<Slot>,
    rx: Receiver<Event>,
    mark: MarkFile,
    started: Sender<Result<(), CallError>>,
) {
    if !handshake(&slot, &rx, inner.config.start_wait) {
        let gone = slot
            .with_proc(|p| {
                p.end_all();
                p.all_gone()
            })
            .unwrap_or(true);
        let _ = started.send(if gone {
            Err(CallError::NotStarted)
        } else {
            Ok(())
        });
        finish_run(&inner, &slot, Ending::default());
        return;
    }
    let _ = started.send(Ok(()));
    let mut run = Running {
        inner: &inner,
        slot: &slot,
        mark,
        ending: Ending::default(),
    };
    loop {
        match rx.recv_timeout(TICK) {
            Ok(Event::Frame(frame)) => {
                if let Flow::Lost = run.on_frame(*frame) {
                    slot.with_proc(RunProc::end_all);
                    break;
                }
            }
            Ok(Event::Broken) => {
                slot.with_proc(RunProc::end_all);
                break;
            }
            Ok(Event::Eof) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                if slot.with_proc(RunProc::exited).unwrap_or(true) {
                    break;
                }
            }
        }
    }
    let ending = run.ending;
    finish_run(&inner, &slot, ending);
}

struct Running<'a> {
    inner: &'a Inner,
    slot: &'a Slot,
    mark: MarkFile,
    ending: Ending,
}

impl Running<'_> {
    fn on_frame(&mut self, frame: FromRunner) -> Flow {
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
            _ => Flow::Lost,
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
        if let (RunnerCall::Finish(f), Ok(_)) = (body, &reply.outcome) {
            let _ = marks::remove(&self.inner.config.data_dir, &spec.run);
            self.ending.finish = Some(f.record.clone());
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

/// Closes the link, gives the runner `LINK_END` to end by itself and then ends what is left, and reports the end.
fn finish_run(inner: &Inner, slot: &Slot, ending: Ending) {
    slot.close_link();
    let deadline = Instant::now() + LINK_END;
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
    let end = RunEnd {
        outcome: ending.outcome,
        finish: ending.finish,
        end_run,
    };
    inner.parts.marks.end(&slot.spec.run, &end);
    slot.ended.store(true, Ordering::SeqCst);
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
