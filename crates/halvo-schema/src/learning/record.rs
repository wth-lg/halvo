use std::fmt;
use std::num::NonZeroU32;

use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::category::SeedCategory;
use crate::executable::names::Mode;
use crate::executable::step::Locator;
use crate::executable::text::{OwnText, RunId};
use crate::ids::{AgentId, WorkflowId};
use crate::site::{AppId, SiteValue};

use super::json::{self, list, opt, Refusal};
use super::names::{state_name, EditCause, Mark, RunCause, EDIT_STATES, RUN_STATES};
use super::{CLEAN_BOUND, LOCATORS_BOUND, RUNS_BOUND, TARGET_BYTES};

/// A stored day: the local calendar day counted in days from 1970-01-01, at least 1 (contracts.md L0.6). A
/// parser refuses 0, the unset value, with the rule `zero`. It is written as its number.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Day(NonZeroU32);

impl Day {
    /// The day `n`, or none for 0.
    pub fn new(n: u32) -> Option<Day> {
        NonZeroU32::new(n).map(Day)
    }

    /// The number of the day.
    pub fn get(self) -> u32 {
        self.0.get()
    }

    pub(in crate::learning) fn from_raw(raw: &RawValue) -> Result<Day, Refusal> {
        Day::new(json::digits("Day", raw)?).ok_or(Refusal::new("Day", "zero"))
    }
}

impl Serialize for Day {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(self.get())
    }
}

/// The id of an edit: 32 lower-case hex digits. The handler that first stores an edit mints it with
/// `from_random`; any other string is refused with the rule `hex32`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EditId(String);

impl EditId {
    /// Returns the id when `s` is 32 lower-case hex digits, else the refusal with the rule `hex32`.
    pub fn parse(s: &str) -> Result<EditId, Refusal> {
        let hex = s.len() == 32
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !hex {
            return Err(Refusal::new("EditId", "hex32"));
        }
        Ok(EditId(s.to_owned()))
    }

    /// Writes the 16 bytes as 32 lower-case hex digits, in order: the one constructor from outside a parser.
    pub fn from_random(bytes: [u8; 16]) -> EditId {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(32);
        for b in bytes {
            out.push(char::from(DIGITS[usize::from(b >> 4)]));
            out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
        }
        EditId(out)
    }

    /// The 32 digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn from_raw(raw: &RawValue) -> Result<EditId, Refusal> {
        EditId::parse(&json::string("EditId", raw)?)
    }
}

impl fmt::Display for EditId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for EditId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

pub(in crate::learning) fn workflow_id(raw: &RawValue) -> Result<WorkflowId, Refusal> {
    WorkflowId::parse(&json::string("WorkflowId", raw)?).map_err(json::core)
}

pub(in crate::learning) fn agent_id(raw: &RawValue) -> Result<AgentId, Refusal> {
    AgentId::parse(&json::string("AgentId", raw)?).map_err(json::core)
}

fn run_id(raw: &RawValue) -> Result<RunId, Refusal> {
    RunId::parse(&json::string("RunId", raw)?).map_err(json::agent_setup)
}

fn mode(raw: &RawValue) -> Result<Mode, Refusal> {
    Mode::parse(&json::string("Mode", raw)?).map_err(json::agent_setup)
}

fn own_text(raw: &RawValue) -> Result<OwnText, Refusal> {
    OwnText::parse(&json::string("OwnText", raw)?).map_err(json::agent_setup)
}

fn locator(raw: &RawValue) -> Result<Locator, Refusal> {
    Locator::parse(raw.get().as_bytes()).map_err(json::agent_setup)
}

fn seed_category(raw: &RawValue) -> Result<SeedCategory, Refusal> {
    SeedCategory::parse(&json::string("SeedCategory", raw)?).map_err(json::core)
}

fn app_id(raw: &RawValue) -> Result<AppId, Refusal> {
    AppId::parse(&json::string("AppId", raw)?).map_err(json::core)
}

fn site_value(raw: &RawValue) -> Result<SiteValue, Refusal> {
    json::object("SiteValue", raw)?;
    serde_json::from_str::<SiteValue>(raw.get()).map_err(|_| Refusal::new("SiteValue", "site_form"))
}

/// How far a run has come: `Started`, the row of rev 1, written when `start_run` was answered `ok`; or `Ended`,
/// the row of rev 2, written with the body `finish` (contracts.md L1.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Started,
    Ended {
        /// `verified`, or `unverified` with its cause.
        verdict: Verdict,
        /// The compile number that the run loaded, or none.
        base: Option<NonZeroU32>,
        /// The number of distinct steps with a `recovered` log line.
        assisted: u32,
        /// The number of edits stored from the run.
        kept: u32,
        /// The step number of the first counted failure, or none.
        failed_step: Option<NonZeroU32>,
    },
}

impl RunStatus {
    /// The name under the key `state`: `started`, `verified` or `unverified`.
    pub fn name(&self) -> &'static str {
        match self {
            RunStatus::Started => RUN_STATES[0],
            RunStatus::Ended {
                verdict: Verdict::Verified,
                ..
            } => RUN_STATES[1],
            RunStatus::Ended {
                verdict: Verdict::Unverified(_),
                ..
            } => RUN_STATES[2],
        }
    }
}

/// The verdict of an ended run (contracts.md L3.4): verified, or unverified with the cause under the key `cause`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Verified,
    Unverified(RunCause),
}

/// A `learn_run` row: one state of one agent run as the learning handlers saw it (contracts.md L1.2). A row of
/// rev 1 holds the state `started`, the cause null, the base null, the counts 0 and the failed step null; a row
/// of rev 2 holds `verified` with the cause null or `unverified` with a cause. The parser refuses every other
/// row and reads the keys in any order; the writer writes them in the order of the contract.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LearnRun {
    pub workflow: WorkflowId,
    pub agent: AgentId,
    pub run: RunId,
    pub day: Day,
    pub mode: Mode,
    pub status: RunStatus,
}

const RUN_KEYS: [&str; 13] = [
    "v",
    "workflow",
    "agent",
    "run",
    "rev",
    "day",
    "mode",
    "state",
    "cause",
    "base",
    "assisted",
    "kept",
    "failed_step",
];

impl LearnRun {
    /// 1 for a started run, 2 for an ended one.
    pub fn rev(&self) -> u32 {
        match self.status {
            RunStatus::Started => 1,
            RunStatus::Ended { .. } => 2,
        }
    }

    fn from_raw(raw: &RawValue) -> Result<LearnRun, Refusal> {
        let obj = json::object("LearnRun", raw)?;
        obj.exactly(&RUN_KEYS)?;
        obj.read("v", json::version("LearnRun"))?;
        let workflow = obj.read("workflow", workflow_id)?;
        let agent = obj.read("agent", agent_id)?;
        let run = obj.read("run", run_id)?;
        let rev = obj.read("rev", json::number)?;
        let day = obj.read("day", Day::from_raw)?;
        let mode = obj.read("mode", mode)?;
        let state = obj.read("state", |raw| state_name("RunState", &RUN_STATES, raw))?;
        let cause = obj.read("cause", opt(RunCause::from_raw))?;
        let base = obj.read("base", opt(json::number))?;
        let assisted = obj.read("assisted", json::count)?;
        let kept = obj.read("kept", json::count)?;
        let failed_step = obj.read("failed_step", opt(json::number))?;
        let refuse = |key: &'static str, rule: &'static str| Refusal::new("LearnRun", rule).at(key);
        let status = match rev.get() {
            1 => {
                if state != RUN_STATES[0] {
                    return Err(refuse("state", "state_of_rev"));
                }
                if cause.is_some() {
                    return Err(refuse("cause", "cause_of_state"));
                }
                if base.is_some() {
                    return Err(refuse("base", "start_values"));
                }
                if assisted != 0 {
                    return Err(refuse("assisted", "start_values"));
                }
                if kept != 0 {
                    return Err(refuse("kept", "start_values"));
                }
                if failed_step.is_some() {
                    return Err(refuse("failed_step", "start_values"));
                }
                RunStatus::Started
            }
            2 => {
                let verdict = match (state, cause) {
                    (s, _) if s == RUN_STATES[0] => return Err(refuse("state", "state_of_rev")),
                    (s, None) if s == RUN_STATES[1] => Verdict::Verified,
                    (s, Some(cause)) if s == RUN_STATES[2] => Verdict::Unverified(cause),
                    _ => return Err(refuse("cause", "cause_of_state")),
                };
                RunStatus::Ended {
                    verdict,
                    base,
                    assisted,
                    kept,
                    failed_step,
                }
            }
            _ => return Err(refuse("rev", "rev")),
        };
        Ok(LearnRun {
            workflow,
            agent,
            run,
            day,
            mode,
            status,
        })
    }

    /// Reads one row and refuses every other input.
    pub fn parse(bytes: &[u8]) -> Result<LearnRun, Refusal> {
        LearnRun::from_raw(json::whole("LearnRun", bytes)?)
    }

    /// The compact JSON of the row, its keys in the order of the contract.
    pub fn to_json(&self) -> Result<String, Refusal> {
        json::write("LearnRun", self)
    }
}

impl Serialize for LearnRun {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (cause, base, assisted, kept, failed_step) = match &self.status {
            RunStatus::Started => (None, None, 0, 0, None),
            RunStatus::Ended {
                verdict,
                base,
                assisted,
                kept,
                failed_step,
            } => {
                let cause = match verdict {
                    Verdict::Verified => None,
                    Verdict::Unverified(cause) => Some(*cause),
                };
                (cause, *base, *assisted, *kept, *failed_step)
            }
        };
        let mut map = serializer.serialize_map(Some(RUN_KEYS.len()))?;
        map.serialize_entry("v", &1)?;
        map.serialize_entry("workflow", &self.workflow)?;
        map.serialize_entry("agent", &self.agent)?;
        map.serialize_entry("run", &self.run)?;
        map.serialize_entry("rev", &self.rev())?;
        map.serialize_entry("day", &self.day)?;
        map.serialize_entry("mode", &self.mode)?;
        map.serialize_entry("state", self.status.name())?;
        map.serialize_entry("cause", &cause)?;
        map.serialize_entry("base", &base)?;
        map.serialize_entry("assisted", &assisted)?;
        map.serialize_entry("kept", &kept)?;
        map.serialize_entry("failed_step", &failed_step)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for LearnRun {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        json::deserialize("LearnRun", deserializer, LearnRun::from_raw)
    }
}

/// The state of an edit as its row holds it (contracts.md L1.3): a `reverted` row carries the counted cause of
/// the revert, and the other three carry none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditState {
    Provisional,
    Active,
    Reverted(EditCause),
    Withdrawn,
}

impl EditState {
    /// The name under the key `state`.
    pub fn name(&self) -> &'static str {
        match self {
            EditState::Provisional => EDIT_STATES[0],
            EditState::Active => EDIT_STATES[1],
            EditState::Reverted(_) => EDIT_STATES[2],
            EditState::Withdrawn => EDIT_STATES[3],
        }
    }

    /// The cause under the key `cause`: the counted cause of a `reverted` row, else none.
    pub fn cause(&self) -> Option<EditCause> {
        match self {
            EditState::Reverted(cause) => Some(*cause),
            _ => None,
        }
    }
}

/// The two keys of a step's target that an edit may change, and no other (contracts.md L1.3): the description
/// and the learned locators, 1 to `LOCATORS_BOUND`, none twice, in the order in which the executor tries them.
/// Its parser refuses any other key, so a learned target holds no verb, arg, condition, effect, class, bound
/// field, frame, origin or credential mark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LearnedTarget {
    pub desc: OwnText,
    pub locators: Vec<Locator>,
}

const TARGET_KEYS: [&str; 2] = ["desc", "locators"];

impl LearnedTarget {
    /// Checks the bounds of the locator list, that no locator stands twice and the rule of each locator.
    pub fn rules(&self) -> Result<(), Refusal> {
        let refuse = |rule: &'static str| Refusal::new("LearnedTarget", rule).at("locators");
        if self.locators.is_empty() || self.locators.len() > LOCATORS_BOUND {
            return Err(refuse("locators_bound"));
        }
        for (i, locator) in self.locators.iter().enumerate() {
            if self.locators[..i].contains(locator) {
                return Err(refuse("repeated_locator"));
            }
        }
        for locator in &self.locators {
            locator.rules().map_err(json::agent_setup)?;
        }
        let written = serde_json::to_vec(&TargetForm {
            desc: &self.desc,
            locators: &self.locators,
        })
        .map_or(usize::MAX, |bytes| bytes.len());
        if written > TARGET_BYTES {
            return Err(Refusal::new("LearnedTarget", "target_bytes"));
        }
        Ok(())
    }

    fn from_raw(raw: &RawValue) -> Result<LearnedTarget, Refusal> {
        let obj = json::object("LearnedTarget", raw)?;
        obj.exactly(&TARGET_KEYS)?;
        let target = LearnedTarget {
            desc: obj.read("desc", own_text)?,
            locators: obj.read("locators", list(locator))?,
        };
        target.rules()?;
        Ok(target)
    }

    /// Reads one target and refuses every other input.
    pub fn parse(bytes: &[u8]) -> Result<LearnedTarget, Refusal> {
        LearnedTarget::from_raw(json::whole("LearnedTarget", bytes)?)
    }

    /// The compact JSON of the target, or the refusal of a rule it breaks.
    pub fn to_json(&self) -> Result<String, Refusal> {
        self.rules()?;
        json::write("LearnedTarget", self)
    }
}

/// The form that the writer of a target writes, which `rules` measures.
struct TargetForm<'a> {
    desc: &'a OwnText,
    locators: &'a [Locator],
}

impl Serialize for TargetForm<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(TARGET_KEYS.len()))?;
        map.serialize_entry("desc", self.desc)?;
        map.serialize_entry("locators", self.locators)?;
        map.end()
    }
}

impl Serialize for LearnedTarget {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.rules()
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        let mut map = serializer.serialize_map(Some(TARGET_KEYS.len()))?;
        map.serialize_entry("desc", &self.desc)?;
        map.serialize_entry("locators", &self.locators)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for LearnedTarget {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        json::deserialize("LearnedTarget", deserializer, LearnedTarget::from_raw)
    }
}

/// A `learn_edit` row: one revision of one learned target of one step (contracts.md L1.3). `runs` is the source
/// set, 1 to `RUNS_BOUND` run ids, none twice; `first` is at most `last`; `clean` is at most `CLEAN_BOUND`. The
/// parser refuses every other row and reads the keys in any order; the writer writes them in the order of the
/// contract and refuses a row that breaks a rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LearnEdit {
    pub workflow: WorkflowId,
    pub agent: AgentId,
    pub edit: EditId,
    pub rev: NonZeroU32,
    pub base: NonZeroU32,
    pub n: NonZeroU32,
    pub state: EditState,
    pub on: Option<EditId>,
    pub runs: Vec<RunId>,
    pub first: Day,
    pub last: Day,
    pub clean: u32,
    pub target: LearnedTarget,
}

const EDIT_KEYS: [&str; 15] = [
    "v", "workflow", "agent", "edit", "rev", "base", "n", "state", "cause", "on", "runs", "first",
    "last", "clean", "target",
];

impl LearnEdit {
    /// Checks the rules of the contract that join keys of the row: the bounds of `runs` and `clean`, no run
    /// twice, `first` at most `last`, one run in `runs` more than `clean` (the proposing run and one run for
    /// each clean use), and the rules of the target.
    pub fn rules(&self) -> Result<(), Refusal> {
        let refuse =
            |key: &'static str, rule: &'static str| Refusal::new("LearnEdit", rule).at(key);
        if self.runs.is_empty() || self.runs.len() > RUNS_BOUND {
            return Err(refuse("runs", "runs_bound"));
        }
        for (i, run) in self.runs.iter().enumerate() {
            if self.runs[..i].contains(run) {
                return Err(refuse("runs", "repeated_run"));
            }
        }
        if self.first > self.last {
            return Err(refuse("first", "first_after_last"));
        }
        if self.clean > CLEAN_BOUND {
            return Err(refuse("clean", "clean_bound"));
        }
        if self.runs.len() != self.clean as usize + 1 {
            return Err(refuse("clean", "clean_of_runs"));
        }
        self.target.rules()
    }

    fn from_raw(raw: &RawValue) -> Result<LearnEdit, Refusal> {
        let obj = json::object("LearnEdit", raw)?;
        obj.exactly(&EDIT_KEYS)?;
        obj.read("v", json::version("LearnEdit"))?;
        let workflow = obj.read("workflow", workflow_id)?;
        let agent = obj.read("agent", agent_id)?;
        let edit = obj.read("edit", EditId::from_raw)?;
        let rev = obj.read("rev", json::number)?;
        let base = obj.read("base", json::number)?;
        let n = obj.read("n", json::number)?;
        let state = obj.read("state", |raw| state_name("EditState", &EDIT_STATES, raw))?;
        let cause = obj.read("cause", opt(EditCause::from_raw))?;
        let state = match (state, cause) {
            (s, None) if s == EDIT_STATES[0] => EditState::Provisional,
            (s, None) if s == EDIT_STATES[1] => EditState::Active,
            (s, Some(cause)) if s == EDIT_STATES[2] => EditState::Reverted(cause),
            (s, None) if s == EDIT_STATES[3] => EditState::Withdrawn,
            _ => return Err(Refusal::new("LearnEdit", "cause_of_state").at("cause")),
        };
        let edit = LearnEdit {
            workflow,
            agent,
            edit,
            rev,
            base,
            n,
            state,
            on: obj.read("on", opt(EditId::from_raw))?,
            runs: obj.read("runs", list(run_id))?,
            first: obj.read("first", Day::from_raw)?,
            last: obj.read("last", Day::from_raw)?,
            clean: obj.read("clean", json::count)?,
            target: obj.read("target", LearnedTarget::from_raw)?,
        };
        edit.rules()?;
        Ok(edit)
    }

    /// Reads one row and refuses every other input.
    pub fn parse(bytes: &[u8]) -> Result<LearnEdit, Refusal> {
        LearnEdit::from_raw(json::whole("LearnEdit", bytes)?)
    }

    /// The compact JSON of the row, its keys in the order of the contract, or the refusal of a rule it breaks.
    pub fn to_json(&self) -> Result<String, Refusal> {
        self.rules()?;
        json::write("LearnEdit", self)
    }
}

impl Serialize for LearnEdit {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.rules()
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        let mut map = serializer.serialize_map(Some(EDIT_KEYS.len()))?;
        map.serialize_entry("v", &1)?;
        map.serialize_entry("workflow", &self.workflow)?;
        map.serialize_entry("agent", &self.agent)?;
        map.serialize_entry("edit", &self.edit)?;
        map.serialize_entry("rev", &self.rev)?;
        map.serialize_entry("base", &self.base)?;
        map.serialize_entry("n", &self.n)?;
        map.serialize_entry("state", self.state.name())?;
        map.serialize_entry("cause", &self.state.cause())?;
        map.serialize_entry("on", &self.on)?;
        map.serialize_entry("runs", &self.runs)?;
        map.serialize_entry("first", &self.first)?;
        map.serialize_entry("last", &self.last)?;
        map.serialize_entry("clean", &self.clean)?;
        map.serialize_entry("target", &self.target)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for LearnEdit {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        json::deserialize("LearnEdit", deserializer, LearnEdit::from_raw)
    }
}

/// A `learn_routine` row: one state of the routine of one workflow (contracts.md L1.4, L6). `first` and `last`
/// are set together, `first` at most `last`, and both are set when `seen` is above 0; `next` is `last` +
/// `period` when both are set and else null; `missed` is 0 while `next` is null or `day` is at or before `last`;
/// the mark `stale` has a `since` and a `missed` above 0.
/// The parser refuses every other row and reads the keys in any order; the writer writes them in the order of
/// the contract and refuses a row that breaks a rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LearnRoutine {
    pub workflow: WorkflowId,
    pub agent: AgentId,
    pub rev: NonZeroU32,
    pub day: Day,
    pub category: SeedCategory,
    pub site: Option<SiteValue>,
    pub app: AppId,
    pub period: Option<NonZeroU32>,
    pub seen: u32,
    pub first: Option<Day>,
    pub last: Option<Day>,
    pub next: Option<Day>,
    pub missed: u32,
    pub mark: Mark,
    pub since: Option<Day>,
}

const ROUTINE_KEYS: [&str; 16] = [
    "v", "workflow", "agent", "rev", "day", "category", "site", "app", "period", "seen", "first",
    "last", "next", "missed", "mark", "since",
];

impl LearnRoutine {
    /// Checks the rules of the contract that join keys of the row.
    pub fn rules(&self) -> Result<(), Refusal> {
        let refuse =
            |key: &'static str, rule: &'static str| Refusal::new("LearnRoutine", rule).at(key);
        match (self.first, self.last) {
            (Some(first), Some(last)) if first > last => {
                return Err(refuse("first", "first_after_last"))
            }
            (Some(_), None) | (None, Some(_)) => return Err(refuse("first", "first_and_last")),
            _ => {}
        }
        if self.seen > 0 && self.last.is_none() {
            return Err(refuse("seen", "seen_without_days"));
        }
        let next = match (self.last, self.period) {
            (Some(last), Some(period)) => Some(
                last.get()
                    .checked_add(period.get())
                    .and_then(Day::new)
                    .ok_or(refuse("next", "next_of_last_and_period"))?,
            ),
            _ => None,
        };
        if self.next != next {
            return Err(refuse("next", "next_of_last_and_period"));
        }
        if self.next.is_none() && self.missed > 0 {
            return Err(refuse("missed", "missed_without_next"));
        }
        if self.missed > 0 && self.last.is_some_and(|last| self.day <= last) {
            return Err(refuse("missed", "missed_on_or_before_last"));
        }
        if self.mark == Mark::Stale && self.since.is_none() {
            return Err(refuse("since", "stale_without_since"));
        }
        if self.mark == Mark::Stale && self.missed == 0 {
            return Err(refuse("mark", "stale_without_missed"));
        }
        Ok(())
    }

    fn from_raw(raw: &RawValue) -> Result<LearnRoutine, Refusal> {
        let obj = json::object("LearnRoutine", raw)?;
        obj.exactly(&ROUTINE_KEYS)?;
        obj.read("v", json::version("LearnRoutine"))?;
        let routine = LearnRoutine {
            workflow: obj.read("workflow", workflow_id)?,
            agent: obj.read("agent", agent_id)?,
            rev: obj.read("rev", json::number)?,
            day: obj.read("day", Day::from_raw)?,
            category: obj.read("category", seed_category)?,
            site: obj.read("site", opt(site_value))?,
            app: obj.read("app", app_id)?,
            period: obj.read("period", opt(json::number))?,
            seen: obj.read("seen", json::count)?,
            first: obj.read("first", opt(Day::from_raw))?,
            last: obj.read("last", opt(Day::from_raw))?,
            next: obj.read("next", opt(Day::from_raw))?,
            missed: obj.read("missed", json::count)?,
            mark: obj.read("mark", Mark::from_raw)?,
            since: obj.read("since", opt(Day::from_raw))?,
        };
        routine.rules()?;
        Ok(routine)
    }

    /// Reads one row and refuses every other input.
    pub fn parse(bytes: &[u8]) -> Result<LearnRoutine, Refusal> {
        LearnRoutine::from_raw(json::whole("LearnRoutine", bytes)?)
    }

    /// The compact JSON of the row, its keys in the order of the contract, or the refusal of a rule it breaks.
    pub fn to_json(&self) -> Result<String, Refusal> {
        self.rules()?;
        json::write("LearnRoutine", self)
    }
}

impl Serialize for LearnRoutine {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.rules()
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        let mut map = serializer.serialize_map(Some(ROUTINE_KEYS.len()))?;
        map.serialize_entry("v", &1)?;
        map.serialize_entry("workflow", &self.workflow)?;
        map.serialize_entry("agent", &self.agent)?;
        map.serialize_entry("rev", &self.rev)?;
        map.serialize_entry("day", &self.day)?;
        map.serialize_entry("category", &self.category)?;
        map.serialize_entry("site", &self.site)?;
        map.serialize_entry("app", &self.app)?;
        map.serialize_entry("period", &self.period)?;
        map.serialize_entry("seen", &self.seen)?;
        map.serialize_entry("first", &self.first)?;
        map.serialize_entry("last", &self.last)?;
        map.serialize_entry("next", &self.next)?;
        map.serialize_entry("missed", &self.missed)?;
        map.serialize_entry("mark", &self.mark)?;
        map.serialize_entry("since", &self.since)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for LearnRoutine {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        json::deserialize("LearnRoutine", deserializer, LearnRoutine::from_raw)
    }
}
