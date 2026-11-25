//! The closed lists of names of contracts.md, each an enum with every value in the order of
//! its list, `name`, `parse` and a serde form that is the name (I0.6).

macro_rules! names {
    ($(#[$m:meta])* $T:ident { $($V:ident = $s:literal),+ $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $T {
            $(
                #[doc = concat!("`", $s, "`")]
                $V
            ),+
        }

        impl $T {
            /// Every value, in the order of the contract's list.
            pub const ALL: &'static [$T] = &[$($T::$V),+];

            /// The value's name.
            pub const fn name(self) -> &'static str {
                match self {
                    $($T::$V => $s),+
                }
            }

            /// Reads a name of this list; any other string is refused with `malformed` (I0.6).
            pub fn parse(text: &str) -> Result<$T, $crate::types::Cause> {
                match text {
                    $($s => Ok($T::$V),)+
                    _ => Err($crate::types::Cause::Malformed),
                }
            }
        }

        impl ::std::fmt::Display for $T {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.write_str(self.name())
            }
        }

        impl ::serde::Serialize for $T {
            fn serialize<S: ::serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(self.name())
            }
        }
    };
}

pub(crate) use names;

names! {
    /// The seven statuses of an agent, in the order of their groups (I1.2).
    Status {
        NeedsYou = "needs_you",
        Problem = "problem",
        Working = "working",
        WaitingTurn = "waiting_turn",
        AllSet = "all_set",
        Off = "off",
        NotYet = "not_yet",
    }
}

names! {
    /// The nine kinds of item, the only things that put an agent under `needs_you` (I3.1): the
    /// eight of I3.1 and `amounts`, the item of a payment or an order whose amount on the site's
    /// page afterwards could not be matched to the allowed amount (r4 U3.29).
    ItemKind {
        Confirm = "confirm",
        SignIn = "sign_in",
        LinkAccount = "link_account",
        Stuck = "stuck",
        Check = "check",
        Setup = "setup",
        Ready = "ready",
        Show = "show",
        Amounts = "amounts",
    }
}

names! {
    /// The choices of the kinds of I3.1, in the order of first use there, the two of `amounts`
    /// (r4 U3.29), and `present`, the one page choice of a confirmation of the presenter `native`
    /// (I3.4).
    Choice {
        Reject = "reject",
        Allow = "allow",
        Decline = "decline",
        Done = "done",
        TryAgain = "try_again",
        WentThrough = "went_through",
        None = "none",
        Choose = "choose",
        Skip = "skip",
        Start = "start",
        SwitchOff = "switch_off",
        Seen = "seen",
        Present = "present",
    }
}

names! {
    /// The controls of a row (I2.5).
    Control {
        RunNow = "run_now",
        Stop = "stop",
        TakeBack = "take_back",
        SwitchOff = "switch_off",
        SwitchOn = "switch_on",
        ChangeAnswers = "change_answers",
        UseModel = "use_model",
        ThisComputerOnly = "this_computer_only",
        StopSignIn = "stop_sign_in",
        Delete = "delete",
    }
}

names! {
    /// The `action` of `agents_control` (I8.1).
    Action {
        RunNow = "run_now",
        Stop = "stop",
        TakeBack = "take_back",
        SwitchOff = "switch_off",
        SwitchOn = "switch_on",
        StopSignIn = "stop_sign_in",
        UseModel = "use_model",
        ThisComputerOnly = "this_computer_only",
        ChangeAnswers = "change_answers",
    }
}

names! {
    /// `tier` of a configuration (A-A2.1, I2.1).
    Tier {
        OnDevice = "on_device",
        LowCostHosted = "low_cost_hosted",
        Frontier = "frontier",
    }
}

names! {
    /// `reasoning` of a configuration (A-A2.1, I2.1).
    Reasoning {
        Low = "low",
        Medium = "medium",
        High = "high",
    }
}

names! {
    /// `autonomy` of a configuration (A-A2.1, I2.1).
    Autonomy {
        Observe = "observe",
        DraftAndConfirm = "draft_and_confirm",
        Unattended = "unattended",
    }
}

names! {
    /// `runtime` of a configuration (A-A2.1, I2.1).
    Runtime {
        Web = "web",
        GraphMail = "graph_mail",
        GraphCalendar = "graph_calendar",
        File = "file",
        None = "none",
    }
}

names! {
    /// A name of `permissions` of a configuration (A-A2.1, I2.1).
    Permission {
        UserFiles = "user_files",
    }
}

names! {
    /// The `period` of a budget (A-A2.1).
    Period {
        Week = "week",
        Month = "month",
    }
}

names! {
    /// The ten seed categories, in the order of I2.3 (A-A1.1).
    Category {
        UtilityBillPayment = "utility_bill_payment",
        TenantRent = "tenant_rent",
        LeaseRenewalOrChange = "lease_renewal_or_change",
        TenantApplicationReview = "tenant_application_review",
        Email = "email",
        SpreadsheetFill = "spreadsheet_fill",
        ShopOrder = "shop_order",
        CalendarCleanup = "calendar_cleanup",
        RideOrder = "ride_order",
        DailyChartCheck = "daily_chart_check",
    }
}

names! {
    /// The ten action classes (A-A4.1, I4.1).
    Class {
        Read = "read",
        Prepare = "prepare",
        LocalEdit = "local_edit",
        Send = "send",
        Delete = "delete",
        Submit = "submit",
        Purchase = "purchase",
        Payment = "payment",
        Sign = "sign",
        Credential = "credential",
    }
}

names! {
    /// Where a bound field's value came from (A-A9.3, I4.3).
    BoundFrom {
        Const = "const",
        Slot = "slot",
        Extract = "extract",
        Model = "model",
    }
}

names! {
    /// `from` of a field of an item, the page's word for the source of a value (I3.4).
    FieldFrom {
        You = "you",
        Site = "site",
        Agent = "agent",
        Halvo = "halvo",
    }
}

names! {
    /// `from` of a candidate of a setup parameter (I3.5).
    CandidateFrom {
        Records = "records",
        Site = "site",
        Files = "files",
    }
}

names! {
    /// Where a confirmation is answered (I4.1).
    Presenter {
        Page = "page",
        Native = "native",
    }
}

names! {
    /// A two-valued key of a reply or a record (I0.4).
    YesNo {
        Yes = "yes",
        No = "no",
    }
}

names! {
    /// The reasons of a handback request (A-A6.4).
    HandbackReason {
        SignIn = "sign_in",
        SecondFactor = "second_factor",
        Captcha = "captcha",
        AccountLink = "account_link",
        StepFailed = "step_failed",
        EffectUnknown = "effect_unknown",
    }
}

names! {
    /// The results of a handback (A-A6.4).
    HandbackResult {
        Done = "done",
        Declined = "declined",
        Timeout = "timeout",
    }
}

names! {
    /// The outcomes of a run that a runner's last frame gives (A-A6.5).
    Outcome {
        Completed = "completed",
        NothingToDo = "nothing_to_do",
        Declined = "declined",
        HandedBack = "handed_back",
        ClassMismatch = "class_mismatch",
        Refused = "refused",
        Limit = "limit",
        EffectUnknown = "effect_unknown",
        Failed = "failed",
    }
}

names! {
    /// The fifteen ends of a run: the nine outcomes of A-A6.5 and the coordinator's own six
    /// (I5.6).
    RunEnd {
        Completed = "completed",
        NothingToDo = "nothing_to_do",
        Declined = "declined",
        HandedBack = "handed_back",
        ClassMismatch = "class_mismatch",
        Refused = "refused",
        Limit = "limit",
        EffectUnknown = "effect_unknown",
        Failed = "failed",
        Cancelled = "cancelled",
        Lapsed = "lapsed",
        Interrupted = "interrupted",
        Deleted = "deleted",
        Budget = "budget",
        Kind = "kind",
    }
}

names! {
    /// The failure class of the ends of a run (I5.6).
    Failure {
        Failed = "failed",
        Limit = "limit",
        ClassMismatch = "class_mismatch",
        Refused = "refused",
        Budget = "budget",
        Kind = "kind",
    }
}

names! {
    /// The causes of the end of an agent's newest started run, of the roster's key `last_cause`
    /// (I6.1), for which the roster has a sentence: a part of the agent store's closed list of
    /// causes, whose other names the reader of the key takes as none (ruling of 2026-11-22
    /// on `last_cause`). The seventeen from `workbook` to `data_narrow` are the causes of a failed
    /// run of the runtime file, which the roster says for an agent of that runtime alone (r4
    /// U3.22a, `roster::FILE_CAUSES`);
    /// the nine from `no_workbook` to `read_only` are the names of the agents run's unit r3 U11.3,
    /// `no_access` is the name of its unit r3 U11.4 (r4 U3.22d), `data_encoding` that of its unit
    /// r3 U11.5, and `two_sheets` and `data_narrow` those of its unit r3 U11.9 (r4 U3.28). The next
    /// three, `no_agreement`, `no_model_key` and `over_cap`, are the causes of a compile that handed
    /// its work back before anything was sent to a model's host (the agents run's unit r3 U7.2a),
    /// which the roster says for a run that ended `handed_back` alone (`roster::MODEL_CAUSES`). The
    /// last three are the ends of a run that paid or ordered (r4 U3.29), which the reader takes only
    /// beside the key `left` of the same entry, whether a payment or an order of that run left:
    /// `amount_differs` with `left` `yes`, the page's amount afterwards could not be matched to the
    /// allowed amount; `page_unread` with `left` `yes`, the page afterwards could not be read; and
    /// `amount_not_sent`, the core's name of the store's `amount_differs` with `left` `no`, a
    /// payment or an order that was held back because its prepared request named another amount.
    LastCause {
        NoChannel = "no_channel",
        BrowserStart = "browser_start",
        NoBrowser = "no_browser",
        Workbook = "workbook",
        Sheet = "sheet",
        Changed = "changed",
        StepFailed = "step_failed",
        NoWorkbook = "no_workbook",
        NoSheet = "no_sheet",
        NoRange = "no_range",
        DataShape = "data_shape",
        NoDataFile = "no_data_file",
        DataOutside = "data_outside",
        DataForm = "data_form",
        NotReplaced = "not_replaced",
        ReadOnly = "read_only",
        NoAccess = "no_access",
        DataEncoding = "data_encoding",
        TwoSheets = "two_sheets",
        DataNarrow = "data_narrow",
        NoAgreement = "no_agreement",
        NoModelKey = "no_model_key",
        OverCap = "over_cap",
        Shortfall = "shortfall",
        AmountDiffers = "amount_differs",
        PageUnread = "page_unread",
        AmountNotSent = "amount_not_sent",
    }
}

names! {
    /// The class of a payment's or an order's lines on a row and of a run (r4 U3.29), the class of
    /// the agent's category (way (i) of the coordinator's log line 20557), each named as its class
    /// of `Class`.
    PayClass {
        Payment = "payment",
        Purchase = "purchase",
    }
}

names! {
    /// The ends of a run that paid or ordered that a row and a run's line say (r4 U3.29), each the
    /// name of its last cause in `LastCause`.
    PaidEnd {
        AmountDiffers = "amount_differs",
        PageUnread = "page_unread",
        AmountNotSent = "amount_not_sent",
    }
}

impl PaidEnd {
    /// The end of the last cause `cause`, or none for a cause of another run.
    pub const fn of(cause: LastCause) -> Option<PaidEnd> {
        match cause {
            LastCause::AmountDiffers => Some(PaidEnd::AmountDiffers),
            LastCause::PageUnread => Some(PaidEnd::PageUnread),
            LastCause::AmountNotSent => Some(PaidEnd::AmountNotSent),
            _ => None,
        }
    }
}

impl PayClass {
    /// The class of the agent's category (way (i) of the coordinator's log line 20557):
    /// `utility_bill_payment` and `tenant_rent` pay, `shop_order` and `ride_order` order, and any
    /// other category has none, so that its lines read as the lines of a failed run.
    pub const fn of(category: Category) -> Option<PayClass> {
        match category {
            Category::UtilityBillPayment | Category::TenantRent => Some(PayClass::Payment),
            Category::ShopOrder | Category::RideOrder => Some(PayClass::Purchase),
            _ => None,
        }
    }
}

names! {
    /// The phases of a live run (I5.6); `ended` is no phase of a live run.
    LivePhase {
        Starting = "starting",
        Working = "working",
        Waiting = "waiting",
        Stopping = "stopping",
    }
}

names! {
    /// The mode of a run (A-A7.2, I5.5).
    Mode {
        Compile = "compile",
        Replay = "replay",
    }
}

names! {
    /// Who asked for a start (I5.5, the `trigger` of `run_started` in I7.1).
    Trigger {
        Person = "person",
        Schedule = "schedule",
    }
}

names! {
    /// The trigger of a workflow (W-C2.4).
    WorkflowTrigger {
        AppContext = "app_context",
        Schedule = "schedule",
        Manual = "manual",
    }
}

names! {
    /// The four names of the workflow run's `WorkflowState`; a dismissed workflow is no
    /// value of it (W-C2.4, I5.10).
    WorkflowState {
        Proposed = "proposed",
        Approved = "approved",
        Active = "active",
        Paused = "paused",
    }
}

names! {
    /// The state of an agent in the reply of I6.1 and in `set_state` (A-A7.2).
    AgentState {
        Approved = "approved",
        Active = "active",
        Paused = "paused",
    }
}

names! {
    /// The answer for one effect in a decision (A-A6.3).
    EffectAnswer {
        AllowOnce = "allow_once",
        RejectOnce = "reject_once",
    }
}

names! {
    /// The seven causes of a lapse of a confirmation (I4.2): the six and `answer_lost`, taken
    /// from `answered` alone after an answer's move that did not return (ruling RD-84).
    LapseCause {
        Digest = "digest",
        RunEnded = "run_ended",
        WaitLimit = "wait_limit",
        Cancelled = "cancelled",
        NoRecord = "no_record",
        AppEnded = "app_ended",
        AnswerLost = "answer_lost",
    }
}

/// The cause of a `hold_set` event: the hold that was set, with the run whose refused sign-in
/// the hold `sign_in_refused` answers (I7.1; ruling RD-73 4.2), as halvo-schema's `Held` holds it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HoldSetCause {
    /// `person`: the hold `off`.
    Person,
    /// `failures`.
    Failures,
    /// `sign_in_refused`, with the run that it answers.
    SignInRefused {
        /// `run`.
        run: super::RunId,
    },
}

impl HoldSetCause {
    /// The names of the list of I7.1, in its order.
    pub const NAMES: &'static [&'static str] = &["person", "failures", "sign_in_refused"];

    /// The value's name.
    pub const fn name(&self) -> &'static str {
        match self {
            HoldSetCause::Person => "person",
            HoldSetCause::Failures => "failures",
            HoldSetCause::SignInRefused { .. } => "sign_in_refused",
        }
    }
}

impl ::std::fmt::Display for HoldSetCause {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        f.write_str(self.name())
    }
}

names! {
    /// The key of a line of a card: the settings lines of I2.2, and the count of the agent's
    /// records taken off since Halvo opened (`card.records_dropped`; ruling RD-64 2.6).
    LineKey {
        Level = "level",
        RunsOn = "runs_on",
        Asks = "asks",
        Time = "time",
        Files = "files",
        Spend = "spend",
        RecordsDropped = "records_dropped",
    }
}

names! {
    /// What a card's read of `Owner::agent_history` answered, where the card names it in its key
    /// `history` (I2.6; the coordinator's word on R6 of U2.4t): `busy`, so the card holds no run.
    HistoryRead {
        Busy = "busy",
    }
}

names! {
    /// The type of a command's argument in `commands.json` (I8.3).
    ArgType {
        Count = "count",
        Id = "id",
        Name = "name",
        Text = "text",
    }
}

names! {
    /// The setup parameters of I3.5, in the order of first use there.
    Param {
        Start = "start",
        Payee = "payee",
        Instrument = "instrument",
        ReplySenders = "reply_senders",
        Workbook = "workbook",
        Sheet = "sheet",
        Range = "range",
        Source = "source",
        Seller = "seller",
    }
}

impl RunEnd {
    /// The end's member of the failure class, or none (I5.6).
    pub const fn failure(self) -> Option<Failure> {
        match self {
            RunEnd::Failed => Some(Failure::Failed),
            RunEnd::Limit => Some(Failure::Limit),
            RunEnd::ClassMismatch => Some(Failure::ClassMismatch),
            RunEnd::Refused => Some(Failure::Refused),
            RunEnd::Budget => Some(Failure::Budget),
            RunEnd::Kind => Some(Failure::Kind),
            _ => None,
        }
    }
}

impl From<Outcome> for RunEnd {
    fn from(o: Outcome) -> RunEnd {
        match o {
            Outcome::Completed => RunEnd::Completed,
            Outcome::NothingToDo => RunEnd::NothingToDo,
            Outcome::Declined => RunEnd::Declined,
            Outcome::HandedBack => RunEnd::HandedBack,
            Outcome::ClassMismatch => RunEnd::ClassMismatch,
            Outcome::Refused => RunEnd::Refused,
            Outcome::Limit => RunEnd::Limit,
            Outcome::EffectUnknown => RunEnd::EffectUnknown,
            Outcome::Failed => RunEnd::Failed,
        }
    }
}

impl From<Failure> for RunEnd {
    fn from(f: Failure) -> RunEnd {
        match f {
            Failure::Failed => RunEnd::Failed,
            Failure::Limit => RunEnd::Limit,
            Failure::ClassMismatch => RunEnd::ClassMismatch,
            Failure::Refused => RunEnd::Refused,
            Failure::Budget => RunEnd::Budget,
            Failure::Kind => RunEnd::Kind,
        }
    }
}

impl From<bool> for YesNo {
    fn from(b: bool) -> YesNo {
        if b {
            YesNo::Yes
        } else {
            YesNo::No
        }
    }
}
