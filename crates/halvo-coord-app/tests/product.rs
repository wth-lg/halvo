//! The product's ports of the app on a Mac (unit U5.4a) against the owner of the derived store, with no built app and
//! no capture daemon. This test binary takes one of three roles by its arguments (`harness = false`):
//!
//! - with the owner's arguments (`--data-dir=..`) it is the owner, `halvo_owner::run()`;
//! - with the runner's arguments of the supervisor library (`--run ..`) it is a stand-in runner, which says `hello`,
//!   loads its run, finishes it with a record and says `done`;
//! - else it runs its tests, each in its own scratch data directory under the target's tmp directory, the ones
//!   whose names hold an argument (all of them with none; `--exact` asks for whole names).
//!
//! A stand-in of the daemon's server serves each control connection as the daemon does: a run mark by
//! `mark_request`, which it counts, and any other request by `dispatch_unknown` with the owner's channel. The relay
//! of the role `interface` is a request on the owner's channel, as the daemon's relay will send it.

fn main() {
    #[cfg(target_os = "macos")]
    mac::main();
}

#[cfg(target_os = "macos")]
mod mac {
    use std::io::{PipeReader, PipeWriter, Write};
    use std::num::NonZeroU32;
    use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
    use std::os::unix::net::UnixStream;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::time::{Duration, Instant as Mono, SystemTime, UNIX_EPOCH};

    use halvo_agent_client::link::product::ClientEnds;
    use halvo_agent_client::link::StreamLink;
    use halvo_agent_client::OwnerLink;
    use halvo_control::client::{Client, ClientError, Connector, Transport};
    use halvo_control::proto::{decode_request, Decoded};
    use halvo_control::r2::{dispatch_unknown, mark_request, OwnerChannel, RunEdge, RunMarked};
    use halvo_control::wire::{read_frame, write_frame, REPLY_MAX_BYTES, REQUEST_MAX_BYTES};
    use halvo_coord::items::ConfirmAnswer;
    use halvo_coord_app::product::{self, Calls, Places, ProductHub};
    use halvo_coord_app::{Card, Core, Dialog, Reply};
    use halvo_schema_core::category::SeedCategory;
    use halvo_schema_core::ids::{Position, RequestId, StoreId, SummaryId};
    use halvo_schema_core::record::{
        Episode, FloorLayer, Instant, ObservedStep, Ordinal, Part, Tagged, Text,
    };
    use halvo_schema_core::request::{
        decode_reply, AnalysisRole, AnalysisWrite, InterfaceDecision, InterfaceRead, InterfaceRole,
        ReplyResult,
    };
    use halvo_schema_core::site::AppId;
    use serde_json::{json, Value};

    const KEY: [u8; 32] = [7; 32];
    const DAY_NS: i64 = 86_400_000_000_000;
    const FRAME_MAX: usize = 262_144;
    const WAIT: Duration = Duration::from_secs(30);

    fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.first().is_some_and(|a| a.starts_with("--data-dir=")) {
            std::process::exit(halvo_owner::run());
        }
        if args.first().map(String::as_str) == Some("--run") {
            std::process::exit(runner(&args));
        }
        let tests: [(&str, fn()); 4] = [
            (
                "a_run_now_of_a_switched_on_agent_runs_its_runner_and_the_store_holds_the_runs_record",
                a_run_now_of_a_switched_on_agent_runs_its_runner_and_the_store_holds_the_runs_record,
            ),
            (
                "after_a_run_the_snapshot_holds_the_agents_row_with_that_run_as_last_and_no_note",
                after_a_run_the_snapshot_holds_the_agents_row_with_that_run_as_last_and_no_note,
            ),
            (
                "the_start_removes_the_checkpoint_of_an_ended_run_of_an_earlier_life",
                the_start_removes_the_checkpoint_of_an_ended_run_of_an_earlier_life,
            ),
            (
                "a_run_of_an_earlier_life_left_by_a_killed_app_is_ended_interrupted_with_one_end_mark",
                a_run_of_an_earlier_life_left_by_a_killed_app_is_ended_interrupted_with_one_end_mark,
            ),
        ];
        let exact = args.iter().any(|a| a == "--exact");
        let filters: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();
        let chosen: Vec<_> = tests
            .iter()
            .filter(|(name, _)| {
                filters.is_empty()
                    || filters.iter().any(|f| {
                        if exact {
                            *name == f.as_str()
                        } else {
                            name.contains(f.as_str())
                        }
                    })
            })
            .collect();
        println!(
            "\nrunning {} test{}",
            chosen.len(),
            if chosen.len() == 1 { "" } else { "s" }
        );
        let mut failed = Vec::new();
        for (name, test) in &chosen {
            let ok = std::panic::catch_unwind(test).is_ok();
            println!("test {name} ... {}", if ok { "ok" } else { "FAILED" });
            if !ok {
                failed.push(*name);
            }
        }
        println!(
            "\ntest result: {}. {} passed; {} failed",
            if failed.is_empty() { "ok" } else { "FAILED" },
            chosen.len() - failed.len(),
            failed.len()
        );
        if !failed.is_empty() {
            std::process::exit(101);
        }
    }

    /// The stand-in runner: `hello`, `load`, `finish` with a record of the outcome `completed` on the local day of
    /// its `--utc-offset`, and `done`. A store reply that is no success makes it say `done` with `failed`.
    fn runner(args: &[String]) -> i32 {
        let arg = |name: &str| {
            args.iter()
                .position(|a| a == name)
                .and_then(|i| args.get(i + 1))
                .cloned()
                .unwrap_or_default()
        };
        let (run, agent, workflow, mode) = (
            arg("--run"),
            arg("--agent"),
            arg("--workflow"),
            arg("--mode"),
        );
        let offset: i64 = arg("--utc-offset").parse().unwrap_or(0);
        let mut input = std::io::stdin().lock();
        let mut output = std::io::stdout().lock();
        let mut send =
            |json: String| halvo_agent_client::wire::write_frame(&mut output, &json).is_ok();
        if !send(format!(
            r#"{{"t":"hello","run":"{run}","agent":"{agent}","v":1}}"#
        )) {
            return 1;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let day = (i64::try_from(now).unwrap() + offset).div_euclid(86_400);
        let record = json!({
            "v": 1, "workflow": workflow, "agent": agent, "run": run, "day": day,
            "mode": mode, "outcome": "completed", "playbook": null,
            "steps": 0, "prompts": 0, "handbacks": 0, "confirmations": 0, "effects": 0, "unknown": 0,
            "model_calls": 0, "tokens_in": 0, "tokens_out": 0, "wall_s": 1, "model": null,
            "before_start": null, "cut": null, "no_key": null, "not_to_account": null,
            "incomplete": null, "reply_to_other": null, "more_pages": null, "held_recipient": null,
            "organized": null, "other_task": null, "held_earlier": null, "earlier_page": null,
        });
        let bodies = [
            format!(r#"{{"op":"load","agent":"{agent}","run":"{run}"}}"#),
            format!(r#"{{"op":"finish","agent":"{agent}","run":"{run}","record":{record}}}"#),
        ];
        let mut outcome = "completed";
        for (n, body) in bodies.iter().enumerate() {
            let frame = format!(
                r#"{{"t":"store","request":{{"v":1,"id":"{:032x}","request":"runner","body":{body}}}}}"#,
                n + 1
            );
            if !send(frame) {
                return 1;
            }
            let reply = halvo_agent_client::wire::read_frame(&mut input)
                .ok()
                .flatten()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
            let stored =
                reply.is_some_and(|r| r["t"] == "store_reply" && r["reply"].get("ok").is_some());
            if !stored {
                outcome = "failed";
                break;
            }
        }
        if !send(format!(r#"{{"t":"done","outcome":"{outcome}"}}"#)) {
            return 1;
        }
        0
    }

    /// The test's ends of the owner's channel: the end that it writes and every frame that the owner writes.
    struct Channel {
        writes: UnixStream,
        frames: Receiver<Vec<u8>>,
    }

    fn is_report(frame: &[u8]) -> bool {
        serde_json::from_slice::<Value>(frame)
            .ok()
            .and_then(|v| {
                v.as_object()
                    .map(|o| o.len() == 1 && o.contains_key("report"))
            })
            .unwrap_or(false)
    }

    impl Channel {
        /// The next frame that is no report, within `within`.
        fn next(&mut self, within: Duration) -> Result<Vec<u8>, RecvTimeoutError> {
            let deadline = Mono::now() + within;
            loop {
                let frame = self
                    .frames
                    .recv_timeout(deadline.saturating_duration_since(Mono::now()))?;
                if !is_report(&frame) {
                    return Ok(frame);
                }
            }
        }

        /// One request of the role `interface` on the channel, and the owner's reply.
        fn relay(&mut self, request: &[u8]) -> Vec<u8> {
            write_frame(&mut self.writes, request, REQUEST_MAX_BYTES).unwrap();
            self.next(WAIT)
                .expect("the owner answers no request of the role interface")
        }
    }

    /// The starter of the daemon's handler of `owner_stream`, over the owner's channel.
    struct Starter<'a>(&'a mut Channel);

    impl OwnerChannel for Starter<'_> {
        fn send_stream(&mut self, frame: &[u8], end: BorrowedFd<'_>) -> std::io::Result<()> {
            halvo_control::r2::send_with_end(self.0.writes.as_fd(), frame, end)
        }

        fn send(&mut self, frame: &[u8]) -> std::io::Result<()> {
            write_frame(&mut self.0.writes, frame, REQUEST_MAX_BYTES)
                .map_err(|e| std::io::Error::other(e.to_string()))
        }

        fn answer(&mut self, within: Duration) -> std::io::Result<Option<Vec<u8>>> {
            match self.0.next(within) {
                Ok(frame) => Ok(Some(frame)),
                Err(RecvTimeoutError::Timeout) => Err(std::io::ErrorKind::TimedOut.into()),
                Err(RecvTimeoutError::Disconnected) => Ok(None),
            }
        }
    }

    /// The test's connector: each connect hands the stand-in server the other end of a new socket pair.
    #[derive(Clone)]
    struct ToServer(Arc<Mutex<Sender<UnixStream>>>);

    impl Connector for ToServer {
        type Transport = UnixStream;

        fn endpoint(&self) -> String {
            "the stand-in server".to_owned()
        }

        fn connect(&self, _: Duration) -> Result<UnixStream, ClientError> {
            let (client, server) = UnixStream::pair().map_err(ClientError::Io)?;
            lock(&self.0)
                .send(server)
                .map_err(|_| ClientError::Io(std::io::ErrorKind::NotConnected.into()))?;
            Ok(client)
        }
    }

    /// One run mark over the test's connector, as the app's client call sends it.
    fn mark_over(to: &ToServer, edge: RunEdge) -> Result<RunMarked, ClientError> {
        let mut stream = to.connect(WAIT)?;
        stream.set_timeout(WAIT).map_err(ClientError::Io)?;
        write_frame(&mut stream, &edge.request(), REQUEST_MAX_BYTES)
            .map_err(|e| ClientError::Protocol(e.to_string()))?;
        stream.end_request().map_err(ClientError::Io)?;
        let reply = read_frame(&mut stream, REPLY_MAX_BYTES)
            .map_err(|e| ClientError::Protocol(e.to_string()))?;
        RunMarked::decode(&reply).ok_or_else(|| ClientError::Protocol("no run_marked".to_owned()))
    }

    /// The counts of the run marks that the stand-in server took.
    #[derive(Default)]
    struct Marks {
        starts: AtomicUsize,
        ends: AtomicUsize,
    }

    /// Serves one control connection as the daemon's server does.
    fn serve_one(stream: UnixStream, channel: &Mutex<Channel>, marks: &Marks) {
        let Ok(frame) = read_frame(&mut &stream, REQUEST_MAX_BYTES) else {
            return;
        };
        let Ok(Decoded::Unknown(request)) = decode_request(&frame) else {
            return;
        };
        if let Some(edge) = mark_request(&request) {
            let Ok(edge) = edge else {
                return;
            };
            match edge {
                RunEdge::Start => marks.starts.fetch_add(1, Ordering::SeqCst),
                RunEdge::End => marks.ends.fetch_add(1, Ordering::SeqCst),
            };
            let reply = RunMarked { edge, stored: true }.encode();
            let _ = write_frame(&mut &stream, &reply, REPLY_MAX_BYTES);
            return;
        }
        let mut channel = lock(channel);
        dispatch_unknown(&request, &stream, &mut Starter(&mut channel));
    }

    /// A running owner of a scratch data directory, its channel, its worker's pipes and the stand-in server.
    struct World {
        dir: PathBuf,
        child: Child,
        channel: Arc<Mutex<Channel>>,
        worker: PipeWriter,
        worker_frames: Receiver<Vec<u8>>,
        marks: Arc<Marks>,
        to: ToServer,
        sent: AtomicU8,
    }

    /// Hands each frame read from `from` to the receiver until the end of the stream.
    fn frames(mut from: impl std::io::Read + Send + 'static) -> Receiver<Vec<u8>> {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(frame) = read_frame(&mut from, FRAME_MAX) {
                if tx.send(frame).is_err() {
                    return;
                }
            }
        });
        rx
    }

    /// Starts this test binary as the owner of `dir`, with `ends` at the descriptors 3 and 4, then 5 and 6, and its
    /// key on standard input.
    #[expect(
        clippy::disallowed_methods,
        reason = "the test starts the owner of the derived store, this binary in the owner's role, in the place of the daemon"
    )]
    fn launch(dir: &Path, ends: Vec<(OwnedFd, OwnedFd)>) -> Child {
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.arg(format!("--data-dir={}", dir.display()))
            .arg("--first-live=1")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        let mut places = Vec::new();
        for (n, (reads, writes)) in ends.iter().enumerate() {
            let r = 3 + 2 * i32::try_from(n).unwrap();
            let name = if n == 0 { "channel" } else { "worker" };
            cmd.arg(format!("--{name}={r},{}", r + 1));
            places.push((reads.as_raw_fd(), r));
            places.push((writes.as_raw_fd(), r + 1));
        }
        // SAFETY: the closure runs in the child between fork and exec and calls fcntl and dup2 alone, which are
        // async-signal-safe; it first copies each end above 9, so that no dup2 writes over an end before it is copied.
        unsafe {
            cmd.pre_exec(move || {
                let mut high = [0; 4];
                for (at, (from, _)) in places.iter().enumerate() {
                    high[at] = libc::fcntl(*from, libc::F_DUPFD_CLOEXEC, 10);
                    if high[at] < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                for (at, (_, to)) in places.iter().enumerate() {
                    if libc::dup2(high[at], *to) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let mut child = cmd.spawn().unwrap();
        drop(ends);
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(&KEY).unwrap();
        drop(stdin);
        child
    }

    impl World {
        fn new(name: &str) -> World {
            let parent = Path::new(env!("CARGO_TARGET_TMPDIR")).join("u54a-product");
            std::fs::create_dir_all(&parent).unwrap();
            let dir = halvo_test_dir::make(&parent, name).unwrap();
            let (owner, test) = UnixStream::pair().unwrap();
            let channel_ends = (
                OwnedFd::from(owner.try_clone().unwrap()),
                OwnedFd::from(owner),
            );
            let (owner_reads, worker): (PipeReader, PipeWriter) = std::io::pipe().unwrap();
            let (worker_reads, owner_writes): (PipeReader, PipeWriter) = std::io::pipe().unwrap();
            let child = launch(
                &dir,
                vec![
                    channel_ends,
                    (OwnedFd::from(owner_reads), OwnedFd::from(owner_writes)),
                ],
            );
            let channel = Arc::new(Mutex::new(Channel {
                frames: frames(test.try_clone().unwrap()),
                writes: test,
            }));
            let marks = Arc::new(Marks::default());
            let (tx, conns) = mpsc::channel::<UnixStream>();
            let served = (Arc::clone(&channel), Arc::clone(&marks));
            std::thread::spawn(move || {
                for stream in conns {
                    let (channel, marks) = (Arc::clone(&served.0), Arc::clone(&served.1));
                    std::thread::spawn(move || serve_one(stream, &channel, &marks));
                }
            });
            World {
                dir,
                child,
                channel,
                worker,
                worker_frames: frames(worker_reads),
                marks,
                to: ToServer(Arc::new(Mutex::new(tx))),
                sent: AtomicU8::new(0),
            }
        }

        fn request_id(&self) -> RequestId {
            RequestId::from_random([self.sent.fetch_add(1, Ordering::SeqCst).wrapping_add(1); 16])
        }

        /// The app's client calls, over the stand-in server and the owner's channel.
        fn calls(&self) -> Calls {
            let (streams, relayed, marked) =
                (self.to.clone(), Arc::clone(&self.channel), self.to.clone());
            Calls {
                owner_stream: Arc::new(move || Client::new(streams.clone()).owner_stream()),
                relay: Arc::new(move |request: &[u8]| Ok(lock(&relayed).relay(request))),
                mark: Arc::new(move |edge| mark_over(&marked, edge)),
            }
        }

        /// The hub over the product's ports of this world, whose runner is this test binary.
        fn hub(&self) -> Arc<ProductHub> {
            let exe = std::env::current_exe().unwrap();
            let places = Places {
                data_dir: self.dir.clone(),
                runner: exe.clone(),
                browser: exe,
                model: None,
            };
            product::hub(self.calls(), places, Box::new(|_| {}), Arc::new(Unseen))
        }

        /// A link of the test's own over an owner stream of the stand-in server.
        fn link(&self) -> StreamLink<impl halvo_agent_client::link::Ends> {
            let to = self.to.clone();
            StreamLink::new(ClientEnds::new(move || {
                Client::new(to.clone()).owner_stream()
            }))
        }

        /// The reply's result of one request of `name` with `body` on `link`.
        fn ask(&self, link: &dyn OwnerLink, name: &str, body: &str) -> Value {
            let id = self.request_id();
            let request = format!(
                r#"{{"v":1,"id":"{}","request":"{name}","body":{body}}}"#,
                id.as_str()
            );
            let reply = decode_reply(&link.request(request.as_bytes()).unwrap()).unwrap();
            match reply.outcome {
                Ok(ReplyResult::Raw(raw)) => serde_json::from_str(raw.get()).unwrap(),
                Ok(ReplyResult::Null) => Value::Null,
                other => panic!("{name} {body}: {other:?}"),
            }
        }

        /// A workflow of the category `calendar_cleanup`, which needs no parameter, proposed for three summaries of
        /// one day two days back by the analysis, so that it has no period and no schedule, and approved by the person, and its agent created and switched on, as an earlier life of the
        /// app leaves them: the workflow and the agent.
        fn agent(&mut self) -> (String, String) {
            let day = u32::try_from(now_ns().div_euclid(DAY_NS)).unwrap() - 2;
            let episodes = (1u8..=3).map(|n| episode(n, day)).collect();
            let write = analysis_write(self.request_id(), episodes, Position { seg: 4, idx: 0 });
            write_frame(&mut self.worker, &write, REQUEST_MAX_BYTES).unwrap();
            let reply = self.worker_frames.recv_timeout(WAIT).unwrap();
            assert!(
                decode_reply(&reply).unwrap().outcome.is_ok(),
                "the analysis write is refused"
            );
            let read = InterfaceRead::workflows(&InterfaceRole::claim(), self.request_id(), None)
                .to_bytes()
                .unwrap();
            let workflow = match decode_reply(&lock(&self.channel).relay(&read))
                .unwrap()
                .outcome
            {
                Ok(ReplyResult::Workflows(page)) => {
                    assert_eq!(page.workflows.len(), 1, "one workflow is proposed");
                    page.workflows[0].workflow.id().clone()
                }
                other => panic!("the read of the workflows answers {other:?}"),
            };
            let approve = InterfaceDecision::approve(
                &InterfaceRole::claim(),
                self.request_id(),
                workflow.clone(),
            )
            .to_bytes()
            .unwrap();
            let approved = decode_reply(&lock(&self.channel).relay(&approve)).unwrap();
            assert!(
                approved.outcome.is_ok(),
                "the approval: {:?}",
                approved.outcome
            );
            let agent = format!("{:032x}", 0xa1);
            let link = self.link();
            let head = format!(r#""workflow":"{}","agent":"{agent}""#, workflow.as_str());
            assert_eq!(
                self.ask(
                    &link,
                    "coordinator_write",
                    &format!(r#"{{"op":"create_agent",{head}}}"#)
                ),
                json!({"rev": 1})
            );
            self.ask(
                &link,
                "coordinator_write",
                &format!(r#"{{"op":"set_state",{head},"state":"active"}}"#),
            );
            (workflow.as_str().to_owned(), agent)
        }

        /// The agent's runs that hold a stored record, newest first, as `agent_history` gives them.
        fn runs(&self, workflow: &str, agent: &str) -> Vec<Value> {
            let body = format!(
                r#"{{"op":"agent_history","workflow":"{workflow}","agent":"{agent}","runs":8}}"#
            );
            self.ask(&self.link(), "coordinator_write", &body)["runs"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        }
    }

    impl Drop for World {
        fn drop(&mut self) {
            if self.child.try_wait().ok().flatten().is_none() {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }

    /// A dialog that is never on screen and answers `reject`: no native confirmation is answered.
    struct Unseen;

    impl Dialog for Unseen {
        fn show(&self, _: &Card, _: Duration, _: &dyn Fn()) -> ConfirmAnswer {
            ConfirmAnswer::Reject
        }
    }

    fn now_ns() -> i64 {
        i64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        )
        .unwrap()
    }

    /// The instant now in nanoseconds and its local day by the Mac's zone.
    fn local_now() -> (i64, i64) {
        let at = now_ns();
        let offset = i64::from(product::zone_offset_s(SystemTime::now()).unwrap());
        (
            at,
            (at.div_euclid(1_000_000_000) + offset).div_euclid(86_400),
        )
    }

    fn analysis_write(id: RequestId, episodes: Vec<Episode>, commit: Position) -> Vec<u8> {
        let store = StoreId::parse(&"1".repeat(32)).unwrap();
        let mut write = AnalysisWrite::new(&AnalysisRole::claim(), id, store);
        for episode in episodes {
            write.push(episode).unwrap();
        }
        write.commit(commit);
        write.to_bytes().unwrap()
    }

    /// An episode of the segment `n` of the category `calendar_cleanup`, of one key for every `n`.
    fn episode(n: u8, day: u32) -> Episode {
        let start = i64::from(day) * DAY_NS + 43_200_000_000_000 + i64::from(n);
        Episode {
            id: SummaryId::from_random([n; 16]),
            category: SeedCategory::CalendarCleanup,
            site: None,
            app: AppId::parse("com.example.calendar").unwrap(),
            day,
            part: Part::Afternoon,
            minutes: 5,
            keys: None,
            pointer: None,
            segments: vec![Ordinal::new(u64::from(n)).unwrap()],
            expires: Instant(9_000_000_000_000_000_000),
            start: Instant(start),
            end: Instant(start + 1_000),
            steps: vec![ObservedStep {
                app: None,
                title: Some(Tagged {
                    value: Text::parse("Tidy calendar").unwrap(),
                    layer: FloorLayer::Floor,
                }),
                input: None,
                site: None,
                text: None,
            }],
        }
    }

    /// Waits until `done` holds, calling it at most every 50 ms within `WAIT`.
    fn until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Mono::now() + WAIT;
        while !done() {
            assert!(Mono::now() < deadline, "{what} within {} s", WAIT.as_secs());
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn run_now(hub: &ProductHub, agent: &str) {
        let reply = hub.call(
            "agents_control",
            Some(&json!({"agent": agent, "action": "run_now"})),
        );
        assert_eq!(reply, Reply::Ok, "Run now");
    }

    /// U5.4a sentence 1: on a Mac a Run now of a switched-on agent starts its runner through the supervisor library,
    /// and the run's record reaches the owner's store through an owner stream of the app's own.
    fn a_run_now_of_a_switched_on_agent_runs_its_runner_and_the_store_holds_the_runs_record() {
        let mut world = World::new("run-now");
        let (workflow, agent) = world.agent();
        let hub = world.hub();
        product::start(&hub);
        assert!(
            world.runs(&workflow, &agent).is_empty(),
            "no run before the Run now"
        );

        run_now(&hub, &agent);
        until("the run's record in the store", || {
            Core::tick(&*hub);
            !world.runs(&workflow, &agent).is_empty()
        });
        let runs = world.runs(&workflow, &agent);
        assert_eq!(runs.len(), 1, "{runs:?}");
        assert_eq!(runs[0]["run"]["outcome"], "completed");
        assert_eq!(runs[0]["run"]["mode"], "replay");
        assert_eq!(runs[0]["run"]["agent"], agent.as_str());
    }

    /// The row of `agent` in the reply of `agents_snapshot`, and the reply.
    fn row_of(hub: &ProductHub, agent: &str) -> (Option<Value>, Value) {
        let snapshot = serde_json::to_value(hub.call("agents_snapshot", Some(&json!({})))).unwrap();
        let row = snapshot["rows"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["agent"] == agent).cloned());
        (row, snapshot)
    }

    /// U5.4a sentence 2: the agents view shows each stored agent and what it last did: after the run of sentence 1
    /// the reply of `agents_snapshot` holds the agent's row with that run as its last, and no note.
    fn after_a_run_the_snapshot_holds_the_agents_row_with_that_run_as_last_and_no_note() {
        let mut world = World::new("snapshot");
        let (workflow, agent) = world.agent();
        let hub = world.hub();
        product::start(&hub);
        run_now(&hub, &agent);
        until("the run's record in the store", || {
            Core::tick(&*hub);
            !world.runs(&workflow, &agent).is_empty()
        });
        let done = halvo_coord::words::text("line.all_set.done", &[]).unwrap();
        until("the agent's row with its last run done", || {
            Core::tick(&*hub);
            row_of(&hub, &agent)
                .0
                .is_some_and(|row| row["line"] == done.as_str())
        });
        let (row, snapshot) = row_of(&hub, &agent);
        let row = row.unwrap();
        assert_eq!(row["workflow"], workflow.as_str());
        assert_eq!(row["status"], "all_set");
        assert_eq!(snapshot["note"], Value::Null, "{snapshot}");
        assert!(
            snapshot["seq"].as_u64().is_some_and(|s| s > 0),
            "{snapshot}"
        );
    }

    /// U5.4a sentence 4: at the app's start, after recovery and before any start of a run, the checkpoints that may
    /// go are removed: the copy of an ended run of an earlier life, whose run's effect on the workbook is none, is
    /// gone after the start.
    fn the_start_removes_the_checkpoint_of_an_ended_run_of_an_earlier_life() {
        let mut world = World::new("checkpoint");
        let (workflow, agent) = world.agent();
        let folder = world.dir.join("files");
        std::fs::create_dir_all(&folder).unwrap();
        let workbook = folder.join("budget.xlsx");
        std::fs::write(&workbook, b"the workbook").unwrap();
        let run = format!("{:032x}", 0xe1);
        let copy_name = halvo_checkpoint::checkpoint_name(
            &workbook,
            &halvo_checkpoint::RunId::parse(&run).unwrap(),
        )
        .unwrap();
        let copy = folder.join(&copy_name);
        let bytes = b"the copy of the workbook";
        std::fs::write(&copy, bytes).unwrap();

        let link = world.link();
        let head = format!(r#""workflow":"{workflow}","agent":"{agent}""#);
        world.ask(
            &link,
            "coordinator_write",
            &format!(r#"{{"op":"start_run",{head},"run":"{run}","mode":"replay"}}"#),
        );
        world.ask(
            &link,
            "runner",
            &format!(r#"{{"op":"load","agent":"{agent}","run":"{run}"}}"#),
        );
        let (at, day) = local_now();
        let own = |text: &str| {
            serde_json::to_string(&halvo_schema::executable::text::OwnValue::new(text)).unwrap()
        };
        let sha = halvo_checkpoint::Sha256::of(bytes).hex();
        for (seq, state, sha256, size) in [
            (1, "planned", "null".to_owned(), "null".to_owned()),
            (2, "made", format!(r#""{sha}""#), bytes.len().to_string()),
        ] {
            let record = format!(
                r#"{{"v":1,"workflow":"{workflow}","agent":"{agent}","run":"{run}","seq":{seq},"day":{day},"at":{at},"folder":{},"workbook":{},"copy":{},"sha256":{sha256},"bytes":{size},"state":"{state}","left":null}}"#,
                own(&folder.display().to_string()),
                own(&workbook.display().to_string()),
                own(copy_name.to_str().unwrap()),
            );
            world.ask(
                &link,
                "runner",
                &format!(
                    r#"{{"op":"checkpoint","agent":"{agent}","run":"{run}","record":{record}}}"#
                ),
            );
        }
        let record = json!({
            "v": 1, "workflow": workflow, "agent": agent, "run": run, "day": day,
            "mode": "replay", "outcome": "completed", "playbook": null,
            "steps": 0, "prompts": 0, "handbacks": 0, "confirmations": 0, "effects": 0, "unknown": 0,
            "model_calls": 0, "tokens_in": 0, "tokens_out": 0, "wall_s": 1, "model": null,
            "before_start": null, "cut": null, "no_key": null, "not_to_account": null,
            "incomplete": null, "reply_to_other": null, "more_pages": null, "held_recipient": null,
            "organized": null, "other_task": null, "held_earlier": null, "earlier_page": null,
        });
        world.ask(
            &link,
            "runner",
            &format!(r#"{{"op":"finish","agent":"{agent}","run":"{run}","record":{record}}}"#),
        );
        assert!(copy.exists(), "the copy stands before the start");

        let hub = world.hub();
        product::start(&hub);
        assert!(
            !copy.exists(),
            "the copy {} is gone after the start",
            copy.display()
        );
        assert!(workbook.exists(), "the workbook stands");
    }

    /// U5.4a sentence 5: a run that died is ended once. A mark under `agents/marks/` of the scratch data directory, as
    /// a killed app leaves it, with the run's `run_started` stored: after the next start the run's record reads
    /// `interrupted` and the stand-in server has taken one mark `run_end` for it. A mark whose runner the reap answers
    /// `alive` gets no end mark: a third life finds a mark that names a process that lives, the test's own owner child
    /// with its creation time and no group of its own, which the reap neither signals nor can find gone.
    fn a_run_of_an_earlier_life_left_by_a_killed_app_is_ended_interrupted_with_one_end_mark() {
        let mut world = World::new("killed");
        let (workflow, agent) = world.agent();
        let run = format!("{:032x}", 0xd1);
        let link = world.link();
        let head = format!(r#""workflow":"{workflow}","agent":"{agent}""#);
        let (at, day) = local_now();
        let started = format!(
            r#"{{"v":1,{head},"seq":1,"day":{day},"at":{at},"event":"run_started","run":"{run}","slot":null,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":null,"trigger":"person","origin":null,"defer_launch":null,"defer_ticks_failed":null}}"#
        );
        assert_eq!(
            world.ask(
                &link,
                "coordinator_write",
                &format!(r#"{{"op":"coord_log",{head},"records":[{started}]}}"#)
            ),
            json!({"stored": 1})
        );
        world.ask(
            &link,
            "coordinator_write",
            &format!(r#"{{"op":"start_run",{head},"run":"{run}","mode":"replay"}}"#),
        );
        let marks = world.dir.join("agents").join("marks");
        std::fs::create_dir_all(&marks).unwrap();
        std::fs::write(marks.join(&run), format!("mark run={run}\n")).unwrap();

        let hub = world.hub();
        product::start(&hub);
        let history = world.ask(
            &link,
            "coordinator_write",
            &format!(r#"{{"op":"coord_history",{head},"after":null}}"#),
        );
        let ended: Vec<&Value> = history["records"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["event"] == "run_ended" && r["run"] == run.as_str())
            .collect();
        assert_eq!(ended.len(), 1, "{history}");
        assert_eq!(ended[0]["cause"], "interrupted", "{history}");
        assert!(!marks.join(&run).exists(), "the mark is gone");
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(world.marks.ends.load(Ordering::SeqCst), 1, "one end mark");
        assert_eq!(
            world.marks.starts.load(Ordering::SeqCst),
            0,
            "no start mark"
        );

        drop(hub);
        let alive = format!("{:032x}", 0xd2);
        let owner = world.child.id();
        let line = halvo_agent_client::marks::proc_line(
            "runner",
            &halvo_agent_client::marks::ProcLine {
                pid: NonZeroU32::new(owner).unwrap(),
                created: halvo_agent_client::process::creation_time(owner).unwrap(),
                group: halvo_agent_client::marks::Group::Job,
            },
        );
        std::fs::write(marks.join(&alive), format!("mark run={alive}\n{line}")).unwrap();
        let next = world.hub();
        product::start(&next);
        assert!(
            marks.join(&alive).exists(),
            "the mark of a runner that lives stays"
        );
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            world.marks.ends.load(Ordering::SeqCst),
            1,
            "no end mark for a run that the reap answers alive"
        );
        assert_eq!(
            world.marks.starts.load(Ordering::SeqCst),
            0,
            "no start mark"
        );
    }
}
