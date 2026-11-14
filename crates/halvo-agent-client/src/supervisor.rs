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
use crate::api::{CallError, Config, EndRun, Parts, ReapAnswer, Reaped, RunEnd, RunSpec};
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
        let slot = Arc::new(Slot::new(spec));
        let mut held = lock(&slot.proc);
        {
            let mut runs = lock(&self.inner.runs);
            if runs.contains_key(&run) {
                return Err(CallError::Started);
            }
            runs.insert(run.clone(), Arc::clone(&slot));
        }
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
                drop(held);
                let _ = marks::remove(data_dir, &run);
                slot.ended.store(true, Ordering::SeqCst);
                self.inner.parts.marks.end(
                    &run,
                    &RunEnd {
                        outcome: None,
                        finish: None,
                        end_run: EndRun::NoRunner,
                    },
                );
                return Err(CallError::NotStarted);
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
        started_rx.recv().unwrap_or(Err(CallError::NotStarted))
    }

    fn fail_made(
        &self,
        slot: &Arc<Slot>,
        mut held: MutexGuard<'_, Option<RunProc>>,
        mut runner: RunProc,
    ) -> Result<(), CallError> {
        runner.end_all();
        let gone = runner.all_gone();
        *held = Some(runner);
        drop(held);
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
        match slot.with_proc(RunProc::end_all) {
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
    started: Sender<Result<(), CallError>>,
) {
    let mut run = Running {
        inner: &inner,
        slot: &slot,
        mark,
        ending: Ending::default(),
        acts: Acts::default(),
    };
    if !run.handshake(&rx, inner.config.start_wait) {
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
            slot.with_proc(RunProc::end_all);
            break;
        }
        if finished.is_none() && run.ending.finish.is_some() {
            finished = Some(Instant::now());
        }
        if finish_over(finished, Instant::now(), inner.config.link_end) {
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

    fn on_frame(&mut self, frame: FromRunner) -> Flow {
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
    let end = RunEnd {
        outcome: ending.outcome,
        finish: ending.finish,
        end_run,
    };
    slot.ended.store(true, Ordering::SeqCst);
    inner.parts.marks.end(&slot.spec.run, &end);
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
