//! The tests of the kept sign-in (A8.3), each on a stand-in of the executor whose browser is a jar of cookies and
//! localStorage items, with no browser and no window. The stand-in takes the place of the browser's start, so a run
//! goes through the start's own path: the frame `browser`, its reply, the rule's arm and the import. The test's
//! supervisor answers the runner's bodies from a store of session parts that outlives a run, so that a second run
//! loads what the first one stored, and every call of the stand-in and every frame is one line of one log.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::num::{NonZeroU32, NonZeroU64};
use std::sync::{mpsc, Arc, Mutex};

use halvo_policy::preset;
use halvo_schema::executable::body::{LoadReply, LoadedBindings, LoadedSession};
use halvo_schema::executable::frame::Browser;
use halvo_schema::executable::names::LogEvent;
use halvo_schema::executable::step::Cond;
use halvo_schema::executable::text::{CreationTime, SessionData};
use halvo_schema::executable::{Cause, ExecutableStep, FromRunner, Outcome, RunnerCall, WebOrigin};
use halvo_schema::{AgentId, Category, WorkflowId};
use halvo_web::cdp::{domain_matches, host_of, OriginStorage, PausedRequest, SessionState, Sign};

use super::{parts_of, TooLarge};
use crate::link::{encode, Link};
use crate::run::{Failure, Runner, Stop};
use crate::web::{Act, Page, Web};

type Log = Arc<Mutex<Vec<String>>>;

fn note(log: &Log, line: impl Into<String>) {
    log.lock().expect("the log").push(line.into());
}

/// The data directory of the runs of these tests, absolute on each system, as `--data-dir` wants it; the stand-in of
/// the browser makes nothing in it.
const DATA_DIR: &str = if cfg!(windows) {
    r"C:\halvo-runner-session-test-data"
} else {
    "/halvo-runner-session-test-data"
};

const A: &str = "http://a.test:8080";
const B: &str = "http://b.test:8081";
const C: &str = "http://c.test:8082";

fn origin(text: &str) -> WebOrigin {
    WebOrigin::parse(text).expect("an origin")
}

/// A cookie of `domain` with `value`, HttpOnly.
fn cookie(name: &str, value: &str, domain: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name, "value": value, "domain": domain, "path": "/", "expires": 2_000_000_000.0, "size": 10,
        "httpOnly": true, "secure": false, "session": false, "sameSite": "Lax", "priority": "Medium",
        "sourceScheme": "NonSecure", "sourcePort": 8080
    })
}

/// A browser's state: `cookies` and, for each origin, its items.
fn jar(cookies: &[serde_json::Value], storage: &[(&str, &[(&str, &str)])]) -> SessionState {
    let storage: Vec<serde_json::Value> = storage
        .iter()
        .map(|(o, items)| serde_json::json!({"origin": o, "items": items}))
        .collect();
    let text =
        serde_json::json!({"origins": [], "cookies": cookies, "storage": storage}).to_string();
    SessionState::from_json(&text).expect("a state")
}

/// What the browser of `jar` exports for `origin`: as the executor's export reads it, the cookies of the origin's
/// host and of its parent domains and the origin's items.
fn export_of(jar: &SessionState, origin: &str) -> SessionState {
    SessionState {
        origins: vec![origin.to_string()],
        cookies: jar
            .cookies
            .iter()
            .filter(|c| domain_matches(&c.domain, host_of(origin)))
            .cloned()
            .collect(),
        storage: jar
            .storage
            .iter()
            .filter(|s| s.origin == origin)
            .cloned()
            .collect(),
    }
}

/// Puts the cookies of `state` into `jar`, each in place of one of the same name, domain and path, and the items of
/// each of its origins in place of that origin's items.
fn put_into(jar: &mut SessionState, state: &SessionState) {
    for c in &state.cookies {
        jar.cookies
            .retain(|k| (&k.name, &k.domain, &k.path) != (&c.name, &c.domain, &c.path));
        jar.cookies.push(c.clone());
    }
    for s in &state.storage {
        jar.storage.retain(|k| k.origin != s.origin);
        jar.storage.push(s.clone());
    }
}

/// The stand-in of the executor: its browser is `jar`; each reading gives the next sign that the test queued (none
/// once the queue is empty); the page holds `address`; every call is one line of the log, with no value of the jar.
/// The clear of a handback's opening clears the localStorage of every origin of the jar and keeps the cookies, as the
/// executor's clears every origin that the run loaded; the person signs in to `person` while the handback is open
/// (at the arm of its mode); an import fails where `import_fails` holds; the first act that does not fail takes the
/// page to the address of `next_site`, and the person then signs in to its state.
struct StandIn {
    log: Log,
    jar: Arc<Mutex<SessionState>>,
    signs: VecDeque<Option<Sign>>,
    address: String,
    act_fails: bool,
    person: Option<SessionState>,
    import_fails: bool,
    next_site: Option<(&'static str, SessionState)>,
}

impl Page for StandIn {
    fn frame(&self) -> Option<FromRunner> {
        Some(FromRunner::Browser(Browser {
            pid: NonZeroU32::new(4242)?,
            created: CreationTime::from_micros(NonZeroU64::new(1)?),
            group: None,
        }))
    }

    fn arm(&mut self, origins: &[WebOrigin]) -> Result<(), ()> {
        let origins: Vec<&str> = origins.iter().map(WebOrigin::as_str).collect();
        note(&self.log, format!("arm [{}]", origins.join(" ")));
        Ok(())
    }

    fn carry_out(
        &mut self,
        _: &[(&Cond, Option<String>)],
        act: &Act<'_>,
        _: &[(&Cond, Option<String>)],
    ) -> Result<Option<String>, ()> {
        note(&self.log, format!("act {}", act.verb()));
        if self.act_fails {
            return Err(());
        }
        if let Some((address, person)) = self.next_site.take() {
            self.address = address.to_string();
            self.person = Some(person);
        }
        Ok(None)
    }

    fn record(&mut self) -> Vec<PausedRequest> {
        Vec::new()
    }

    fn record_after_a_fault(&mut self) -> Vec<PausedRequest> {
        Vec::new()
    }

    fn stop(self: Box<Self>) -> bool {
        note(&self.log, "stop");
        true
    }

    fn sign(&mut self) -> Result<Option<Sign>, ()> {
        note(&self.log, "read");
        Ok(self.signs.pop_front().flatten())
    }

    fn fail_held(&mut self) -> Result<(), ()> {
        Ok(())
    }

    fn close_others(&mut self) -> Result<(), ()> {
        Ok(())
    }

    fn clear_stores(&mut self) -> Result<(), ()> {
        note(&self.log, "clear stores");
        self.jar.lock().expect("the jar").storage.clear();
        Ok(())
    }

    fn load_again(&mut self) -> Result<(), ()> {
        Ok(())
    }

    fn pass_all(&mut self) -> Result<(), ()> {
        if let Some(person) = &self.person {
            put_into(&mut self.jar.lock().expect("the jar"), person);
        }
        Ok(())
    }

    fn address(&mut self) -> Result<String, ()> {
        Ok(self.address.clone())
    }

    fn export(&mut self, origin: &WebOrigin) -> Result<SessionState, ()> {
        note(&self.log, format!("export {}", origin.as_str()));
        Ok(export_of(
            &self.jar.lock().expect("the jar"),
            origin.as_str(),
        ))
    }

    fn import(&mut self, state: &SessionState) -> Result<(), ()> {
        note(&self.log, "import");
        if self.import_fails {
            return Err(());
        }
        put_into(&mut self.jar.lock().expect("the jar"), state);
        Ok(())
    }
}

/// The runner's standard input: the bytes that the supervisor sends, an end of file once every sender is gone.
struct Input {
    from: mpsc::Receiver<Vec<u8>>,
    bytes: Vec<u8>,
    at: usize,
}

impl Read for Input {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.at == self.bytes.len() {
            match self.from.recv() {
                Ok(bytes) => (self.bytes, self.at) = (bytes, 0),
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.bytes.len() - self.at);
        out[..n].copy_from_slice(&self.bytes[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

/// The parts that the store holds, by origin, revision and part: the count of parts and the pieces.
type Parts = BTreeMap<(String, u32, u32), (u32, Vec<String>)>;

/// What the supervisor saw of one run, beside the log.
#[derive(Default)]
struct Seen {
    /// The Debug text of every frame of the runner.
    debug: Vec<String>,
    /// The JSON of every frame of the runner but `store`, and of every answer to it but `store_reply`.
    texts: Vec<String>,
    /// Each log record's event, step and cause.
    logs: Vec<(LogEvent, Option<u32>, Option<Cause>)>,
    /// The outcome of `finish`.
    finish: Option<Outcome>,
    /// The JSON of every `store` frame of the runner.
    stores: Vec<String>,
}

/// The supervisor and the store, on the runner's standard output: it answers each `store` frame at once from `parts`,
/// `browser` with its reply, and each handback request and each `bind` by the next answer that the test queued.
struct Supervisor {
    log: Log,
    seen: Arc<Mutex<Seen>>,
    parts: Arc<Mutex<Parts>>,
    to_runner: mpsc::Sender<Vec<u8>>,
    answers: VecDeque<&'static str>,
    bytes: Vec<u8>,
}

impl Supervisor {
    fn send(&mut self, text: String, store: bool) {
        if !store {
            self.seen.lock().expect("seen").texts.push(text.clone());
        }
        let _ = self.to_runner.send(encode(&text));
    }

    fn answer(&mut self, frame: &FromRunner) {
        let ok = |id: &str, ok: String| {
            format!(r#"{{"t":"store_reply","reply":{{"v":1,"id":"{id}","ok":{ok}}}}}"#)
        };
        match frame {
            FromRunner::Store(s) => {
                let id = s.request.id().as_str().to_string();
                let reply = match s.request.body() {
                    RunnerCall::LoadSession(b) => {
                        note(
                            &self.log,
                            format!(
                                "frame store load_session {} {} {}",
                                b.origin.as_str(),
                                b.rev,
                                b.part
                            ),
                        );
                        let key = (b.origin.as_str().to_string(), b.rev.get(), b.part.get());
                        let (of, pieces) = self.parts.lock().expect("parts")[&key].clone();
                        let data = serde_json::to_string(&pieces).expect("pieces");
                        format!(
                            r#"{{"agent":"{}","origin":"{}","rev":{},"part":{},"of":{of},"data":{data}}}"#,
                            b.agent.as_str(),
                            b.origin.as_str(),
                            b.rev,
                            b.part
                        )
                    }
                    RunnerCall::PutSession(b) => {
                        note(
                            &self.log,
                            format!(
                                "frame store put_session {} {} {} {}",
                                b.origin.as_str(),
                                b.rev,
                                b.part,
                                b.of
                            ),
                        );
                        let key = (b.origin.as_str().to_string(), b.rev.get(), b.part.get());
                        let pieces = b.data.pieces().to_vec();
                        self.parts
                            .lock()
                            .expect("parts")
                            .insert(key, (b.of.get(), pieces));
                        r#"{"stored":1}"#.to_string()
                    }
                    RunnerCall::Log(b) => {
                        note(&self.log, "frame store log");
                        let mut seen = self.seen.lock().expect("seen");
                        for r in &b.records {
                            seen.logs
                                .push((r.event, r.step.map(NonZeroU32::get), r.cause));
                        }
                        format!(r#"{{"stored":{}}}"#, b.records.len())
                    }
                    RunnerCall::DropSession(b) => {
                        note(
                            &self.log,
                            format!("frame store drop_session {}", b.origin.as_str()),
                        );
                        let mut parts = self.parts.lock().expect("parts");
                        let o = b.origin.as_str().to_string();
                        let rev = parts
                            .keys()
                            .filter(|(k, _, _)| *k == o)
                            .map(|(_, r, _)| *r)
                            .max();
                        let dropped = SessionData::dropped().pieces().to_vec();
                        parts.insert((o, rev.unwrap_or(0) + 1, 1), (1, dropped));
                        r#"{"stored":1}"#.to_string()
                    }
                    RunnerCall::Finish(b) => {
                        note(&self.log, "frame store finish");
                        self.seen.lock().expect("seen").finish = Some(b.record.outcome);
                        r#"{"stored":1}"#.to_string()
                    }
                    other => {
                        note(&self.log, format!("frame store {}", other.op()));
                        r#"{"stored":1}"#.to_string()
                    }
                };
                self.send(ok(&id, reply), true);
            }
            FromRunner::Browser(_) => {
                note(&self.log, "frame browser");
                self.send(r#"{"t":"browser_reply"}"#.to_string(), false);
            }
            FromRunner::Handback(h) => {
                note(
                    &self.log,
                    format!("frame handback {}", h.request.reason.as_str()),
                );
                let answer = self
                    .answers
                    .pop_front()
                    .expect("an answer that the test queued");
                self.send(answer.to_string(), false);
            }
            FromRunner::Bind(b) => {
                note(&self.log, format!("frame bind {}", b.origin.as_str()));
                let answer = self
                    .answers
                    .pop_front()
                    .expect("an answer that the test queued");
                self.send(answer.to_string(), false);
            }
            FromRunner::Done(d) => note(&self.log, format!("frame done {}", d.outcome.as_str())),
            other => note(
                &self.log,
                format!("frame {}", other.to_json().unwrap_or_default()),
            ),
        }
    }
}

impl Write for Supervisor {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes.extend_from_slice(bytes);
        while self.bytes.len() >= 4 {
            let length =
                u32::from_be_bytes([self.bytes[0], self.bytes[1], self.bytes[2], self.bytes[3]]);
            let end = 4 + length as usize;
            if self.bytes.len() < end {
                break;
            }
            let json: Vec<u8> = self.bytes.drain(..end).skip(4).collect();
            let frame = FromRunner::parse(&json).expect("a frame of the runner");
            {
                let mut seen = self.seen.lock().expect("seen");
                seen.debug.push(format!("{frame:?}"));
                if matches!(frame, FromRunner::Store(_)) {
                    seen.stores
                        .push(String::from_utf8_lossy(&json).into_owned());
                } else {
                    seen.texts.push(String::from_utf8_lossy(&json).into_owned());
                }
            }
            self.answer(&frame);
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

const DONE: &str = r#"{"t":"handback_reply","result":{"v":1,"outcome":"done","secret":null}}"#;
const DECLINED: &str =
    r#"{"t":"handback_reply","result":{"v":1,"outcome":"declined","secret":null}}"#;
const BOUND_YES: &str = r#"{"t":"bind_reply","bound":"yes"}"#;

/// The store of session parts that the runs of one test share.
#[derive(Default)]
struct World {
    parts: Arc<Mutex<Parts>>,
}

/// One run: the browser's jar as the stand-in starts it, the bound origins, the readings, the page's address, the
/// answers to handback requests and binds, whether the act fails, whether the run has a window, the state that the
/// person signs in to while a handback is open, whether the import fails, and the site that the first act goes to.
struct Run<'a> {
    jar: SessionState,
    bound: &'a [&'a str],
    signs: &'a [Option<Sign>],
    address: &'a str,
    answers: &'a [&'static str],
    act_fails: bool,
    headed: bool,
    person: Option<SessionState>,
    import_fails: bool,
    next_site: Option<(&'static str, SessionState)>,
}

impl Run<'_> {
    fn new<'a>(jar: SessionState, bound: &'a [&'a str]) -> Run<'a> {
        Run {
            jar,
            bound,
            signs: &[],
            address: A,
            answers: &[],
            act_fails: false,
            headed: false,
            person: None,
            import_fails: false,
            next_site: None,
        }
    }
}

/// What one run left: the log, what the supervisor saw, the browser's jar at the end, the run's end and its status.
struct Ran {
    log: Vec<String>,
    seen: Seen,
    jar: SessionState,
    end: String,
}

impl World {
    /// For each origin, the newest revision whose parts are all stored, as the store's `load` lists it.
    fn listed(&self) -> Vec<LoadedSession> {
        let parts = self.parts.lock().expect("parts");
        let mut newest: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        for ((o, rev, _), (of, _)) in parts.iter() {
            let whole = (1..=*of).all(|p| parts.contains_key(&(o.clone(), *rev, p)));
            if whole && newest.get(o).is_none_or(|(r, _)| r < rev) {
                newest.insert(o.clone(), (*rev, *of));
            }
        }
        newest
            .into_iter()
            .map(|(o, (rev, of))| LoadedSession {
                origin: origin(&o),
                rev: NonZeroU32::new(rev).expect("a revision"),
                parts: NonZeroU32::new(of).expect("a count"),
            })
            .collect()
    }

    /// Stores `state` as revision `rev` of `origin`, as a run before would have.
    fn store(&self, origin: &str, rev: u32, state: &SessionState) {
        let parts = parts_of(state).expect("a state within the limit");
        let of = parts.len() as u32;
        let mut held = self.parts.lock().expect("parts");
        for (n, data) in (1..).zip(parts) {
            held.insert((origin.to_string(), rev, n), (of, data.pieces().to_vec()));
        }
    }

    /// The state stored as revision `rev` of `origin`, its parts joined and read back.
    fn stored(&self, origin: &str, rev: u32) -> Option<SessionState> {
        let parts = self.parts.lock().expect("parts");
        let of = parts.get(&(origin.to_string(), rev, 1))?.0;
        let mut text = String::new();
        for n in 1..=of {
            parts
                .get(&(origin.to_string(), rev, n))?
                .1
                .iter()
                .for_each(|p| text.push_str(p));
        }
        super::state_of(&text)
    }

    /// The parts stored for `origin`, by revision and part.
    fn revisions(&self, origin: &str) -> Vec<(u32, u32, u32)> {
        let parts = self.parts.lock().expect("parts");
        parts
            .iter()
            .filter(|((o, _, _), _)| o == origin)
            .map(|((_, rev, part), (of, _))| (*rev, *part, *of))
            .collect()
    }

    /// Carries out `steps` as the run's loop does, on a runner whose browser start gives the stand-in, then ends the
    /// run as `Runner::run` ends it.
    fn run(&self, run: &Run<'_>, steps: &[ExecutableStep]) -> Ran {
        let log: Log = Arc::default();
        let seen: Arc<Mutex<Seen>> = Arc::default();
        let (to_runner, from) = mpsc::channel();
        let supervisor = Supervisor {
            log: Arc::clone(&log),
            seen: Arc::clone(&seen),
            parts: Arc::clone(&self.parts),
            to_runner,
            answers: run.answers.iter().copied().collect(),
            bytes: Vec::new(),
        };
        let input = Input {
            from,
            bytes: Vec::new(),
            at: 0,
        };
        let link = Link::start(Box::new(input), Box::new(supervisor), Box::new(|| {}));
        let (workflow, agent) = (
            WorkflowId::from_random([7; 16]),
            AgentId::from_random([6; 16]),
        );
        let id = "0e".repeat(16);
        let mut words = vec![
            "--run",
            &id,
            "--agent",
            agent.as_str(),
            "--workflow",
            workflow.as_str(),
            "--mode",
            "replay",
            "--browser",
            "none",
            "--data-dir",
            DATA_DIR,
            "--utc-offset",
            "0",
        ];
        if run.headed {
            words.push("--headed");
        }
        let words: Vec<OsString> = words.iter().map(OsString::from).collect();
        let args = crate::args::parse(&words).expect("the arguments");
        let mut runner = Runner::new(args, link);
        let jar = Arc::new(Mutex::new(run.jar.clone()));
        runner.stand_in = Some(Web(Box::new(StandIn {
            log: Arc::clone(&log),
            jar: Arc::clone(&jar),
            signs: run.signs.iter().copied().collect(),
            address: run.address.to_string(),
            act_fails: run.act_fails,
            person: run.person.clone(),
            import_fails: run.import_fails,
            next_site: run.next_site.clone(),
        })));
        let config = preset(Category::UtilityBillPayment, workflow, agent).expect("a preset");
        let loaded = LoadReply {
            config,
            playbook: None,
            next_playbook: NonZeroU32::MIN,
            bindings: Some(LoadedBindings {
                rev: NonZeroU32::MIN,
                origins: run.bound.iter().map(|o| origin(o)).collect(),
                slots: Vec::new(),
            }),
            sessions: self.listed(),
            start: None,
        };
        let mut end = Ok(Outcome::Completed);
        for step in steps {
            match runner.web_step(step, &loaded) {
                Ok(()) => {}
                Err(Failure::Stop(stop)) => {
                    end = Err(stop);
                    break;
                }
                Err(Failure::Step(cause)) => {
                    end = Err(Stop::End(Outcome::Failed, Some(cause)));
                    break;
                }
            }
        }
        let end_text = format!("{end:?}");
        let status = runner.end(end);
        let log = log.lock().expect("the log").clone();
        let seen = std::mem::take(&mut *seen.lock().expect("seen"));
        let jar = jar.lock().expect("the jar").clone();
        Ran {
            log,
            seen,
            jar,
            end: format!("{end_text} {status}"),
        }
    }
}

fn web_step(json: &str) -> ExecutableStep {
    ExecutableStep::parse(json.as_bytes()).expect("a web step")
}

fn click() -> ExecutableStep {
    web_step(
        r#"{"v":1,"n":1,"kind":"web","verb":"click","target":{"desc":"the form's button","frame":[],"credential":"no","locators":[{"by":"test_id","value":"send","name":null}]},"args":[],"pre":[],"post":[],"effect":"none","class":"prepare"}"#,
    )
}

fn handback_step() -> ExecutableStep {
    web_step(
        r#"{"v":1,"n":1,"kind":"web","verb":"handback","target":null,"args":[{"name":"reason","from":"const","value":"sign_in","key":null}],"pre":[],"post":[],"effect":"none","class":"credential"}"#,
    )
}

fn at(log: &[String], line: &str) -> usize {
    log.iter()
        .position(|l| l == line)
        .unwrap_or_else(|| panic!("no line {line:?} in {log:?}"))
}

/// The lines of the log that begin with `start`.
fn lines<'l>(log: &'l [String], start: &str) -> Vec<&'l str> {
    log.iter()
        .filter(|l| l.starts_with(start))
        .map(String::as_str)
        .collect()
}

/// A browser signed in to A and B, with a cookie of C, which the run does not bind.
fn signed_in() -> SessionState {
    jar(
        &[
            cookie("sid", "a-session", "a.test"),
            cookie("pref", "b-pref", ".b.test"),
            cookie("other", "c-session", "c.test"),
        ],
        &[(A, &[("token", "a-token")]), (C, &[("token", "c-token")])],
    )
}

/// A8.3, U5.2 (1), as the coordinator's ruling of log line 14366 turns it: a run whose browser lives at its end stores
/// anew, before `finish` and before the browser stops, the state of each site whose kept sign-in it imported whole and
/// of each site that it bound at a handback, the cookies of its host and parent domains and its localStorage, by
/// `put_session` as the next revision of that site: the listed one plus 1, else 1; so does a run that failed. A bound
/// site that the run neither imported nor bound, and a site that it does not bind, get nothing. A run that ends at a
/// sign-in field stores nothing for that site; the handback's opening stores, before it clears them, the other sites
/// that the run kept so far (U5.2f); a run whose browser never started stores nothing.
#[test]
fn a_run_end_stores_the_state_of_each_bound_origin() {
    let world = World::default();
    world.store(
        A,
        3,
        &export_of(&jar(&[cookie("kept", "a-kept", "a.test")], &[]), A),
    );
    let ran = world.run(&Run::new(signed_in(), &[A, B]), &[click()]);
    let puts = lines(&ran.log, "frame store put_session");
    assert_eq!(
        puts,
        ["frame store put_session http://a.test:8080 4 1 1"],
        "{:?}",
        ran.log
    );
    assert!(
        at(&ran.log, puts[0]) < at(&ran.log, "stop"),
        "{:?}",
        ran.log
    );
    assert!(
        at(&ran.log, puts[0]) < at(&ran.log, "frame store finish"),
        "{:?}",
        ran.log
    );
    assert_eq!(world.stored(A, 4), Some(export_of(&ran.jar, A)));
    let a = world.stored(A, 4).expect("A's revision");
    let mut values: Vec<&str> = a.cookies.iter().map(|c| c.value.as_str()).collect();
    values.sort_unstable();
    assert_eq!(
        (values, a.item_count()),
        (vec!["a-kept", "a-session"], 1),
        "A's own cookies, the imported one among them, and its item alone"
    );
    assert!(
        world.revisions(B).is_empty() && world.revisions(C).is_empty(),
        "nothing of a site that the run did not import or bind"
    );
    assert_eq!(ran.seen.finish, Some(Outcome::Completed));

    let world = World::default();
    world.store(A, 1, &export_of(&signed_in(), A));
    let mut failing = Run::new(SessionState::default(), &[A, B]);
    failing.act_fails = true;
    let ran = world.run(&failing, &[click()]);
    assert_eq!(ran.seen.finish, Some(Outcome::Failed), "{}", ran.end);
    assert_eq!(
        lines(&ran.log, "frame store put_session"),
        ["frame store put_session http://a.test:8080 2 1 1"],
        "a failed run stores too: {:?}",
        ran.log
    );

    let world = World::default();
    let mut binding = Run::new(SessionState::default(), &[B]);
    binding.headed = true;
    binding.signs = &[Some(Sign::Secret), None];
    binding.answers = &[DONE, BOUND_YES];
    binding.person = Some(signed_in());
    let ran = world.run(&binding, &[click()]);
    assert_eq!(
        lines(&ran.log, "frame store put_session"),
        ["frame store put_session http://a.test:8080 1 1 1"],
        "the site bound at the handback alone: {:?}",
        ran.log
    );
    assert_eq!(world.stored(A, 1), Some(export_of(&signed_in(), A)));

    let world = World::default();
    world.store(A, 1, &export_of(&signed_in(), A));
    world.store(B, 1, &export_of(&signed_in(), B));
    let mut at_field = Run::new(SessionState::default(), &[A, B]);
    at_field.headed = true;
    at_field.signs = &[Some(Sign::Secret)];
    at_field.answers = &[DECLINED];
    let ran = world.run(&at_field, &[click()]);
    assert_eq!(ran.seen.finish, Some(Outcome::Declined), "{}", ran.end);
    assert_eq!(
        lines(&ran.log, "frame store put_session"),
        ["frame store put_session http://b.test:8081 2 1 1"],
        "nothing for the site at whose field the run ended; the opening stores the other kept site: {:?}",
        ran.log
    );

    let world = World::default();
    let ran = world.run(&Run::new(signed_in(), &[A, B]), &[handback_step()]);
    assert!(
        lines(&ran.log, "frame store put_session").is_empty()
            && lines(&ran.log, "export").is_empty(),
        "no browser started, nothing exported: {:?}",
        ran.log
    );
}

/// A8.3, U5.2 (2): when the run's browser starts, after the frame `browser`, its reply and the rule's arm, and before
/// the first reading and the first act, the runner loads every part of the newest revision of each origin of the
/// bindings that the load lists, and of no other, and imports it: the next run's browser holds what the run before
/// stored, a state of two parts among it.
#[test]
fn the_next_run_starts_with_the_stored_state() {
    let world = World::default();
    let long = "t".repeat(60_000);
    let first = jar(
        &[cookie("sid", "a-session", "a.test")],
        &[(A, &[("token", "a-token"), ("cache", &long)])],
    );
    world.store(
        C,
        1,
        &export_of(&jar(&[cookie("sid", "c", "c.test")], &[]), C),
    );
    let mut signing = Run::new(SessionState::default(), &[A]);
    signing.headed = true;
    signing.signs = &[Some(Sign::Secret), None];
    signing.answers = &[DONE, BOUND_YES];
    signing.person = Some(first.clone());
    let ran = world.run(&signing, &[click()]);
    assert_eq!(
        lines(&ran.log, "frame store put_session"),
        [
            "frame store put_session http://a.test:8080 1 1 2",
            "frame store put_session http://a.test:8080 1 2 2"
        ],
        "{:?}",
        ran.log
    );

    let ran = world.run(&Run::new(SessionState::default(), &[A]), &[click()]);
    let loads = lines(&ran.log, "frame store load_session");
    assert_eq!(
        loads,
        [
            "frame store load_session http://a.test:8080 1 1",
            "frame store load_session http://a.test:8080 1 2"
        ],
        "the newest revision of the bound origin alone: {:?}",
        ran.log
    );
    let import = at(&ran.log, "import");
    assert!(at(&ran.log, "frame browser") < at(&ran.log, "arm [http://a.test:8080]"));
    assert!(at(&ran.log, "arm [http://a.test:8080]") < at(&ran.log, loads[0]));
    assert!(at(&ran.log, loads[1]) < import, "{:?}", ran.log);
    assert!(import < at(&ran.log, "read"), "{:?}", ran.log);
    assert!(import < at(&ran.log, "act click"), "{:?}", ran.log);
    assert_eq!(
        export_of(&ran.jar, A),
        export_of(&first, A),
        "the browser holds run 1's state"
    );
    assert_eq!(
        world.stored(A, 2),
        Some(export_of(&first, A)),
        "and stores it as revision 2"
    );
}

/// A7.4, U5.2 (3): a state of more than 64 parts, here of a site bound at a handback, is not stored; the run's log
/// gets one `stopped` line with no step and the cause `session_size`, and the run's outcome stays. A state of 64 parts,
/// here a kept sign-in that the run imported, is stored whole.
#[test]
fn a_state_over_the_limit_is_not_stored() {
    let fill = |n: usize| -> SessionState {
        let bare = export_of(&jar(&[], &[(A, &[("k", "")])]), A)
            .to_json()
            .len();
        jar(
            &[cookie("sid", "b", "b.test")],
            &[(A, &[("k", &"x".repeat(n - bare))])],
        )
    };
    let limit = 64 * 48 * 1024 / 4 * 3;
    assert_eq!(export_of(&fill(limit), A).to_json().len(), limit);
    assert_eq!(
        parts_of(&export_of(&fill(limit), A)).map(|p| p.len()),
        Ok(64)
    );
    assert_eq!(
        parts_of(&export_of(&fill(limit + 1), A)).map(|p| p.len()),
        Err(TooLarge)
    );

    let world = World::default();
    let mut signing = Run::new(SessionState::default(), &[A]);
    signing.headed = true;
    signing.signs = &[Some(Sign::Secret), None];
    signing.answers = &[DONE, BOUND_YES];
    signing.person = Some(fill(limit + 1));
    let ran = world.run(&signing, &[click()]);
    assert!(
        lines(&ran.log, "frame store put_session").is_empty(),
        "{:?}",
        ran.log
    );
    let stopped: Vec<_> = ran
        .seen
        .logs
        .iter()
        .filter(|(event, _, _)| *event == LogEvent::Stopped)
        .collect();
    assert_eq!(
        stopped,
        [&(LogEvent::Stopped, None, Some(Cause::SessionSize))],
        "one stopped line"
    );
    assert_eq!(ran.seen.finish, Some(Outcome::Completed));
    assert!(ran.log.contains(&"frame done completed".to_string()));

    let world = World::default();
    world.store(A, 1, &export_of(&fill(limit), A));
    let ran = world.run(&Run::new(SessionState::default(), &[A]), &[click()]);
    let puts = lines(&ran.log, "frame store put_session");
    assert_eq!(puts.len(), 64, "{:?}", ran.log);
    assert_eq!(
        puts[63],
        "frame store put_session http://a.test:8080 2 64 64"
    );
    assert!(ran.seen.logs.is_empty(), "no stopped line");
    assert_eq!(world.stored(A, 2), Some(export_of(&fill(limit), A)));
}

/// The marker of a kept sign-in: a cookie's value and an item's value.
const MARKER: &str = "halvo-kept-sign-in-marker-u52";

/// The variable that makes the test the run whose standard error its own first run reads.
const CUSTODY_CHILD: &str = "HALVO_RUNNER_SESSION_CUSTODY_CHILD";

/// The browser's state of the person's sign-in in test 5: the marker as a cookie's value and as an item's value.
fn custody_state() -> SessionState {
    jar(
        &[cookie("sid", MARKER, "a.test")],
        &[(A, &[("token", MARKER)])],
    )
}

/// The marker as the store holds it (verifier-100's case): the first 48 signs of the kept part's base64 text, and the
/// 24 signs of that text that encode 18 bytes of the marker alone.
fn stored_needles() -> [String; 2] {
    let state = export_of(&custody_state(), A);
    let parts = parts_of(&state).expect("a state within the limit");
    let text = parts[0].pieces().concat();
    let at = state
        .to_json()
        .find(MARKER)
        .expect("the marker in the state's JSON");
    let from = (at / 3 + 1) * 4;
    [text[..48].to_string(), text[from..from + 24].to_string()]
}

/// A8.3, U5.2 (5): a marker value of a kept sign-in goes through one run that signs in by a handback and stores it,
/// through the next run, which loads and imports it and stores it again, and through a third run with a window, which
/// imports it and stores it at the opening of a handback at another site (U5.2f). No line of the stand-in's log, no
/// frame's Debug text, no frame of the runner but `store` and no answer to it but `store_reply`, a handback request
/// among them, no run's end and no line of standard error holds it, as plain text or as the store holds it (two
/// needles of its base64 text, which a `store` frame of the first run and the opening's `store` frame hold); the marker
/// went into the store and back into the browser. The test runs the three runs in a second process of its own binary
/// and reads that process's standard error.
#[test]
fn no_line_and_no_text_holds_a_byte_of_a_kept_sign_in() {
    if std::env::var_os(CUSTODY_CHILD).is_some() {
        three_runs_hold_the_marker_in_store_frames_alone();
        return;
    }
    let out = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "session::tests::no_line_and_no_text_holds_a_byte_of_a_kept_sign_in",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CUSTODY_CHILD, "1")
        .output()
        .expect("the second process");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("1 passed"), "the runs ran: {stdout}");
    assert!(
        stderr.contains("run browser=stopped removed=yes"),
        "the runner's lines reach this standard error: {stderr}"
    );
    for needle in stored_needles().iter().map(String::as_str).chain([MARKER]) {
        assert!(
            !stderr.contains(needle),
            "a line of standard error holds {needle}"
        );
    }
}

fn three_runs_hold_the_marker_in_store_frames_alone() {
    let world = World::default();
    let mut signing = Run::new(SessionState::default(), &[A]);
    signing.signs = &[Some(Sign::Secret), None];
    signing.answers = &[DONE, BOUND_YES];
    signing.person = Some(custody_state());
    let one = world.run(&signing, &[click()]);
    assert!(
        one.log
            .contains(&"frame bind http://a.test:8080".to_string()),
        "{:?}",
        one.log
    );
    let kept = world.stored(A, 1).expect("the kept sign-in");
    assert_eq!(
        kept.cookies[0].value, MARKER,
        "the marker went into the store"
    );
    let two = world.run(&Run::new(SessionState::default(), &[A]), &[click()]);
    assert_eq!(
        two.jar.storage,
        [OriginStorage {
            origin: A.to_string(),
            items: vec![("token".to_string(), MARKER.to_string())]
        }],
        "the marker came back into the browser"
    );
    let mut opening = Run::new(SessionState::default(), &[A, B]);
    opening.headed = true;
    opening.address = B;
    opening.signs = &[Some(Sign::Secret), None];
    opening.answers = &[DONE, BOUND_YES];
    let three = world.run(&opening, &[click()]);
    let put = "frame store put_session http://a.test:8080 3 1 1";
    assert!(
        at(&three.log, put) < at(&three.log, "clear stores"),
        "run 3 stores the kept sign-in at the opening: {:?}",
        three.log
    );
    let needles = stored_needles();
    for needle in &needles {
        assert!(
            one.seen.stores.iter().any(|s| s.contains(needle.as_str())),
            "the known positive: a store frame of run 1 holds {needle}"
        );
        assert!(
            three
                .seen
                .stores
                .iter()
                .any(|s| s.contains(needle.as_str())),
            "the known positive: the store frame of run 3's opening holds {needle}"
        );
    }
    for ran in [&one, &two, &three] {
        for text in ran
            .log
            .iter()
            .chain(&ran.seen.debug)
            .chain(&ran.seen.texts)
            .chain([&ran.end])
        {
            for needle in needles.iter().map(String::as_str).chain([MARKER]) {
                assert!(!text.contains(needle), "a text holds {needle}: {text}");
            }
        }
        assert!(!ran.seen.texts.is_empty() && !ran.seen.debug.is_empty());
    }
    assert!(one
        .seen
        .texts
        .iter()
        .any(|t| t.contains(r#""t":"handback""#)));
}

/// U5.2 (4), the coordinator's rulings of log lines 14230 and 14239: in a run with no window that imported a kept
/// sign-in, a credential field opens no handback: the step writes `step_failed` with the cause `unauthorized`, the
/// line that the roster's `refused_run` reads, the runner sends `drop_session` for the page's origin, and the run ends
/// `failed` with the cause `step_failed`; it acts on nothing, starts no browser again and stores nothing for that
/// origin, and stores the other origin's state. The store's drop row (here the supervisor's, in the form of
/// `drop_session`) is the newest revision of that origin, so the next run, which the interface starts with a window,
/// imports nothing of it and the other origin's state, opens a handback at the field and keeps the new sign-in as the
/// revision after the drop; the handback's opening stores the other origin's state before it clears it (U5.2f).
#[test]
fn a_run_without_a_window_ends_at_a_credential_field_and_drops_that_sign_in() {
    let world = World::default();
    world.store(A, 1, &export_of(&signed_in(), A));
    world.store(B, 1, &export_of(&signed_in(), B));
    let mut refused = Run::new(SessionState::default(), &[A, B]);
    refused.signs = &[Some(Sign::Secret)];
    let ran = world.run(&refused, &[click()]);
    assert_eq!(lines(&ran.log, "import").len(), 2, "{:?}", ran.log);
    assert_eq!(
        ran.seen.logs,
        [
            (LogEvent::StepFailed, Some(1), Some(Cause::Unauthorized)),
            (LogEvent::Stopped, None, Some(Cause::StepFailed))
        ]
    );
    assert_eq!(
        lines(&ran.log, "frame store drop_session"),
        ["frame store drop_session http://a.test:8080"]
    );
    assert!(at(&ran.log, "frame store drop_session http://a.test:8080") < at(&ran.log, "stop"));
    assert_eq!(ran.seen.finish, Some(Outcome::Failed));
    assert!(ran.log.contains(&"frame done failed".to_string()));
    assert!(
        lines(&ran.log, "frame handback").is_empty(),
        "no handback: {:?}",
        ran.log
    );
    assert!(lines(&ran.log, "act").is_empty(), "no act: {:?}", ran.log);
    assert_eq!(
        (
            lines(&ran.log, "frame browser").len(),
            lines(&ran.log, "stop").len()
        ),
        (1, 1),
        "one browser"
    );
    assert_eq!(
        lines(&ran.log, "frame store put_session"),
        ["frame store put_session http://b.test:8081 2 1 1"],
        "nothing for the origin at whose field the run ended"
    );
    assert_eq!(
        world.revisions(A),
        [(1, 1, 1), (2, 1, 1)],
        "the drop is A's revision 2"
    );

    let mut windowed = Run::new(SessionState::default(), &[A, B]);
    windowed.headed = true;
    windowed.signs = &[Some(Sign::Secret), None];
    windowed.answers = &[DONE, BOUND_YES];
    let ran = world.run(&windowed, &[click()]);
    assert!(
        ran.log
            .contains(&"frame store load_session http://a.test:8080 2 1".to_string()),
        "{:?}",
        ran.log
    );
    assert_eq!(
        lines(&ran.log, "import").len(),
        1,
        "B's state alone: {:?}",
        ran.log
    );
    assert_eq!(
        export_of(&ran.jar, A),
        export_of(&SessionState::default(), A),
        "nothing of A"
    );
    assert_eq!(
        lines(&ran.log, "frame handback"),
        ["frame handback sign_in"],
        "{:?}",
        ran.log
    );
    assert_eq!(ran.seen.finish, Some(Outcome::Completed), "{}", ran.end);
    assert_eq!(
        lines(&ran.log, "frame store put_session"),
        [
            "frame store put_session http://b.test:8081 3 1 1",
            "frame store put_session http://a.test:8080 3 1 1"
        ],
        "B at the handback's opening, before its clear; the site bound at the handback at the end"
    );
}

/// The round of log line 14366, pr-review-65's own case 2 with its assertions turned: a run with no window whose import
/// of the kept sign-in fails (a fault of the executor) ends, and its end stores nothing: the kept sign-in stays the
/// newest listed revision, and the next run imports it.
#[test]
fn a_run_whose_import_fails_keeps_the_listed_revision() {
    let world = World::default();
    world.store(A, 1, &export_of(&signed_in(), A));
    let mut failing = Run::new(SessionState::default(), &[A]);
    failing.import_fails = true;
    let one = world.run(&failing, &[click()]);
    assert_eq!(one.seen.finish, Some(Outcome::Failed), "{}", one.end);
    assert!(
        lines(&one.log, "frame store put_session").is_empty(),
        "{:?}",
        one.log
    );
    let listed: Vec<(String, u32)> = world
        .listed()
        .iter()
        .map(|s| (s.origin.as_str().to_string(), s.rev.get()))
        .collect();
    assert_eq!(listed, [(A.to_string(), 1)]);

    let two = world.run(&Run::new(SessionState::default(), &[A]), &[click()]);
    assert!(two
        .log
        .contains(&"frame store load_session http://a.test:8080 1 1".to_string()));
    assert_eq!(export_of(&two.jar, A), export_of(&signed_in(), A));
    assert!(lines(&two.log, "frame store drop_session").is_empty());
}

/// The round of log line 14366, pr-review-65's own case 1 with its assertions turned: a run in which nobody signed in
/// (no import, no handback) stores nothing, not even the cookies that the site set, so the store lists no revision and
/// the roster's `session` reads `no` (`an_entry_names_state_slots_session_and_the_last_run` of halvo-agent-store); the
/// next run imports nothing and, at the site's first sign-in field, opens a handback as before this change.
#[test]
fn a_run_that_signed_in_nothing_stores_nothing() {
    let world = World::default();
    let visited = jar(
        &[cookie("seen", "1", "a.test")],
        &[(A, &[("theme", "dark")])],
    );
    let mut first = Run::new(visited, &[A]);
    first.headed = true;
    first.signs = &[None];
    let one = world.run(&first, &[click()]);
    assert_eq!(one.seen.finish, Some(Outcome::Completed), "{}", one.end);
    assert!(
        lines(&one.log, "frame store put_session").is_empty(),
        "{:?}",
        one.log
    );
    assert!(world.listed().is_empty(), "no revision is listed");

    let mut second = Run::new(SessionState::default(), &[A]);
    second.signs = &[Some(Sign::Secret)];
    second.answers = &[DECLINED];
    let two = world.run(&second, &[click()]);
    assert!(lines(&two.log, "import").is_empty(), "{:?}", two.log);
    assert_eq!(
        lines(&two.log, "frame handback"),
        ["frame handback sign_in"]
    );
    assert!(lines(&two.log, "frame store drop_session").is_empty());
    assert!(!two
        .seen
        .logs
        .contains(&(LogEvent::StepFailed, Some(1), Some(Cause::Unauthorized))));
}

/// The round of log line 14366, static-analyzer-67's F1: in a run with a window that imported the kept sign-ins of two
/// sites, a handback at the first clears what the pages of every site keep for later, the second site's localStorage
/// among it; the opening stores the second site's state before it clears it, its localStorage among it (U5.2f), and
/// the end stores the site that it bound at the handback.
#[test]
fn a_handback_in_a_run_with_a_window_keeps_the_local_storage_of_another_bound_origin() {
    let world = World::default();
    let b_state = export_of(
        &jar(
            &[cookie("sid", "b-session", "b.test")],
            &[(B, &[("token", "b-token")])],
        ),
        B,
    );
    world.store(A, 1, &export_of(&signed_in(), A));
    world.store(B, 1, &b_state);
    let mut run = Run::new(SessionState::default(), &[A, B]);
    run.headed = true;
    run.signs = &[Some(Sign::Secret), None];
    run.answers = &[DONE, BOUND_YES];
    run.person = Some(signed_in());
    let ran = world.run(&run, &[click()]);
    assert!(at(&ran.log, "clear stores") < at(&ran.log, "frame handback sign_in"));
    assert_eq!(
        lines(&ran.log, "frame store put_session"),
        [
            "frame store put_session http://b.test:8081 2 1 1",
            "frame store put_session http://a.test:8080 2 1 1"
        ],
        "{:?}",
        ran.log
    );
    assert_eq!(world.revisions(B), [(1, 1, 1), (2, 1, 1)]);
    assert_eq!(world.stored(B, 2), Some(b_state.clone()));
    assert_eq!(world.stored(B, 1), Some(b_state));
}

/// U5.2f, the coordinator's ruling of log line 14407 (verifier-100's W1): with nothing kept before, the person signs in
/// at a handback of site A and then at a handback of site B in one run, and each binds. B's opening stores A's state
/// as it stood before that opening cleared what the pages keep for later, its localStorage among it, as A's revision
/// 1; the end stores B's as B's revision 1; the run ends completed. The next run, with no window, imports both and
/// opens no handback.
#[test]
fn two_sign_ins_in_one_run_are_both_kept() {
    let world = World::default();
    let a_state = jar(
        &[cookie("sid", "a-session", "a.test")],
        &[(A, &[("token", "a-token")])],
    );
    let b_state = jar(
        &[cookie("sid", "b-session", "b.test")],
        &[(B, &[("token", "b-token")])],
    );
    let mut run = Run::new(SessionState::default(), &[A, B]);
    run.headed = true;
    run.signs = &[Some(Sign::Secret), None, Some(Sign::Secret), None];
    run.answers = &[DONE, BOUND_YES, DONE, BOUND_YES];
    run.person = Some(a_state.clone());
    run.next_site = Some((B, b_state.clone()));
    let ran = world.run(&run, &[click(), click()]);
    let listed: Vec<(String, u32, u32)> = world
        .listed()
        .iter()
        .map(|s| (s.origin.as_str().to_string(), s.rev.get(), s.parts.get()))
        .collect();
    assert_eq!(
        listed,
        [(A.to_string(), 1, 1), (B.to_string(), 1, 1)],
        "a whole revision of each site: {:?}",
        ran.log
    );
    assert_eq!(ran.seen.finish, Some(Outcome::Completed), "{}", ran.end);
    assert_eq!(
        lines(&ran.log, "frame bind"),
        [
            "frame bind http://a.test:8080",
            "frame bind http://b.test:8081"
        ]
    );
    let clears: Vec<usize> = (0..ran.log.len())
        .filter(|&i| ran.log[i] == "clear stores")
        .collect();
    let put = at(&ran.log, "frame store put_session http://a.test:8080 1 1 1");
    assert!(
        clears.len() == 2 && clears[0] < put && put < clears[1],
        "A is stored at B's opening, before its clear: {:?}",
        ran.log
    );
    let a = world.stored(A, 1).expect("A's revision");
    assert_eq!(
        a,
        export_of(&a_state, A),
        "A's state as it stood before the clear"
    );
    assert_eq!(a.item_count(), 1, "its localStorage among it");
    assert_eq!(
        export_of(&ran.jar, A).item_count(),
        0,
        "the clear took A's item from the browser"
    );
    assert_eq!(world.stored(B, 1), Some(export_of(&b_state, B)));

    let next = world.run(&Run::new(SessionState::default(), &[A, B]), &[click()]);
    assert_eq!(lines(&next.log, "import").len(), 2, "{:?}", next.log);
    assert_eq!(export_of(&next.jar, A), export_of(&a_state, A));
    assert_eq!(export_of(&next.jar, B), export_of(&b_state, B));
    assert!(
        lines(&next.log, "frame handback").is_empty(),
        "asked at neither site: {:?}",
        next.log
    );
    assert_eq!(next.seen.finish, Some(Outcome::Completed), "{}", next.end);
}
