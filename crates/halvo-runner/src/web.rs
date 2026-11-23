//! The agent's browser of a run (A11.1, A6.2, A9.2). The first web step that acts starts it with halvo-web's
//! executor behind a relay of its own, the runner's own binary as the relay's guard (`--relay-guard`, `crate::main`).
//! Once the browser runs and before any page loads, the runner writes the frame `browser` and reads
//! `browser_reply`; then it arms the request rule with the origins of the bindings in `Mode::Fail`: the rule fails
//! every load outside those origins and every state-changing request, and a request of the run's page that it failed
//! ends the run `refused` with the cause `origin` (`end_of_step`). At every end of the run it stops the browser and
//! removes the directories that its start made. The runner drives the browser through `Page`, which the browser of
//! a run (`Live`) gives and which the unit tests of crate::handback give with a stand-in of the executor.
//!
//! A web step acts only where its class is `read` or `prepare` (`acts`): `navigate`, `extract`, `wait`, and `click`,
//! `fill`, `select`, `press` and `upload` with the effect `none`. Before it acts, the runner reads the page
//! (crate::handback): where the page shows a credential field it hands the browser to the person, and acts only
//! after a sign-in that ended in a bind. Its conditions `pre` are read fresh before the act and its conditions `post`
//! after it, and `holds` decides each on what the executor read. A step of the verb `handback` and a step whose
//! target, or the target of one of its conditions, is marked `credential` act on nothing: each reads the page when
//! the browser runs, and is done after a handback that ended in a bind; a step of the verb `handback` with no
//! credential field on the page, or before any browser runs, is done; every other answers `step_failed`. A web step
//! that fails on the run's page, its act or a step marked `credential` of no effect class, reads the page a second
//! time before its failure is written, and hands back with the reason `sign_in` where a link or a button asks for a
//! sign-in by its name (r3 U5.14, `Runner::asked_at_a_failure`), but in a run with no window that imported a kept
//! sign-in; behind the bind of that handback the act that failed is asked once more, and a second failure fails the
//! step as before. A step with the effect `request`, a web step of an effect class (`payment`, `purchase`, `submit`,
//! `sign`), is the browser's request maker's (crate::web::maker), which this build holds under the feature `loopback`
//! alone: without it such a step answers `step_failed` before any browser starts, and `Live` lets no held request
//! through.

use std::collections::HashMap;
use std::num::{NonZeroU32, NonZeroU64};
use std::path::{Path, PathBuf};
use std::time::Duration;

use halvo_control::paths::DataDir;
use halvo_policy::class_of;
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::frame::Browser;
use halvo_schema::executable::names::{Check, LocatorBy, Source, Verb, YesNo};
use halvo_schema::executable::step::{Arg, Cond, Locator, Target, WebTarget};
use halvo_schema::executable::text::CreationTime;
use halvo_schema::executable::{
    ActionClass, Cause, ExecutableStep, FromRunner, Outcome, RunId, ToRunner, WebOrigin,
};
use halvo_schema::SeedCategory;
use halvo_sheet::Value;
#[cfg(feature = "loopback")]
use halvo_web::cdp::HeldRequest;
use halvo_web::cdp::{
    self, CrashMeans, Executor, Fault, Mode, PausedRequest, Resolved, SessionState, Sign,
    StartOptions,
};
use halvo_web::guard::GuardProgram;
use halvo_web::relay::{Entry, List, Names, Relay};
use std::collections::BTreeSet;

use crate::link::Broken;
use crate::run::{Failure, Runner, Stop};

#[cfg(feature = "loopback")]
mod maker;
#[cfg(all(test, feature = "loopback"))]
pub(crate) use maker::amounts_named;

/// The name of the profile directory in `<data directory>/agents/profiles/<run id>/` (A8.3); the executor's start
/// makes it and its temporary directory beside it.
pub(crate) const PROFILE: &str = "profile";

/// How long after a web step that failed or acted on the page the runner reads the rule's record for a request
/// that the rule failed.
const REFUSAL_WAIT: Duration = Duration::from_millis(500);

/// The run's browser as the web steps and the handback drive it (A6.4, A11.1). Each call ends when the executor
/// has answered; a fault of the executor is `Err(())`.
pub(crate) trait Page: Send {
    /// The frame `browser` of the browser's first process.
    fn frame(&self) -> Option<FromRunner>;
    /// Arms the request rule in `Mode::Fail` with `origins`, and lets the relay reach their hosts and ports alone.
    fn arm(&mut self, origins: &[WebOrigin]) -> Result<(), ()>;
    /// Reads the conditions `pre` fresh and acts only where each holds, carries out the act, then reads the
    /// conditions `post` fresh; the text that `extract` read.
    fn carry_out(
        &mut self,
        pre: &[(&Cond, Option<String>)],
        act: &Act<'_>,
        post: &[(&Cond, Option<String>)],
    ) -> Result<Option<String>, ()>;
    /// The rule's record of every request that it paused since it was armed, each with whether it was let through.
    fn record(&mut self) -> Vec<PausedRequest>;
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
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    /// Stops the browser and removes what its start made; whether the profile and its temporary directory are gone.
    fn stop(self: Box<Self>) -> bool;
    /// The reading of A6.4: the strongest sign of a credential field that the page shows (A8.2), none for none.
    fn sign(&mut self) -> Result<Option<Sign>, ()>;
    /// The second reading (r3 U5.14), made after a web step that failed alone: whether the page shows a link or a
    /// button that asks for a sign-in by its name (halvo-web's `Executor::asks_for_a_sign_in`). The stand-ins of the
    /// unit tests of crate::session and crate::web::maker take the default, a page that asks for none.
    fn asks_for_a_sign_in(&mut self) -> Result<bool, ()> {
        Ok(false)
    }
    /// Fails every request that the rule holds paused.
    fn fail_held(&mut self) -> Result<(), ()>;
    /// Closes every page of the run but the top one.
    fn close_others(&mut self) -> Result<(), ()>;
    /// Clears, for every origin that the run has loaded, what a page can keep for later, and keeps the cookies.
    fn clear_stores(&mut self) -> Result<(), ()>;
    /// Loads the top document's address again with a GET navigation.
    fn load_again(&mut self) -> Result<(), ()>;
    /// Arms the handback's mode: the rule lets every request of the person's pages through, and the relay reaches
    /// every host and port.
    fn pass_all(&mut self) -> Result<(), ()>;
    /// The address that the top page holds now.
    fn address(&mut self) -> Result<String, ()>;
    /// The browser's cookies of the host of `origin` and of its parent domains, and the origin's localStorage (A8.3).
    fn export(&mut self, origin: &WebOrigin) -> Result<SessionState, ()>;
    /// Sets the cookies of `state` and the localStorage of each of its origins.
    fn import(&mut self, state: &SessionState) -> Result<(), ()>;
    /// Arms the request rule in `Mode::Hold` with `origins`, as `arm` arms it in `Mode::Fail`: a state-changing
    /// request to one of them stays held until `let_through` or `fail_held` (crate::web::maker); and notes, before the
    /// act, the top document and, where `frame` names the frame of the step's target, that frame's document, so that
    /// the read after the reply waits in the frame of the released request (r3 U5.10-B). These six methods stand in a
    /// build with the feature `loopback` alone; their defaults, which the stand-ins of the unit tests of
    /// crate::handback and crate::session take, hold nothing and let nothing leave.
    #[cfg(feature = "loopback")]
    fn arm_hold(&mut self, _origins: &[WebOrigin], _frame: &[WebOrigin]) -> Result<(), ()> {
        Err(())
    }
    /// The state-changing requests that the rule holds now, each with the executor's id.
    #[cfg(feature = "loopback")]
    fn held(&mut self) -> Vec<HeldRequest> {
        Vec::new()
    }
    /// Lets the held request `id` leave, once.
    #[cfg(feature = "loopback")]
    fn let_through(&mut self, _id: u64) -> Result<(), ()> {
        Err(())
    }
    /// The HTTP status of the response of the request that `let_through` let leave, none before it (the coordinator's
    /// ruling of log line 14732).
    #[cfg(feature = "loopback")]
    fn released_status(&mut self) -> Option<i64> {
        None
    }
    /// Every amount that the page after the reply names, in minor units, read once the document that `arm_hold`
    /// noted in the frame of the request that `let_through` let leave has given way to a loaded one; none where that
    /// frame is not one that `arm_hold` noted.
    #[cfg(feature = "loopback")]
    fn page_amounts(&mut self) -> Result<Vec<u64>, ()> {
        Err(())
    }
    /// The executor's cause word of the newest `carry_out` that failed, none before one; the browser's maker writes
    /// it on its line of a failed read.
    #[cfg(feature = "loopback")]
    fn last_fault(&mut self) -> Option<&'static str> {
        None
    }
}

/// The run's browser: the browser of a run, or in a unit test a stand-in of the executor.
pub(crate) struct Web(pub(crate) Box<dyn Page>);

/// The browser of a run, the relay that is its only route, the directory that holds its profile, and the runtime
/// on which the executor and the relay run. The executor is dropped before the relay and the runtime.
struct Live {
    executor: Executor,
    relay: Relay,
    run_dir: PathBuf,
    runtime: tokio::runtime::Runtime,
    /// How many entries of the rule's record the lines on standard error have read.
    told: usize,
    /// The origins that the rule let a request of reach since the browser started, as the browser writes them.
    loaded: BTreeSet<String>,
    /// The origins of the newest arm in `Mode::Fail`, whose loopback hosts keep their mark in the handback's mode.
    armed: Vec<WebOrigin>,
    /// The wait for the document that replaces the one that `arm_hold` noted, before the act, in the frame of the
    /// released request, run once by `page_amounts`.
    #[cfg(feature = "loopback")]
    after_release: Option<AfterRelease>,
    /// The frame of the request that `let_through` let leave, as the browser named it in its paused event.
    #[cfg(feature = "loopback")]
    released: Option<String>,
    /// The cause word of the newest `carry_out` that failed (`Page::last_fault`).
    #[cfg(feature = "loopback")]
    fault: Option<&'static str>,
}

/// The wait for the document that replaces the one noted at `arm_hold` in the frame that the browser names by the
/// given id, the frame of the released request; a failure where that frame is not a noted one.
#[cfg(feature = "loopback")]
type AfterRelease =
    Box<dyn FnOnce(&Executor, &tokio::runtime::Runtime, Option<&str>) -> Result<(), ()> + Send>;

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

/// The executor's locators of a target, of the top document or inside a frame (`frame_origins`); refused for a target
/// with no locator that the executor takes.
fn locators(target: &WebTarget) -> Result<Vec<cdp::Locator>, Fault> {
    let locators: Vec<cdp::Locator> = target.locators.iter().filter_map(locator).collect();
    if locators.is_empty() {
        return Err(Fault::new("target", "no locator that the executor takes"));
    }
    Ok(locators)
}

/// The origins of the frames that hold the target, from the top document inward, each as the browser writes an
/// origin (`rule_origin`), by which the executor finds the frame (the coordinator's ruling of log line 15706, point
/// 1); none for a target of the top document.
fn frame_origins(target: &WebTarget) -> Vec<String> {
    target.frame.iter().map(rule_origin).collect()
}

/// Whether this build lets a web step act: its class, as halvo-policy's `class_of` gives it from the step's own
/// keys for the agent's category, is `read` or `prepare`, and neither its target nor the target of one of its
/// conditions is a credential field. A target with `credential` `yes`, a step with the effect `request` and the
/// verb `handback` do not act.
pub(crate) fn acts(category: SeedCategory, step: &ExecutableStep) -> bool {
    !marks_a_credential(step)
        && matches!(
            class_of(category, step),
            ActionClass::Read | ActionClass::Prepare
        )
}

/// Whether the step is a web step of an effect class: its class, as `class_of` gives it from the step's own keys for
/// the agent's category, is `payment`, `purchase`, `submit` or `sign`, the classes of a step with the effect `request`.
fn of_an_effect_class(category: SeedCategory, step: &ExecutableStep) -> bool {
    matches!(
        class_of(category, step),
        ActionClass::Payment | ActionClass::Purchase | ActionClass::Submit | ActionClass::Sign
    )
}

/// Whether the step's own target, or the target of one of its conditions, is marked `credential`.
fn marks_a_credential(step: &ExecutableStep) -> bool {
    let credential = |target: &WebTarget| target.credential == YesNo::Yes;
    matches!(&step.target, Some(Target::Web(target)) if credential(target))
        || step
            .pre
            .iter()
            .chain(&step.post)
            .filter_map(|cond| cond.target.as_ref())
            .any(credential)
}

/// One control as the executor read it, fresh: its text, its value, and whether it is enabled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Control {
    pub(crate) text: String,
    pub(crate) value: String,
    pub(crate) enabled: bool,
}

/// What the executor read, fresh, for one condition: the address of the top document for `url_is`; for every other
/// check, how many controls of the target's document, the top one or that of the frame that its frame origins name,
/// the target's first locator to find any finds, and the one control where it finds one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Read {
    Url(String),
    Controls { count: usize, one: Option<Control> },
}

/// Whether the condition `check` with the text `arg` of its arg holds on what the executor read (A3.3): `url_is`
/// where the page's address starts with the arg; `present` where the target finds one control; `enabled` where that
/// control is enabled; `text_is` and `value_is` where its text or its value is the arg; `count_is` where the target
/// finds as many controls as the arg, a count in decimal digits. A read of another kind than the check's holds
/// nothing.
pub(crate) fn holds(check: Check, arg: Option<&str>, read: &Read) -> bool {
    let one = match read {
        Read::Controls { count: 1, one } => one.as_ref(),
        _ => None,
    };
    match (check, read) {
        (Check::UrlIs, Read::Url(url)) => arg.is_some_and(|a| url.starts_with(a)),
        (Check::CountIs, Read::Controls { count, .. }) => {
            arg.filter(|a| !a.is_empty() && a.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|a| a.parse::<usize>().ok())
                == Some(*count)
        }
        (Check::Present, _) => one.is_some(),
        (Check::Enabled, _) => one.is_some_and(|c| c.enabled),
        (Check::TextIs, _) => one.is_some_and(|c| arg == Some(c.text.as_str())),
        (Check::ValueIs, _) => one.is_some_and(|c| arg == Some(c.value.as_str())),
        _ => false,
    }
}

/// Where the text of an arg comes from: the bindings of the load and what the run's earlier steps kept.
struct Values<'r> {
    loaded: &'r LoadReply,
    kept: &'r HashMap<String, Vec<Vec<Value>>>,
}

impl Values<'_> {
    /// The text of an arg: its value for `const`, the value of the slot that it names for `slot`, the text that an
    /// earlier step kept as one cell under its key for `extract`; none for `model` and for a value that is missing.
    fn of(&self, arg: &Arg) -> Option<String> {
        match arg.from {
            Source::Const => arg.value.as_ref().map(|v| v.as_str().to_string()),
            Source::Slot => {
                let key = arg.key.as_ref()?;
                let bindings = self.loaded.bindings.as_ref()?;
                bindings
                    .slots
                    .iter()
                    .find(|s| s.name == *key)
                    .map(|s| s.value.joined())
            }
            Source::Extract => match self.kept.get(arg.key.as_ref()?.as_str())?.as_slice() {
                [row] => match row.as_slice() {
                    [Value::Text(text)] => Some(text.clone()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    }

    /// The text of the step's arg `name`.
    fn arg(&self, step: &ExecutableStep, name: &str) -> Option<String> {
        self.of(step.args.iter().find(|a| a.name.as_str() == name)?)
    }

    /// Each condition with the text of its arg; none where an arg cannot be read or a target has no locator that the
    /// executor takes.
    fn conditions<'s>(&self, conds: &'s [Cond]) -> Option<Vec<(&'s Cond, Option<String>)>> {
        conds
            .iter()
            .map(|cond| {
                if cond.target.as_ref().is_some_and(|t| locators(t).is_err()) {
                    return None;
                }
                match &cond.arg {
                    Some(arg) => self.of(arg).map(|text| (cond, Some(text))),
                    None => Some((cond, None)),
                }
            })
            .collect()
    }
}

/// The act of a web step that acts, with the texts of its args.
pub(crate) enum Act<'s> {
    Open(String),
    Extract(&'s WebTarget),
    Click(&'s WebTarget),
    Fill(&'s WebTarget, String),
    Select(&'s WebTarget, String),
    Press(&'s WebTarget, String),
    Upload(&'s WebTarget, PathBuf),
    Wait(Duration),
}

impl<'s> Act<'s> {
    /// The act of `step`; none where its verb and target do not fit, its target has no locator that the executor
    /// takes, or an arg cannot be read, and none for a `fill`, a `press` or an `upload` whose target names a frame (the
    /// coordinator's rulings of log lines 16133 and 16181 on U5.10-F): the executor's check of the focus that a text
    /// or a key reaches is shown in a frame by no run, and a file set in a frame is one that the frame's script can
    /// read, so no text, no key and no file goes into a frame, and the step answers `step_failed` before the browser
    /// is asked anything for it.
    fn of(step: &'s ExecutableStep, values: &Values) -> Option<Act<'s>> {
        let target = match &step.target {
            Some(Target::Web(target)) if locators(target).is_err() => return None,
            Some(Target::Web(target))
                if !target.frame.is_empty()
                    && matches!(step.verb, Verb::Fill | Verb::Press | Verb::Upload) =>
            {
                return None
            }
            Some(Target::Web(target)) => Some(target),
            _ => None,
        };
        let arg = |name| values.arg(step, name);
        Some(match (step.verb, target) {
            (Verb::Navigate, None) => Act::Open(arg("url")?),
            (Verb::Wait, None) => Act::Wait(Duration::from_millis(arg("ms")?.parse().ok()?)),
            (Verb::Extract, Some(t)) => Act::Extract(t),
            (Verb::Click, Some(t)) => Act::Click(t),
            (Verb::Fill, Some(t)) => Act::Fill(t, arg("text")?),
            (Verb::Select, Some(t)) => Act::Select(t, arg("option")?),
            (Verb::Press, Some(t)) => Act::Press(t, arg("key")?),
            (Verb::Upload, Some(t)) => Act::Upload(t, PathBuf::from(arg("file")?)),
            _ => return None,
        })
    }

    /// Whether the act works on a control of the page, after which the page can still make a request.
    fn on_a_control(&self) -> bool {
        !matches!(self, Act::Open(_) | Act::Extract(_) | Act::Wait(_))
    }

    /// The verb of the act.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "the stand-in of the unit tests logs the verb")
    )]
    pub(crate) fn verb(&self) -> &'static str {
        match self {
            Act::Open(_) => "navigate",
            Act::Extract(_) => "extract",
            Act::Click(_) => "click",
            Act::Fill(..) => "fill",
            Act::Select(..) => "select",
            Act::Press(..) => "press",
            Act::Upload(..) => "upload",
            Act::Wait(_) => "wait",
        }
    }
}

/// The ref of the one control that the target's first locator to find any finds, in the top document or in the
/// document of the frame that the target's frame origins name; refused where the executor's fresh read marks it a
/// credential field, a read in the executor's own world of that document.
async fn one_control(executor: &mut Executor, target: &WebTarget) -> cdp::Res<String> {
    let origins = frame_origins(target);
    let frame: Vec<&str> = origins.iter().map(String::as_str).collect();
    let reference = executor.resolve_one_in(&locators(target)?, &frame).await?;
    if executor.control(&reference).await?.credential {
        return Err(Fault::new("credential", "a credential field"));
    }
    Ok(reference)
}

/// What the executor reads, fresh, for `cond`; refused for a credential field, whose text and value the runner
/// never reads.
async fn read_for(executor: &mut Executor, cond: &Cond) -> cdp::Res<Read> {
    let Some(target) = &cond.target else {
        let frame = executor.main_frame().await?;
        return Ok(Read::Url(executor.frame_url(&frame).await?));
    };
    let locators = locators(target)?;
    let origins = frame_origins(target);
    let frame: Vec<&str> = origins.iter().map(String::as_str).collect();
    match executor.resolve_in(&locators, &frame).await? {
        Resolved::None => Ok(Read::Controls {
            count: 0,
            one: None,
        }),
        Resolved::Many { .. } => Ok(Read::Controls {
            count: executor.count_in(&locators, &frame).await?,
            one: None,
        }),
        Resolved::One { reference, .. } => {
            let control = executor.control(&reference).await?;
            if control.credential {
                return Err(Fault::new("credential", "a credential field"));
            }
            Ok(Read::Controls {
                count: 1,
                one: Some(Control {
                    text: control.text,
                    value: control.value,
                    enabled: control.enabled,
                }),
            })
        }
    }
}

/// The value that `extract` keeps of the text that it read: the text for `parse` `text`, the amount for `amount`;
/// a fault for a text that is no amount and for another word.
fn parsed(parse: &str, text: String) -> Result<Value, ()> {
    match parse {
        "text" => Ok(Value::Text(text)),
        "amount" => halvo_web::extract_amount(&text)
            .map(|minor| Value::Number(minor as f64 / 100.0))
            .ok_or(()),
        _ => Err(()),
    }
}

/// Reads each condition fresh, in order, and refuses at the first that does not hold.
async fn hold(executor: &mut Executor, conds: &[(&Cond, Option<String>)]) -> cdp::Res<()> {
    for (cond, arg) in conds {
        let read = read_for(executor, cond).await?;
        if !holds(cond.check, arg.as_deref(), &read) {
            return Err(Fault::new("condition", cond.check.as_str()));
        }
    }
    Ok(())
}

/// Carries out the act on the page; the text of the control that `extract` reads. `fill` and `press` read the
/// element that holds the focus immediately before their one insert or their key, the element that the text
/// reaches, and send nothing unless it is an input, a textarea, a select or an element editable by its own
/// attribute that reads no credential mark (`Executor::fill_unless_credential`,
/// `Executor::press_unless_credential`).
async fn carry(executor: &mut Executor, act: &Act<'_>) -> cdp::Res<Option<String>> {
    match act {
        Act::Open(url) => executor.open(url).await?,
        Act::Wait(pause) => tokio::time::sleep(*pause).await,
        Act::Extract(target) => {
            let reference = one_control(executor, target).await?;
            return executor.read(&reference).await.map(Some);
        }
        Act::Click(target) => {
            let reference = one_control(executor, target).await?;
            executor.click(&reference).await?;
        }
        Act::Fill(target, text) => {
            let reference = one_control(executor, target).await?;
            executor.fill_unless_credential(&reference, text).await?;
        }
        Act::Select(target, option) => {
            let reference = one_control(executor, target).await?;
            executor.select(&reference, option).await?;
        }
        Act::Press(target, key) => {
            let reference = one_control(executor, target).await?;
            executor.press_unless_credential(&reference, key).await?;
        }
        Act::Upload(target, file) => {
            let reference = one_control(executor, target).await?;
            executor.upload(&reference, file).await?;
        }
    }
    Ok(None)
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
    requests.iter().any(|r| failed(r) && of_the_page(r))
}

/// Whether the rule failed a request of its record: it was not let through, and it is not one that `Mode::Hold`
/// holds (crate::web::maker), whose record says `held` until it is let through or failed.
fn failed(request: &PausedRequest) -> bool {
    !request.let_through && request.outcome != cdp::Outcome::Held
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
fn end_of_step<T>(done: Result<T, ()>, record: &[PausedRequest]) -> Result<T, Failure> {
    if refused_by_the_page(record) {
        return Err(Stop::End(Outcome::Refused, Some(Cause::Origin)).into());
    }
    done.map_err(|()| Failure::Step(Cause::StepFailed))
}

impl Live {
    /// Starts the browser `browser` with its profile `<data directory>/agents/profiles/<run id>/profile`, headless
    /// unless `headed`, behind a new relay that reaches nothing until `arm`; the cause of the run's end when the start
    /// fails, after which nothing of the start is left that the executor made.
    fn start(browser: &Path, headed: bool, data_dir: &DataDir, run: &RunId) -> Result<Live, Cause> {
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
            Ok((executor, relay)) => Ok(Live {
                executor,
                relay,
                run_dir,
                runtime,
                told: 0,
                loaded: BTreeSet::new(),
                armed: Vec::new(),
                #[cfg(feature = "loopback")]
                after_release: None,
                #[cfg(feature = "loopback")]
                released: None,
                #[cfg(feature = "loopback")]
                fault: None,
            }),
            Err(cause) => {
                let _ = std::fs::remove_dir(&run_dir);
                Err(cause)
            }
        }
    }

    /// Sets the relay's list to exactly the host and port of each origin, each bound by the person's own word, which
    /// closes every open tunnel to another host and port, and arms the request rule with the origins in `mode`.
    fn arm_in(&mut self, origins: &[WebOrigin], mode: Mode) -> Result<(), ()> {
        let requests = self.executor.requests();
        self.note_loaded(&requests);
        let handle = self.relay.handle();
        let mut hosts = BTreeSet::new();
        let mut allowed = Vec::with_capacity(origins.len());
        for origin in origins {
            let (host, port) = cdp::host_port(origin.as_str()).ok_or(())?;
            hosts.insert((host, port));
            allowed.push(rule_origin(origin));
        }
        handle.set_list(List::Hosts(hosts.clone()));
        for (host, port) in &hosts {
            handle.allow(host, *port, Entry::BoundLoopback);
        }
        self.armed = origins.to_vec();
        let executor = &mut self.executor;
        self.runtime
            .block_on(executor.arm_rule(allowed, mode))
            .map_err(drop)
    }

    /// Notes the origin of every request of the rule's record that the rule let through.
    fn note_loaded(&mut self, requests: &[PausedRequest]) {
        for request in requests.iter().filter(|r| r.let_through) {
            self.loaded.insert(request.origin.clone());
        }
    }
}

/// `Err(())` for a fault of the executor in the call `call` of the handback, after one line on standard error that
/// names the call and the fault's cause word, and no detail of it.
fn told<T>(call: &str, done: cdp::Res<T>) -> Result<T, ()> {
    done.map_err(|fault| crate::line(&format!("run page=failed call={call} cause={}", fault.0)))
}

impl Page for Live {
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

    /// Sets the relay's list to exactly the host and port of each origin, each bound by the person's own word, which
    /// closes every open tunnel to another host and port, and arms the request rule with the origins in `Mode::Fail`.
    fn arm(&mut self, origins: &[WebOrigin]) -> Result<(), ()> {
        self.arm_in(origins, Mode::Fail)
    }

    #[cfg(feature = "loopback")]
    /// Arms the rule in `Mode::Hold`, then notes, while the page is idle and before the act, the top document and,
    /// where `frame` names the frame of the step's target, that frame with the document whose origin the walk of
    /// `Executor::frame_and_loader_in` read there: the executor's read of the frame tree fails while the page's
    /// navigation is held (the hosted macos job of run 45028032532, `call=let_through cause=frame`), so
    /// `page_amounts` waits, in the frame of the released request, for the document that replaces the one noted
    /// here (r3 U5.10-B, the coordinator's ruling of log line 16745).
    fn arm_hold(&mut self, origins: &[WebOrigin], frame: &[WebOrigin]) -> Result<(), ()> {
        self.arm_in(origins, Mode::Hold)?;
        self.released = None;
        let path: Vec<String> = frame.iter().map(rule_origin).collect();
        let executor = &self.executor;
        let done = self.runtime.block_on(async move {
            let top = executor.main_frame().await?;
            let before = executor.loader(&top).await?;
            let mut noted = vec![(top, before)];
            if !path.is_empty() {
                let path: Vec<&str> = path.iter().map(String::as_str).collect();
                noted.push(executor.frame_and_loader_in(&path).await?);
            }
            Ok::<_, Fault>(noted)
        });
        let noted = told("arm_hold", done)?;
        self.after_release = Some(Box::new(move |executor, runtime, released| {
            let Some((frame, before)) =
                released.and_then(|id| noted.iter().find(|(frame, _)| frame.inner() == id))
            else {
                crate::line("run page=failed call=page_after cause=frame");
                return Err(());
            };
            told(
                "page_after",
                runtime.block_on(executor.wait_loaded_after(frame, Some(before))),
            )
        }));
        Ok(())
    }

    #[cfg(feature = "loopback")]
    fn held(&mut self) -> Vec<HeldRequest> {
        self.executor.paused()
    }

    #[cfg(feature = "loopback")]
    /// Lets the held request leave and sends the page no other command: a read of the page fails while its
    /// navigation is held. It notes the frame that the browser named for that request, in which `page_amounts`
    /// reads the page after the reply.
    fn let_through(&mut self, id: u64) -> Result<(), ()> {
        let frame = self
            .executor
            .paused()
            .into_iter()
            .find(|held| held.id == id)
            .map(|held| held.frame);
        let executor = &mut self.executor;
        told(
            "let_through",
            self.runtime.block_on(executor.let_through(id)),
        )?;
        self.released = frame;
        Ok(())
    }

    #[cfg(feature = "loopback")]
    fn released_status(&mut self) -> Option<i64> {
        self.executor.released_status()
    }

    #[cfg(feature = "loopback")]
    fn last_fault(&mut self) -> Option<&'static str> {
        self.fault
    }

    /// The names and values of every line of the page's snapshot (the top document and every frame in it), each read
    /// by `maker::amounts_named`.
    #[cfg(feature = "loopback")]
    fn page_amounts(&mut self) -> Result<Vec<u64>, ()> {
        let wait = self.after_release.take().ok_or(())?;
        wait(&self.executor, &self.runtime, self.released.as_deref())?;
        let executor = &mut self.executor;
        let lines = told("snapshot", self.runtime.block_on(executor.snapshot()))?;
        Ok(lines
            .iter()
            .flat_map(|line| {
                let mut named = maker::amounts_named(&line.name);
                named.extend(
                    line.value
                        .as_deref()
                        .map(maker::amounts_named)
                        .unwrap_or_default(),
                );
                named
            })
            .collect())
    }

    fn carry_out(
        &mut self,
        pre: &[(&Cond, Option<String>)],
        act: &Act<'_>,
        post: &[(&Cond, Option<String>)],
    ) -> Result<Option<String>, ()> {
        let executor = &mut self.executor;
        self.runtime
            .block_on(async move {
                hold(executor, pre).await?;
                let read = carry(executor, act).await?;
                hold(executor, post).await?;
                Ok(read)
            })
            .map_err(|_fault: Fault| {
                #[cfg(feature = "loopback")]
                {
                    self.fault = Some(_fault.0);
                }
            })
    }

    /// The rule's record; one line on standard error (`failed_line`) for each request in it that the rule failed
    /// (`failed`) and that no earlier line named.
    fn record(&mut self) -> Vec<PausedRequest> {
        let requests = self.executor.requests();
        if requests.len() < self.told {
            self.told = 0;
        }
        for request in requests[self.told..].iter().filter(|r| failed(r)) {
            crate::line(&failed_line(request));
        }
        self.told = requests.len();
        self.note_loaded(&requests);
        requests
    }

    /// Stops the browser and removes the profile, its temporary directory and the run's directory that holds them.
    fn stop(self: Box<Self>) -> bool {
        let Live {
            executor,
            relay,
            run_dir,
            runtime,
            ..
        } = *self;
        let stopped = runtime.block_on(executor.stop_and_remove());
        relay.stop();
        let _ = std::fs::remove_dir(&run_dir);
        stopped.removed
    }

    fn sign(&mut self) -> Result<Option<Sign>, ()> {
        let executor = &mut self.executor;
        told("sign", self.runtime.block_on(executor.sign()))
    }

    fn asks_for_a_sign_in(&mut self) -> Result<bool, ()> {
        let executor = &mut self.executor;
        told(
            "asks_for_a_sign_in",
            self.runtime.block_on(executor.asks_for_a_sign_in()),
        )
    }

    fn fail_held(&mut self) -> Result<(), ()> {
        let executor = &mut self.executor;
        let done = self.runtime.block_on(async move {
            for held in executor.paused() {
                executor.fail(held.id).await?;
            }
            Ok(())
        });
        told("fail_held", done)
    }

    fn close_others(&mut self) -> Result<(), ()> {
        let executor = &self.executor;
        told(
            "close_others",
            self.runtime.block_on(executor.close_other_pages()),
        )
    }

    fn clear_stores(&mut self) -> Result<(), ()> {
        let requests = self.executor.requests();
        self.note_loaded(&requests);
        let origins: Vec<String> = self.loaded.iter().cloned().collect();
        let executor = &self.executor;
        told(
            "clear_stores",
            self.runtime.block_on(executor.clear_stores(&origins)),
        )
    }

    fn load_again(&mut self) -> Result<(), ()> {
        let executor = &mut self.executor;
        told("load_again", self.runtime.block_on(executor.load_again()))
    }

    /// Sets the relay's list to every host and port, the loopback hosts of the newest arm keeping their mark, and
    /// arms the request rule in `Mode::Handback`.
    fn pass_all(&mut self) -> Result<(), ()> {
        let requests = self.executor.requests();
        self.note_loaded(&requests);
        let handle = self.relay.handle();
        handle.set_list(List::Every);
        for origin in &self.armed {
            if let Some((host, port)) = cdp::host_port(origin.as_str()) {
                handle.allow(&host, port, Entry::BoundLoopback);
            }
        }
        let executor = &mut self.executor;
        told(
            "pass_all",
            self.runtime
                .block_on(executor.arm_rule(Vec::new(), Mode::Handback)),
        )
    }

    fn address(&mut self) -> Result<String, ()> {
        let executor = &self.executor;
        let done = self.runtime.block_on(async move {
            let frame = executor.main_frame().await?;
            executor.frame_url(&frame).await
        });
        told("address", done)
    }

    /// The executor's export of the origin as the browser writes it, so that the page that reads its storage is the
    /// origin's own.
    fn export(&mut self, origin: &WebOrigin) -> Result<SessionState, ()> {
        let origins = [rule_origin(origin)];
        let executor = &self.executor;
        told(
            "export",
            self.runtime.block_on(executor.export_state(&origins)),
        )
    }

    fn import(&mut self, state: &SessionState) -> Result<(), ()> {
        let executor = &self.executor;
        told(
            "import",
            self.runtime.block_on(executor.import_state(state)),
        )
    }
}

impl Web {
    /// The browser of a run, started as `Live::start` starts it.
    fn start(browser: &Path, headed: bool, data_dir: &DataDir, run: &RunId) -> Result<Web, Cause> {
        Live::start(browser, headed, data_dir, run).map(|live| Web(Box::new(live)))
    }

    /// Stops the browser and removes what its start made; whether the profile and its temporary directory are gone.
    fn stop(self) -> bool {
        self.0.stop()
    }
}

impl Runner {
    /// A web step. A step of the verb `handback`, or one that marks a credential (`marks_a_credential`), acts on
    /// nothing: where the browser runs it reads the page (crate::handback), and it is done after a handback that
    /// ended in a bind; a step of the verb `handback` is done where the page shows no credential field or no browser
    /// runs; every other such step answers `step_failed`. Every other step that does not act (`acts`) answers
    /// `step_failed` before any browser starts, and so does one whose args or targets, or those of its conditions,
    /// cannot be read or have no locator that the executor takes. A target, of the step or of a condition, inside a
    /// frame is found in the document of the frame that its frame origins name (`frame_origins`); a `fill`, a `press`
    /// or an `upload` whose target names a frame (`Act::of`) and a step of an effect class that reaches into a frame
    /// (`reaches_a_frame`) answer `step_failed` before any act. The browser
    /// starts at the first step that acts, and the runner reads the page before each act, which goes on only after a
    /// handback that ended in a bind, or where the page shows no credential field. After an act that failed, with no
    /// failed request of the page in the rule's record, it reads the page again, so that a field that turned into a
    /// credential field at the act's own focus, which the executor refuses, opens a handback; after one that ended in
    /// a bind the step is done. Where that reading finds no credential field, and where a step marked `credential` of
    /// no effect class meets a running browser's page with none, the second reading (`asked_at_a_failure`, r3 U5.14)
    /// hands back with the reason `sign_in` at a link or a button that asks for a sign-in, in place of the failure; a
    /// run with no window that imported a kept sign-in reads no sign (the coordinator's ruling of log line 16376).
    /// After one that ended in a bind the act that failed is asked once more, with the reading of the page before it
    /// and without a second reading after it, so that a second failure fails the step as before (the coordinator's
    /// ruling of log line 16106 (3)); a step marked `credential`, which acts on nothing, is done after that bind. A
    /// step that does not fail makes no second reading.
    /// A condition of `pre` that does not hold acts on nothing; a control that the locators do not find, an act that
    /// fails and a condition of `post` that does not hold fail the step with `step_failed`. At the step's end a
    /// request of the run's page that the rule failed ends the run `refused` with the cause `origin` (A9.2,
    /// `end_of_step`), the record read again for `REFUSAL_WAIT` after a step that failed or that acted on a control.
    /// An `extract` that kept a value notes its target and its parse under the value's name, so that the browser's
    /// maker reads the value again right before its act. A web step of an effect class goes to the browser's
    /// maker (`Runner::effect_step`, crate::web::maker) in a build with the feature `loopback`, and answers
    /// `step_failed` before any browser starts in every other build.
    pub(crate) fn web_step(
        &mut self,
        step: &ExecutableStep,
        loaded: &LoadReply,
    ) -> Result<(), Failure> {
        let failed = || Failure::Step(Cause::StepFailed);
        let handback = step.verb == Verb::Handback;
        if handback || marks_a_credential(step) {
            let handed = match self.web.take() {
                Some(mut web) => {
                    let read = self.read_page(&mut *web.0, loaded, step.n);
                    self.web = Some(web);
                    read?
                }
                None => false,
            };
            return if handed
                || handback
                || (!of_an_effect_class(loaded.config.category, step)
                    && self.asked_at_a_failure(loaded, step.n)?)
            {
                Ok(())
            } else {
                Err(failed())
            };
        }
        if of_an_effect_class(loaded.config.category, step) {
            if self.reaches_a_frame_beyond_the_maker(step) {
                return Err(failed());
            }
            #[cfg(feature = "loopback")]
            return self.effect_step(step, loaded);
            #[cfg(not(feature = "loopback"))]
            return Err(failed());
        }
        if !acts(loaded.config.category, step) {
            return Err(failed());
        }
        let values = Values {
            loaded,
            kept: &self.kept,
        };
        let act = Act::of(step, &values).ok_or_else(failed)?;
        let pre = values.conditions(&step.pre).ok_or_else(failed)?;
        let post = values.conditions(&step.post).ok_or_else(failed)?;
        let as_parsed = match act {
            Act::Extract(_) => Some((
                values.arg(step, "as").ok_or_else(failed)?,
                values.arg(step, "parse").ok_or_else(failed)?,
            )),
            _ => None,
        };
        self.browser(loaded)?;
        let mut asked = false;
        loop {
            let mut web = self.web.take().ok_or_else(failed)?;
            let read = self.read_page(&mut *web.0, loaded, step.n);
            self.web = Some(web);
            read?;
            let web = &mut self.web.as_mut().ok_or_else(failed)?.0;
            let done = web
                .carry_out(&pre, &act, &post)
                .and_then(|read| match (&as_parsed, read) {
                    (Some((_, parse)), Some(text)) => parsed(parse, text).map(Some),
                    _ => Ok(None),
                });
            let record = if done.is_err() || act.on_a_control() {
                web.record_after_a_fault()
            } else {
                web.record()
            };
            if done.is_err() && !refused_by_the_page(&record) {
                let mut web = self.web.take().ok_or_else(failed)?;
                let read = self.read_page(&mut *web.0, loaded, step.n);
                self.web = Some(web);
                if read? {
                    return Ok(());
                }
                if !asked && self.asked_at_a_failure(loaded, step.n)? {
                    asked = true;
                    continue;
                }
            }
            if let (Some((name, parse)), Some(value)) = (as_parsed, end_of_step(done, &record)?) {
                if let Act::Extract(target) = act {
                    self.extracts.insert(name.clone(), (target.clone(), parse));
                }
                self.kept.insert(name, vec![vec![value]]);
            }
            return Ok(());
        }
    }

    /// The second reading of a web step that failed (r3 U5.14, the coordinator's ruling of log line 14127 (1)), made
    /// after the reading of the fields has found none and before the failure is written: where the run's browser runs
    /// and its page shows a link or a button that asks for a sign-in by its name (`Page::asks_for_a_sign_in`), the
    /// runner opens a handback of the reason `sign_in` in place of the failure, by the opening and with the ends of a
    /// handback at a credential field (`Runner::open_handback`); true after one that ended in a bind. False where no
    /// browser runs, where the page shows no such link or button, and where the second reading fails: the step fails
    /// as before. In a run with no window that imported a kept sign-in (crate::session), where the opening would end
    /// the run for want of a window (`Runner::no_window`), the sign is not read at all: the step fails as before and
    /// no kept sign-in is dropped (the coordinator's ruling of log line 16376).
    fn asked_at_a_failure(
        &mut self,
        loaded: &LoadReply,
        step: NonZeroU32,
    ) -> Result<bool, Failure> {
        if self.sessions.imported && !self.args.headed {
            return Ok(false);
        }
        let Some(mut web) = self.web.take() else {
            return Ok(false);
        };
        let read = match web.0.asks_for_a_sign_in() {
            Ok(true) => self.open_handback(&mut *web.0, loaded, step, Sign::Secret),
            Ok(false) | Err(()) => Ok(false),
        };
        self.web = Some(web);
        read
    }

    /// Whether an `extract` of this run read the value of `arg` in a frame (`extracts`).
    fn read_in_a_frame(&self, arg: &Arg) -> bool {
        matches!(arg.from, Source::Extract)
            && arg
                .key
                .as_ref()
                .and_then(|key| self.extracts.get(key.as_str()))
                .is_some_and(|(target, _)| !target.frame.is_empty())
    }

    /// Every frame path that the step names, each from the top document inward: its own target's, the target's of
    /// each of its conditions, and the target's of each `extract` of this run whose value one of its args or of its
    /// conditions' args takes (`extracts`); a path of the top document is empty (r3 U5.10-B part two, the coordinator's
    /// ruling of log line 17176).
    #[cfg(feature = "loopback")]
    fn frame_paths<'s>(&'s self, step: &'s ExecutableStep) -> Vec<&'s [WebOrigin]> {
        let mut paths = Vec::new();
        if let Some(Target::Web(target)) = &step.target {
            paths.push(&target.frame[..]);
        }
        let conds = || step.pre.iter().chain(&step.post);
        paths.extend(
            conds()
                .filter_map(|cond| cond.target.as_ref())
                .map(|t| &t.frame[..]),
        );
        let args = step
            .args
            .iter()
            .chain(conds().filter_map(|cond| cond.arg.as_ref()));
        for arg in args.filter(|arg| matches!(arg.from, Source::Extract)) {
            if let Some((target, _)) = arg
                .key
                .as_ref()
                .and_then(|key| self.extracts.get(key.as_str()))
            {
                paths.push(&target.frame[..]);
            }
        }
        paths
    }

    /// Whether the step reaches into a frame: one of the frame paths that it names (`frame_paths`) is not empty, its
    /// target or the target of one of its conditions naming a frame, or one of its args or of the args of its
    /// conditions taking a value that an `extract` of this run read in a frame (the coordinator's ruling of log line
    /// 16325, point 1). The browser's maker then reads the top page's address before any read of a value, any arm and
    /// any act, and acts only where its origin and every origin of every frame path that the step names are origins
    /// of the run (the coordinator's rulings of log lines 16745, point 2, and 17176; crate::web::maker).
    #[cfg(feature = "loopback")]
    fn reaches_a_frame(&self, step: &ExecutableStep) -> bool {
        self.frame_paths(step).iter().any(|path| !path.is_empty())
    }

    /// Whether the step reaches into a frame by a way that the browser's maker does not take (r3 U5.10-B part two, the
    /// coordinator's ruling of log line 16745: the maker takes the ways that its three real-browser tests walk, and no
    /// other): the target of one of its conditions `pre` names a frame, or the arg of one of its conditions, `pre` or
    /// `post`, takes a value that an `extract` of this run read in a frame. A web step of an effect class that does
    /// answers `step_failed` before any act; one whose own target or a condition `post`'s target names a frame, or
    /// whose arg takes a value read in a frame, goes to the maker, which records the frame document's origin for such
    /// a value (`read_again`). A `fill`, a `press` and an `upload` whose target names a frame fail in `Act::of`.
    fn reaches_a_frame_beyond_the_maker(&self, step: &ExecutableStep) -> bool {
        step.pre
            .iter()
            .filter_map(|cond| cond.target.as_ref())
            .any(|target| !target.frame.is_empty())
            || step
                .pre
                .iter()
                .chain(&step.post)
                .filter_map(|cond| cond.arg.as_ref())
                .any(|arg| self.read_in_a_frame(arg))
    }

    /// The run's browser, started at the first call: the frame `browser` once it runs, its `browser_reply`, the
    /// rule armed with the origins of the bindings, and the kept sign-ins of those origins imported (crate::session).
    /// A start that fails ends the run `failed` with the start's
    /// cause, and a runner without `--data-dir` has no place for a profile and fails the step. The program is the
    /// one of `--browser`; given none, the one that `find` finds under the runner's roots, read after the data
    /// directory, and where it finds none the run ends `failed` with the cause `no_browser` before anything is made.
    fn browser(&mut self, loaded: &LoadReply) -> Result<&mut Web, Failure> {
        if self.web.is_none() {
            let data_dir = self
                .args
                .data_dir
                .clone()
                .ok_or(Failure::Step(Cause::StepFailed))?;
            let program = match &self.args.browser {
                Some(given) => given.clone(),
                None => crate::find::find(&self.roots)
                    .ok_or(Stop::End(Outcome::Failed, Some(Cause::NoBrowser)))?,
            };
            let web = self
                .start_web(&program, &data_dir)
                .map_err(|cause| Stop::End(Outcome::Failed, Some(cause)))?;
            let frame = web.0.frame();
            self.web = Some(web);
            let frame = frame.ok_or(Stop::End(Outcome::Failed, Some(Cause::BrowserStart)))?;
            match self.link.ask(&frame)? {
                ToRunner::BrowserReply(_) => {}
                _ => return Err(Broken::Frame("not_a_browser_reply").into()),
            }
            self.check()?;
            let origins = loaded.bindings.as_ref().map_or(&[][..], |b| &b.origins[..]);
            let web = self.web.as_mut().ok_or(Failure::Step(Cause::StepFailed))?;
            web.0
                .arm(origins)
                .map_err(|()| Failure::Step(Cause::StepFailed))?;
            self.import_sessions(loaded)?;
        }
        self.web.as_mut().ok_or(Failure::Step(Cause::StepFailed))
    }

    /// The start of the run's browser by `Web::start`; in a unit test, the stand-in that the test gave, where it gave
    /// one.
    fn start_web(&mut self, program: &Path, data_dir: &DataDir) -> Result<Web, Cause> {
        #[cfg(test)]
        if let Some(web) = self.stand_in.take() {
            return Ok(web);
        }
        Web::start(program, self.args.headed, data_dir, &self.args.run)
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
    use std::sync::{Arc, Mutex};

    use halvo_policy::preset;
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

    /// U4.1w part 1 (2): each of the six checks holds on a read that fits it and fails on one that does not, and a
    /// read of another kind than the check's holds nothing.
    #[test]
    fn each_check_holds_or_fails_on_what_was_read() {
        use super::Read;
        let url = Read::Url("http://127.0.0.1:8080/form?step=2".to_string());
        let one = |text: &str, value: &str, enabled: bool| Read::Controls {
            count: 1,
            one: Some(Control {
                text: text.to_string(),
                value: value.to_string(),
                enabled,
            }),
        };
        let ada = one("Name", "Ada", true);
        let off = one("Send", "", false);
        let none = Read::Controls {
            count: 0,
            one: None,
        };
        let three = Read::Controls {
            count: 3,
            one: None,
        };
        let cases: [(Check, Option<&str>, &Read, bool); 22] = [
            (Check::UrlIs, Some("http://127.0.0.1:8080/form"), &url, true),
            (
                Check::UrlIs,
                Some("http://127.0.0.1:8080/other"),
                &url,
                false,
            ),
            (
                Check::UrlIs,
                Some("http://127.0.0.1:8080/form"),
                &ada,
                false,
            ),
            (Check::Present, None, &ada, true),
            (Check::Present, None, &none, false),
            (Check::Present, None, &three, false),
            (Check::Present, None, &url, false),
            (Check::Enabled, None, &ada, true),
            (Check::Enabled, None, &off, false),
            (Check::Enabled, None, &none, false),
            (Check::TextIs, Some("Name"), &ada, true),
            (Check::TextIs, Some("Ada"), &ada, false),
            (Check::TextIs, Some("Name"), &three, false),
            (Check::ValueIs, Some("Ada"), &ada, true),
            (Check::ValueIs, Some("Name"), &ada, false),
            (Check::ValueIs, Some(""), &none, false),
            (Check::CountIs, Some("3"), &three, true),
            (Check::CountIs, Some("0"), &none, true),
            (Check::CountIs, Some("1"), &ada, true),
            (Check::CountIs, Some("2"), &three, false),
            (Check::CountIs, Some("+3"), &three, false),
            (Check::CountIs, Some("3"), &url, false),
        ];
        for (check, arg, read, want) in cases {
            assert_eq!(
                holds(check, arg, read),
                want,
                "{check:?} with {arg:?} on {read:?}"
            );
        }
    }

    const CLICK: &str = r#"{"v":1,"n":1,"kind":"web","verb":"click","target":{"desc":"d","frame":[],"credential":"no","locators":[{"by":"test_id","value":"agree","name":null}]},"args":[],"pre":[],"post":[],"effect":"none","class":"prepare"}"#;
    const FILL: &str = r#"{"v":1,"n":1,"kind":"web","verb":"fill","target":{"desc":"d","frame":[],"credential":"no","locators":[{"by":"test_id","value":"name","name":null}]},"args":[{"name":"text","from":"const","value":"Ada","key":null}],"pre":[],"post":[{"check":"value_is","target":{"desc":"d","frame":[],"credential":"no","locators":[{"by":"test_id","value":"name","name":null}]},"arg":{"name":"text","from":"const","value":"Ada","key":null}}],"effect":"none","class":"prepare"}"#;
    const WAIT: &str = r#"{"v":1,"n":1,"kind":"web","verb":"wait","target":null,"args":[{"name":"ms","from":"const","value":"100","key":null}],"pre":[],"post":[],"effect":"none","class":"read"}"#;
    const CREDENTIAL_FILL: &str = r#"{"v":1,"n":1,"kind":"web","verb":"fill","target":{"desc":"d","frame":[],"credential":"yes","locators":[{"by":"test_id","value":"password","name":null}]},"args":[{"name":"text","from":"const","value":"x","key":null}],"pre":[],"post":[],"effect":"none","class":"credential"}"#;
    const CREDENTIAL_CONDITION: &str = r#"{"v":1,"n":1,"kind":"web","verb":"click","target":{"desc":"d","frame":[],"credential":"no","locators":[{"by":"test_id","value":"agree","name":null}]},"args":[],"pre":[{"check":"value_is","target":{"desc":"d","frame":[],"credential":"yes","locators":[{"by":"test_id","value":"password","name":null}]},"arg":{"name":"text","from":"const","value":"x","key":null}}],"post":[],"effect":"none","class":"prepare"}"#;
    const REQUEST: &str = r#"{"v":1,"n":1,"kind":"web","verb":"click","target":{"desc":"d","frame":[],"credential":"no","locators":[{"by":"test_id","value":"pay","name":null}]},"args":[{"name":"payee","from":"const","value":"p","key":null},{"name":"amount","from":"const","value":"1.00","key":null},{"name":"instrument","from":"const","value":"i","key":null},{"name":"origin","from":"const","value":"http://127.0.0.1:8080","key":null}],"pre":[],"post":[],"effect":"request","class":"payment"}"#;
    const HANDBACK: &str = r#"{"v":1,"n":1,"kind":"web","verb":"handback","target":null,"args":[{"name":"reason","from":"const","value":"sign_in","key":null}],"pre":[],"post":[],"effect":"none","class":"credential"}"#;

    /// U4.1w part 1 (5), U5.1: a web step acts where halvo-policy classes it `read` or `prepare`; a target with
    /// `credential` `yes` (of the step or of a condition) answers `step_failed` from the runner before any browser
    /// starts, and a step of the verb `handback` is done with no page to read: no frame, no browser, where the runner
    /// has a data directory and a start would end the run with `browser_start`. A step with the effect `request` does
    /// not act either; in this test build, which has the feature `loopback`, it is the browser's maker's
    /// (crate::web::maker), and in a build without the feature it answers `step_failed` before any browser starts.
    #[test]
    fn only_read_and_prepare_steps_act() {
        let step = |json: &str| ExecutableStep::parse(json.as_bytes()).expect("a step");
        let category = SeedCategory::UtilityBillPayment;
        for json in [CLICK, FILL, WAIT, NAVIGATE] {
            assert!(acts(category, &step(json)), "acts: {json}");
        }
        let refused = [CREDENTIAL_FILL, CREDENTIAL_CONDITION, REQUEST, HANDBACK];
        for json in refused {
            assert!(!acts(category, &step(json)), "does not act: {json}");
        }

        let exe = std::env::current_exe().expect("the test binary");
        let target = exe.ancestors().nth(3).expect("the target directory");
        let data = halvo_test_dir::make(&target.join("tmp").join("halvo-runner-unit"), "web-acts")
            .expect("a data directory")
            .to_string_lossy()
            .into_owned();
        let (workflow, agent) = (
            WorkflowId::from_random([7; 16]),
            AgentId::from_random([6; 16]),
        );
        let config = preset(
            Category::UtilityBillPayment,
            workflow.clone(),
            agent.clone(),
        )
        .expect("a preset");
        let loaded = LoadReply {
            config,
            playbook: None,
            next_playbook: NonZeroU32::MIN,
            bindings: None,
            sessions: Vec::new(),
            start: None,
        };
        let run = "0c".repeat(16);
        let words = [
            "--run",
            &run,
            "--agent",
            agent.as_str(),
            "--workflow",
            workflow.as_str(),
            "--mode",
            "replay",
            "--browser",
            "none",
            "--data-dir",
            &data,
            "--utc-offset",
            "0",
        ];
        let words: Vec<OsString> = words.iter().map(OsString::from).collect();
        let args = crate::args::parse(&words).expect("the arguments");
        let out = Arc::new(Mutex::new(Vec::new()));
        let link = Link::start(
            Box::new(std::io::empty()),
            Box::new(Out(Arc::clone(&out))),
            Box::new(|| {}),
        );
        let mut runner = Runner::new(args, link);
        for json in [CREDENTIAL_FILL, CREDENTIAL_CONDITION] {
            let got = runner.web_step(&step(json), &loaded);
            assert!(
                matches!(got, Err(Failure::Step(Cause::StepFailed))),
                "step_failed for {json}: {got:?}"
            );
            assert!(runner.web.is_none(), "no browser for {json}");
        }
        let got = runner.web_step(&step(HANDBACK), &loaded);
        assert!(matches!(got, Ok(())), "done for {HANDBACK}: {got:?}");
        assert!(runner.web.is_none(), "no browser for {HANDBACK}");
        assert!(out.lock().expect("lock").is_empty(), "no frame");
    }

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
            end_of_step::<()>(Err(()), &record),
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

    /// The stand-in of the executor of `a_step_acts_on_a_target_inside_the_frame_that_its_step_names`: its reading
    /// shows no credential field, each condition and each act is one line of the log with the frame origins of its
    /// target, every condition holds, and an extract reads `12.34`.
    struct FramePage(Arc<Mutex<Vec<String>>>);

    /// The frame origins of a target, as one text.
    fn frame_text(target: &WebTarget) -> String {
        let origins: Vec<&str> = target.frame.iter().map(WebOrigin::as_str).collect();
        format!("[{}]", origins.join(" "))
    }

    impl Page for FramePage {
        fn frame(&self) -> Option<FromRunner> {
            None
        }

        fn arm(&mut self, _: &[WebOrigin]) -> Result<(), ()> {
            Ok(())
        }

        fn carry_out(
            &mut self,
            pre: &[(&Cond, Option<String>)],
            act: &Act<'_>,
            post: &[(&Cond, Option<String>)],
        ) -> Result<Option<String>, ()> {
            let mut log = self.0.lock().expect("the log");
            for (word, conds) in [("pre", pre), ("post", post)] {
                for (cond, _) in conds {
                    let frame = cond.target.as_ref().map_or("none".to_string(), frame_text);
                    log.push(format!("{word} {} in {frame}", cond.check.as_str()));
                }
            }
            let target = match act {
                Act::Extract(t) | Act::Click(t) | Act::Fill(t, _) | Act::Select(t, _) => Some(*t),
                Act::Press(t, _) | Act::Upload(t, _) => Some(*t),
                Act::Open(_) | Act::Wait(_) => None,
            };
            let frame = target.map_or("none".to_string(), frame_text);
            log.push(format!("act {} in {frame}", act.verb()));
            Ok(matches!(act, Act::Extract(_)).then(|| "12.34".to_string()))
        }

        fn record(&mut self) -> Vec<PausedRequest> {
            Vec::new()
        }

        fn stop(self: Box<Self>) -> bool {
            true
        }

        fn sign(&mut self) -> Result<Option<Sign>, ()> {
            self.0.lock().expect("the log").push("read".to_string());
            Ok(None)
        }

        fn fail_held(&mut self) -> Result<(), ()> {
            Ok(())
        }

        fn close_others(&mut self) -> Result<(), ()> {
            Ok(())
        }

        fn clear_stores(&mut self) -> Result<(), ()> {
            Ok(())
        }

        fn load_again(&mut self) -> Result<(), ()> {
            Ok(())
        }

        fn pass_all(&mut self) -> Result<(), ()> {
            Ok(())
        }

        fn address(&mut self) -> Result<String, ()> {
            Ok("http://127.0.0.1:8080/pay".to_string())
        }

        fn export(&mut self, _: &WebOrigin) -> Result<SessionState, ()> {
            Err(())
        }

        fn import(&mut self, _: &SessionState) -> Result<(), ()> {
            Err(())
        }
    }

    /// The top document's origin and the two frame origins of the frame tests.
    const TOP: &str = "http://127.0.0.1:8080";
    const PAY: &str = "http://127.0.0.1:8081";
    const INNER: &str = "http://127.0.0.1:8082";

    /// The JSON of a web target found by the one locator `locator` inside the frames of the origins `frame`, the
    /// outermost first.
    fn framed(locator: &str, frame: &[&str]) -> String {
        let origins: Vec<String> = frame.iter().map(|o| format!("\"{o}\"")).collect();
        format!(
            r#"{{"desc":"d","frame":[{}],"credential":"no","locators":[{locator}]}}"#,
            origins.join(",")
        )
    }

    /// U5.10-F (the coordinator's ruling of log line 15706, points 1 and 2): a web step whose target names the frame
    /// that holds it by the frame's origins, from the top document inward, is carried to the page with that target, as
    /// a step on the top document is, and so is a condition on such a target: the extract of the amount with a
    /// condition `present` on it, the select of the payment method, and a click in a frame inside the frame. The page
    /// is read first, as before each act, and the extract keeps its amount. A step of an effect class whose condition
    /// `pre`'s target names the frame, or whose condition's arg is a text read in the frame, and a `fill`, a `press`
    /// and an `upload` whose target names a frame, each answer `step_failed`, and the stand-in page sees no read and no
    /// act of them (the coordinator's words on this unit: no text, key or file goes into a frame; r3 U5.10-B part two
    /// gives the browser's maker a step of an effect class whose own target or condition `post`'s target names a
    /// frame, or whose amount is a value read in one, by the coordinator's ruling of log line 16745).
    #[test]
    fn a_step_acts_on_a_target_inside_the_frame_that_its_step_names() {
        let amount = framed(r#"{"by":"test_id","value":"amount","name":null}"#, &[PAY]);
        let extract = format!(
            r#"{{"v":1,"n":1,"kind":"web","verb":"extract","target":{amount},"args":[{{"name":"as","from":"const","value":"amount","key":null}},{{"name":"parse","from":"const","value":"amount","key":null}}],"pre":[{{"check":"present","target":{amount},"arg":null}}],"post":[],"effect":"none","class":"read"}}"#
        );
        let select = format!(
            r#"{{"v":1,"n":2,"kind":"web","verb":"select","target":{},"args":[{{"name":"option","from":"const","value":"Card ending 4242","key":null}}],"pre":[],"post":[],"effect":"none","class":"prepare"}}"#,
            framed(
                r#"{"by":"role","value":"combobox","name":"Payment method"}"#,
                &[PAY]
            )
        );
        let click = format!(
            r#"{{"v":1,"n":3,"kind":"web","verb":"click","target":{},"args":[],"pre":[],"post":[],"effect":"none","class":"prepare"}}"#,
            framed(
                r#"{"by":"role","value":"button","name":"Show terms"}"#,
                &[PAY, INNER]
            )
        );
        let payee = framed(r#"{"by":"test_id","value":"payee","name":null}"#, &[PAY]);
        let payee_text = format!(
            r#"{{"v":1,"n":4,"kind":"web","verb":"extract","target":{payee},"args":[{{"name":"as","from":"const","value":"payee_text","key":null}},{{"name":"parse","from":"const","value":"text","key":null}}],"pre":[],"post":[],"effect":"none","class":"read"}}"#
        );
        let (workflow, agent) = (
            WorkflowId::from_random([7; 16]),
            AgentId::from_random([6; 16]),
        );
        let config = preset(
            Category::UtilityBillPayment,
            workflow.clone(),
            agent.clone(),
        )
        .expect("a preset");
        let origins = [TOP, PAY, INNER].map(|o| WebOrigin::parse(o).expect("an origin"));
        let loaded = LoadReply {
            config,
            playbook: None,
            next_playbook: NonZeroU32::MIN,
            bindings: Some(LoadedBindings {
                rev: NonZeroU32::MIN,
                origins: origins.to_vec(),
                slots: vec![halvo_schema::executable::record::Slot {
                    name: halvo_schema::executable::text::Name::parse("document").expect("a name"),
                    value: halvo_schema::executable::text::OwnValue::new("meter.jpg"),
                }],
            }),
            sessions: Vec::new(),
            start: None,
        };
        let run = "0e".repeat(16);
        let words = [
            "--run",
            &run,
            "--agent",
            agent.as_str(),
            "--workflow",
            workflow.as_str(),
            "--mode",
            "replay",
            "--browser",
            "none",
            "--utc-offset",
            "0",
        ];
        let words: Vec<OsString> = words.iter().map(OsString::from).collect();
        let args = crate::args::parse(&words).expect("the arguments");
        let out = Arc::new(Mutex::new(Vec::new()));
        let link = Link::start(
            Box::new(std::io::empty()),
            Box::new(Out(Arc::clone(&out))),
            Box::new(|| {}),
        );
        let mut runner = Runner::new(args, link);
        let page_log = Arc::new(Mutex::new(Vec::new()));
        runner.web = Some(Web(Box::new(FramePage(Arc::clone(&page_log)))));
        for json in [&extract, &select, &click, &payee_text] {
            let step = ExecutableStep::parse(json.as_bytes()).expect("a step");
            let got = runner.web_step(&step, &loaded);
            assert!(
                got.is_ok(),
                "step {}: {got:?}, log {:?}",
                step.n,
                page_log.lock().expect("the log")
            );
        }
        let log = page_log.lock().expect("the log").clone();
        assert_eq!(
            log,
            [
                "read".to_string(),
                format!("pre present in [{PAY}]"),
                format!("act extract in [{PAY}]"),
                "read".to_string(),
                format!("act select in [{PAY}]"),
                "read".to_string(),
                format!("act click in [{PAY} {INNER}]"),
                "read".to_string(),
                format!("act extract in [{PAY}]"),
            ]
        );
        assert_eq!(
            runner.kept.get("amount"),
            Some(&vec![vec![Value::Number(12.34)]])
        );

        let pay_now = r#"{"by":"test_id","value":"pay-now","name":null}"#;
        let judged_by_the_frame = format!(
            r#"{{"v":1,"n":4,"kind":"web","verb":"click","target":{},"args":[{{"name":"payee","from":"const","value":"p","key":null}},{{"name":"amount","from":"const","value":"12.34","key":null}},{{"name":"instrument","from":"const","value":"i","key":null}},{{"name":"origin","from":"const","value":"{PAY}","key":null}}],"pre":[{{"check":"text_is","target":{},"arg":{{"name":"arg","from":"extract","value":null,"key":"payee_text"}}}}],"post":[],"effect":"request","class":"payment"}}"#,
            framed(pay_now, &[]),
            framed(r#"{"by":"test_id","value":"payee","name":null}"#, &[])
        );
        let judged_in_the_frame = format!(
            r#"{{"v":1,"n":4,"kind":"web","verb":"click","target":{},"args":[{{"name":"payee","from":"const","value":"p","key":null}},{{"name":"amount","from":"const","value":"12.34","key":null}},{{"name":"instrument","from":"const","value":"i","key":null}},{{"name":"origin","from":"const","value":"{PAY}","key":null}}],"pre":[{{"check":"present","target":{payee},"arg":null}}],"post":[],"effect":"request","class":"payment"}}"#,
            framed(pay_now, &[])
        );
        let typed = |verb: &str, arg: &str| {
            format!(
                r#"{{"v":1,"n":5,"kind":"web","verb":"{verb}","target":{},"args":[{arg}],"pre":[],"post":[],"effect":"none","class":"prepare"}}"#,
                framed(r#"{"by":"test_id","value":"note","name":null}"#, &[PAY])
            )
        };
        let fill = typed(
            "fill",
            r#"{"name":"text","from":"const","value":"Ada","key":null}"#,
        );
        let press = typed(
            "press",
            r#"{"name":"key","from":"const","value":"Enter","key":null}"#,
        );
        let upload = typed(
            "upload",
            r#"{"name":"file","from":"slot","value":null,"key":"document"}"#,
        );
        for json in [
            &judged_by_the_frame,
            &judged_in_the_frame,
            &fill,
            &press,
            &upload,
        ] {
            let step = ExecutableStep::parse(json.as_bytes()).expect("a step");
            let got = runner.web_step(&step, &loaded);
            let now = page_log.lock().expect("the log").clone();
            assert!(
                matches!(got, Err(Failure::Step(Cause::StepFailed))) && now == log,
                "a step that this build makes no act of in a frame fails before any act: {got:?}, {now:?}, {json}"
            );
        }
    }
}
