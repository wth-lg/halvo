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
use halvo_web::cdp::{self, CrashMeans, Executor, Mode, StartOptions};
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

    /// Whether the rule failed a request since it was armed: a request to an origin outside the list, or a
    /// state-changing request, which `Mode::Fail` fails to every origin.
    fn refused(&self) -> bool {
        self.executor.requests().iter().any(|r| !r.let_through)
    }

    /// `refused`, read again until `REFUSAL_WAIT` has passed: the executor records a request that it failed once the
    /// browser has answered the fail, which can be after the step that the fail made fail has returned.
    fn refused_after_a_fault(&self) -> bool {
        let until = std::time::Instant::now() + REFUSAL_WAIT;
        loop {
            if self.refused() {
                return true;
            }
            if std::time::Instant::now() >= until {
                return false;
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
        } = self;
        let stopped = runtime.block_on(executor.stop_and_remove());
        relay.stop();
        let _ = std::fs::remove_dir(&run_dir);
        stopped.removed
    }
}

impl Runner {
    /// A step of the verb `navigate` or `extract`, with no condition: the browser starts at the first such step.
    /// A request that the rule failed during the step ends the run `refused` with the cause `origin` (A9.2); any
    /// other fault of the step is `step_failed`.
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
        let refused = match &self.web {
            Some(web) if done.is_err() => web.refused_after_a_fault(),
            Some(web) => web.refused(),
            None => false,
        };
        if refused {
            return Err(Stop::End(Outcome::Refused, Some(Cause::Origin)).into());
        }
        done.map_err(|()| failed())
    }

    /// The run's browser, started at the first call: the frame `browser` once it runs, its `browser_reply`, and
    /// the rule armed with the origins of the bindings. A start that fails ends the run `failed` with the start's
    /// cause, and a runner without `--data-dir` has no place for a profile and fails the step.
    fn browser(&mut self, loaded: &LoadReply) -> Result<&mut Web, Failure> {
        if self.web.is_none() {
            let data_dir = self
                .args
                .data_dir
                .as_ref()
                .ok_or(Failure::Step(Cause::StepFailed))?;
            let web = Web::start(
                &self.args.browser,
                self.args.headed,
                data_dir,
                &self.args.run,
            )
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
