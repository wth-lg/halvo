//! The compile's drive (r3 U7.2b; contracts A12.4, A3, A9.5, A6.4 and A6.5): a hosted model works out the steps of
//! an agent's task from snapshots of the agent's own browser. It serves a compile of a `web` agent with no stored
//! playbook whose tier is a hosted tier that the person allowed (crate::model) and whose category has a goal text of
//! the driver trial (`utility_bill_payment`, the task bill; `shop_order`, the task order).
//!
//! At its head, before the browser starts, before the key is asked and before any hold, it asks the router's pick
//! (crate::route); a pick that hands back, and a category with no goal text, end the run `handed_back` with
//! `shortfall`. Step 1 is the runner's own `navigate` to the slot `start`. Then, until the model answers `done`:
//! the reading of A6.4, which hands the browser to the person at a credential field and adds a `handback` step for
//! each handback that ended in a bind, so that no request holds a page with a credential field; the page's lines,
//! masked so that no text of a slot of the load leaves (crate::model::snapshot, whose makers alone make a request's
//! snapshot); the limit `wall`, the person's stop and the limit `tokens` of A6.5, read before the pick of each
//! request; the pick again; ONE request through `ask`, the one way of the runner to its model; the limits again, with
//! the reply's tokens counted. Each action of a reply is carried out by
//! `Runner::web_step` as the step that it becomes, the code that replays it, and the step is stored (`put_playbook`)
//! once it is done. A reply that makes no step adds one line to the list `Done so far:` and counts toward `wall` and
//! `tokens` alone; no count of asks and no repeated reply ends a compile. The texts of a request are the trial's,
//! byte for byte (crate::model::texts, crate::words).

use std::num::NonZeroU32;

use halvo_schema::executable::body::{LoadReply, PutPlaybook};
use halvo_schema::executable::names::LogEvent;
use halvo_schema::executable::{Cause, ExecutableStep, Outcome, RunnerCall, WebOrigin};
use halvo_schema::SeedCategory;
use halvo_web::cdp::{Sign, SnapshotLine};

use crate::action::{self, Action};
use crate::model::ask::{Answer, Grammar, ModelRequest, OUTPUT_LIMIT};
use crate::model::snapshot::Snapshot;
use crate::model::texts;
use crate::route::{self, Call};
use crate::run::{cause_of, Failure, Runner, Stop};
use crate::words;

/// The most steps of a playbook (A3.4), read by `Runner::keep` before each store: a 65th step is carried out and not
/// stored, and the compile ends `failed` with `step_failed`, since the closed list `Cause` holds no `length`. The limit
/// `steps` of every preset is lower and ends a compile first, but a `handback` step that the reading writes counts
/// toward no `steps`.
const STEPS_MAX: usize = 64;

/// The door's refusals of a request that left and got no action from the host: an answer cut at the output limit
/// and one that the host declined. The compile goes on with no line of its own, and the request counts toward the
/// limit `tokens` at `OUTPUT_LIMIT`. Every other refusal ends the run `failed` with `step_failed`.
const NO_ACTION: [&str; 2] = ["cut", "declined"];

/// The goal text of a category: the trial's goal file of its task.
fn goal(category: SeedCategory) -> Option<&'static str> {
    match category {
        SeedCategory::UtilityBillPayment => Some(texts::GOAL_BILL),
        SeedCategory::ShopOrder => Some(texts::GOAL_ORDER),
        _ => None,
    }
}

/// The `system` of every request of a compile: the boundary text, a blank line and the fixed text of the action's
/// form (r3 U0.8d), as the trial's `system_of` makes it for a hosted ask of the grammar `action`.
pub(crate) fn system() -> String {
    format!("{}\n\n{}", texts::BOUNDARY, words::ACTION_FORM_HOSTED)
}

/// A text of a step's JSON: the string in JSON's quoted form.
fn quoted(text: &str) -> String {
    serde_json::Value::String(text.to_owned()).to_string()
}

/// An arg of A3.1 whose value comes from `from` (`const` with `value`, or `slot` or `extract` with `key`).
fn arg(name: &str, from: &str, value: &str) -> String {
    if from == "const" {
        format!(
            "{{\"name\":{},\"from\":\"const\",\"value\":{},\"key\":null}}",
            quoted(name),
            quoted(value)
        )
    } else {
        format!(
            "{{\"name\":{},\"from\":{},\"value\":null,\"key\":{}}}",
            quoted(name),
            quoted(from),
            quoted(value)
        )
    }
}

/// The word of a role locator for a line's role: the executor's own word that its `map_role` reads back as that role
/// (a line of plain text shows `text` and is found as `StaticText`, a frame shows `frame` and is found as `Iframe`);
/// every other role as the line shows it.
fn locator_role(role: &str) -> &str {
    match role {
        "text" => "StaticText",
        "frame" => "Iframe",
        other => other,
    }
}

/// The web target of A3.1 that a snapshot line names: the line's role and accessible name as a role locator, its
/// frame, and `credential` `no` (a compile acts on no line of a page with a credential field).
fn target(line: &SnapshotLine) -> String {
    let frame = line.frame.iter().map(|f| quoted(f)).collect::<Vec<_>>();
    format!(
        "{{\"desc\":{},\"frame\":[{}],\"credential\":\"no\",\"locators\":[{{\"by\":\"role\",\"value\":{},\"name\":{}}}]}}",
        quoted(&format!("{} {}", line.role, line.name)),
        frame.join(","),
        quoted(locator_role(line.role)),
        quoted(&line.name)
    )
}

/// The step of A3.1 numbered `n` of `verb` with `target` and `args`, of the effect `none`, its class by the policy's
/// `class_of` for `category`; none where the parser of A3 refuses it.
fn step(
    category: SeedCategory,
    n: usize,
    verb: &str,
    target: Option<String>,
    args: &[String],
) -> Option<ExecutableStep> {
    step_of_effect(category, n, verb, target, args, "none")
}

/// The step of `step` with the effect `effect`. The parser of A3 reads the bound fields of an effect step by the class
/// that the step's JSON names, so an effect step is written with the class of its category's effect step of A9.4
/// (`payment`, `purchase`) and every other with `read`; `class_of` then gives the class again.
fn step_of_effect(
    category: SeedCategory,
    n: usize,
    verb: &str,
    target: Option<String>,
    args: &[String],
    effect: &str,
) -> Option<ExecutableStep> {
    let class = match (effect, category) {
        ("request", SeedCategory::UtilityBillPayment) => "payment",
        ("request", SeedCategory::ShopOrder) => "purchase",
        _ => "read",
    };
    let line = format!(
        "{{\"v\":1,\"n\":{n},\"kind\":\"web\",\"verb\":{},\"target\":{},\"args\":[{}],\"pre\":[],\"post\":[],\"effect\":{},\"class\":{}}}",
        quoted(verb),
        target.unwrap_or_else(|| "null".to_owned()),
        args.join(","),
        quoted(effect),
        quoted(class)
    );
    let mut step = ExecutableStep::parse(line.as_bytes()).ok()?;
    step.class = halvo_policy::class_of(category, &step);
    Some(step)
}

/// The extracts that A9.4 reads as amounts: the amount due of a bill and the total of an order. Every other extract
/// keeps the page's text.
const AMOUNTS: [&str; 2] = ["amount_due", "total"];

/// The step that an action on the snapshot line `line` becomes, numbered `n`.
fn step_of(
    category: SeedCategory,
    n: usize,
    action: &Action,
    line: &SnapshotLine,
) -> Option<ExecutableStep> {
    let on = Some(target(line));
    match action {
        Action::Click { .. } => step(category, n, "click", on, &[]),
        Action::Fill { slot, .. } => step(category, n, "fill", on, &[arg("text", "slot", slot)]),
        Action::Select { slot, .. } => {
            step(category, n, "select", on, &[arg("option", "slot", slot)])
        }
        Action::Press { key, .. } => step(
            category,
            n,
            "press",
            on,
            &[arg("key", "const", key.as_str())],
        ),
        Action::Extract { name, .. } => {
            let parse = if AMOUNTS.contains(&name.as_str()) {
                "amount"
            } else {
                "text"
            };
            step(
                category,
                n,
                "extract",
                on,
                &[arg("as", "const", name), arg("parse", "const", parse)],
            )
        }
        Action::Handback | Action::Done => None,
    }
}

/// The bound fields of the effect step of a category (A9.4), with the origin of its state-changing request, and the
/// extracts that they need; none for a category with no such step.
fn bound_fields(
    category: SeedCategory,
    origin: &str,
) -> Option<(Vec<String>, &'static [&'static str])> {
    match category {
        SeedCategory::UtilityBillPayment => Some((
            vec![
                arg("payee", "slot", "payee"),
                arg("amount", "extract", "amount_due"),
                arg("instrument", "slot", "instrument"),
                arg("origin", "const", origin),
            ],
            &["amount_due"],
        )),
        SeedCategory::ShopOrder => Some((
            vec![
                arg("seller", "slot", "seller"),
                arg("items", "extract", "items"),
                arg("amount", "extract", "total"),
                arg("instrument", "slot", "instrument"),
                arg("origin", "const", origin),
            ],
            &["items", "total"],
        )),
        _ => None,
    }
}

/// The extracts that the bound fields of the category's effect step of A9.4 need, none for a category with no such
/// step: what a click or a press of a compile reads again before its act in a build with the maker (r3 U7.10).
#[cfg_attr(not(feature = "loopback"), allow(dead_code))]
pub(crate) fn needed_extracts(category: SeedCategory) -> &'static [&'static str] {
    bound_fields(category, "").map_or(&[], |(_, needed)| needed)
}

/// The effect step of A9.4 that a click or a press on `line` becomes where its state-changing request to `origin`, an
/// origin of the bindings, was failed or held by the rule: the same act with the effect `request` and the bound fields
/// of the category; with the extracts that they need.
fn effect_step(
    category: SeedCategory,
    n: usize,
    action: &Action,
    line: &SnapshotLine,
    origin: &str,
) -> Option<(ExecutableStep, &'static [&'static str])> {
    let (mut args, needed) = bound_fields(category, origin)?;
    let verb = match action {
        Action::Click { .. } => "click",
        Action::Press { key, .. } => {
            args.insert(0, arg("key", "const", key.as_str()));
            "press"
        }
        _ => return None,
    };
    let step = step_of_effect(category, n, verb, Some(target(line)), &args, "request")?;
    Some((step, needed))
}

/// Step 1 of every compile: the runner's own `navigate` to the slot `start` (A12.4).
fn navigate_start(category: SeedCategory) -> Option<ExecutableStep> {
    step(
        category,
        1,
        "navigate",
        None,
        &[arg("url", "slot", "start")],
    )
}

/// The `handback` step that the runner writes where it handed back (A12.4, A6.4), numbered `n`.
fn handback_step(category: SeedCategory, n: usize) -> Option<ExecutableStep> {
    step(
        category,
        n,
        "handback",
        None,
        &[arg("reason", "const", "sign_in")],
    )
}

/// An action that was carried out, whose line of the list gets its form when the next snapshot is read.
struct Carried {
    at: usize,
    action: Action,
    before: String,
    /// Whether a state-changing request of it left (an effect step that its maker carried out).
    request: bool,
}

/// What one compile holds while it goes.
struct Drive {
    category: SeedCategory,
    number: NonZeroU32,
    goal: &'static str,
    steps: Vec<ExecutableStep>,
    list: Vec<String>,
    carried: Option<Carried>,
    /// The page's text before the newest stored step, for the rule of a repeat.
    last_text: Option<String>,
    tokens: u64,
}

impl Drive {
    /// The number of the next step.
    fn next(&self) -> usize {
        self.steps.len() + 1
    }
}

/// The end of a run whose router lets no request of it leave.
fn shortfall() -> Stop {
    Stop::End(Outcome::HandedBack, Some(Cause::Shortfall))
}

/// The end of a run whose compile cannot go on.
fn failed(cause: Cause) -> Stop {
    Stop::End(Outcome::Failed, Some(cause))
}

impl Runner {
    /// The compile's drive of the agent of `loaded` (the module's doc): `completed` once the model answers `done`.
    pub(crate) fn compile(&mut self, loaded: &LoadReply) -> Result<Outcome, Stop> {
        route::picked(loaded, Call::Compile).map_err(|_| shortfall())?;
        let goal = goal(loaded.config.category).ok_or_else(shortfall)?;
        let category = loaded.config.category;
        let mut drive = Drive {
            category,
            number: loaded.next_playbook,
            goal,
            steps: Vec::new(),
            list: vec![words::NAVIGATE_START.to_owned()],
            carried: None,
            last_text: None,
            tokens: 0,
        };
        let first = navigate_start(category).ok_or_else(|| failed(Cause::StepFailed))?;
        self.within(&loaded.config.limits)?;
        self.check()?;
        match self.web_step(&first, loaded) {
            Ok(()) => self.log(LogEvent::StepOk, Some(first.n), None)?,
            Err(Failure::Stop(stop)) => return Err(stop),
            Err(Failure::Step(cause)) => {
                return Err(self.failed_step(&first, cause, loaded.config.runtime))
            }
        }
        self.steps += 1;
        self.keep(&mut drive, first)?;
        loop {
            let (lines, snapshot) = self.read_for_a_request(&mut drive, loaded)?;
            let text = snapshot.text().to_owned();
            self.within_wall()?;
            self.check()?;
            self.within_tokens(&drive, loaded)?;
            let picked = route::picked(loaded, Call::Compile).map_err(|_| shortfall())?;
            let request = ModelRequest {
                system: system(),
                prompt: words::frame(
                    drive.goal,
                    &slot_names(loaded),
                    &self.kept_names(),
                    &drive.list,
                    words::NEXT,
                ),
                snapshot: Some(snapshot),
                grammar: Grammar::Action,
            };
            let reply = match self.ask(&picked, &request)? {
                Answer::Reply {
                    text,
                    tokens_in,
                    tokens_out,
                } => {
                    drive.tokens = drive
                        .tokens
                        .saturating_add(tokens_in)
                        .saturating_add(tokens_out);
                    Some(text)
                }
                Answer::Refused(word) if NO_ACTION.contains(&word.as_str()) => {
                    drive.tokens = drive.tokens.saturating_add(OUTPUT_LIMIT);
                    None
                }
                Answer::Refused(_) => return Err(failed(Cause::StepFailed)),
            };
            self.within_wall()?;
            self.within_tokens(&drive, loaded)?;
            let Some(reply) = reply else {
                continue;
            };
            let action = match action::parse(&reply) {
                Ok(action) => action,
                Err(refused) => {
                    drive.list.push(words::reply_refused(&reply, refused));
                    continue;
                }
            };
            match &action {
                Action::Done => return Ok(Outcome::Completed),
                Action::Handback => self.model_handback(&mut drive, loaded)?,
                _ => self.act(&mut drive, loaded, &action, &lines, text)?,
            }
        }
    }

    /// The reading of A6.4 before a request and the page's lines after it: a `handback` step for each handback that
    /// ended in a bind, the form of the list's line of the action carried out last, and the page's text. THE LINES
    /// THAT LEAVE ARE THE LINES THAT ARE READ (the static seat's F2 at 3048dbfd): where the lines of the snapshot hold
    /// one with the credential mark, the runner hands back as at the reading's own finding (the reason `sign_in`) and
    /// reads the page again from the top; the lines that pass this check are the request's, and no reading of the
    /// page comes behind it.
    fn read_for_a_request(
        &mut self,
        drive: &mut Drive,
        loaded: &LoadReply,
    ) -> Result<(Vec<SnapshotLine>, Snapshot), Stop> {
        let mut handbacks = 0;
        let lines = loop {
            let n = NonZeroU32::new(drive.next() as u32).unwrap_or(NonZeroU32::MAX);
            let mut web = self.web.take().ok_or_else(|| failed(Cause::StepFailed))?;
            let read = self.read_page(&mut *web.0, loaded, n);
            let handed_by_the_reading = matches!(read, Ok(true));
            let lines = match read {
                Ok(false) => web.0.snapshot().map_err(|()| failed(Cause::StepFailed)),
                Ok(true) => Ok(Vec::new()),
                Err(failure) => Err(match failure {
                    Failure::Stop(stop) => stop,
                    Failure::Step(cause) => failed(cause),
                }),
            };
            let lines = match lines {
                Ok(lines) => lines,
                Err(stop) => {
                    self.web = Some(web);
                    return Err(stop);
                }
            };
            let marked = lines.iter().any(|line| line.credential);
            if !handed_by_the_reading && !marked {
                self.web = Some(web);
                break lines;
            }
            let handed = if marked {
                self.open_handback(&mut *web.0, loaded, n, Sign::Secret)
            } else {
                Ok(true)
            };
            self.web = Some(web);
            let bound = match handed {
                Ok(bound) => bound,
                Err(Failure::Stop(stop)) => return Err(stop),
                Err(Failure::Step(cause)) => return Err(failed(cause)),
            };
            handbacks += 1;
            if handbacks > crate::handback::IN_A_ROW {
                return Err(Stop::End(Outcome::HandedBack, None));
            }
            if bound {
                drive.list.push(words::HANDBACK.to_owned());
                let step = handback_step(drive.category, drive.next())
                    .ok_or_else(|| failed(Cause::StepFailed))?;
                self.keep(drive, step)?;
            }
        };
        let snapshot = Snapshot::masked(&lines, slots(loaded));
        if let Some(carried) = drive.carried.take() {
            drive.list[carried.at] = words::carried(
                &carried.action,
                carried.request,
                handbacks == 0 && carried.before == snapshot.text(),
            );
        }
        Ok((lines, snapshot))
    }

    /// The limit `tokens` of A6.5: the run ends `limit` where the tokens of its requests are at or above the newest
    /// load's `model_tokens`.
    fn within_tokens(&self, drive: &Drive, loaded: &LoadReply) -> Result<(), Stop> {
        if drive.tokens >= u64::from(loaded.config.limits.model_tokens) {
            return Err(Stop::End(Outcome::Limit, Some(Cause::Tokens)));
        }
        Ok(())
    }

    /// The model's `handback` (A12.4): the `handback` step, carried out as a replay carries it out (it hands the
    /// browser to the person where the page shows a credential field), and stored.
    fn model_handback(&mut self, drive: &mut Drive, loaded: &LoadReply) -> Result<(), Stop> {
        let step =
            handback_step(drive.category, drive.next()).ok_or_else(|| failed(Cause::StepFailed))?;
        match self.web_step(&step, loaded) {
            Ok(()) => {}
            Err(Failure::Stop(stop)) => return Err(stop),
            Err(Failure::Step(cause)) => return Err(failed(cause)),
        }
        drive.list.push(words::HANDBACK.to_owned());
        self.keep(drive, step)
    }

    /// An action on a line of the page: refused, with its line of the list, where the page holds no such line, where
    /// it fills, selects or presses on a line marked credential, where it names a slot that the load does not hold,
    /// or where the page did not take it; else carried out by `web_step` as the step that it becomes, and stored
    /// unless it repeats the step stored last on an unchanged page.
    fn act(
        &mut self,
        drive: &mut Drive,
        loaded: &LoadReply,
        action: &Action,
        lines: &[SnapshotLine],
        text: String,
    ) -> Result<(), Stop> {
        let reference = action.reference().unwrap_or_default();
        let Some(line) = lines.iter().find(|line| line.reference == reference) else {
            drive
                .list
                .push(words::refused(action, &words::no_reference(reference)));
            return Ok(());
        };
        let typing = matches!(
            action,
            Action::Fill { .. } | Action::Select { .. } | Action::Press { .. }
        );
        if typing && line.credential {
            drive.list.push(words::refused(action, words::CREDENTIAL));
            return Ok(());
        }
        if let Action::Fill { slot, .. } | Action::Select { slot, .. } = action {
            if crate::connect::slot(loaded, slot).is_none() {
                drive
                    .list
                    .push(words::refused(action, &words::no_slot(slot)));
                return Ok(());
            }
        }
        let Some(step) = step_of(drive.category, drive.next(), action, line) else {
            drive
                .list
                .push(words::refused(action, &words::page_refused("step_form")));
            return Ok(());
        };
        self.within(&loaded.config.limits)?;
        self.check()?;
        self.compiling = matches!(action, Action::Click { .. } | Action::Press { .. });
        let done = self.web_step(&step, loaded);
        self.compiling = false;
        #[cfg(feature = "loopback")]
        if let Some(held) = self.held_click.take() {
            return self.effect_held(drive, loaded, action, line, held, text);
        }
        if let (Ok(()), Some(origin)) = (&done, self.effect_origin.take()) {
            return self.effect(drive, loaded, action, line, &origin);
        }
        match done {
            Ok(()) => self.log(LogEvent::StepOk, Some(step.n), None)?,
            Err(Failure::Stop(stop)) => return Err(stop),
            Err(Failure::Step(cause)) => {
                let why = match action {
                    Action::Select { .. } => {
                        words::select_cause(reference, line.role, cause.as_str())
                    }
                    _ => words::page_refused(cause.as_str()),
                };
                drive.list.push(words::refused(action, &why));
                return Ok(());
            }
        }
        self.steps += 1;
        drive.carried = Some(Carried {
            at: drive.list.len(),
            action: action.clone(),
            before: text.clone(),
            request: false,
        });
        drive.list.push(words::carried(action, false, false));
        let repeat = drive
            .steps
            .last()
            .is_some_and(|last| same_step(last, &step))
            && drive.last_text.as_deref() == Some(text.as_str());
        if !repeat {
            drive.last_text = Some(text);
            self.keep(drive, step)?;
        }
        Ok(())
    }

    /// The effect step of a click or a press whose state-changing request to `origin`, an origin of the bindings, the
    /// rule failed (`ClickWay::Fail`; (d1), the coordinator's log line 22533): written from the action by A9.4 and
    /// stored, then `step_failed` before any act, as every build without the browser's maker carries an effect step
    /// out: the run ends `failed` and the compile's number stays unfinished (A3.4). No second act is made (r3 U7.10,
    /// the coordinator's point M2). A category with no effect step of A9.4 ends the run `failed` with `step_failed`; a
    /// missing extract that the step's fields name, with `provenance`.
    fn effect(
        &mut self,
        drive: &mut Drive,
        loaded: &LoadReply,
        action: &Action,
        line: &SnapshotLine,
        origin: &WebOrigin,
    ) -> Result<(), Stop> {
        let (step, needed) =
            effect_step(drive.category, drive.next(), action, line, origin.as_str())
                .ok_or_else(|| failed(Cause::StepFailed))?;
        if needed.iter().any(|name| !self.kept.contains_key(*name)) {
            return Err(failed(Cause::Provenance));
        }
        self.keep(drive, step.clone())?;
        self.within(&loaded.config.limits)?;
        self.check()?;
        Err(self.failed_step(&step, Cause::StepFailed, loaded.config.runtime))
    }

    /// The effect step of a click or a press whose state-changing request the rule HELD at its own act
    /// (`ClickWay::Hold`, r3 U7.10; the coordinator's rulings of 2026-11-26, P1 and M2): written from the action by
    /// A9.4 with the held request's origin and stored before its card; then the held request goes to its card by the
    /// browser's maker (`Runner::release_a_compiles_request`), with no second act, and leaves ONCE at the person's
    /// `allow_once`. Where the act holds not exactly one request, the category has no effect step of A9.4, an extract
    /// that the step's fields name is missing, or the run's limits or the person's stop end the run first, every held
    /// request is failed and nothing leaves.
    #[cfg(feature = "loopback")]
    fn effect_held(
        &mut self,
        drive: &mut Drive,
        loaded: &LoadReply,
        action: &Action,
        line: &SnapshotLine,
        held: crate::web::HeldClick,
        text: String,
    ) -> Result<(), Stop> {
        let stop = |failure: Failure| match failure {
            Failure::Stop(stop) => stop,
            Failure::Step(cause) => failed(cause),
        };
        let Some(origin) = held.origin() else {
            return Err(stop(self.let_nothing_leave(&held, "count")));
        };
        let Some((step, needed)) =
            effect_step(drive.category, drive.next(), action, line, origin.as_str())
        else {
            let _ = self.let_nothing_leave(&held, "no_effect_step");
            return Err(failed(Cause::StepFailed));
        };
        if needed.iter().any(|name| !self.kept.contains_key(*name)) {
            let _ = self.let_nothing_leave(&held, "provenance");
            return Err(failed(Cause::Provenance));
        }
        let kept = self
            .keep(drive, step.clone())
            .and_then(|()| self.within(&loaded.config.limits))
            .and_then(|()| self.check());
        if let Err(end) = kept {
            let _ = self.let_nothing_leave(&held, "stop");
            return Err(end);
        }
        match self.release_a_compiles_request(&step, loaded, held) {
            Ok(()) => self.log(LogEvent::StepOk, Some(step.n), None)?,
            Err(Failure::Stop(stop)) => return Err(stop),
            Err(Failure::Step(cause)) => {
                return Err(self.failed_step(&step, cause, loaded.config.runtime))
            }
        }
        self.steps += 1;
        drive.carried = Some(Carried {
            at: drive.list.len(),
            action: action.clone(),
            before: text,
            request: true,
        });
        drive.list.push(words::carried(action, true, false));
        Ok(())
    }

    /// Stores `step` under the compile's number (`put_playbook`) and keeps it; the run's `finish` names the number
    /// from the first stored step on.
    fn keep(&mut self, drive: &mut Drive, step: ExecutableStep) -> Result<(), Stop> {
        if drive.steps.len() >= STEPS_MAX {
            return Err(failed(Cause::StepFailed));
        }
        let call = RunnerCall::PutPlaybook(PutPlaybook {
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            number: drive.number,
            steps: vec![step.clone()],
        });
        match self.store(call)? {
            Ok(_) => {
                self.playbook = Some(drive.number);
                drive.steps.push(step);
                Ok(())
            }
            Err(error) => Err(failed(cause_of(error))),
        }
    }

    /// The names of the values that this run kept, in order.
    fn kept_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.kept.keys().cloned().collect();
        names.sort();
        names
    }
}

/// The load's slots, in the load's order.
pub(crate) fn slots(loaded: &LoadReply) -> &[halvo_schema::executable::record::Slot] {
    loaded
        .bindings
        .as_ref()
        .map_or(&[][..], |bindings| &bindings.slots[..])
}

/// The names of the load's slots but `start`, in the load's order.
fn slot_names(loaded: &LoadReply) -> Vec<String> {
    loaded
        .bindings
        .as_ref()
        .map(|bindings| {
            bindings
                .slots
                .iter()
                .map(|slot| slot.name.as_str().to_owned())
                .filter(|name| name != "start")
                .collect()
        })
        .unwrap_or_default()
}

/// Whether two steps do the same thing: the same verb, target and args (their numbers aside).
fn same_step(a: &ExecutableStep, b: &ExecutableStep) -> bool {
    a.verb == b.verb && a.target == b.target && a.args == b.args
}

#[cfg(test)]
mod tests;
