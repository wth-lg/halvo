use super::names::names;
use super::{AgentId, AgentState, Choice, DecisionEntry, HoldSetCause, ItemId, ItemKind};
use super::{LapseCause, Mode, Name, Origin, RunEnd, RunId, Trigger, WorkflowId};
use std::num::NonZeroU32;

/// A record `coord_event` of I7.1: the first seven keys and the event with its own keys.
///
/// ```
/// use halvo_coord::types::{AgentId, Event, What, WorkflowId};
/// use std::num::NonZeroU32;
/// let e = Event {
///     workflow: WorkflowId::parse("00000000000000000000000000000001").unwrap(),
///     agent: AgentId::parse("00000000000000000000000000000002").unwrap(),
///     seq: NonZeroU32::MIN,
///     day: 20_760,
///     at: 0,
///     what: What::SlotSkipped { slot: None },
/// };
/// ```
///
/// An event with `seq` 0 cannot be written:
///
/// ```compile_fail,E0308
/// use halvo_coord::types::{AgentId, Event, What, WorkflowId};
/// let e = Event {
///     workflow: WorkflowId::parse("00000000000000000000000000000001").unwrap(),
///     agent: AgentId::parse("00000000000000000000000000000002").unwrap(),
///     seq: 0,
///     day: 20_760,
///     at: 0,
///     what: What::SlotSkipped { slot: None },
/// };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    /// `workflow`.
    pub workflow: WorkflowId,
    /// `agent`.
    pub agent: AgentId,
    /// `seq`: rises by 1 per agent, from 1.
    pub seq: NonZeroU32,
    /// `day`: days since 1970-01-01.
    pub day: u32,
    /// `at`: nanoseconds since 1970-01-01T00:00:00Z, `Clock::now()` times 1,000,000.
    pub at: i64,
    /// `event` and the keys that it sets.
    pub what: What,
}

/// The events of I7.1 with the keys that each sets; every other key is null.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum What {
    /// `run_started`.
    RunStarted {
        /// `run`.
        run: RunId,
        /// `trigger`.
        trigger: Trigger,
        /// `slot`: the day of the slot, or null.
        slot: Option<u32>,
    },
    /// `run_ended`.
    RunEnded {
        /// `run`.
        run: RunId,
        /// `cause`: the end of I5.6.
        cause: RunEnd,
    },
    /// `item_raised`.
    ItemRaised {
        /// `item`.
        item: ItemId,
        /// `kind`, with `run` for an item that a run raised.
        raised: Raised,
    },
    /// `item_answered`: one row per answer, and for a confirmation one row per effect.
    ItemAnswered {
        /// `item`.
        item: ItemId,
        /// `kind` and `answer`, with the effect's keys for a confirmation.
        answer: Answered,
    },
    /// `item_lapsed`.
    ItemLapsed {
        /// `item`.
        item: ItemId,
        /// `kind` and `cause`.
        lapsed: Lapsed,
    },
    /// `item_released`: one row per entry of the released decision, kind `confirm`.
    ItemReleased {
        /// `item`.
        item: ItemId,
        /// `effect`, `approval`, `digest` and `answer`.
        entry: DecisionEntry,
    },
    /// `hold_set`.
    HoldSet {
        /// `cause`: the hold that was set.
        cause: HoldSetCause,
    },
    /// `hold_cleared`.
    HoldCleared {
        /// `cause`: the hold that ended, with `run` and `answer` for `check`.
        cause: Cleared,
    },
    /// `slot_skipped`.
    SlotSkipped {
        /// `slot`: the day of the slot, or null.
        slot: Option<u32>,
    },
}

/// The kind and the answer of an `item_answered` row: each kind with one of its own choices of
/// I3.1, and a confirmation with the entry of one effect (I7.1).
///
/// ```
/// use halvo_coord::types::{Answered, CheckAnswer, ItemKind};
/// let row = Answered::Check(CheckAnswer::WentThrough);
/// assert_eq!(row.kind(), ItemKind::Check);
/// assert_eq!(row.choice().map(|c| c.name()), Some("went_through"));
/// ```
///
/// A kind with the answer of another kind cannot be written:
///
/// ```compile_fail,E0308
/// use halvo_coord::types::{Answered, CheckAnswer};
/// let row = Answered::SignIn(CheckAnswer::WentThrough);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answered {
    /// `confirm`: the entry of one effect, `effect`, `approval`, `digest` and `answer`.
    Confirm(DecisionEntry),
    /// `sign_in`: of its first step or of its second.
    SignIn(SignInAnswer),
    /// `link_account`, with the request's origin.
    LinkAccount(LinkAnswer),
    /// `stuck`.
    Stuck(DoneAnswer),
    /// `check`.
    Check(CheckAnswer),
    /// `setup`.
    Setup(SetupAnswer),
    /// `ready`.
    Ready(ReadyAnswer),
    /// `show`.
    Show(DoneAnswer),
}

impl Answered {
    /// The row's `kind`.
    pub const fn kind(&self) -> ItemKind {
        match self {
            Answered::Confirm(_) => ItemKind::Confirm,
            Answered::SignIn(_) => ItemKind::SignIn,
            Answered::LinkAccount(_) => ItemKind::LinkAccount,
            Answered::Stuck(_) => ItemKind::Stuck,
            Answered::Check(_) => ItemKind::Check,
            Answered::Setup(_) => ItemKind::Setup,
            Answered::Ready(_) => ItemKind::Ready,
            Answered::Show(_) => ItemKind::Show,
        }
    }

    /// The row's `answer` as a choice, or none for a confirmation, whose answer is the entry's.
    pub const fn choice(&self) -> Option<Choice> {
        match self {
            Answered::Confirm(_) => None,
            Answered::SignIn(SignInAnswer::First(a)) | Answered::Stuck(a) => Some(a.choice()),
            Answered::SignIn(SignInAnswer::Second(a)) => Some(a.answer.choice()),
            Answered::LinkAccount(a) => Some(a.answer.choice()),
            Answered::Show(a) => Some(a.choice()),
            Answered::Check(a) => Some(a.choice()),
            Answered::Setup(a) => Some(a.choice()),
            Answered::Ready(a) => Some(a.choice()),
        }
    }
}

names! {
    /// The two choices of `sign_in`, `link_account`, `stuck` and `show` (I3.1).
    DoneAnswer {
        Decline = "decline",
        Done = "done",
    }
}

/// The answer of a `sign_in` item (I3.1, I7.1): of its first step, `decline` or `done`, or of
/// its second, `decline` or `allow` with the key `origin`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignInAnswer {
    /// The first step.
    First(DoneAnswer),
    /// The second step.
    Second(BindAnswer),
}

names! {
    /// The two choices of the second step of a `sign_in` item, the safe one first, and the
    /// answer that `Runs::answer_bind` gives the run (I3.1, I5.10).
    BindResult {
        Decline = "decline",
        Allow = "allow",
    }
}

impl BindResult {
    /// The choice.
    pub const fn choice(self) -> Choice {
        match self {
            BindResult::Decline => Choice::Decline,
            BindResult::Allow => Choice::Allow,
        }
    }
}

/// The answer of the second step of a `sign_in` item with the key `origin`: the whole origin,
/// whose site's name the step showed (I7.1, ruling IG-14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindAnswer {
    /// `answer`: `decline` or `allow`.
    pub answer: BindResult,
    /// `origin`.
    pub origin: Origin,
}

/// The answer of a `link_account` item with the key `origin`: the request's whole origin, whose
/// site's name the item showed (I7.1, ruling IG-14).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkAnswer {
    /// `answer`: `decline` or `done`.
    pub answer: DoneAnswer,
    /// `origin`.
    pub origin: Origin,
}

names! {
    /// The two choices of `check` (I3.1).
    CheckAnswer {
        TryAgain = "try_again",
        WentThrough = "went_through",
    }
}

names! {
    /// The two choices of `setup` (I3.1).
    SetupAnswer {
        None = "none",
        Choose = "choose",
    }
}

names! {
    /// The two choices of `ready` (I3.1).
    ReadyAnswer {
        Skip = "skip",
        Start = "start",
    }
}

impl DoneAnswer {
    /// The choice.
    pub const fn choice(self) -> Choice {
        match self {
            DoneAnswer::Decline => Choice::Decline,
            DoneAnswer::Done => Choice::Done,
        }
    }
}

impl CheckAnswer {
    /// The choice.
    pub const fn choice(self) -> Choice {
        match self {
            CheckAnswer::TryAgain => Choice::TryAgain,
            CheckAnswer::WentThrough => Choice::WentThrough,
        }
    }
}

impl SetupAnswer {
    /// The choice.
    pub const fn choice(self) -> Choice {
        match self {
            SetupAnswer::None => Choice::None,
            SetupAnswer::Choose => Choice::Choose,
        }
    }
}

impl ReadyAnswer {
    /// The choice.
    pub const fn choice(self) -> Choice {
        match self {
            ReadyAnswer::Skip => Choice::Skip,
            ReadyAnswer::Start => Choice::Start,
        }
    }
}

/// The `kind` and the `cause` of an `item_lapsed` row: a confirmation with one of the six
/// causes of I4.2, an item of another kind with `run_ended`, `app_ended`, `wait_limit`,
/// `cancelled` or `check` (I7.1).
///
/// ```
/// use halvo_coord::types::{ItemLapseCause, LapseCause, Lapsed, OtherLapse};
/// let ready = Lapsed::Ready(OtherLapse::Check);
/// assert_eq!(ready.cause(), ItemLapseCause::Check);
/// let confirm = Lapsed::Confirm(LapseCause::Digest);
/// assert_eq!(confirm.cause(), ItemLapseCause::Digest);
/// ```
///
/// An item of another kind than `confirm` that lapses with `digest` cannot be written:
///
/// ```compile_fail,E0308
/// use halvo_coord::types::{LapseCause, Lapsed};
/// let ready = Lapsed::Ready(LapseCause::Digest);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lapsed {
    /// `confirm`, with one of the six causes of I4.2.
    Confirm(LapseCause),
    /// `sign_in`.
    SignIn(OtherLapse),
    /// `link_account`.
    LinkAccount(OtherLapse),
    /// `stuck`.
    Stuck(OtherLapse),
    /// `check`.
    Check(OtherLapse),
    /// `setup`.
    Setup(OtherLapse),
    /// `ready`.
    Ready(OtherLapse),
    /// `show`.
    Show(OtherLapse),
}

names! {
    /// The causes of a lapse of an item of another kind than `confirm` (I7.1).
    OtherLapse {
        RunEnded = "run_ended",
        AppEnded = "app_ended",
        WaitLimit = "wait_limit",
        Cancelled = "cancelled",
        Check = "check",
    }
}

names! {
    /// The `cause` of an `item_lapsed` row (I7.1): the six causes of I4.2 and `check`, which only
    /// an item of another kind than `confirm` takes (ruling RD-35).
    ItemLapseCause {
        Digest = "digest",
        RunEnded = "run_ended",
        WaitLimit = "wait_limit",
        Cancelled = "cancelled",
        NoRecord = "no_record",
        AppEnded = "app_ended",
        Check = "check",
    }
}

impl ItemLapseCause {
    /// The row's cause of a confirmation that lapsed with `cause`.
    pub const fn of(cause: LapseCause) -> ItemLapseCause {
        match cause {
            LapseCause::Digest => ItemLapseCause::Digest,
            LapseCause::RunEnded => ItemLapseCause::RunEnded,
            LapseCause::WaitLimit => ItemLapseCause::WaitLimit,
            LapseCause::Cancelled => ItemLapseCause::Cancelled,
            LapseCause::NoRecord => ItemLapseCause::NoRecord,
            LapseCause::AppEnded => ItemLapseCause::AppEnded,
        }
    }
}

impl OtherLapse {
    /// The row's cause.
    pub const fn cause(self) -> ItemLapseCause {
        match self {
            OtherLapse::RunEnded => ItemLapseCause::RunEnded,
            OtherLapse::AppEnded => ItemLapseCause::AppEnded,
            OtherLapse::WaitLimit => ItemLapseCause::WaitLimit,
            OtherLapse::Cancelled => ItemLapseCause::Cancelled,
            OtherLapse::Check => ItemLapseCause::Check,
        }
    }
}

impl Lapsed {
    /// The row's `kind`.
    pub const fn kind(&self) -> ItemKind {
        match self {
            Lapsed::Confirm(_) => ItemKind::Confirm,
            Lapsed::SignIn(_) => ItemKind::SignIn,
            Lapsed::LinkAccount(_) => ItemKind::LinkAccount,
            Lapsed::Stuck(_) => ItemKind::Stuck,
            Lapsed::Check(_) => ItemKind::Check,
            Lapsed::Setup(_) => ItemKind::Setup,
            Lapsed::Ready(_) => ItemKind::Ready,
            Lapsed::Show(_) => ItemKind::Show,
        }
    }

    /// The row's `cause`.
    pub const fn cause(&self) -> ItemLapseCause {
        match self {
            Lapsed::Confirm(c) => ItemLapseCause::of(*c),
            Lapsed::SignIn(o)
            | Lapsed::LinkAccount(o)
            | Lapsed::Stuck(o)
            | Lapsed::Check(o)
            | Lapsed::Setup(o)
            | Lapsed::Ready(o)
            | Lapsed::Show(o) => o.cause(),
        }
    }
}

/// The `kind` and the `run` of an `item_raised` row, by the sources of I3.1: a kind that a
/// request of a run raises names that run, `check` names the run of a handback
/// `effect_unknown` or none for the hold `check`, and `setup` and `ready` name none.
///
/// ```
/// use halvo_coord::types::{Raised, RunId};
/// let run = RunId::parse("00000000000000000000000000000004").unwrap();
/// let raised = Raised::Confirm(run.clone());
/// assert_eq!(raised.run(), Some(&run));
/// ```
///
/// A confirmation with no run cannot be written:
///
/// ```compile_fail,E0308
/// use halvo_coord::types::Raised;
/// let raised = Raised::Confirm(None);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Raised {
    /// `confirm`, from a confirm request of the run.
    Confirm(RunId),
    /// `sign_in`, from a handback request of the run.
    SignIn(RunId),
    /// `link_account`, from a handback request of the run.
    LinkAccount(RunId),
    /// `stuck`, from a handback request of the run.
    Stuck(RunId),
    /// `check`, from a handback `effect_unknown` of the run, or from the hold `check` with
    /// none.
    Check(Option<RunId>),
    /// `setup`, from the hold `setup`.
    Setup,
    /// `ready`, from the scheduler.
    Ready,
    /// `show`, from the first run in the mode `compile`.
    Show(RunId),
}

impl Raised {
    /// The row's `kind`.
    pub const fn kind(&self) -> ItemKind {
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
    pub const fn run(&self) -> Option<&RunId> {
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

/// The cause of a `hold_cleared` event: the hold that ended (I5.4, I7.1).
///
/// ```
/// use halvo_coord::types::{CheckAnswer, Cleared, RunId};
/// let run = RunId::parse("00000000000000000000000000000004").unwrap();
/// let cleared = Cleared::Check { run, answer: CheckAnswer::TryAgain };
/// ```
///
/// A check cleared by an answer other than its two cannot be written:
///
/// ```compile_fail,E0308
/// use halvo_coord::types::{Choice, Cleared, RunId};
/// let run = RunId::parse("00000000000000000000000000000004").unwrap();
/// let cleared = Cleared::Check { run, answer: Choice::Present };
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cleared {
    /// `person`: the hold `off` ended.
    Person,
    /// `failures`: the hold `failures` ended.
    Failures,
    /// `check`: the hold `check` of a run ended by the person's answer.
    Check {
        /// `run`.
        run: RunId,
        /// `answer`: `try_again` or `went_through`.
        answer: CheckAnswer,
    },
}

/// A coordinator body of A-A7.2 that `Owner::send` carries (I5.10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    /// `create_agent`.
    CreateAgent {
        /// `workflow`.
        workflow: WorkflowId,
        /// `agent`.
        agent: AgentId,
    },
    /// `set_bindings`: the slots alone, which the owner merges by name into the newest
    /// revision; it holds no origin, which only the supervisor library adds (the agent-setup
    /// run's CP-36).
    SetBindings {
        /// `workflow`.
        workflow: WorkflowId,
        /// `agent`.
        agent: AgentId,
        /// `slots`.
        slots: Vec<Slot>,
    },
    /// `set_state`.
    SetState {
        /// `workflow`.
        workflow: WorkflowId,
        /// `agent`.
        agent: AgentId,
        /// `state`.
        state: AgentState,
    },
    /// `start_run`.
    StartRun {
        /// `workflow`.
        workflow: WorkflowId,
        /// `agent`.
        agent: AgentId,
        /// `run`.
        run: RunId,
        /// `mode`.
        mode: Mode,
    },
    /// `end_run`: the agent store stores the run's record with the outcome `failed` when none
    /// is stored, and changes nothing for a run that has ended (A-A7.2, I5.10).
    EndRun {
        /// `run`.
        run: RunId,
    },
}

/// A slot of `set_bindings`: its name and its value in pieces (A-A7.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    /// `name`.
    pub name: Name,
    /// `value`.
    pub value: Vec<String>,
}

/// The JSON value of a reply of the owner that is no error (I5.10).
pub type Reply = serde_json::Value;
