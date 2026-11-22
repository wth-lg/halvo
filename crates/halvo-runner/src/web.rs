//! The agent's browser of a run (A11.1, A6.2, A9.2), first cut: the first step of the verb `navigate` or `extract`
//! starts it with halvo-web's executor behind a relay of its own, the runner's own binary as the relay's guard
//! (`--relay-guard`, `crate::main`). Once the browser runs and before any page loads, the runner writes the frame
//! `browser` and reads `browser_reply`; then it arms the request rule with the origins of the bindings in
//! `Mode::Fail`, the one mode of this build, so that a request that the rule fails, a load outside those origins or
//! a state-changing request, ends the run `refused` with the cause `origin`. At every end of the run it stops the
//! browser and removes the directories that its start made. The other seven web verbs, the conditions of a step
//! and `Mode::Hold` are the second cut's.

use std::num::{NonZeroU32, NonZeroU64};
use std::path::{Path, PathBuf};

use halvo_control::paths::DataDir;
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::frame::Browser;
use halvo_schema::executable::names::{LocatorBy, Source, Verb};
use halvo_schema::executable::step::{Locator, Target, WebTarget};
use halvo_schema::executable::text::CreationTime;
use halvo_schema::executable::{
    Cause, ExecutableStep, FromRunner, Outcome, RunId, ToRunner, WebOrigin,
};
use halvo_sheet::Value;
use halvo_web::cdp::{self, CrashMeans, Executor, Mode, PausedRequest, StartOptions};
use halvo_web::guard::GuardProgram;
use halvo_web::relay::{Entry, Names, Relay};

use crate::link::Broken;
use crate::run::{Failure, Runner, Stop};

/// The name of the profile directory in `<data directory>/agents/profiles/<run id>/` (A8.3); the executor's start
/// makes it and its temporary directory beside it.
pub(crate) const PROFILE: &str = "profile";

/// How long after a web step that failed the runner reads the rule's record for a request that the rule failed.
const REFUSAL_WAIT: std::time::Duration = std::time::Duration::from_millis(500);

/// The browser of a run, the relay that is its only route, the directory that holds its profile, and the runtime
/// on which the executor and the relay run. The executor is dropped before the relay and the runtime.
pub(crate) struct Web {
    executor: Executor,
    relay: Relay,
    run_dir: PathBuf,
    runtime: tokio::runtime::Runtime,
    /// How many entries of the rule's record the lines on standard error have read.
    told: usize,
}

/// The cause of the run's end for a start that failed: `no_channel` for a browser that lived and gave no first
/// reply, `browser_start` for every other failure of the start (A11.1, U1.8 (b)).
fn start_cause(cause: &str) -> Cause {
    if cause == "no_channel" {
        Cause::NoChannel
    } else {
        Cause::BrowserStart
    }
}

/// The origin as the request rule compares it: as the browser writes a request's origin, with no port where the
/// port is the scheme's own.
fn rule_origin(origin: &WebOrigin) -> String {
    let text = origin.as_str();
    let bare = match text.split_once("://") {
        Some(("http", _)) => text.strip_suffix(":80"),
        Some(("https", _)) => text.strip_suffix(":443"),
        _ => None,
    };
    bare.unwrap_or(text).to_string()
}

/// The executor's locator for a locator of a step: by `test_id`, or by `role` with its accessible name; none for
/// the other ways, which the executor does not take.
fn locator(locator: &Locator) -> Option<cdp::Locator> {
    match locator.by {
        LocatorBy::TestId => Some(cdp::Locator::TestId(locator.value.as_str().to_string())),
        LocatorBy::Role => Some(cdp::Locator::Role(
            cdp::map_role(locator.value.as_str())?,
            locator.name.as_ref()?.as_str().to_string(),
        )),
        _ => None,
    }
}

/// The text of the step's arg `name`: its value for `const`, the value of the slot that it names for `slot`.
fn arg_text(step: &ExecutableStep, name: &str, loaded: &LoadReply) -> Option<String> {
    let arg = step.args.iter().find(|a| a.name.as_str() == name)?;
    match arg.from {
        Source::Const => arg.value.as_ref().map(|v| v.as_str().to_string()),
        Source::Slot => {
            let key = arg.key.as_ref()?;
            let bindings = loaded.bindings.as_ref()?;
            bindings
                .slots
                .iter()
                .find(|s| s.name == *key)
                .map(|s| s.value.joined())
        }
        _ => None,
    }
}

/// Whether a request in the rule's record is of the run's page: its record carries a frame. The record's frame is
/// the `frameId` of the browser's paused event as the browser gave it (`decide` in halvo-web's `cdp/wire.rs`), so
/// every frame that the browser names counts as the page's: the main frame of the executor's page, a frame inside
/// it, or the frame of another tab, which in the run's browser only the page opens; a request of a worker of the
/// page counts where the browser names a frame for it; and a request for which the browser names no frame counts as
/// the browser's own level. That a real browser names no frame for its own requests is not shown by a test here:
/// `failed_line` writes the level of each failed request, so that a hosted run shows it.
fn of_the_page(request: &PausedRequest) -> bool {
    !request.frame.is_empty()
}

/// Whether the rule failed a request of the run's page (A9.2). A request of the browser's own level that the rule
/// failed stays failed, and nothing of it left, but it sets no outcome: the person's site never made it.
fn refused_by_the_page(requests: &[PausedRequest]) -> bool {
    requests.iter().any(|r| !r.let_through && of_the_page(r))
}

/// The line on standard error for a request that the rule failed: its method, origin, resource type and level
/// (`page` or `browser`), and no path, no query and no header.
fn failed_line(request: &PausedRequest) -> String {
    format!(
        "run request=failed method={} origin={} resource={} level={}",
        request.method,
        request.origin,
        request.resource,
        if of_the_page(request) {
            "page"
        } else {
            "browser"
        }
    )
}

/// The end of a web step from what the step did and the rule's record at its end: the run `refused` with the cause
/// `origin` where the record holds a failed request of the run's page (`refused_by_the_page`), whatever the step
/// did; else the step's own end, `step_failed` for a fault.
fn end_of_step(done: Result<(), ()>, record: &[PausedRequest]) -> Result<(), Failure> {
    if refused_by_the_page(record) {
        return Err(Stop::End(Outcome::Refused, Some(Cause::Origin)).into());
    }
    done.map_err(|()| Failure::Step(Cause::StepFailed))
}

impl Web {
    /// Starts the browser `browser` with its profile `<data directory>/agents/profiles/<run id>/profile`, headless
    /// unless `headed`, behind a new relay that reaches nothing until `arm`; the cause of the run's end when the start
    /// fails, after which nothing of the start is left that the executor made.
    fn start(browser: &Path, headed: bool, data_dir: &DataDir, run: &RunId) -> Result<Web, Cause> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|_| Cause::BrowserStart)?;
        let run_dir = data_dir
            .path()
            .join("agents")
            .join("profiles")
            .join(run.as_str());
        std::fs::create_dir_all(&run_dir).map_err(|_| Cause::BrowserStart)?;
        let started = GuardProgram::this_program()
            .map_err(|_| Cause::BrowserStart)
            .and_then(|guard| {
                let relay = runtime
                    .block_on(Relay::start(Names::System))
                    .map_err(|_| Cause::BrowserStart)?;
                let options = StartOptions {
                    browser: browser.to_path_buf(),
                    profile: run_dir.join(PROFILE),
                    headless: !headed,
                    relay: relay.handle(),
                    guard,
                    crash_means: CrashMeans::Product,
                };
                match runtime.block_on(Executor::start(&options)) {
                    Ok(executor) => Ok((executor, relay)),
                    Err(failed) => {
                        crate::line(&format!(
                            "run browser=failed cause={} removed={}",
                            failed.cause,
                            if failed.removed { "yes" } else { "no" }
                        ));
                        Err(start_cause(failed.cause))
                    }
                }
            });
        match started {
            Ok((executor, relay)) => Ok(Web {
                executor,
                relay,
                run_dir,
                runtime,
                told: 0,
            }),
            Err(cause) => {
                let _ = std::fs::remove_dir(&run_dir);
                Err(cause)
            }
        }
    }

    /// The frame `browser` of the browser's first process: its number, creation time and, on macOS, group.
    fn frame(&self) -> Option<FromRunner> {
        let root = self.executor.root();
        Some(FromRunner::Browser(Browser {
            pid: NonZeroU32::new(root.pid)?,
            created: CreationTime::from_micros(NonZeroU64::new(root.started_us)?),
            group: if cfg!(windows) {
                None
            } else {
                NonZeroU32::new(root.pgid)
            },
        }))
    }

    /// Lets the relay reach the host and port of each origin, each bound by the person's own word, and arms the
    /// request rule with the origins in `Mode::Fail`.
    fn arm(&mut self, origins: &[WebOrigin]) -> Result<(), ()> {
        let handle = self.relay.handle();
        let mut allowed = Vec::with_capacity(origins.len());
        for origin in origins {
            let (host, port) = cdp::host_port(origin.as_str()).ok_or(())?;
            handle.allow(&host, port, Entry::BoundLoopback);
            allowed.push(rule_origin(origin));
        }
        let executor = &mut self.executor;
        self.runtime
            .block_on(executor.arm_rule(allowed, Mode::Fail))
            .map_err(drop)
    }

    fn open(&mut self, url: &str) -> Result<(), ()> {
        let executor = &mut self.executor;
        self.runtime.block_on(executor.open(url)).map_err(drop)
    }

    /// The text of the one control of the top document that the target's first locator to find any finds.
    fn read(&mut self, target: &WebTarget) -> Result<String, ()> {
        let locators: Vec<cdp::Locator> = target.locators.iter().filter_map(locator).collect();
        if locators.is_empty() || !target.frame.is_empty() {
            return Err(());
        }
        let executor = &mut self.executor;
        self.runtime
            .block_on(async move {
                let reference = executor.resolve_one(&locators, &[]).await?;
                executor.read(&reference).await
            })
            .map_err(drop)
    }

    /// The rule's record of every request that it paused since it was armed, each with whether it was let through;
    /// one line on standard error (`failed_line`) for each request in it that the rule failed and that no earlier
    /// line named.
    fn record(&mut self) -> Vec<PausedRequest> {
        let requests = self.executor.requests();
        if requests.len() < self.told {
            self.told = 0;
        }
        for request in requests[self.told..].iter().filter(|r| !r.let_through) {
            crate::line(&failed_line(request));
        }
        self.told = requests.len();
        requests
    }

    /// `record`, read again until it holds a failed request of the run's page or `REFUSAL_WAIT` has passed: the
    /// executor records a request that it failed once the browser has answered the fail, which can be after the
    /// step that the fail made fail has returned.
    fn record_after_a_fault(&mut self) -> Vec<PausedRequest> {
        let until = std::time::Instant::now() + REFUSAL_WAIT;
        loop {
            let requests = self.record();
            if refused_by_the_page(&requests) || std::time::Instant::now() >= until {
                return requests;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// Stops the browser and removes the profile, its temporary directory and the run's directory that holds them;
    /// whether the profile and its temporary directory are gone.
    fn stop(self) -> bool {
        let Web {
            executor,
            relay,
            run_dir,
            runtime,
            ..
        } = self;
        let stopped = runtime.block_on(executor.stop_and_remove());
        relay.stop();
        let _ = std::fs::remove_dir(&run_dir);
        stopped.removed
    }
}

impl Runner {
    /// A step of the verb `navigate` or `extract`, with no condition: the browser starts at the first such step.
    /// A request of the run's page that the rule failed during the step ends the run `refused` with the cause
    /// `origin` (A9.2, `end_of_step`); any other fault of the step is `step_failed`.
    pub(crate) fn web_step(
        &mut self,
        step: &ExecutableStep,
        loaded: &LoadReply,
    ) -> Result<(), Failure> {
        let failed = || Failure::Step(Cause::StepFailed);
        if !step.pre.is_empty() || !step.post.is_empty() {
            return Err(failed());
        }
        let done = match (step.verb, &step.target) {
            (Verb::Navigate, None) => {
                let url = arg_text(step, "url", loaded).ok_or_else(failed)?;
                self.browser(loaded)?.open(&url)
            }
            (Verb::Extract, Some(Target::Web(target))) => {
                let name = arg_text(step, "as", loaded).ok_or_else(failed)?;
                let parse = arg_text(step, "parse", loaded).ok_or_else(failed)?;
                let text = self.browser(loaded)?.read(target);
                text.and_then(|text| match parse.as_str() {
                    "text" => Ok(Value::Text(text)),
                    "amount" => halvo_web::extract_amount(&text)
                        .map(|minor| Value::Number(minor as f64 / 100.0))
                        .ok_or(()),
                    _ => Err(()),
                })
                .map(|value| {
                    self.kept.insert(name, vec![vec![value]]);
                })
            }
            _ => return Err(failed()),
        };
        let record = match &mut self.web {
            Some(web) if done.is_err() => web.record_after_a_fault(),
            Some(web) => web.record(),
            None => Vec::new(),
        };
        end_of_step(done, &record)
    }

    /// The run's browser, started at the first call: the frame `browser` once it runs, its `browser_reply`, and
    /// the rule armed with the origins of the bindings. A start that fails ends the run `failed` with the start's
    /// cause, and a runner without `--data-dir` has no place for a profile and fails the step. The program is the
    /// one of `--browser`; given none, the one that `find` finds under the runner's roots, read after the data
    /// directory, and where it finds none the run ends `failed` with the cause `no_browser` before anything is made.
    fn browser(&mut self, loaded: &LoadReply) -> Result<&mut Web, Failure> {
        if self.web.is_none() {
            let data_dir = self
                .args
                .data_dir
                .as_ref()
                .ok_or(Failure::Step(Cause::StepFailed))?;
            let program = match &self.args.browser {
                Some(given) => given.clone(),
                None => crate::find::find(&self.roots)
                    .ok_or(Stop::End(Outcome::Failed, Some(Cause::NoBrowser)))?,
            };
            let web = Web::start(&program, self.args.headed, data_dir, &self.args.run)
                .map_err(|cause| Stop::End(Outcome::Failed, Some(cause)))?;
            let frame = web.frame();
            self.web = Some(web);
            let frame = frame.ok_or(Stop::End(Outcome::Failed, Some(Cause::BrowserStart)))?;
            match self.link.ask(&frame)? {
                ToRunner::BrowserReply(_) => {}
                _ => return Err(Broken::Frame("not_a_browser_reply").into()),
            }
            self.check()?;
            let origins = loaded.bindings.as_ref().map_or(&[][..], |b| &b.origins[..]);
            let web = self.web.as_mut().ok_or(Failure::Step(Cause::StepFailed))?;
            web.arm(origins)
                .map_err(|()| Failure::Step(Cause::StepFailed))?;
        }
        self.web.as_mut().ok_or(Failure::Step(Cause::StepFailed))
    }

    /// Stops the run's browser, when it started one, and removes its profile; one line on standard error says
    /// whether the directories are gone.
    pub(crate) fn stop_browser(&mut self) {
        if let Some(web) = self.web.take() {
            let removed = web.stop();
            crate::line(&format!(
                "run browser=stopped removed={}",
                if removed { "yes" } else { "no" }
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::io::{Read, Write};
    use std::num::NonZeroU32;

    use halvo_schema::executable::body::{LoadedBindings, LoadedPlaybook, StepsReply, Stored};
    use halvo_schema::executable::frame::{StoreReply, StoreReplyFrame};
    use halvo_schema::executable::names::LogEvent;
    use halvo_schema::executable::{RunnerCall, RunnerReply};
    use halvo_schema::{AgentId, Category, WorkflowId};

    use crate::find::Roots;
    use crate::link::{encode, Link};

    const NAVIGATE: &str = r#"{"v":1,"n":1,"kind":"web","verb":"navigate","target":null,"args":[{"name":"url","from":"const","value":"http://127.0.0.1:9/","key":null}],"pre":[],"post":[],"effect":"none","class":"read"}"#;

    /// The in-memory supervisor and store of a web agent whose stored playbook is one step `navigate`: it answers
    /// every `store` frame and records every frame until the end of the runner's output, and it closes the link at
    /// the first frame that waits for a reply other than the store's, such as `browser` or `handback`.
    fn supervise(
        mut from_runner: impl Read,
        mut to_runner: impl Write,
        agent: AgentId,
        workflow: WorkflowId,
    ) -> Vec<FromRunner> {
        let config = halvo_policy::preset(Category::UtilityBillPayment, workflow, agent.clone())
            .expect("a preset");
        let step = ExecutableStep::parse(NAVIGATE.as_bytes()).expect("a navigate step");
        let mut frames = Vec::new();
        loop {
            let mut head = [0u8; 4];
            if from_runner.read_exact(&mut head).is_err() {
                break;
            }
            let mut body = vec![0u8; u32::from_be_bytes(head) as usize];
            if from_runner.read_exact(&mut body).is_err() {
                break;
            }
            let frame = FromRunner::parse(&body).expect("the runner writes frames that parse");
            let s = match &frame {
                FromRunner::Store(s) => s,
                FromRunner::Hello(_) | FromRunner::Progress(_) | FromRunner::Done(_) => {
                    frames.push(frame);
                    continue;
                }
                _ => {
                    frames.push(frame);
                    break;
                }
            };
            let answer = match s.request.body() {
                RunnerCall::Load(_) => RunnerReply::Load(LoadReply {
                    config: config.clone(),
                    playbook: Some(LoadedPlaybook {
                        number: NonZeroU32::MIN,
                        steps: NonZeroU32::MIN,
                    }),
                    next_playbook: NonZeroU32::MIN.saturating_add(1),
                    bindings: Some(LoadedBindings {
                        rev: NonZeroU32::MIN,
                        origins: Vec::new(),
                        slots: Vec::new(),
                    }),
                    sessions: Vec::new(),
                    start: None,
                }),
                RunnerCall::LoadSteps(_) => RunnerReply::Steps(StepsReply {
                    agent: agent.clone(),
                    steps: vec![step.clone()],
                }),
                RunnerCall::Log(b) => RunnerReply::Stored(Stored {
                    stored: b.records.len() as u32,
                }),
                _ => RunnerReply::Stored(Stored { stored: 1 }),
            };
            let reply = ToRunner::StoreReply(StoreReplyFrame {
                reply: StoreReply {
                    id: Some(s.request.id().clone()),
                    outcome: Ok(answer),
                },
            });
            frames.push(frame);
            let json = reply.to_json().expect("a frame");
            if to_runner.write_all(&encode(&json)).is_err() {
                break;
            }
        }
        frames
    }

    /// U9.10 (3): a run given no `--browser` whose roots hold no browser ends `failed`: its one log row is `stopped`
    /// with the cause `no_browser`, its record and its `done` say `failed`, no frame `browser` is written and no
    /// profile directory is made. The run is carried out in this process over an empty root.
    #[test]
    fn a_run_that_finds_no_browser_ends_with_no_browser() {
        let exe = std::env::current_exe().expect("the test binary");
        let target = exe
            .ancestors()
            .nth(3)
            .expect("the target directory above deps");
        let dir = halvo_test_dir::make(&target.join("tmp").join("halvo-runner-unit"), "no-browser")
            .expect("make the scratch directory");
        let empty = halvo_test_dir::make(&dir, "roots").expect("make the empty root");
        let data = halvo_test_dir::make(&dir, "data").expect("make the data directory");
        let run = RunId::from_random([0x71; 16]);
        let agent = AgentId::from_random([0x72; 16]);
        let workflow = WorkflowId::from_random([0x73; 16]);
        let data_text = data.to_string_lossy().into_owned();
        let words = [
            "--run",
            run.as_str(),
            "--agent",
            agent.as_str(),
            "--workflow",
            workflow.as_str(),
            "--mode",
            "replay",
            "--data-dir",
            data_text.as_str(),
            "--utc-offset",
            "0",
        ]
        .map(OsString::from);
        let args = crate::args::parse(&words).expect("the arguments");
        assert!(args.browser.is_none(), "no --browser");
        let (to_runner_read, to_runner) = std::io::pipe().expect("a pipe");
        let (from_runner, from_runner_write) = std::io::pipe().expect("a pipe");
        let link = Link::start(
            Box::new(to_runner_read),
            Box::new(from_runner_write),
            Box::new(|| {}),
        );
        let supervisor =
            std::thread::spawn(move || supervise(from_runner, to_runner, agent, workflow));
        let mut runner = Runner::new(args, link);
        runner.roots = Roots {
            applications: Some(empty.clone()),
            program_files_x86: Some(empty.clone()),
            program_files: Some(empty.clone()),
            local_app_data: Some(empty.clone()),
        };
        let status = runner.run();
        let frames = supervisor.join().expect("the supervisor");

        let rows: Vec<(LogEvent, Option<u32>, Option<Cause>)> = frames
            .iter()
            .filter_map(|frame| match frame {
                FromRunner::Store(s) => match s.request.body() {
                    RunnerCall::Log(b) => Some(b.records.clone()),
                    _ => None,
                },
                _ => None,
            })
            .flatten()
            .map(|l| (l.event, l.step.map(|s| s.get()), l.cause))
            .collect();
        assert_eq!(rows, [(LogEvent::Stopped, None, Some(Cause::NoBrowser))]);
        let finish = frames.iter().find_map(|frame| match frame {
            FromRunner::Store(s) => match s.request.body() {
                RunnerCall::Finish(f) => Some(f.record.outcome),
                _ => None,
            },
            _ => None,
        });
        assert_eq!(finish, Some(Outcome::Failed), "the run's record");
        let done = frames.iter().find_map(|frame| match frame {
            FromRunner::Done(d) => Some(d.outcome),
            _ => None,
        });
        assert_eq!(done, Some(Outcome::Failed), "the frame done");
        assert_eq!(status, 0, "the record was stored and done written");
        assert!(
            !frames.iter().any(|f| matches!(f, FromRunner::Browser(_))),
            "no frame browser"
        );
        assert!(
            !data.join("agents").join("profiles").exists(),
            "a profile directory was made under {}",
            data.display()
        );
        assert_eq!(
            std::fs::read_dir(&empty).expect("the root").count(),
            0,
            "the root stays empty"
        );
    }

    use halvo_web::cdp::Outcome as Answered;

    /// A request of the rule's record: `method` to `origin` at `path`, made in `frame` (empty: the browser's own
    /// level), let through or failed by the rule.
    fn request(
        method: &str,
        origin: &str,
        path: &str,
        frame: &str,
        let_through: bool,
    ) -> PausedRequest {
        PausedRequest {
            id: None,
            method: method.to_string(),
            origin: origin.to_string(),
            path: path.to_string(),
            frame: frame.to_string(),
            resource: if frame.is_empty() {
                "Other"
            } else {
                "Document"
            }
            .to_string(),
            let_through,
            outcome: if let_through {
                Answered::Continued
            } else {
                Answered::Failed
            },
        }
    }

    /// The page's own load of its bound origin, let through.
    fn the_page_s_load() -> PausedRequest {
        request("GET", "http://127.0.0.1:8080", "/read", "F1", true)
    }

    /// U4.1wm (1): a request of the browser's own level to an origin outside the bindings is failed, and its record
    /// says so, but the step ends as the step did: a navigate that loaded ends well and the run goes on to its end.
    #[test]
    fn a_failed_request_of_the_browser_s_own_level_leaves_the_run_completed() {
        let own = request("GET", "https://accounts.example.com", "/list", "", false);
        assert!(!own.let_through && own.outcome == Answered::Failed);
        let record = [the_page_s_load(), own];
        assert!(!refused_by_the_page(&record));
        assert!(matches!(end_of_step(Ok(()), &record), Ok(())));
        assert!(matches!(
            end_of_step(Err(()), &record),
            Err(Failure::Step(Cause::StepFailed))
        ));
    }

    /// U4.1wm (2): a request of the page that the rule failed, a load outside the bindings or a state-changing
    /// request, ends the run `refused` with the cause `origin`, as before, also where a failed request of the
    /// browser's own level stands beside it, and whatever the step did.
    #[test]
    fn a_failed_request_of_the_page_ends_the_run_refused() {
        for failed in [
            request("GET", "http://127.0.0.1:9090", "/read", "F1", false),
            request("POST", "http://127.0.0.1:8080", "/sink", "F2", false),
        ] {
            let own = request("GET", "https://accounts.example.com", "/list", "", false);
            let record = [the_page_s_load(), own, failed];
            assert!(refused_by_the_page(&record));
            for done in [Ok(()), Err(())] {
                assert!(matches!(
                    end_of_step(done, &record),
                    Err(Failure::Stop(Stop::End(
                        Outcome::Refused,
                        Some(Cause::Origin)
                    )))
                ));
            }
        }
    }

    /// U4.1wm, the line on standard error: method, origin, resource type and level, and nothing of the path, the
    /// query or a header.
    #[test]
    fn the_line_of_a_failed_request_names_no_path() {
        let page = request(
            "POST",
            "https://pay.example.com",
            "/a/b?card=4111",
            "F2",
            false,
        );
        assert_eq!(
            failed_line(&page),
            "run request=failed method=POST origin=https://pay.example.com resource=Document level=page"
        );
        let own = request(
            "GET",
            "https://accounts.example.com",
            "/list?x=1",
            "",
            false,
        );
        let line = failed_line(&own);
        assert_eq!(
            line,
            "run request=failed method=GET origin=https://accounts.example.com resource=Other level=browser"
        );
        for part in ["/list", "x=1", "/a/b", "card"] {
            assert!(
                !line.contains(part) && !failed_line(&page).contains(part),
                "{part}"
            );
        }
    }
}
