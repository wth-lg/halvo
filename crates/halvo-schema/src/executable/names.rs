use super::json::names;

names! {
    /// The ten classes of an action. The money classes are `purchase` and `payment`; the confirm classes are
    /// `send`, `delete`, `submit`, `purchase`, `payment` and `sign`.
    pub enum ActionClass {
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

impl ActionClass {
    /// `purchase` or `payment`.
    pub fn is_money(self) -> bool {
        matches!(self, ActionClass::Purchase | ActionClass::Payment)
    }

    /// `send`, `delete`, `submit`, `purchase`, `payment` or `sign`.
    pub fn is_confirm(self) -> bool {
        matches!(
            self,
            ActionClass::Send
                | ActionClass::Delete
                | ActionClass::Submit
                | ActionClass::Purchase
                | ActionClass::Payment
                | ActionClass::Sign
        )
    }

    /// A class of a state-changing request of the browser: `submit`, `purchase`, `payment` or `sign`.
    pub fn is_browser_effect(self) -> bool {
        matches!(
            self,
            ActionClass::Submit | ActionClass::Purchase | ActionClass::Payment | ActionClass::Sign
        )
    }

    /// The bound fields of the class in their order; empty for `read`, `prepare` and `credential`, which make no
    /// effect.
    pub fn bound_fields(self) -> &'static [FieldName] {
        match self {
            ActionClass::Payment => PAYMENT,
            ActionClass::Purchase => PURCHASE,
            ActionClass::Send => SEND,
            ActionClass::Delete => DELETE,
            ActionClass::Submit => SUBMIT,
            ActionClass::Sign => SIGN,
            ActionClass::LocalEdit => LOCAL_EDIT,
            ActionClass::Read | ActionClass::Prepare | ActionClass::Credential => &[],
        }
    }
}

const PAYMENT: &[FieldName] = &[
    FieldName::Payee,
    FieldName::Amount,
    FieldName::Instrument,
    FieldName::Origin,
];
const PURCHASE: &[FieldName] = &[
    FieldName::Seller,
    FieldName::Items,
    FieldName::Amount,
    FieldName::Instrument,
    FieldName::Origin,
];
const SEND: &[FieldName] = &[FieldName::To, FieldName::Subject, FieldName::Body];
const DELETE: &[FieldName] = &[FieldName::Items];
const SUBMIT: &[FieldName] = &[FieldName::Origin, FieldName::Fields];
const SIGN: &[FieldName] = &[FieldName::Document, FieldName::Party];
const LOCAL_EDIT: &[FieldName] = &[FieldName::File, FieldName::Range];

names! {
    /// The classes that the list `allow` of a configuration may hold: `send`, `delete` and `submit`, and no other,
    /// so that no money, signing or credential class can stand in it. What must not compile, a money class as an
    /// allowed class:
    ///
    /// ```compile_fail
    /// let _ = halvo_schema::executable::names::AllowedClass::Payment;
    /// ```
    ///
    /// Its twin, which differs in the one name and compiles:
    ///
    /// ```
    /// let _ = halvo_schema::executable::names::AllowedClass::Send;
    /// ```
    pub enum AllowedClass {
        Send = "send",
        Delete = "delete",
        Submit = "submit",
    }
}

impl From<AllowedClass> for ActionClass {
    fn from(class: AllowedClass) -> ActionClass {
        match class {
            AllowedClass::Send => ActionClass::Send,
            AllowedClass::Delete => ActionClass::Delete,
            AllowedClass::Submit => ActionClass::Submit,
        }
    }
}

names! {
    /// How much an agent may do without asking, as an upper limit to the rule of each class.
    pub enum Autonomy {
        Observe = "observe",
        DraftAndConfirm = "draft_and_confirm",
        Unattended = "unattended",
    }
}

names! {
    /// The tier of the model, with no vendor name.
    pub enum ModelTier {
        OnDevice = "on_device",
        LowCostHosted = "low_cost_hosted",
        Frontier = "frontier",
    }
}

names! {
    /// What the reply of `model_room` finds of the person's count (r7 8.1L): `none`, no count is stored; `reads`, the
    /// stored count reads; `unread`, a row of the count stands and does not read.
    pub enum RoomState {
        None = "none",
        Reads = "reads",
        Unread = "unread",
    }
}

names! {
    /// The row of the model settings that the model child reads.
    pub enum Reasoning {
        Low = "low",
        Medium = "medium",
        High = "high",
    }
}

names! {
    /// The runtime of an agent.
    pub enum Runtime {
        Web = "web",
        GraphMail = "graph_mail",
        GraphCalendar = "graph_calendar",
        File = "file",
        None = "none",
    }
}

names! {
    /// The closed list of permissions.
    pub enum Permission {
        UserFiles = "user_files",
    }
}

names! {
    /// The period of a budget.
    pub enum Period {
        Week = "week",
        Month = "month",
    }
}

names! {
    /// A two-valued key: `yes` or `no`, since a record holds no boolean.
    pub enum YesNo {
        Yes = "yes",
        No = "no",
    }
}

names! {
    /// The states of an effect.
    pub enum EffectState {
        Refused = "refused",
        Cleared = "cleared",
        Held = "held",
        Approved = "approved",
        Rejected = "rejected",
        Releasing = "releasing",
        Committed = "committed",
        Unknown = "unknown",
        NotMade = "not_made",
    }
}

impl EffectState {
    /// `refused`, `rejected`, `committed`, `unknown` or `not_made`, the states that no move leaves.
    pub fn is_final(self) -> bool {
        matches!(
            self,
            EffectState::Refused
                | EffectState::Rejected
                | EffectState::Committed
                | EffectState::Unknown
                | EffectState::NotMade
        )
    }
}

names! {
    /// The one closed list of the `cause` keys of `agent_effect` and `agent_log`. `secret` ends a run whose token
    /// store could not be read, with the outcome `failed`. `unauthorized` stands in a `step_failed` line, the line of
    /// a step whose adapter call the service answered with 401. `not_sent`, `not_replaced`, `unauthorized` and
    /// `throttled` are the causes of an effect that was not made: each stands in a `not_made` row and in no other row
    /// of an effect, and a `not_made` row holds one of the four (A5.2). `not_selected` stands in a `stopped` line, the
    /// line of a send whose message the reply rule no longer selects at the load before its `begin`; `no_start`
    /// stands in a `stopped` line, the line of the list step of a run whose load holds no start row; neither stands in
    /// a row of an effect. `no_channel` and `browser_start` are causes of a browser start that failed (A11.1), and
    /// `senders_unread` of a slot of senders that its reader refuses at the load before the list (A5.1).
    /// `reproposed` is the cause of the runner's end of an effect that it then proposes again, and stands only in a
    /// `rejected` row of an effect with a null approval (A7.4). `no_browser` stands in a `stopped` line, the line of a
    /// run given no `--browser` whose runner found no browser at the fixed places of its system. `amount_differs` and
    /// `amount_unread` stand in the `stopped` line of the step of a `payment` or a `purchase` whose effect is
    /// `committed`, from the runner's read of the page after the reply (r3 U4.1w-2): `amount_differs` where the page
    /// names another amount than the kept `amount`, after which the run makes no further step and ends with that cause;
    /// `amount_unread` where the page names no amount, after which the run goes on. `page_unread` stands in that
    /// `stopped` line where the page after the reply could not be read at all (no new document within the step's
    /// time, or a failed read of it), after which the run makes no further step and ends with that cause
    /// (r3 U5.10-B). Eight names are the causes of a run of the runtime `file` that ends `failed` at the fact that
    /// each names (r3 U11.3): `no_workbook`, no file or folder at the workbook's path, or a path that is no absolute
    /// path; `no_sheet`, no sheet of that name; `no_range`, cells that are no range; `data_shape`, data that has not
    /// the cells' rows and columns; `no_data_file`, no file at the data file's path, or a path that is no absolute
    /// path; `data_outside`, a data file outside the workbook's folder; `data_form`, a data file out of its form;
    /// `read_only`, a workbook that the person cannot write. Each stands in the `step_failed` line of the file step
    /// and in the run's `stopped` line, and in no row of an effect. `not_replaced` stands there too where the system
    /// refused the rename of the edit's replace, as for a workbook that another program holds open. `no_access`
    /// stands there where the system refused, for the permission, the open or the read of the workbook or of the
    /// data file, the walk to it, or the making of a file in the workbook's folder (r3 U11.4); a refused rename
    /// keeps `not_replaced`. `data_encoding` stands there where the data file is not UTF-8 (r3 U11.5).
    /// `two_sheets` stands there where two sheets of the workbook bear the sheet's name, and `data_narrow` where a
    /// field of the data file is number signs alone, as a spreadsheet program writes a value whose column was too
    /// narrow to show it (r3 U11.9).
    pub enum Cause {
        Approval = "approval",
        Digest = "digest",
        Used = "used",
        Unknown = "unknown",
        Autonomy = "autonomy",
        Checkpoint = "checkpoint",
        Class = "class",
        ClassMismatch = "class_mismatch",
        Origin = "origin",
        Provenance = "provenance",
        Declined = "declined",
        Timeout = "timeout",
        SessionSize = "session_size",
        StepFailed = "step_failed",
        Steps = "steps",
        Wall = "wall",
        Tokens = "tokens",
        Sheet = "sheet",
        Workbook = "workbook",
        Adapter = "adapter",
        Effect = "effect",
        Ended = "ended",
        Changed = "changed",
        Secret = "secret",
        Unauthorized = "unauthorized",
        NotSent = "not_sent",
        NotReplaced = "not_replaced",
        Throttled = "throttled",
        NotSelected = "not_selected",
        NoStart = "no_start",
        NoChannel = "no_channel",
        BrowserStart = "browser_start",
        SendersUnread = "senders_unread",
        Reproposed = "reproposed",
        NoBrowser = "no_browser",
        AmountDiffers = "amount_differs",
        AmountUnread = "amount_unread",
        PageUnread = "page_unread",
        NoWorkbook = "no_workbook",
        NoSheet = "no_sheet",
        NoRange = "no_range",
        DataShape = "data_shape",
        NoDataFile = "no_data_file",
        DataOutside = "data_outside",
        DataForm = "data_form",
        ReadOnly = "read_only",
        NoAccess = "no_access",
        DataEncoding = "data_encoding",
        TwoSheets = "two_sheets",
        DataNarrow = "data_narrow",
    }
}

names! {
    /// The state of a row of `agent_reply`, one state of the reply to one message: `drafted` when its draft is made,
    /// `begun` when its send begins, `committed` when the send is committed, `unknown` when the outcome of the send is
    /// not known, `lost` when a later PATCH of the draft answers 404 or fails its checks, and `withheld` when the
    /// draft that the mail service made in the same call is not a plain reply to the sender alone: its one recipient
    /// is not, byte for byte, the sender, it has no recipient or more than one, it holds a copy or blind-copy address,
    /// or it stands in another conversation.
    pub enum ReplyState {
        Drafted = "drafted",
        Begun = "begun",
        Committed = "committed",
        Unknown = "unknown",
        Lost = "lost",
        Withheld = "withheld",
    }
}

names! {
    /// The outcome of a run, of the frame `done` and of the record `agent_run`.
    pub enum Outcome {
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
    /// The mode of a run.
    pub enum Mode {
        Compile = "compile",
        Replay = "replay",
    }
}

names! {
    /// The events of a line of a run's log. `start_accepted` is the line that A7.4 gives the handler of `start_run` to
    /// build, line 1 of its run, with no step, cause, model or figure; it says that the store accepted the start.
    pub enum LogEvent {
        StartAccepted = "start_accepted",
        StepOk = "step_ok",
        StepFailed = "step_failed",
        Recovered = "recovered",
        Handback = "handback",
        Confirm = "confirm",
        ModelCall = "model_call",
        Checkpoint = "checkpoint",
        Stopped = "stopped",
    }
}

names! {
    /// The reason of a handback request.
    pub enum HandbackReason {
        SignIn = "sign_in",
        SecondFactor = "second_factor",
        Captcha = "captcha",
        AccountLink = "account_link",
        StepFailed = "step_failed",
        EffectUnknown = "effect_unknown",
    }
}

names! {
    /// The outcome of a handback result.
    pub enum HandbackOutcome {
        Done = "done",
        Declined = "declined",
        Timeout = "timeout",
    }
}

names! {
    /// The answer of a `secret_reply`: the secret is `stored`, `none` is stored, or the store is `unreadable`. Only
    /// `none` says that no secret is stored.
    pub enum SecretAnswer {
        Stored = "stored",
        None = "none",
        Unreadable = "unreadable",
    }
}

names! {
    /// The member of the answer `unreadable`: the secret store was `locked`, or a `fault` kept it from being read.
    pub enum Unreadable {
        Locked = "locked",
        Fault = "fault",
    }
}

names! {
    /// The answer of an approval, which lasts for one effect.
    pub enum Answer {
        AllowOnce = "allow_once",
        RejectOnce = "reject_once",
    }
}

names! {
    /// The way by which an `add_origin` body adds an origin to the bindings, after an act with the person's answer:
    /// the sign-in that the person finished and allowed, the account that the person linked, or the confirmation
    /// that the person allowed for a request to a new origin.
    pub enum Way {
        SignIn = "sign_in",
        AccountLink = "account_link",
        NewOrigin = "new_origin",
    }
}

names! {
    /// The state of a row of a checkpoint, kind `agent_checkpoint`: `planned` before `prepare`, `made` once the copy
    /// is made, and `gone` or `left` as the supervisor library marks it.
    pub enum CheckpointState {
        Planned = "planned",
        Made = "made",
        Gone = "gone",
        Left = "left",
    }
}

names! {
    /// The state of a checkpoint in the reply of `list_checkpoints`, whose newest row is `planned` or `made`.
    pub enum ListedState {
        Planned = "planned",
        Made = "made",
    }
}

names! {
    /// The state that a `mark_checkpoint` body stores: the copy is `gone`, removed or found absent, or `left`.
    pub enum MarkState {
        Gone = "gone",
        Left = "left",
    }
}

names! {
    /// Why the check of a checkpoint's copy missed, so that the copy is `left`.
    pub enum LeftReason {
        Changed = "changed",
        Link = "link",
        NotFile = "not_file",
        Name = "name",
        Unreadable = "unreadable",
    }
}

names! {
    /// The state that a `set_state` body asks for.
    pub enum AgentState {
        Approved = "approved",
        Active = "active",
        Paused = "paused",
    }
}

names! {
    /// The kind of an executable step.
    pub enum StepKind {
        Web = "web",
        Api = "api",
        File = "file",
    }
}

names! {
    /// The verbs of an executable step: nine of the kind `web`, `call` of the kind `api`, `read` and `write` of
    /// the kind `file`.
    pub enum Verb {
        Navigate = "navigate",
        Click = "click",
        Fill = "fill",
        Select = "select",
        Press = "press",
        Upload = "upload",
        Extract = "extract",
        Wait = "wait",
        Handback = "handback",
        Call = "call",
        Read = "read",
        Write = "write",
    }
}

impl Verb {
    /// The kind of step that has the verb.
    pub fn kind(self) -> StepKind {
        match self {
            Verb::Call => StepKind::Api,
            Verb::Read | Verb::Write => StepKind::File,
            _ => StepKind::Web,
        }
    }
}

names! {
    /// Whether carrying out a step issues a state-changing request of the browser.
    pub enum StepEffect {
        None = "none",
        Request = "request",
    }
}

names! {
    /// How a locator finds a control.
    pub enum LocatorBy {
        Role = "role",
        Label = "label",
        Text = "text",
        TestId = "test_id",
        Css = "css",
    }
}

names! {
    /// The adapter of an api step.
    pub enum Adapter {
        GraphMail = "graph_mail",
        GraphCalendar = "graph_calendar",
    }
}

names! {
    /// The operations of the adapters: three of `graph_mail`, two of `graph_calendar`.
    pub enum ApiOp {
        MailList = "mail_list",
        MailDraftReply = "mail_draft_reply",
        MailSend = "mail_send",
        CalendarList = "calendar_list",
        CalendarDelete = "calendar_delete",
    }
}

impl ApiOp {
    /// The adapter that offers the operation.
    pub fn adapter(self) -> Adapter {
        match self {
            ApiOp::MailList | ApiOp::MailDraftReply | ApiOp::MailSend => Adapter::GraphMail,
            ApiOp::CalendarList | ApiOp::CalendarDelete => Adapter::GraphCalendar,
        }
    }

    /// The class of the operation: `read`, `prepare`, `send` or `delete`.
    pub fn class(self) -> ActionClass {
        match self {
            ApiOp::MailList | ApiOp::CalendarList => ActionClass::Read,
            ApiOp::MailDraftReply => ActionClass::Prepare,
            ApiOp::MailSend => ActionClass::Send,
            ApiOp::CalendarDelete => ActionClass::Delete,
        }
    }
}

names! {
    /// Where the value of an arg or a bound field comes from.
    pub enum Source {
        Const = "const",
        Slot = "slot",
        Extract = "extract",
        Model = "model",
    }
}

names! {
    /// The check of a condition.
    pub enum Check {
        UrlIs = "url_is",
        Present = "present",
        Enabled = "enabled",
        TextIs = "text_is",
        ValueIs = "value_is",
        CountIs = "count_is",
    }
}

names! {
    /// The outputs that the model writes in a run: `reply_body` is the only one.
    pub enum ModelOutput {
        ReplyBody = "reply_body",
    }
}

names! {
    /// The names of the bound fields of the classes.
    pub enum FieldName {
        Payee = "payee",
        Amount = "amount",
        Instrument = "instrument",
        Origin = "origin",
        Seller = "seller",
        Items = "items",
        To = "to",
        Subject = "subject",
        Body = "body",
        Fields = "fields",
        Document = "document",
        Party = "party",
        File = "file",
        Range = "range",
    }
}
