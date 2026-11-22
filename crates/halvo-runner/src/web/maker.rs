//! The browser's request maker (r3 U4.1w-2; the coordinator's rulings of log lines 13348, 14717 and 14732), in a
//! build with the feature `loopback` alone, as the mail's send: no build that a person can use makes a state-changing
//! web request before the owner answers D36. A web step of an effect class (`payment`, `purchase`, `submit`, `sign`)
//! arms the run's origins in `Mode::Hold` for that step alone, acts, and gives the ONE request that the act holds to
//! `Runner::release` as a [`Paused`] of this maker, which names itself in `always_asks`: no request of it leaves
//! without an `approved` row, whatever the policy clears. After `allow_once` the request leaves once; after
//! `reject_once`, and at every other end of the confirm path, the held request is failed and nothing leaves. Where
//! the act holds no request, or more than one, every held request is failed, no card is shown and the step answers
//! `step_failed`. When the confirm path has ended the rule is armed in `Mode::Fail` again.
//!
//! What the maker reads as made: the HTTP status of the response of the request that left, as the rule paused it,
//! 200 alone; every other status is answered, and no response within `STEP_TIMEOUT` is no reply, both of which leave
//! the effect not known. After a reply read as made the runner reads the page for a `payment` and a `purchase`
//! (`page_amounts`, `amounts_named`). The maker reads no body of a request.

use std::num::NonZeroU32;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::names::{FieldName, Source};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::step::WebTarget;
use halvo_schema::executable::text::OwnValue;
use halvo_schema::executable::{ActionClass, Bound, BoundField, Cause, ExecutableStep, WebOrigin};
use halvo_web::cdp::{HeldRequest, STEP_TIMEOUT};

use super::{class_of, end_of_step, refused_by_the_page, rule_origin, Act, Values, Web};
use crate::release::{Left, MakerFacts, Paused, ReadPage, Replied};
use crate::run::{Failure, Runner};

/// How long after its act an effect step waits for the first held request.
const HOLD_WAIT: Duration = Duration::from_secs(5);

/// How long after the first held request the step waits for a second one before it counts them.
const SETTLE: Duration = Duration::from_millis(300);

/// How often a wait reads the rule's held requests or the status again.
const POLL: Duration = Duration::from_millis(20);

/// The one success status of a web request of an effect (the coordinator's ruling of log line 14732).
const MADE: i64 = 200;

/// The run's browser, shared between the runner and the paused request of its maker while the confirm path runs.
type Shared = Arc<Mutex<Option<Web>>>;

fn lock(web: &Shared) -> MutexGuard<'_, Option<Web>> {
    web.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Every amount that `text` names in the form that `extract_amount` reads (digits, one `.` and two decimals), each
/// run of digits, `.` and `,` with the `,` taken out and a `.` or `,` at either end left off, in minor units.
pub(crate) fn amounts_named(text: &str) -> Vec<u64> {
    text.split(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
        .map(|run| run.trim_matches(|c| c == '.' || c == ',').replace(',', ""))
        .filter_map(|run| halvo_web::extract_amount(&run))
        .collect()
}

/// Where a bound field of the step comes from: the const of its arg, the slot that its arg names, or the value that
/// an earlier `extract` kept, read again with that extract's target and parse.
enum From {
    Const(String),
    Slot(String),
    Extract { target: WebTarget, parse: String },
}

/// The one held request of a web step of an effect class, paused: dropped before it is let through, it is failed.
struct Held {
    web: Shared,
    id: u64,
    origin: WebOrigin,
    class: ActionClass,
    step: NonZeroU32,
    fields: Vec<(FieldName, From)>,
    /// The value of each slot that the first `bound` read, by the slot's name.
    seen: Vec<(String, String)>,
    left: bool,
}

impl Held {
    /// The value of an extract read again on the page with its target, as its parse keeps it (the text, or the
    /// amount in the form `<units>.<two decimals>`), and the origin of the page that holds it.
    fn read_again(&self, target: &WebTarget, parse: &str) -> Result<(String, WebOrigin), Cause> {
        let failed = |part: &str| {
            crate::line(&format!("run effect=failed at=read_again part={part}"));
            Cause::StepFailed
        };
        let mut guard = lock(&self.web);
        let web = guard.as_mut().ok_or_else(|| failed("browser"))?;
        let text = web
            .0
            .carry_out(&[], &Act::Extract(target), &[])
            .ok()
            .flatten()
            .ok_or_else(|| failed("extract"))?;
        let value = match parse {
            "text" => text,
            "amount" => {
                let minor = halvo_web::extract_amount(&text).ok_or_else(|| failed("amount"))?;
                format!("{}.{:02}", minor / 100, minor % 100)
            }
            _ => return Err(failed("parse")),
        };
        let address = web.0.address().map_err(|()| failed("address"))?;
        let origin = WebOrigin::of_address(&address).map_err(|_| failed("origin"))?;
        Ok((value, origin))
    }
}

impl Paused for Held {
    fn class(&self) -> ActionClass {
        self.class
    }

    /// Each field from its source: a const as the step holds it, a slot from `slots`, the slots of the class in the
    /// newest load (`changed` where one is gone), and an extract read again on the page while the request is held,
    /// with the origin of the page.
    fn bound(&mut self, slots: &[Slot]) -> Result<Bound, Cause> {
        let mut fields = Vec::with_capacity(self.fields.len());
        let mut seen = Vec::new();
        for (name, from) in &self.fields {
            let (source, origin, value) = match from {
                From::Const(value) => (Source::Const, None, value.clone()),
                From::Slot(key) => {
                    let value = slots
                        .iter()
                        .find(|s| s.name.as_str() == key)
                        .map(|s| s.value.joined())
                        .ok_or(Cause::Changed)?;
                    seen.push((key.clone(), value.clone()));
                    (Source::Slot, None, value)
                }
                From::Extract { target, parse } => {
                    let (value, origin) = self.read_again(target, parse)?;
                    (Source::Extract, Some(origin), value)
                }
            };
            fields.push(BoundField {
                name: *name,
                from: source,
                origin,
                value: OwnValue::new(&value),
            });
        }
        if self.seen.is_empty() {
            self.seen = seen;
        }
        Bound::new(self.class, fields).map_err(|_| Cause::StepFailed)
    }

    fn facts(&self) -> MakerFacts {
        MakerFacts {
            request_origin: Some(self.origin.clone()),
            ..MakerFacts::default()
        }
    }

    fn release(mut self: Box<Self>) -> Box<dyn Left> {
        self.left = true;
        let went = lock(&self.web)
            .as_mut()
            .is_some_and(|web| web.0.let_through(self.id).is_ok());
        Box::new(Sent {
            web: Arc::clone(&self.web),
            went,
        })
    }

    /// The same held request where every slot that it read is unchanged in `slots`: the page made the request, and
    /// the maker makes none again. Where a slot changed the request is failed and the answer is `changed`.
    fn renew(self: Box<Self>, slots: &[Slot]) -> Result<Box<dyn Paused>, Cause> {
        let unchanged = self.seen.iter().all(|(key, value)| {
            slots
                .iter()
                .any(|s| s.name.as_str() == key && s.value.joined() == *value)
        });
        if unchanged {
            Ok(self)
        } else {
            Err(Cause::Changed)
        }
    }

    fn step(&self) -> Option<NonZeroU32> {
        Some(self.step)
    }

    fn of_a_web_step(&self) -> bool {
        true
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        if !self.left {
            if let Some(web) = lock(&self.web).as_mut() {
                let _ = web.0.fail_held();
            }
        }
    }
}

/// The request that the maker let leave.
struct Sent {
    web: Shared,
    went: bool,
}

impl Left for Sent {
    fn reply(self: Box<Self>) -> Replied {
        self.reply_and_page().0
    }

    /// Made for the status 200 alone, with the read of the page after it; answered for every other status; no reply
    /// where the browser did not take the release or no status came within `STEP_TIMEOUT`.
    fn reply_and_page(self: Box<Self>) -> (Replied, Option<ReadPage>) {
        if !self.went {
            return (Replied::NoReply, None);
        }
        let until = Instant::now() + STEP_TIMEOUT;
        let status = loop {
            let status = lock(&self.web)
                .as_mut()
                .and_then(|web| web.0.released_status());
            if status.is_some() || Instant::now() >= until {
                break status;
            }
            std::thread::sleep(POLL);
        };
        match status {
            Some(MADE) => {
                let web = self.web;
                let read: ReadPage = Box::new(move || {
                    lock(&web)
                        .as_mut()
                        .and_then(|web| web.0.page_amounts().ok())
                        .unwrap_or_default()
                });
                (Replied::Made, Some(read))
            }
            Some(_) => (Replied::Answered, None),
            None => (Replied::NoReply, None),
        }
    }
}

/// The held requests after the act: the first within `HOLD_WAIT`, then all of them after `SETTLE`.
fn held_after_the_act(web: &mut Web) -> Vec<HeldRequest> {
    let until = Instant::now() + HOLD_WAIT;
    while web.0.held().is_empty() && Instant::now() < until {
        std::thread::sleep(POLL);
    }
    std::thread::sleep(SETTLE);
    web.0.held()
}

/// The origin of the bindings that the rule compares with the browser's text of a held request's origin, else the
/// origin of that text.
fn origin_of(held: &HeldRequest, origins: &[WebOrigin]) -> Option<WebOrigin> {
    origins
        .iter()
        .find(|o| rule_origin(o) == held.origin)
        .cloned()
        .or_else(|| WebOrigin::of_address(&held.origin).ok())
}

impl Runner {
    /// The origins that the rule holds after the run's last arm in `Mode::Fail`: those of the bindings of `loaded`
    /// and every origin bound in this run (crate::handback).
    fn run_origins(&self, loaded: &LoadReply) -> Vec<WebOrigin> {
        let mut origins = loaded
            .bindings
            .as_ref()
            .map_or_else(Vec::new, |b| b.origins.clone());
        for bound in &self.bound {
            if !origins.contains(bound) {
                origins.push(bound.clone());
            }
        }
        origins
    }

    /// The source of each bound field of the step's class, in the class's order, from the step's arg of that name;
    /// none where an arg is missing, comes from the model, or names an extract that no step of this run noted.
    fn sources(&self, step: &ExecutableStep, class: ActionClass) -> Option<Vec<(FieldName, From)>> {
        class
            .bound_fields()
            .iter()
            .map(|name| {
                let arg = step
                    .args
                    .iter()
                    .find(|a| a.name.as_str() == name.as_str())?;
                let from = match arg.from {
                    Source::Const => From::Const(arg.value.as_ref()?.as_str().to_string()),
                    Source::Slot => From::Slot(arg.key.as_ref()?.as_str().to_string()),
                    Source::Extract => {
                        let (target, parse) = self.extracts.get(arg.key.as_ref()?.as_str())?;
                        From::Extract {
                            target: target.clone(),
                            parse: parse.clone(),
                        }
                    }
                    _ => return None,
                };
                Some((*name, from))
            })
            .collect()
    }

    /// A web step of an effect class (the module's text): its act and conditions as a web step that acts takes them,
    /// the page read first for a credential field (crate::handback), the rule armed in `Mode::Hold` with the run's
    /// origins for the act, the one held request given to `release`, and the rule armed in `Mode::Fail` again. A
    /// request of the page that the rule failed ends the run `refused` with the cause `origin` (`end_of_step`).
    pub(crate) fn effect_step(
        &mut self,
        step: &ExecutableStep,
        loaded: &LoadReply,
    ) -> Result<(), Failure> {
        let failed = || Failure::Step(Cause::StepFailed);
        let class = class_of(loaded.config.category, step);
        let values = Values {
            loaded,
            kept: &self.kept,
        };
        let act = Act::of(step, &values).ok_or_else(failed)?;
        let pre = values.conditions(&step.pre).ok_or_else(failed)?;
        let post = values.conditions(&step.post).ok_or_else(failed)?;
        let sources = self.sources(step, class).ok_or_else(failed)?;
        self.browser(loaded)?;
        let mut web = self.web.take().ok_or_else(failed)?;
        let read = self.read_page(&mut *web.0, loaded, step.n);
        self.web = Some(web);
        read?;
        let origins = self.run_origins(loaded);
        let web = self.web.as_mut().ok_or_else(failed)?;
        if web.0.arm_hold(&origins).is_err() {
            let _ = web.0.arm(&origins);
            return Err(failed());
        }
        let done = web.0.carry_out(&pre, &act, &post);
        let held = if done.is_ok() {
            held_after_the_act(web)
        } else {
            web.0.held()
        };
        let record = web.0.record_after_a_fault();
        let one = match held.as_slice() {
            [one] if done.is_ok() && !refused_by_the_page(&record) => {
                origin_of(one, &origins).map(|origin| (one.id, origin))
            }
            _ => None,
        };
        let Some((id, origin)) = one else {
            crate::line(&format!(
                "run effect=failed at=act act={} held={} refused={}",
                if done.is_ok() { "done" } else { "failed" },
                held.len(),
                if refused_by_the_page(&record) {
                    "yes"
                } else {
                    "no"
                }
            ));
            let _ = web.0.fail_held();
            let _ = web.0.arm(&origins);
            return end_of_step(Err(()), &record);
        };
        let shared: Shared = Arc::new(Mutex::new(self.web.take()));
        let request = Held {
            web: Arc::clone(&shared),
            id,
            origin,
            class,
            step: step.n,
            fields: sources,
            seen: Vec::new(),
            left: false,
        };
        let end = self.release(vec![Box::new(request)]);
        if let Err(crate::run::Stop::End(outcome, cause)) = &end {
            crate::line(&format!(
                "run effect=ended outcome={} cause={}",
                outcome.as_str(),
                cause.map_or("none", |c| c.as_str())
            ));
        }
        self.web = lock(&shared).take();
        let rearmed = self.web.as_mut().map_or(Err(()), |web| web.0.arm(&origins));
        end?;
        rearmed.map_err(|()| failed())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use halvo_schema::executable::names::{AllowedClass, Answer, LogEvent};
    use halvo_schema::executable::step::{Cond, Target};
    use halvo_schema::executable::{EffectState, FromRunner, Outcome};
    use halvo_schema::Category;
    use halvo_web::cdp::{PausedRequest, SessionState, Sign};

    use super::*;
    use crate::release::suite::{run_web_step, slot, Plan, Record};
    use crate::web::Page;

    type Log = Arc<Mutex<Vec<String>>>;

    /// The values that earlier steps kept, by name.
    type Kept = HashMap<String, Vec<Vec<halvo_sheet::Value>>>;

    /// The target and the parse of the extract that kept each value, by name.
    type Extracts = HashMap<String, (WebTarget, String)>;

    /// The bound origin of the agent in every test.
    const BOUND: &str = "http://127.0.0.1:8080";

    /// A step `click` of the button `pay-now` with the effect `request`, of the class `payment`: the payee and the
    /// instrument from their slots, the amount from the value that an earlier extract kept as `amount`, and the
    /// origin of the bindings.
    const PAY: &str = r#"{"v":1,"n":3,"kind":"web","verb":"click","target":{"desc":"d","frame":[],"credential":"no","locators":[{"by":"test_id","value":"pay-now","name":null}]},"args":[{"name":"payee","from":"slot","value":null,"key":"payee"},{"name":"amount","from":"extract","value":null,"key":"amount"},{"name":"instrument","from":"slot","value":null,"key":"instrument"},{"name":"origin","from":"const","value":"http://127.0.0.1:8080","key":null}],"pre":[],"post":[],"effect":"request","class":"payment"}"#;

    /// A step `click` of the button `send` with the effect `request`, of the class `submit` for an agent of the
    /// category `email`.
    const SUBMIT: &str = r#"{"v":1,"n":2,"kind":"web","verb":"click","target":{"desc":"d","frame":[],"credential":"no","locators":[{"by":"test_id","value":"send","name":null}]},"args":[{"name":"origin","from":"const","value":"http://127.0.0.1:8080","key":null},{"name":"fields","from":"const","value":"note=hello","key":null}],"pre":[],"post":[],"effect":"request","class":"submit"}"#;

    /// The step `extract` that kept the amount.
    const EXTRACT: &str = r#"{"v":1,"n":2,"kind":"web","verb":"extract","target":{"desc":"the amount","frame":[],"credential":"no","locators":[{"by":"test_id","value":"amount","name":null}]},"args":[{"name":"as","from":"const","value":"amount","key":null},{"name":"parse","from":"const","value":"amount","key":null}],"pre":[],"post":[],"effect":"none","class":"read"}"#;

    fn note(log: &Log, line: impl Into<String>) {
        log.lock().expect("the log").push(line.into());
    }

    fn lines(log: &Log) -> Vec<String> {
        log.lock().expect("the log").clone()
    }

    /// A held request of the rule, POST to the bound origin, under the executor's id `id`.
    fn held(id: u64) -> HeldRequest {
        HeldRequest {
            id,
            method: "POST".to_string(),
            origin: BOUND.to_string(),
            path: "/charge".to_string(),
            frame: "F1".to_string(),
        }
    }

    /// The stand-in of the executor: an arm in `Mode::Fail` fails what it holds, a click holds the requests that the
    /// test gave, an extract reads `12.34`, a release of a held id gives the status that the test gave, the page after
    /// it names the amounts that the test gave, and every call is one line of the log.
    struct StandIn {
        log: Log,
        holds: Vec<HeldRequest>,
        click_holds: Vec<HeldRequest>,
        status: Option<i64>,
        released: bool,
        amounts: Vec<u64>,
    }

    impl StandIn {
        fn new(log: &Log, click_holds: Vec<HeldRequest>) -> StandIn {
            StandIn {
                log: Arc::clone(log),
                holds: Vec::new(),
                click_holds,
                status: Some(200),
                released: false,
                amounts: vec![1234],
            }
        }
    }

    fn named(origins: &[WebOrigin]) -> String {
        origins
            .iter()
            .map(WebOrigin::as_str)
            .collect::<Vec<_>>()
            .join(" ")
    }

    impl Page for StandIn {
        fn frame(&self) -> Option<FromRunner> {
            None
        }

        fn arm(&mut self, origins: &[WebOrigin]) -> Result<(), ()> {
            note(&self.log, format!("arm fail [{}]", named(origins)));
            self.holds.clear();
            Ok(())
        }

        fn carry_out(
            &mut self,
            _: &[(&Cond, Option<String>)],
            act: &Act<'_>,
            _: &[(&Cond, Option<String>)],
        ) -> Result<Option<String>, ()> {
            note(&self.log, format!("act {}", act.verb()));
            match act {
                Act::Extract(_) => Ok(Some("12.34".to_string())),
                Act::Click(_) => {
                    self.holds = self.click_holds.clone();
                    Ok(None)
                }
                _ => Ok(None),
            }
        }

        fn record(&mut self) -> Vec<PausedRequest> {
            Vec::new()
        }

        fn record_after_a_fault(&mut self) -> Vec<PausedRequest> {
            Vec::new()
        }

        fn stop(self: Box<Self>) -> bool {
            true
        }

        fn sign(&mut self) -> Result<Option<Sign>, ()> {
            Ok(None)
        }

        fn fail_held(&mut self) -> Result<(), ()> {
            note(&self.log, format!("fail held {}", self.holds.len()));
            self.holds.clear();
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
            Ok(format!("{BOUND}/pay"))
        }

        fn export(&mut self, _: &WebOrigin) -> Result<SessionState, ()> {
            Err(())
        }

        fn import(&mut self, _: &SessionState) -> Result<(), ()> {
            Err(())
        }

        fn arm_hold(&mut self, origins: &[WebOrigin]) -> Result<(), ()> {
            note(&self.log, format!("arm hold [{}]", named(origins)));
            Ok(())
        }

        fn held(&mut self) -> Vec<HeldRequest> {
            self.holds.clone()
        }

        fn let_through(&mut self, id: u64) -> Result<(), ()> {
            note(&self.log, format!("let through {id}"));
            if !self.holds.iter().any(|h| h.id == id) {
                return Err(());
            }
            self.holds.clear();
            self.released = true;
            Ok(())
        }

        fn released_status(&mut self) -> Option<i64> {
            self.status.filter(|_| self.released)
        }

        fn page_amounts(&mut self) -> Result<Vec<u64>, ()> {
            note(&self.log, "read page");
            Ok(self.amounts.clone())
        }
    }

    fn step(json: &str) -> ExecutableStep {
        ExecutableStep::parse(json.as_bytes()).expect("a step")
    }

    /// The values and the extract that the step `extract` of the amount left: `12.34` as `amount`.
    fn kept_amount() -> (Kept, Extracts) {
        let Some(Target::Web(target)) = step(EXTRACT).target else {
            panic!("a web target")
        };
        (
            HashMap::from([(
                "amount".to_string(),
                vec![vec![halvo_sheet::Value::Number(12.34)]],
            )]),
            HashMap::from([("amount".to_string(), (target, "amount".to_string()))]),
        )
    }

    /// The payment step run on the stand-in with `click_holds`, answered as `plan` says.
    fn pay(log: &Log, click_holds: Vec<HeldRequest>, plan: Plan) -> Record {
        let (kept, extracts) = kept_amount();
        run_web_step(
            Category::UtilityBillPayment,
            (
                vec![WebOrigin::parse(BOUND).expect("an origin")],
                vec![
                    slot("payee", "Larkspur Power"),
                    slot("instrument", "Card ending 4417"),
                ],
            ),
            Web(Box::new(StandIn::new(log, click_holds))),
            &step(PAY),
            kept,
            extracts,
            plan,
        )
    }

    fn rows(record: &Record) -> Vec<(LogEvent, Option<u32>, Option<Cause>)> {
        record
            .logs()
            .iter()
            .map(|l| (l.event, l.step.map(NonZeroU32::get), l.cause))
            .collect()
    }

    /// The position of the first line of `log` that is `line`.
    fn at(log: &[String], line: &str) -> usize {
        log.iter()
            .position(|l| l == line)
            .unwrap_or_else(|| panic!("no line {line} in {log:?}"))
    }

    /// r3 U4.1w-2 (1): an effect step arms the bindings' origins in `Mode::Hold` for that step alone, acts, and gives
    /// its one held request to the confirm path; after `allow_once` the request leaves once, the reply 200 is made and
    /// committed, the page after it is read, and the rule is armed in `Mode::Fail` again.
    #[test]
    fn an_effect_step_leaves_once_after_the_yes() {
        let log: Log = Arc::default();
        let record = pay(&log, vec![held(7)], Plan::plain());
        let log = lines(&log);
        let hold = format!("arm hold [{BOUND}]");
        assert!(at(&log, &hold) < at(&log, "act click"), "{log:?}");
        assert_eq!(
            log.iter().filter(|l| l.starts_with("let through")).count(),
            1,
            "{log:?}"
        );
        assert!(at(&log, "act click") < at(&log, "let through 7"));
        assert!(at(&log, "let through 7") < at(&log, "read page"));
        assert_eq!(
            log.iter().rev().find(|l| l.starts_with("arm ")),
            Some(&format!("arm fail [{BOUND}]")),
            "the rule is armed in Mode::Fail again: {log:?}"
        );
        assert!(!log.iter().any(|l| l.starts_with("fail held")), "{log:?}");
        assert_eq!(record.confirms().len(), 1, "one card");
        assert_eq!(
            record.states(),
            [
                EffectState::Held,
                EffectState::Approved,
                EffectState::Releasing,
                EffectState::Committed
            ]
        );
        assert_eq!(rows(&record), [(LogEvent::Confirm, None, None)]);
        assert_eq!(record.done(), Some(Outcome::Completed));
    }

    /// r3 U4.1w-2 (2): after `reject_once` the held request is failed and nothing leaves; and a request of the maker
    /// that the policy clears, a `submit` that the load allows, does not leave without an `approved` row either: the
    /// effect is ended, the request failed, and the run ends `refused` with the cause `approval`.
    #[test]
    fn a_rejected_effect_step_fails_its_held_request() {
        let log: Log = Arc::default();
        let record = pay(
            &log,
            vec![held(7)],
            Plan {
                answer: Answer::RejectOnce,
                ..Plan::plain()
            },
        );
        let log = lines(&log);
        assert!(!log.iter().any(|l| l.starts_with("let through")), "{log:?}");
        assert!(at(&log, "fail held 1") < log.len() - 1, "{log:?}");
        assert_eq!(log.last(), Some(&format!("arm fail [{BOUND}]")));
        assert_eq!(record.states(), [EffectState::Held, EffectState::Rejected]);
        assert_eq!(record.done(), Some(Outcome::Declined));

        let log: Log = Arc::default();
        let record = run_web_step(
            Category::Email,
            (
                vec![WebOrigin::parse(BOUND).expect("an origin")],
                Vec::new(),
            ),
            Web(Box::new(StandIn::new(&log, vec![held(9)]))),
            &step(SUBMIT),
            HashMap::new(),
            HashMap::new(),
            Plan {
                allow_at: Some(|_| vec![AllowedClass::Submit]),
                ..Plan::plain()
            },
        );
        let log = lines(&log);
        assert!(!log.iter().any(|l| l.starts_with("let through")), "{log:?}");
        assert!(log.iter().any(|l| l == "fail held 1"), "{log:?}");
        assert!(record.confirms().is_empty(), "no card");
        assert_eq!(record.states().first(), Some(&EffectState::Cleared));
        assert!(!record.states().contains(&EffectState::Releasing));
        assert_eq!(record.done(), Some(Outcome::Refused));
        assert_eq!(
            rows(&record).last(),
            Some(&(LogEvent::Stopped, None, Some(Cause::Approval)))
        );
    }

    /// r3 U4.1w-2 (3): where the act holds two requests, both are failed, no card is shown and no effect row stands,
    /// and the step answers `step_failed`; where it holds none, the same.
    #[test]
    fn two_held_requests_fail_the_step_with_no_card() {
        for holds in [vec![held(7), held(8)], Vec::new()] {
            let count = holds.len();
            let log: Log = Arc::default();
            let record = pay(&log, holds, Plan::plain());
            let log = lines(&log);
            assert!(!log.iter().any(|l| l.starts_with("let through")), "{log:?}");
            assert!(
                log.iter().any(|l| *l == format!("fail held {count}")),
                "{log:?}"
            );
            assert_eq!(log.last(), Some(&format!("arm fail [{BOUND}]")));
            assert!(record.confirms().is_empty(), "no card");
            assert!(record.states().is_empty(), "no effect row");
            assert_eq!(
                rows(&record),
                [
                    (LogEvent::StepFailed, Some(3), Some(Cause::StepFailed)),
                    (LogEvent::Stopped, None, Some(Cause::StepFailed)),
                ]
            );
            assert_eq!(record.done(), Some(Outcome::Failed));
        }
    }
}
