//! `learn-gate drift --case FILE --arm admit --link product` (contracts.md L10.4, L11.5): the command plays the one
//! case twice in the arm `admit`, first on a gate host of its own on the memory store and then through the product's
//! owner (`halvo-owner`, the binary beside `learn-gate`) over the agent-setup run's owner link (`StreamLink`), and
//! prints the two case lines, with `store=memory` and `store=owner`, and the product line.
//!
//! Through the owner every occurrence falls on the owner's local date and there is no restart. No product host hands
//! out the rows of the family `learn`, so the gate counts none through the owner: the owner line holds
//! `reverted_good=0 edit_rows=0`, and `result=ok` rests on the five integers that the product can show (occurrences,
//! drifts, assisted, failed, harm), with `served_learned` (the occurrence after the run whose proposal was taken is
//! served the learned locator first) and `listed` (`list_refinements` over the link holds an entry of an edit).
//!
//! The owner's store is the gate's own, under the command's temporary directory, opened by the owner with a key that
//! `hand_key` makes from the system's generator and writes on the owner's standard input alone. The gate starts the
//! owner on a Mac alone; on another system the command is refused as `usage`.

use std::io::Write;
use std::path::Path;

use crate::cli::{Flags, Refusal, Report};

/// The key's length in bytes: the owner reads exactly 32 bytes on its standard input.
const KEY_LEN: usize = 32;

/// THE ONE PLACE OF THE GATE THAT MAKES A STORE KEY (U8.1 sentence 5): 32 bytes from the system's generator, written
/// on `input`, the owner's standard input, which is then closed. The bytes stand in no other place: the buffer is
/// zeroed after the write, and no error text holds a byte of it.
#[cfg_attr(
    all(not(target_os = "macos"), not(test)),
    expect(dead_code, reason = "the owner is started on a Mac alone")
)]
fn hand_key(mut input: impl Write) -> Result<(), String> {
    let mut key = [0u8; KEY_LEN];
    let made = getrandom::fill(&mut key).map_err(|_| "the system gives no random bytes".to_owned());
    let written = made.and_then(|()| {
        input
            .write_all(&key)
            .and_then(|()| input.flush())
            .map_err(|_| "the key cannot be written on the owner's standard input".to_owned())
    });
    key.fill(0);
    std::hint::black_box(&key);
    drop(input);
    written
}

/// `learn-gate drift --case FILE --arm admit --link product` (L10.4).
pub fn run(args: &[String]) -> Result<Report, Refusal> {
    let mut flags = Flags::new(args);
    let link = flags.value("--link")?;
    let case = flags.value("--case")?;
    let arm = flags.value("--arm")?;
    flags.done()?;
    if link != Some("product") {
        return Err(Refusal::usage("--link takes product"));
    }
    if arm != Some("admit") {
        return Err(Refusal::usage(
            "--link product takes --arm admit, the one arm of the product",
        ));
    }
    let case = case.ok_or_else(|| Refusal::usage("--link product takes --case FILE"))?;
    product(Path::new(case))
}

/// Off a Mac the gate cannot start the owner with the system's safe calls that it uses on a Mac.
#[cfg(not(target_os = "macos"))]
fn product(_case: &Path) -> Result<Report, Refusal> {
    Err(Refusal::usage(
        "--link product runs on a Mac alone: the gate starts the product's owner there alone",
    ))
}

#[cfg(target_os = "macos")]
fn product(case: &Path) -> Result<Report, Refusal> {
    mac::product(case)
}

#[cfg(target_os = "macos")]
mod mac {
    use std::collections::BTreeMap;
    use std::io::{PipeReader, PipeWriter};
    use std::os::fd::{AsFd, AsRawFd, OwnedFd};
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc::{self, Receiver};
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::{Duration, Instant as Mono, SystemTime, UNIX_EPOCH};

    use halvo_agent_client::link::{End, Ends, Refusal as EndRefusal, StreamLink};
    use halvo_agent_client::OwnerLink;
    use halvo_agent_store::Now;
    use halvo_schema::executable::RunnerReply;
    use halvo_schema::record::{FloorLayer, ObservedStep, Tagged, Text};
    use halvo_schema::request::{
        decode_reply, AnalysisRole, AnalysisWrite, InterfaceDecision, InterfaceRead, InterfaceRole,
        ReplyResult,
    };
    use halvo_schema::{
        AppId, Episode, Instant, Ordinal, Part, Position, RequestId, SeedCategory, StoreId,
        SummaryId,
    };
    use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};
    use rustix::io::{fcntl_setfd, FdFlags};
    use serde_json::Value;

    use super::hand_key;
    use crate::case::read_file;
    use crate::cli::{Cause, Refusal, Report, StoreName};
    use crate::derived::Scratch;
    use crate::drift::case_line;
    use crate::host::PlayHost;
    use crate::stub::{play_all, play_case, Arm, Play, Tally};
    use crate::targets;

    const DAY_NS: i64 = 86_400_000_000_000;
    /// How long the gate waits for one answer of the owner on the starter's channel.
    const ANSWER_WAIT: Duration = Duration::from_secs(30);
    /// How long the gate waits for the owner to end after it closed the starter's channel.
    const END_WAIT: Duration = Duration::from_secs(10);
    /// The frame `stream` on a Mac: the end comes beside it, in one SCM_RIGHTS message.
    const STREAM_BESIDE: &[u8] = br#"{"stream":{"handles":null}}"#;
    const TAKEN: &[u8] = br#"{"stream":"taken"}"#;

    pub(super) fn product(path: &Path) -> Result<Report, Refusal> {
        let targets = targets::read()?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Refusal::new(Cause::Case, "the case's file has no name"))?;
        let case = read_file(path, name, targets.get("gate", "calendar_days_min"))
            .map_err(|detail| Refusal::new(Cause::Case, detail))?;
        let failed = |detail: String| {
            Refusal::new(
                Cause::Case,
                format!(
                    "{}.jsonl: an occurrence did not complete: {detail}",
                    case.name
                ),
            )
        };
        let memory = play_case(&case, Arm::Admit, false).map_err(failed)?;

        let binary = owner_binary()?;
        let scratch = Scratch::make().map_err(|detail| Refusal::new(Cause::Store, detail))?;
        let dir = scratch
            .play_dir()
            .map_err(|detail| Refusal::new(Cause::Store, detail))?;
        let host =
            OwnerHost::start(&binary, &dir).map_err(|detail| Refusal::new(Cause::Store, detail))?;
        let mut play = Play::with_host(&case, Arm::Admit, 1, host);
        let owner = play_all(&mut play, false).map_err(failed)?;
        let listed = play.host.listed().map_err(failed)?;
        let served = play.host.served_learned;
        play.host
            .end()
            .map_err(|detail| Refusal::new(Cause::Store, detail))?;
        drop(play);
        drop(scratch);

        let same = |t: &Tally| {
            let c = &t.counts;
            [c.occurrences, c.drifts, c.assisted, c.failed, c.harm]
        };
        let ok = same(&memory) == same(&owner) && served && listed;
        let yes = |b: bool| if b { "yes" } else { "no" };
        let lines = vec![
            case_line(&case, Arm::Admit, StoreName::Memory, &memory),
            case_line(&case, Arm::Admit, StoreName::Memory, &owner).replacen(
                " store=memory ",
                " store=owner ",
                1,
            ),
            format!(
                "product served_learned={} listed={} result={}",
                yes(served),
                yes(listed),
                if ok { "ok" } else { "miss" }
            ),
        ];
        Ok(Report { lines, ok })
    }

    /// The owner's binary, `halvo-owner` in the directory of `learn-gate`'s own binary.
    fn owner_binary() -> Result<PathBuf, Refusal> {
        let missing = || {
            Refusal::new(
                Cause::Store,
                "the product's owner, halvo-owner, does not stand beside learn-gate",
            )
        };
        let exe = std::env::current_exe().map_err(|_| missing())?;
        let owner = exe.with_file_name("halvo-owner");
        if owner.is_file() {
            Ok(owner)
        } else {
            Err(missing())
        }
    }

    /// Whether a frame of the starter's channel is a report (`{"report":..}`), which the gate does not read.
    fn is_report(frame: &[u8]) -> bool {
        serde_json::from_slice::<Value>(frame)
            .ok()
            .and_then(|v| {
                v.as_object()
                    .map(|o| o.len() == 1 && o.contains_key("report"))
            })
            .unwrap_or(false)
    }

    /// The gate's end of the owner's starter channel: the socket on which it writes, and every frame of the owner
    /// that is no report, read by a thread of its own so that the owner's reports never fill the socket.
    struct Starter {
        socket: UnixStream,
        frames: Receiver<Vec<u8>>,
    }

    impl Starter {
        fn new(socket: UnixStream) -> Result<Starter, String> {
            let mut reads = socket
                .try_clone()
                .map_err(|_| "the starter's channel cannot be read".to_owned())?;
            let (tx, frames) = mpsc::channel();
            std::thread::spawn(move || {
                while let Ok(Some(frame)) = read_frame(&mut reads, SERVER_FRAME_MAX) {
                    if !is_report(&frame) && tx.send(frame).is_err() {
                        return;
                    }
                }
            });
            Ok(Starter { socket, frames })
        }

        fn answer(&self) -> Result<Vec<u8>, String> {
            self.frames
                .recv_timeout(ANSWER_WAIT)
                .map_err(|_| "the owner gives no answer on the starter's channel".to_owned())
        }

        /// One interface request on the starter's channel and the result of its reply.
        fn interface(&mut self, request: &[u8]) -> Result<ReplyResult, String> {
            write_frame(&mut self.socket, request, CLIENT_FRAME_MAX)
                .map_err(|_| "a request cannot be written on the starter's channel".to_owned())?;
            let reply = decode_reply(&self.answer()?)
                .map_err(|_| "the owner's reply cannot be read".to_owned())?;
            reply
                .outcome
                .map_err(|name| format!("the owner refuses an interface request: {name:?}"))
        }

        /// A new stream: the owner's end goes to it beside the frame `stream`, and the gate's end comes back once the
        /// owner answers `taken`. The gate keeps its copy of the owner's end until the answer has come.
        fn stream(&mut self) -> Result<End, String> {
            let (mine, owners) =
                UnixStream::pair().map_err(|_| "a stream cannot be made".to_owned())?;
            halvo_control::r2::send_with_end(self.socket.as_fd(), STREAM_BESIDE, owners.as_fd())
                .map_err(|_| "a stream cannot be handed to the owner".to_owned())?;
            let answer = self.answer()?;
            drop(owners);
            if answer != TAKEN {
                return Err("the owner does not take a stream".to_owned());
            }
            Ok(End::new(OwnedFd::from(mine)))
        }
    }

    /// The link's `Ends`: each end is a new stream that the gate hands the owner on the starter's channel.
    #[derive(Clone)]
    struct GateEnds(Arc<Mutex<Starter>>);

    impl Ends for GateEnds {
        fn end(&self) -> Result<End, EndRefusal> {
            let mut starter = self.0.lock().unwrap_or_else(PoisonError::into_inner);
            starter.stream().map_err(|_| EndRefusal::Internal)
        }
    }

    /// The gate host of the product (contracts.md L10.4, L11.5): the owner binary on the gate's own data directory,
    /// started with its starter's channel and the worker's channel, the requests of the stub runner over
    /// `StreamLink`, and what the gate reads of the replies for the product line.
    struct OwnerHost {
        child: Child,
        starter: GateEnds,
        /// The gate's ends of the worker's channel: the one on which it writes, and the one on which it reads.
        worker: Option<(PipeWriter, PipeReader)>,
        link: StreamLink<GateEnds>,
        requests: u64,
        /// The owner's day, as the agent store took it for the start row that the last `load` reply holds.
        day: Option<u32>,
        workflow: String,
        agent: String,
        replay_runs: Vec<String>,
        /// The steps that the last replay run was served, by `n`.
        served: BTreeMap<u64, Value>,
        /// The first locator of each step that a taken replay proposal changed, until the next `load_steps` reply.
        proposed: Option<BTreeMap<u64, Value>>,
        served_learned: bool,
        ended: bool,
    }

    /// Marks an end inheritable: the owner gets it at its number.
    fn inheritable(fd: impl AsFd) -> Result<(), String> {
        fcntl_setfd(fd, FdFlags::empty())
            .map_err(|_| "an end cannot be handed to the owner".to_owned())
    }

    impl OwnerHost {
        /// Starts the owner on `dir` with the key of `hand_key` on its standard input, the lowest live ordinal 1, the
        /// starter's channel (one socket pair, the owner's end and a copy of it, because the owner refuses two ends
        /// of one number) and the worker's channel (two pipes).
        fn start(binary: &Path, dir: &Path) -> Result<OwnerHost, String> {
            let fail = |what: &str| format!("{what} cannot be made");
            let (owners, mine) = UnixStream::pair().map_err(|_| fail("the starter's channel"))?;
            let copy = owners
                .try_clone()
                .map_err(|_| fail("the starter's channel"))?;
            let (worker_reads, worker_requests) =
                std::io::pipe().map_err(|_| fail("the worker's channel"))?;
            let (worker_replies, worker_writes) =
                std::io::pipe().map_err(|_| fail("the worker's channel"))?;
            inheritable(&owners)?;
            inheritable(&copy)?;
            inheritable(&worker_reads)?;
            inheritable(&worker_writes)?;
            let mut child = Command::new(binary)
                .arg(format!("--data-dir={}", dir.display()))
                .arg("--first-live=1")
                .arg(format!(
                    "--channel={},{}",
                    owners.as_raw_fd(),
                    copy.as_raw_fd()
                ))
                .arg(format!(
                    "--worker={},{}",
                    worker_reads.as_raw_fd(),
                    worker_writes.as_raw_fd()
                ))
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| "the owner does not start".to_owned())?;
            drop((owners, copy, worker_reads, worker_writes));
            let input = child.stdin.take();
            let handed = match input {
                Some(input) => hand_key(input),
                None => Err("the owner has no standard input".to_owned()),
            };
            if let Err(detail) = handed {
                let _ = child.kill();
                let _ = child.wait();
                return Err(detail);
            }
            let starter = GateEnds(Arc::new(Mutex::new(Starter::new(mine)?)));
            Ok(OwnerHost {
                child,
                link: StreamLink::new(starter.clone()),
                starter,
                worker: Some((worker_requests, worker_replies)),
                requests: 0,
                day: None,
                workflow: String::new(),
                agent: String::new(),
                replay_runs: Vec::new(),
                served: BTreeMap::new(),
                proposed: None,
                served_learned: false,
                ended: false,
            })
        }

        fn request_id(&mut self) -> RequestId {
            self.requests += 1;
            let mut bytes = [0u8; 16];
            bytes[8..].copy_from_slice(&self.requests.to_be_bytes());
            RequestId::from_random(bytes)
        }

        /// One request of the role `runner` or `coordinator` over the link, and the raw result of its reply.
        fn send(&mut self, request: &str, body: &[u8]) -> Result<String, String> {
            let body =
                std::str::from_utf8(body).map_err(|_| "a body that is no UTF-8".to_owned())?;
            let id = self.request_id();
            let text = format!(
                r#"{{"v":1,"id":"{}","request":"{request}","body":{body}}}"#,
                id.as_str()
            );
            let reply = self
                .link
                .request(text.as_bytes())
                .map_err(|_| "the owner link gives no reply".to_owned())?;
            let reply =
                decode_reply(&reply).map_err(|_| "the owner's reply cannot be read".to_owned())?;
            match reply.outcome {
                Ok(ReplyResult::Raw(raw)) => Ok(raw.get().to_owned()),
                Ok(ReplyResult::Null) => Ok("null".to_owned()),
                Ok(_) => Err("the owner answers with a page".to_owned()),
                Err(name) => Err(format!("{name:?}")),
            }
        }

        /// The owner's day of now, as the owner reads its clock: the agent store's day of the last start row that a
        /// `load` reply held, or the day of UTC before any.
        fn day(&self) -> u32 {
            self.day
                .unwrap_or_else(|| u32::try_from(self.instant().0.div_euclid(DAY_NS)).unwrap_or(0))
        }

        fn instant(&self) -> Instant {
            let ns = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
                .unwrap_or(0);
            Instant(ns)
        }

        /// The three summaries of the workflow, one a day on the three days before today by UTC, which the owner's
        /// recurrence step `Proposer` proposes as one workflow.
        fn episodes(&self) -> Result<Vec<Episode>, String> {
            let today = self.instant().0.div_euclid(DAY_NS);
            let mut episodes = Vec::new();
            for n in 1u8..=3 {
                let day = today - 4 + i64::from(n);
                let start = day * DAY_NS + DAY_NS / 2 + i64::from(n);
                let bad = |what: &str| format!("the summary's {what}");
                episodes.push(Episode {
                    id: SummaryId::from_random([n; 16]),
                    category: SeedCategory::UtilityBillPayment,
                    site: None,
                    app: AppId::parse(crate::derived::APP).map_err(|_| bad("app"))?,
                    day: u32::try_from(day).map_err(|_| bad("day"))?,
                    part: Part::Evening,
                    minutes: 5,
                    keys: None,
                    pointer: None,
                    segments: vec![Ordinal::new(u64::from(n)).map_err(|_| bad("ordinal"))?],
                    expires: Instant(i64::MAX),
                    start: Instant(start),
                    end: Instant(start + 1_000),
                    steps: vec![ObservedStep {
                        app: None,
                        title: Some(Tagged {
                            value: Text::parse("Pay bill").map_err(|_| bad("title"))?,
                            layer: FloorLayer::Floor,
                        }),
                        input: None,
                        site: None,
                        text: None,
                    }],
                });
            }
            Ok(episodes)
        }

        /// `list_refinements` over the link for the play's workflow and agent: whether its list holds an entry of an
        /// edit.
        fn listed(&mut self) -> Result<bool, String> {
            let body = format!(
                r#"{{"op":"list_refinements","workflow":"{}","agent":"{}"}}"#,
                self.workflow, self.agent
            );
            let reply = self.send("coordinator_write", body.as_bytes())?;
            let reply: Value =
                serde_json::from_str(&reply).map_err(|_| "a list that is no JSON".to_owned())?;
            let entries = reply["refinements"]
                .as_array()
                .ok_or_else(|| "no list of refinements".to_owned())?;
            Ok(entries
                .iter()
                .any(|e| e["what"].as_str().is_some_and(|w| w.starts_with("target_"))))
        }

        /// Ends the owner: the starter's channel and the worker's channel close, and the owner exits 0.
        fn end(&mut self) -> Result<(), String> {
            self.ended = true;
            let starter = self
                .starter
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let _ = starter.socket.shutdown(std::net::Shutdown::Write);
            drop(starter);
            drop(self.worker.take());
            let deadline = Mono::now() + END_WAIT;
            loop {
                match self.child.try_wait() {
                    Ok(Some(status)) if status.success() => return Ok(()),
                    Ok(Some(_)) => return Err("the owner ends with a failure".to_owned()),
                    Ok(None) if Mono::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(20));
                    }
                    _ => {
                        let _ = self.child.kill();
                        let _ = self.child.wait();
                        return Err("the owner does not end".to_owned());
                    }
                }
            }
        }

        /// What the gate notes of a runner body and its reply: the owner's day from a `load` reply's start row, the
        /// steps served to a replay run, and whether a replay run after a taken proposal is served its locator first.
        fn note(&mut self, body: &Value, reply: &RunnerReply) {
            let run = body["run"].as_str().unwrap_or_default();
            let replay = self.replay_runs.iter().any(|r| r == run);
            match (body["op"].as_str(), reply) {
                (Some("load"), RunnerReply::Load(load)) => {
                    if let Some(start) = &load.start {
                        self.day = Some(start.day);
                    }
                }
                (Some("load_steps"), RunnerReply::Steps(steps)) if replay => {
                    let served: BTreeMap<u64, Value> = steps
                        .steps
                        .iter()
                        .filter_map(|s| serde_json::to_value(s).ok())
                        .filter_map(|s| Some((s["n"].as_u64()?, s)))
                        .collect();
                    if let Some(proposed) = self.proposed.take() {
                        self.served_learned |= !proposed.is_empty()
                            && proposed.iter().all(|(n, first)| {
                                served.get(n).map(|s| &s["target"]["locators"][0]) == Some(first)
                            });
                    }
                    self.served = served;
                }
                (Some("put_playbook"), RunnerReply::Stored(stored))
                    if replay && stored.stored > 0 =>
                {
                    let changed: BTreeMap<u64, Value> = body["steps"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|s| Some((s["n"].as_u64()?, s)))
                        .filter(|(n, s)| self.served.get(n) != Some(*s))
                        .map(|(n, s)| (n, s["target"]["locators"][0].clone()))
                        .collect();
                    self.proposed = Some(changed);
                }
                _ => {}
            }
        }
    }

    impl Drop for OwnerHost {
        fn drop(&mut self) {
            if !self.ended {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }

    impl PlayHost for OwnerHost {
        /// The product links `LearnHandlers` always, the one arm that it has.
        fn link(&mut self, _arm: Arm) {}

        /// The owner reads its own clock: every occurrence falls on its local date.
        fn set_day(&mut self, _day: u32) {}

        fn now(&self) -> Now {
            Now { at: self.instant() }
        }

        fn request_day(&self) -> u32 {
            self.day()
        }

        /// One batch of three summaries on the worker's channel, after which the owner's `Proposer` proposes the
        /// workflow, then the read `workflows` and the approval on the starter's channel.
        fn store_workflow(&mut self, _n: u32, _workflow: &str) -> Result<String, String> {
            let store =
                StoreId::parse(&"1".repeat(32)).map_err(|_| "the batch's store id".to_owned())?;
            let id = self.request_id();
            let mut write = AnalysisWrite::new(&AnalysisRole::claim(), id, store);
            for episode in self.episodes()? {
                write
                    .push(episode)
                    .map_err(|_| "a summary that the write refuses".to_owned())?;
            }
            write.commit(Position { seg: 1, idx: 0 });
            let bytes = write
                .to_bytes()
                .map_err(|_| "a batch that its writer refuses".to_owned())?;
            let (requests, replies) = self
                .worker
                .as_mut()
                .ok_or_else(|| "the worker's channel is closed".to_owned())?;
            write_frame(requests, &bytes, CLIENT_FRAME_MAX)
                .map_err(|_| "the batch cannot be written on the worker's channel".to_owned())?;
            let reply = read_frame(replies, SERVER_FRAME_MAX)
                .ok()
                .flatten()
                .ok_or_else(|| "the owner gives no reply on the worker's channel".to_owned())?;
            match decode_reply(&reply).map(|r| r.outcome) {
                Ok(Ok(ReplyResult::Null)) => {}
                _ => return Err("the owner refuses the batch".to_owned()),
            }
            let role = InterfaceRole::claim();
            let read = InterfaceRead::workflows(&role, self.request_id(), None)
                .to_bytes()
                .map_err(|_| "a read that its writer refuses".to_owned())?;
            let starter = Arc::clone(&self.starter.0);
            let mut starter = starter.lock().unwrap_or_else(PoisonError::into_inner);
            let ReplyResult::Workflows(page) = starter.interface(&read)? else {
                return Err("the read of the workflows gives no page".to_owned());
            };
            let [item] = page.workflows.as_slice() else {
                return Err("the owner's store does not hold one workflow".to_owned());
            };
            let id = item.workflow.id().clone();
            let approve = InterfaceDecision::approve(&role, self.request_id(), id.clone())
                .to_bytes()
                .map_err(|_| "a decision that its writer refuses".to_owned())?;
            starter.interface(&approve)?;
            Ok(id.as_str().to_owned())
        }

        fn coordinator(&mut self, body: &[u8]) -> Result<(), String> {
            let value: Value =
                serde_json::from_slice(body).map_err(|_| "a body that is no JSON".to_owned())?;
            match value["op"].as_str() {
                Some("create_agent") => {
                    self.workflow = value["workflow"].as_str().unwrap_or_default().to_owned();
                    self.agent = value["agent"].as_str().unwrap_or_default().to_owned();
                }
                Some("start_run") if value["mode"] == "replay" => {
                    if let Some(run) = value["run"].as_str() {
                        self.replay_runs.push(run.to_owned());
                    }
                }
                _ => {}
            }
            self.send("coordinator_write", body).map(|_| ())
        }

        fn runner(&mut self, body: &[u8]) -> Result<RunnerReply, String> {
            let reply = self.send("runner", body)?;
            let reply = RunnerReply::parse(reply.as_bytes())
                .map_err(|_| "a runner reply that its parser refuses".to_owned())?;
            if let Ok(value) = serde_json::from_slice::<Value>(body) {
                self.note(&value, &reply);
            }
            Ok(reply)
        }

        /// No product host hands out the rows of the family `learn`: the gate counts none through the owner.
        fn learn_rows(&mut self, _kind: &str) -> Result<Vec<String>, String> {
            Ok(Vec::new())
        }

        fn ended(&mut self) -> Result<(), String> {
            Ok(())
        }

        /// Through the product there is no restart: the owner keeps its one `LearnHandlers` for its life.
        fn restart(&mut self) -> Result<u32, String> {
            Ok(0)
        }

        fn keep_after_compile(&mut self) -> Result<(), String> {
            Ok(())
        }

        fn back_to_after_compile(&mut self) -> Result<(), String> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{hand_key, KEY_LEN};

    /// U8.1 sentence 5 (R11; the coordinator's ruling of log line 11703 (2)): the link's one key function writes
    /// exactly 32 bytes from the system's generator on the owner's standard input: two keys differ and neither is all
    /// zero. No byte of a key is printed.
    #[test]
    fn the_link_makes_its_32_key_bytes_from_the_system_generator() {
        let mut first = Vec::new();
        let mut second = Vec::new();
        assert!(hand_key(&mut first).is_ok(), "the first key is not written");
        assert!(
            hand_key(&mut second).is_ok(),
            "the second key is not written"
        );
        assert!(first.len() == KEY_LEN, "the first key is not 32 bytes long");
        assert!(
            second.len() == KEY_LEN,
            "the second key is not 32 bytes long"
        );
        assert!(
            first != second,
            "two keys of the system's generator are the same"
        );
        assert!(
            first.iter().any(|b| *b != 0) && second.iter().any(|b| *b != 0),
            "a key is all zero"
        );
    }
}
