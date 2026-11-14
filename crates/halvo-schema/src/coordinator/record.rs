use std::fmt;
use std::num::NonZeroU32;

use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::executable::{ApprovalId, Digest, EffectId, RunId, WebOrigin};
use crate::ids::{AgentId, WorkflowId};
use crate::record::Instant;

use super::json::{self, opt, Obj, Refusal};
use super::names::{
    BindResult, CheckAnswer, ConfirmLapse, DoneAnswer, EffectAnswer, Event, HoldCause,
    HoldSetCause, ItemKind, OtherKind, OtherLapse, ReadyAnswer, RunEnd, SetupAnswer, Trigger,
};

/// The id of an item: 32 lower-case hex digits from 16 random bytes (contracts.md I0.5).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ItemId(String);

impl ItemId {
    /// Returns the id when `s` is 32 lower-case hex digits, else the refusal with the rule `hex32`.
    pub fn parse(s: &str) -> Result<ItemId, Refusal> {
        let hex = s.len() == 32
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !hex {
            return Err(Refusal::new("ItemId", "hex32"));
        }
        Ok(ItemId(s.to_owned()))
    }

    /// Writes the 16 bytes as 32 lower-case hex digits, in order.
    pub fn from_random(bytes: [u8; 16]) -> ItemId {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(32);
        for b in bytes {
            out.push(char::from(DIGITS[usize::from(b >> 4)]));
            out.push(char::from(DIGITS[usize::from(b & 0x0f)]));
        }
        ItemId(out)
    }

    /// The 32 digits.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn from_raw(raw: &RawValue) -> Result<ItemId, Refusal> {
        ItemId::parse(&json::string("ItemId", raw)?)
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for ItemId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

/// One entry of a confirmation's decision: `effect`, `approval`, `digest` and `answer` (the agent-setup run's
/// A6.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub effect: EffectId,
    pub approval: ApprovalId,
    pub digest: Digest,
    pub answer: EffectAnswer,
}

/// The `kind` and the `run` of `item_raised`: a kind that a run's request raises names the run, `check` names
/// the run of a handback or none for the hold `check`, and `setup` and `ready` name none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Raised {
    Confirm(RunId),
    SignIn(RunId),
    LinkAccount(RunId),
    Stuck(RunId),
    Check(Option<RunId>),
    Setup,
    Ready,
    Show(RunId),
}

impl Raised {
    /// The row's `kind`.
    pub fn kind(&self) -> ItemKind {
        match self {
            Raised::Confirm(_) => ItemKind::Confirm,
            Raised::SignIn(_) => ItemKind::SignIn,
            Raised::LinkAccount(_) => ItemKind::LinkAccount,
            Raised::Stuck(_) => ItemKind::Stuck,
            Raised::Check(_) => ItemKind::Check,
            Raised::Setup => ItemKind::Setup,
            Raised::Ready => ItemKind::Ready,
            Raised::Show(_) => ItemKind::Show,
        }
    }

    /// The row's `run`, or none.
    pub fn run(&self) -> Option<&RunId> {
        match self {
            Raised::Confirm(r)
            | Raised::SignIn(r)
            | Raised::LinkAccount(r)
            | Raised::Stuck(r)
            | Raised::Show(r) => Some(r),
            Raised::Check(r) => r.as_ref(),
            Raised::Setup | Raised::Ready => None,
        }
    }
}

/// The answer of the second step of a `sign_in` item with the whole origin whose site's name the step showed
/// (contracts.md I7.1, ruling IG-19).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindAnswer {
    pub answer: BindResult,
    pub origin: WebOrigin,
}

/// The answer of a `sign_in` item: of its first step, with no origin, or of its second, with the origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignInAnswer {
    First(DoneAnswer),
    Second(BindAnswer),
}

/// The answer of a `link_account` item with the request's origin (contracts.md I7.1, ruling IG-19).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkAnswer {
    pub answer: DoneAnswer,
    pub origin: WebOrigin,
}

/// The `kind` and the `answer` of `item_answered`: each kind with one of its own choices, a confirmation with the
/// entry of one effect, and `setup` with the choice `choose` alone, because the person's `none` stores no row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answered {
    Confirm(Entry),
    SignIn(SignInAnswer),
    LinkAccount(LinkAnswer),
    Stuck(DoneAnswer),
    Check(CheckAnswer),
    Setup,
    Ready(ReadyAnswer),
    Show(DoneAnswer),
}

impl Answered {
    /// The row's `kind`.
    pub fn kind(&self) -> ItemKind {
        match self {
            Answered::Confirm(_) => ItemKind::Confirm,
            Answered::SignIn(_) => ItemKind::SignIn,
            Answered::LinkAccount(_) => ItemKind::LinkAccount,
            Answered::Stuck(_) => ItemKind::Stuck,
            Answered::Check(_) => ItemKind::Check,
            Answered::Setup => ItemKind::Setup,
            Answered::Ready(_) => ItemKind::Ready,
            Answered::Show(_) => ItemKind::Show,
        }
    }
}

/// The `kind` and the `cause` of `item_lapsed`: a confirmation with one of the six causes of I4.2, an item of
/// another kind with a cause of `OtherLapse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lapsed {
    Confirm(ConfirmLapse),
    Other { kind: OtherKind, cause: OtherLapse },
}

impl Lapsed {
    /// Refuses the cause `check` for a kind other than `ready`, with the rule `check_not_ready`: `check` is the
    /// lapse of an open `ready` item that the item `check` replaces (contracts.md I7.1, ruling RD-5).
    pub fn rules(&self) -> Result<(), Refusal> {
        match self {
            Lapsed::Other {
                kind,
                cause: OtherLapse::Check,
            } if *kind != OtherKind::Ready => {
                Err(Refusal::new("CoordEvent", "check_not_ready").at("cause"))
            }
            _ => Ok(()),
        }
    }
}

/// The `cause` of `hold_cleared`: the hold that ended, with the `run` and the `answer` of the check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cleared {
    Person,
    Failures,
    Check { run: RunId, answer: CheckAnswer },
}

/// An event of `coord_event` with the keys that it sets; every other key of the row is null.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum What {
    RunStarted {
        run: RunId,
        trigger: Trigger,
        slot: Option<u32>,
    },
    RunEnded {
        run: RunId,
        cause: RunEnd,
    },
    ItemRaised {
        item: ItemId,
        raised: Raised,
    },
    ItemAnswered {
        item: ItemId,
        answered: Answered,
    },
    ItemLapsed {
        item: ItemId,
        lapsed: Lapsed,
    },
    ItemReleased {
        item: ItemId,
        entry: Entry,
    },
    HoldSet {
        cause: HoldSetCause,
    },
    HoldCleared {
        cleared: Cleared,
    },
    SlotSkipped {
        slot: Option<u32>,
    },
}

impl What {
    /// The row's `event`.
    pub fn event(&self) -> Event {
        match self {
            What::RunStarted { .. } => Event::RunStarted,
            What::RunEnded { .. } => Event::RunEnded,
            What::ItemRaised { .. } => Event::ItemRaised,
            What::ItemAnswered { .. } => Event::ItemAnswered,
            What::ItemLapsed { .. } => Event::ItemLapsed,
            What::ItemReleased { .. } => Event::ItemReleased,
            What::HoldSet { .. } => Event::HoldSet,
            What::HoldCleared { .. } => Event::HoldCleared,
            What::SlotSkipped { .. } => Event::SlotSkipped,
        }
    }
}

/// A record of the kind `coord_event` (contracts.md I7.1): `{"v":1,"workflow":..,"agent":..,"seq":..,"day":..,
/// "at":..,"event":..,"run":..,"slot":..,"item":..,"kind":..,"effect":..,"approval":..,"digest":..,"answer":..,
/// "cause":..,"trigger":..,"origin":..}`, every key always written, those that the event does not set as null.
/// `seq` rises by 1 per agent from 1; `at` is nanoseconds since 1970-01-01T00:00:00Z. This reader is the guard of
/// what a row holds: the registry's `check_record` takes free text under `origin` and free names for this kind, so
/// a `coord_event` is committed only after `CoordEvent::parse` or `CoordLog::parse` has read it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoordEvent {
    pub workflow: WorkflowId,
    pub agent: AgentId,
    pub seq: NonZeroU32,
    pub day: u32,
    pub at: Instant,
    pub what: What,
}

const KEYS: [&str; 18] = [
    "v", "workflow", "agent", "seq", "day", "at", "event", "run", "slot", "item", "kind", "effect",
    "approval", "digest", "answer", "cause", "trigger", "origin",
];

const OWN: [&str; 11] = [
    "run", "slot", "item", "kind", "effect", "approval", "digest", "answer", "cause", "trigger",
    "origin",
];

const ENTRY: [&str; 6] = ["item", "kind", "effect", "approval", "digest", "answer"];

fn run_id(raw: &RawValue) -> Result<RunId, Refusal> {
    RunId::parse(&json::string("RunId", raw)?).map_err(json::agent_setup)
}

fn effect_id(raw: &RawValue) -> Result<EffectId, Refusal> {
    EffectId::parse(&json::string("EffectId", raw)?).map_err(json::agent_setup)
}

fn approval_id(raw: &RawValue) -> Result<ApprovalId, Refusal> {
    ApprovalId::parse(&json::string("ApprovalId", raw)?).map_err(json::agent_setup)
}

fn digest(raw: &RawValue) -> Result<Digest, Refusal> {
    Digest::parse(&json::string("Digest", raw)?).map_err(json::agent_setup)
}

fn web_origin(raw: &RawValue) -> Result<WebOrigin, Refusal> {
    WebOrigin::parse(&json::string("WebOrigin", raw)?).map_err(json::agent_setup)
}

pub(in crate::coordinator) fn workflow_id(raw: &RawValue) -> Result<WorkflowId, Refusal> {
    WorkflowId::parse(&json::string("WorkflowId", raw)?).map_err(json::core)
}

pub(in crate::coordinator) fn agent_id(raw: &RawValue) -> Result<AgentId, Refusal> {
    AgentId::parse(&json::string("AgentId", raw)?).map_err(json::core)
}

fn entry(obj: &Obj<'_>) -> Result<Entry, Refusal> {
    Ok(Entry {
        effect: obj.read("effect", effect_id)?,
        approval: obj.read("approval", approval_id)?,
        digest: obj.read("digest", digest)?,
        answer: obj.read("answer", EffectAnswer::from_raw)?,
    })
}

fn raised(obj: &Obj<'_>, kind: ItemKind) -> Result<Raised, Refusal> {
    let with_run = ["item", "kind", "run"];
    if matches!(kind, ItemKind::Setup | ItemKind::Ready) {
        obj.null_but(&OWN, &["item", "kind"])?;
    } else {
        obj.null_but(&OWN, &with_run)?;
    }
    Ok(match kind {
        ItemKind::Confirm => Raised::Confirm(obj.read("run", run_id)?),
        ItemKind::SignIn => Raised::SignIn(obj.read("run", run_id)?),
        ItemKind::LinkAccount => Raised::LinkAccount(obj.read("run", run_id)?),
        ItemKind::Stuck => Raised::Stuck(obj.read("run", run_id)?),
        ItemKind::Check => Raised::Check(obj.read("run", opt(run_id))?),
        ItemKind::Setup => Raised::Setup,
        ItemKind::Ready => Raised::Ready,
        ItemKind::Show => Raised::Show(obj.read("run", run_id)?),
    })
}

fn answered(obj: &Obj<'_>, kind: ItemKind) -> Result<Answered, Refusal> {
    let plain = ["item", "kind", "answer"];
    let with_origin = ["item", "kind", "answer", "origin"];
    Ok(match kind {
        ItemKind::Confirm => {
            obj.null_but(&OWN, &ENTRY)?;
            Answered::Confirm(entry(obj)?)
        }
        ItemKind::SignIn => {
            obj.null_but(&OWN, &with_origin)?;
            match obj.read("origin", opt(web_origin))? {
                None => match obj.read("answer", DoneAnswer::from_raw) {
                    Ok(answer) => Answered::SignIn(SignInAnswer::First(answer)),
                    Err(_) if obj.read("answer", BindResult::from_raw).is_ok() => {
                        return Err(Refusal::new("CoordEvent", "no_origin").at("origin"))
                    }
                    Err(refused) => return Err(refused),
                },
                Some(origin) => match obj.read("answer", BindResult::from_raw) {
                    Ok(answer) => {
                        Answered::SignIn(SignInAnswer::Second(BindAnswer { answer, origin }))
                    }
                    Err(_) if obj.read("answer", DoneAnswer::from_raw).is_ok() => {
                        return Err(Refusal::new("CoordEvent", "not_null").at("origin"))
                    }
                    Err(refused) => return Err(refused),
                },
            }
        }
        ItemKind::LinkAccount => {
            obj.null_but(&OWN, &with_origin)?;
            Answered::LinkAccount(LinkAnswer {
                answer: obj.read("answer", DoneAnswer::from_raw)?,
                origin: obj.read("origin", web_origin)?,
            })
        }
        ItemKind::Stuck => {
            obj.null_but(&OWN, &plain)?;
            Answered::Stuck(obj.read("answer", DoneAnswer::from_raw)?)
        }
        ItemKind::Check => {
            obj.null_but(&OWN, &plain)?;
            Answered::Check(obj.read("answer", CheckAnswer::from_raw)?)
        }
        ItemKind::Setup => {
            obj.null_but(&OWN, &plain)?;
            obj.read("answer", SetupAnswer::from_raw)?;
            Answered::Setup
        }
        ItemKind::Ready => {
            obj.null_but(&OWN, &plain)?;
            Answered::Ready(obj.read("answer", ReadyAnswer::from_raw)?)
        }
        ItemKind::Show => {
            obj.null_but(&OWN, &plain)?;
            Answered::Show(obj.read("answer", DoneAnswer::from_raw)?)
        }
    })
}

fn lapsed(obj: &Obj<'_>, kind: ItemKind) -> Result<Lapsed, Refusal> {
    obj.null_but(&OWN, &["item", "kind", "cause"])?;
    Ok(if kind == ItemKind::Confirm {
        Lapsed::Confirm(obj.read("cause", ConfirmLapse::from_raw)?)
    } else {
        let lapsed = Lapsed::Other {
            kind: obj.read("kind", OtherKind::from_raw)?,
            cause: obj.read("cause", OtherLapse::from_raw)?,
        };
        lapsed.rules()?;
        lapsed
    })
}

fn start_rules(trigger: Trigger, slot: Option<u32>) -> Result<(), Refusal> {
    match (trigger, slot) {
        (Trigger::Schedule, None) => Err(Refusal::new("CoordEvent", "no_slot").at("slot")),
        _ => Ok(()),
    }
}

fn what(obj: &Obj<'_>, event: Event) -> Result<What, Refusal> {
    Ok(match event {
        Event::RunStarted => {
            obj.null_but(&OWN, &["run", "slot", "trigger"])?;
            let run = obj.read("run", run_id)?;
            let trigger = obj.read("trigger", Trigger::from_raw)?;
            let slot = obj.read("slot", opt(json::count))?;
            start_rules(trigger, slot)?;
            What::RunStarted { run, trigger, slot }
        }
        Event::RunEnded => {
            obj.null_but(&OWN, &["run", "cause"])?;
            What::RunEnded {
                run: obj.read("run", run_id)?,
                cause: obj.read("cause", RunEnd::from_raw)?,
            }
        }
        Event::ItemRaised => {
            let item = obj.read("item", ItemId::from_raw)?;
            let kind = obj.read("kind", ItemKind::from_raw)?;
            What::ItemRaised {
                item,
                raised: raised(obj, kind)?,
            }
        }
        Event::ItemAnswered => {
            let item = obj.read("item", ItemId::from_raw)?;
            let kind = obj.read("kind", ItemKind::from_raw)?;
            What::ItemAnswered {
                item,
                answered: answered(obj, kind)?,
            }
        }
        Event::ItemLapsed => {
            let item = obj.read("item", ItemId::from_raw)?;
            let kind = obj.read("kind", ItemKind::from_raw)?;
            What::ItemLapsed {
                item,
                lapsed: lapsed(obj, kind)?,
            }
        }
        Event::ItemReleased => {
            obj.null_but(&OWN, &ENTRY)?;
            let item = obj.read("item", ItemId::from_raw)?;
            if obj.read("kind", ItemKind::from_raw)? != ItemKind::Confirm {
                return Err(Refusal::new("CoordEvent", "not_confirm").at("kind"));
            }
            What::ItemReleased {
                item,
                entry: entry(obj)?,
            }
        }
        Event::HoldSet => {
            obj.null_but(&OWN, &["cause"])?;
            What::HoldSet {
                cause: obj.read("cause", HoldSetCause::from_raw)?,
            }
        }
        Event::HoldCleared => {
            let cleared = match obj.read("cause", HoldCause::from_raw)? {
                HoldCause::Person => {
                    obj.null_but(&OWN, &["cause"])?;
                    Cleared::Person
                }
                HoldCause::Failures => {
                    obj.null_but(&OWN, &["cause"])?;
                    Cleared::Failures
                }
                HoldCause::Check => {
                    obj.null_but(&OWN, &["cause", "run", "answer"])?;
                    Cleared::Check {
                        run: obj.read("run", run_id)?,
                        answer: obj.read("answer", CheckAnswer::from_raw)?,
                    }
                }
            };
            What::HoldCleared { cleared }
        }
        Event::SlotSkipped => {
            obj.null_but(&OWN, &["slot"])?;
            What::SlotSkipped {
                slot: obj.read("slot", opt(json::count))?,
            }
        }
    })
}

impl CoordEvent {
    pub(in crate::coordinator) fn from_raw(raw: &RawValue) -> Result<CoordEvent, Refusal> {
        let obj = json::object("CoordEvent", raw)?;
        obj.exactly(&KEYS)?;
        obj.read("v", json::version("CoordEvent"))?;
        let workflow = obj.read("workflow", workflow_id)?;
        let agent = obj.read("agent", agent_id)?;
        let seq = obj.read("seq", json::number)?;
        let day = obj.read("day", json::count)?;
        let at = obj.read("at", json::instant)?;
        let event = obj.read("event", Event::from_raw)?;
        Ok(CoordEvent {
            workflow,
            agent,
            seq,
            day,
            at,
            what: what(&obj, event)?,
        })
    }

    /// Reads one record and refuses every other input.
    pub fn parse(bytes: &[u8]) -> Result<CoordEvent, Refusal> {
        CoordEvent::from_raw(json::whole("CoordEvent", bytes)?)
    }

    /// The compact JSON of the record, its keys in the order of the kind entry.
    pub fn to_json(&self) -> Result<String, Refusal> {
        self.rules()?;
        json::write("CoordEvent", self)
    }

    /// Checks the two rules that the types leave to a value: the cause `check` lapses a `ready` item alone, and a
    /// `run_started` of the trigger `schedule` names its slot, else the rule `no_slot` at `slot` (contracts.md
    /// I5.3, I7.1).
    pub fn rules(&self) -> Result<(), Refusal> {
        match &self.what {
            What::ItemLapsed { lapsed, .. } => lapsed.rules(),
            What::RunStarted { trigger, slot, .. } => start_rules(*trigger, *slot),
            _ => Ok(()),
        }
    }

    fn cells(&self) -> Vec<(&'static str, Cell<'_>)> {
        let text = |key: &'static str, value: &'static str| (key, Cell::Text(value));
        match &self.what {
            What::RunStarted { run, trigger, slot } => vec![
                ("run", Cell::Text(run.as_str())),
                ("slot", slot.map_or(Cell::Null, Cell::Count)),
                text("trigger", trigger.as_str()),
            ],
            What::RunEnded { run, cause } => vec![
                ("run", Cell::Text(run.as_str())),
                text("cause", cause.as_str()),
            ],
            What::ItemRaised { item, raised } => {
                let mut cells = vec![
                    ("item", Cell::Text(item.as_str())),
                    text("kind", raised.kind().as_str()),
                ];
                if let Some(run) = raised.run() {
                    cells.push(("run", Cell::Text(run.as_str())));
                }
                cells
            }
            What::ItemAnswered { item, answered } => match plain_answer(answered) {
                Err(entry) => entry_cells(item, entry),
                Ok((answer, origin)) => {
                    let mut cells = vec![
                        ("item", Cell::Text(item.as_str())),
                        text("kind", answered.kind().as_str()),
                        text("answer", answer),
                    ];
                    if let Some(origin) = origin {
                        cells.push(("origin", Cell::Text(origin.as_str())));
                    }
                    cells
                }
            },
            What::ItemLapsed { item, lapsed } => {
                let (kind, cause) = match lapsed {
                    Lapsed::Confirm(cause) => (ItemKind::Confirm, cause.as_str()),
                    Lapsed::Other { kind, cause } => (kind.kind(), cause.as_str()),
                };
                vec![
                    ("item", Cell::Text(item.as_str())),
                    text("kind", kind.as_str()),
                    text("cause", cause),
                ]
            }
            What::ItemReleased { item, entry } => entry_cells(item, entry),
            What::HoldSet { cause } => vec![text("cause", cause.as_str())],
            What::HoldCleared { cleared } => match cleared {
                Cleared::Person => vec![text("cause", HoldCause::Person.as_str())],
                Cleared::Failures => vec![text("cause", HoldCause::Failures.as_str())],
                Cleared::Check { run, answer } => vec![
                    ("run", Cell::Text(run.as_str())),
                    text("answer", answer.as_str()),
                    text("cause", HoldCause::Check.as_str()),
                ],
            },
            What::SlotSkipped { slot } => vec![("slot", slot.map_or(Cell::Null, Cell::Count))],
        }
    }
}

fn entry_cells<'a>(item: &'a ItemId, entry: &'a Entry) -> Vec<(&'static str, Cell<'a>)> {
    vec![
        ("item", Cell::Text(item.as_str())),
        ("kind", Cell::Text(ItemKind::Confirm.as_str())),
        ("effect", Cell::Text(entry.effect.as_str())),
        ("approval", Cell::Text(entry.approval.as_str())),
        ("digest", Cell::Text(entry.digest.as_str())),
        ("answer", Cell::Text(entry.answer.as_str())),
    ]
}

fn plain_answer(answered: &Answered) -> Result<(&'static str, Option<&WebOrigin>), &Entry> {
    Ok(match answered {
        Answered::Confirm(entry) => return Err(entry),
        Answered::SignIn(SignInAnswer::First(a)) | Answered::Stuck(a) | Answered::Show(a) => {
            (a.as_str(), None)
        }
        Answered::SignIn(SignInAnswer::Second(b)) => (b.answer.as_str(), Some(&b.origin)),
        Answered::LinkAccount(l) => (l.answer.as_str(), Some(&l.origin)),
        Answered::Check(a) => (a.as_str(), None),
        Answered::Setup => (SetupAnswer::Choose.as_str(), None),
        Answered::Ready(a) => (a.as_str(), None),
    })
}

#[derive(Clone, Copy)]
enum Cell<'a> {
    Null,
    Text(&'a str),
    Count(u32),
}

impl Serialize for Cell<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Cell::Null => serializer.serialize_unit(),
            Cell::Text(text) => serializer.serialize_str(text),
            Cell::Count(n) => serializer.serialize_u32(*n),
        }
    }
}

impl Serialize for CoordEvent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.rules()
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        let mut map = serializer.serialize_map(Some(KEYS.len()))?;
        map.serialize_entry("v", &1u32)?;
        map.serialize_entry("workflow", &self.workflow)?;
        map.serialize_entry("agent", &self.agent)?;
        map.serialize_entry("seq", &self.seq)?;
        map.serialize_entry("day", &self.day)?;
        map.serialize_entry("at", &self.at)?;
        map.serialize_entry("event", &self.what.event())?;
        let cells = self.cells();
        for key in OWN {
            let cell = cells
                .iter()
                .find(|(k, _)| *k == key)
                .map_or(Cell::Null, |(_, cell)| *cell);
            map.serialize_entry(key, &cell)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for CoordEvent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        json::deserialize("CoordEvent", deserializer, CoordEvent::from_raw)
    }
}
