use std::collections::BTreeSet;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use halvo_schema::category::SeedCategory;
use halvo_schema::executable::body::{
    Accepted, AddOrigin, CheckpointsReply, CoordinatorCall, CoordinatorReply, CreateAgent, EndRun,
    Finish, ListCheckpoints, ListedCheckpoint, Load, LoadReplies, LoadRepliesReply, LoadReply,
    LoadSession, LoadSteps, LoadedBindings, LoadedPlaybook, LoadedSession, MarkCheckpoint,
    PutCheckpoint, PutEffect, PutLog, PutPlaybook, PutReplyRow, PutSession, RevReply, RunnerCall,
    RunnerReply, SessionReply, SetBindings, SetState, StartRun, StateReply, StepsReply, Stored,
};
use halvo_schema::executable::bound::{Bound, BoundField, KeptField};
use halvo_schema::executable::config::{AgentConfig, Budget, Caps, Limits};
use halvo_schema::executable::frame::{
    Approval, Bind, BindReply, Browser, BrowserReply, Cancel, Confirm, ConfirmDecision,
    ConfirmEffect, ConfirmReply, ConfirmRequest, Done, FromRunner, Handback, HandbackReply,
    HandbackRequest, HandbackResult, Hello, Progress, ReplyError, RunError, Secret, SecretReply,
    Store, StoreReply, StoreReplyFrame, StoreRequest, ToRunner,
};
use halvo_schema::executable::names::*;
use halvo_schema::executable::record::{
    AgentCheckpoint, AgentEffect, AgentLog, AgentReply, AgentRun, AgentStart, Slot,
};
use halvo_schema::executable::step::{
    ApiTarget, Arg, Cond, ExecutableStep, FileTarget, Locator, Playbook, Target, WebTarget,
};
use halvo_schema::executable::text::{
    ApprovalId, CreationTime, Digest, EffectId, Endpoint, Name, Null, OwnText, OwnValue,
    ProviderId, RunId, SecretText, SessionData, WebOrigin, V1,
};
use halvo_schema::executable::{self, ERROR_NAMES, NAMES};
use halvo_schema::record::Instant;
use halvo_schema::request::{ErrorName, RunnerRequest, RunnerRole};
use halvo_schema::{AgentId, RequestId, WorkflowId};

const TABLE_MADE_BY_NODE: &str = "origin-oracle.tsv";

const FILES: [&str; 13] = [
    "configs.jsonl",
    "runs.jsonl",
    "playbook.jsonl",
    "bounds.jsonl",
    "effects.jsonl",
    "checkpoints.jsonl",
    "replies.jsonl",
    "runner-bodies.jsonl",
    "coordinator-bodies.jsonl",
    "runner-replies.jsonl",
    "coordinator-replies.jsonl",
    "from-runner.jsonl",
    "to-runner.jsonl",
];

fn n(x: u32) -> NonZeroU32 {
    NonZeroU32::new(x).unwrap()
}

fn name(s: &str) -> Name {
    Name::parse(s).unwrap()
}

fn own(s: &str) -> OwnText {
    OwnText::parse(s).unwrap()
}

fn origin(s: &str) -> WebOrigin {
    WebOrigin::parse(s).unwrap()
}

fn workflow() -> WorkflowId {
    WorkflowId::from_random([0x30; 16])
}

fn agent() -> AgentId {
    AgentId::from_random([0x40; 16])
}

fn run() -> RunId {
    RunId::from_random([0x50; 16])
}

fn effect_id(b: u8) -> EffectId {
    EffectId::from_random([b; 16])
}

fn digest(c: char) -> Digest {
    Digest::parse(&c.to_string().repeat(64)).unwrap()
}

const UTILITY: &str = "http://utility.test:18090";
const PAY: &str = "http://pay.test:18090";

fn config(
    category: SeedCategory,
    autonomy: Autonomy,
    caps: (YesNo, YesNo, YesNo),
    limits: (u32, u32, u32),
    runtime: Runtime,
) -> AgentConfig {
    AgentConfig {
        v: V1,
        workflow: workflow(),
        agent: agent(),
        rev: n(1),
        category,
        tier: ModelTier::OnDevice,
        reasoning: Reasoning::Medium,
        autonomy,
        allow: Vec::new(),
        caps: Caps {
            untrusted: caps.0,
            private: caps.1,
            external: caps.2,
        },
        limits: Limits {
            steps: limits.0,
            wall_s: limits.1,
            model_tokens: limits.2,
        },
        budget: None,
        permissions: Vec::new(),
        runtime,
        playbook: None,
    }
}

fn configs() -> Vec<AgentConfig> {
    use YesNo::{No, Yes};
    let bill = config(
        SeedCategory::UtilityBillPayment,
        Autonomy::DraftAndConfirm,
        (Yes, Yes, Yes),
        (20, 120, 0),
        Runtime::Web,
    );
    let mut fill = config(
        SeedCategory::SpreadsheetFill,
        Autonomy::DraftAndConfirm,
        (No, Yes, No),
        (20, 120, 0),
        Runtime::File,
    );
    fill.permissions = vec![Permission::UserFiles];
    let ride = config(
        SeedCategory::RideOrder,
        Autonomy::Observe,
        (Yes, Yes, No),
        (1, 1, 0),
        Runtime::None,
    );
    let mut routed = config(
        SeedCategory::Email,
        Autonomy::Unattended,
        (No, Yes, Yes),
        (20, 120, 4000),
        Runtime::GraphMail,
    );
    routed.rev = n(3);
    routed.tier = ModelTier::Frontier;
    routed.reasoning = Reasoning::High;
    routed.allow = vec![AllowedClass::Send, AllowedClass::Delete];
    routed.budget = Some(Budget {
        amount: 2000,
        period: Period::Month,
    });
    routed.playbook = Some(n(2));
    vec![bill, fill, ride, routed]
}

fn agent_run(outcome: Outcome, mode: Mode, unknown: u32) -> AgentRun {
    AgentRun {
        v: V1,
        workflow: workflow(),
        agent: agent(),
        run: run(),
        day: 20768,
        mode: Some(mode),
        outcome,
        playbook: Some(n(1)),
        steps: 9,
        prompts: 2,
        handbacks: 1,
        confirmations: 1,
        effects: 1,
        unknown,
        model_calls: 3,
        tokens_in: 2400,
        tokens_out: 120,
        wall_s: 41,
        model: Some(name("qwen3_1_7b")),
        before_start: None,
        cut: None,
        no_key: None,
        not_to_account: None,
        incomplete: None,
        reply_to_other: None,
        more_pages: None,
        held_recipient: None,
        organized: None,
        other_task: None,
        held_earlier: None,
        earlier_page: None,
    }
}

fn runs() -> Vec<AgentRun> {
    let mut replay = agent_run(Outcome::Completed, Mode::Replay, 0);
    replay.model = None;
    replay.model_calls = 0;
    replay.tokens_in = 0;
    replay.tokens_out = 0;
    let mut ended = agent_run(Outcome::Failed, Mode::Replay, 1);
    ended.mode = None;
    ended.playbook = None;
    ended.steps = 0;
    ended.prompts = 0;
    ended.handbacks = 0;
    ended.confirmations = 0;
    ended.model_calls = 0;
    ended.tokens_in = 0;
    ended.tokens_out = 0;
    ended.wall_s = 0;
    ended.model = None;
    let mut replied = agent_run(Outcome::Completed, Mode::Replay, 0);
    replied.before_start = Some(2);
    replied.cut = Some(YesNo::Yes);
    replied.no_key = Some(0);
    replied.not_to_account = Some(1);
    replied.incomplete = Some(0);
    replied.reply_to_other = Some(1);
    replied.more_pages = Some(YesNo::No);
    replied.held_recipient = Some(1);
    replied.other_task = Some(0);
    replied.held_earlier = Some(3);
    replied.earlier_page = Some(1);
    vec![
        replay,
        agent_run(Outcome::Failed, Mode::Compile, 1),
        agent_run(Outcome::EffectUnknown, Mode::Replay, 1),
        ended,
        replied,
    ]
}

fn locator(by: LocatorBy, value: &str, accessible: Option<&str>) -> Locator {
    Locator {
        by,
        value: own(value),
        name: accessible.map(own),
    }
}

fn web_target(desc: &str, frame: &[&str], credential: YesNo, locators: Vec<Locator>) -> Target {
    Target::Web(web(desc, frame, credential, locators))
}

fn web(desc: &str, frame: &[&str], credential: YesNo, locators: Vec<Locator>) -> WebTarget {
    WebTarget {
        desc: own(desc),
        frame: frame.iter().map(|o| origin(o)).collect(),
        credential,
        locators,
    }
}

fn arg(arg_name: &str, from: Source, value: Option<&str>, key: Option<&str>) -> Arg {
    Arg {
        name: name(arg_name),
        from,
        value: value.map(own),
        key: key.map(name),
    }
}

fn c(arg_name: &str, value: &str) -> Arg {
    arg(arg_name, Source::Const, Some(value), None)
}

fn s(arg_name: &str, key: &str) -> Arg {
    arg(arg_name, Source::Slot, None, Some(key))
}

fn x(arg_name: &str, key: &str) -> Arg {
    arg(arg_name, Source::Extract, None, Some(key))
}

fn m(arg_name: &str) -> Arg {
    arg(arg_name, Source::Model, None, Some("reply_body"))
}

struct S {
    kind: StepKind,
    verb: Verb,
    target: Option<Target>,
    args: Vec<Arg>,
    pre: Vec<Cond>,
    post: Vec<Cond>,
    effect: StepEffect,
    class: ActionClass,
}

fn plain(verb: Verb, target: Option<Target>, args: Vec<Arg>, class: ActionClass) -> S {
    S {
        kind: verb.kind(),
        verb,
        target,
        args,
        pre: Vec::new(),
        post: Vec::new(),
        effect: StepEffect::None,
        class,
    }
}

fn api(op: ApiOp, args: Vec<Arg>) -> S {
    plain(
        Verb::Call,
        Some(Target::Api(ApiTarget {
            adapter: op.adapter(),
            op,
        })),
        args,
        op.class(),
    )
}

fn steps() -> Vec<ExecutableStep> {
    use YesNo::{No, Yes};
    let amount = || {
        web(
            "text Amount due",
            &[],
            No,
            vec![
                locator(LocatorBy::TestId, "amount-due", None),
                locator(LocatorBy::Role, "text", Some("Amount due")),
            ],
        )
    };
    let pay_button = || {
        web_target(
            "button Pay",
            &[PAY],
            No,
            vec![
                locator(LocatorBy::Role, "button", Some("Pay")),
                locator(LocatorBy::Css, "form .pay", None),
            ],
        )
    };
    let list = vec![
        S {
            post: vec![Cond {
                check: Check::UrlIs,
                target: None,
                arg: Some(c("url", "http://utility.test:18090/bill")),
            }],
            ..plain(
                Verb::Navigate,
                None,
                vec![s("url", "start")],
                ActionClass::Read,
            )
        },
        plain(
            Verb::Handback,
            None,
            vec![c("reason", "sign_in")],
            ActionClass::Credential,
        ),
        plain(Verb::Wait, None, vec![c("ms", "500")], ActionClass::Read),
        S {
            pre: vec![Cond {
                check: Check::Present,
                target: Some(amount()),
                arg: None,
            }],
            ..plain(
                Verb::Extract,
                Some(Target::Web(amount())),
                vec![c("as", "amount_due"), c("parse", "amount")],
                ActionClass::Read,
            )
        },
        S {
            post: vec![Cond {
                check: Check::ValueIs,
                target: Some(web(
                    "textbox Payee",
                    &[PAY],
                    No,
                    vec![locator(LocatorBy::Label, "Payee", None)],
                )),
                arg: Some(s("text", "payee")),
            }],
            ..plain(
                Verb::Fill,
                Some(web_target(
                    "textbox Payee",
                    &[PAY],
                    No,
                    vec![locator(LocatorBy::Label, "Payee", None)],
                )),
                vec![s("text", "payee")],
                ActionClass::Prepare,
            )
        },
        plain(
            Verb::Select,
            Some(web_target(
                "combobox Plan",
                &[],
                No,
                vec![locator(LocatorBy::Role, "combobox", Some("Plan"))],
            )),
            vec![c("option", "Monthly")],
            ActionClass::Prepare,
        ),
        plain(
            Verb::Press,
            Some(web_target(
                "textbox Password",
                &[],
                Yes,
                vec![locator(LocatorBy::Text, "Password", None)],
            )),
            vec![c("key", "Tab")],
            ActionClass::Credential,
        ),
        plain(
            Verb::Upload,
            Some(web_target(
                "button Attach",
                &[],
                No,
                vec![locator(LocatorBy::TestId, "attach", None)],
            )),
            vec![s("file", "statement")],
            ActionClass::Prepare,
        ),
        S {
            effect: StepEffect::Request,
            pre: vec![Cond {
                check: Check::Enabled,
                target: Some(web(
                    "button Pay",
                    &[PAY],
                    No,
                    vec![locator(LocatorBy::Role, "button", Some("Pay"))],
                )),
                arg: None,
            }],
            ..plain(
                Verb::Click,
                Some(pay_button()),
                vec![
                    s("payee", "payee"),
                    x("amount", "amount_due"),
                    s("instrument", "instrument"),
                    c("origin", PAY),
                ],
                ActionClass::Payment,
            )
        },
        api(ApiOp::MailList, vec![]),
        api(ApiOp::MailDraftReply, vec![m("body")]),
        api(
            ApiOp::MailSend,
            vec![x("to", "to"), x("subject", "subject"), m("body")],
        ),
        api(ApiOp::CalendarList, vec![]),
        api(ApiOp::CalendarDelete, vec![x("items", "events")]),
        plain(
            Verb::Read,
            Some(Target::File(FileTarget {
                file: name("source"),
                sheet: None,
                range: None,
            })),
            vec![c("as", "rows")],
            ActionClass::Read,
        ),
        plain(
            Verb::Write,
            Some(Target::File(FileTarget {
                file: name("workbook"),
                sheet: Some(name("sheet")),
                range: Some(name("range")),
            })),
            vec![x("values", "rows")],
            ActionClass::LocalEdit,
        ),
    ];
    list.into_iter()
        .zip(1..)
        .map(|(s, i)| ExecutableStep {
            v: V1,
            n: n(i),
            kind: s.kind,
            verb: s.verb,
            target: s.target,
            args: s.args,
            pre: s.pre,
            post: s.post,
            effect: s.effect,
            class: s.class,
        })
        .collect()
}

fn field(field_name: FieldName, from: Source, at: Option<&str>, value: &str) -> BoundField {
    BoundField {
        name: field_name,
        from,
        origin: at.map(origin),
        value: OwnValue::new(value),
    }
}

fn bounds() -> Vec<Bound> {
    use FieldName as F;
    use Source::{Const, Extract, Model, Slot};
    vec![
        Bound::new(
            ActionClass::Payment,
            vec![
                field(F::Payee, Slot, None, "Northwind Power"),
                field(F::Amount, Extract, Some(UTILITY), "84.20"),
                field(F::Instrument, Slot, None, "card 4242"),
                field(F::Origin, Const, None, PAY),
            ],
        ),
        Bound::new(
            ActionClass::Purchase,
            vec![
                field(F::Seller, Slot, None, "Corner Shop"),
                field(
                    F::Items,
                    Extract,
                    Some("http://shop.test:18090"),
                    "2 x tea, 1 x rice",
                ),
                field(F::Amount, Extract, Some("http://shop.test:18090"), "12.40"),
                field(F::Instrument, Slot, None, "card 4242"),
                field(F::Origin, Const, None, PAY),
            ],
        ),
        Bound::new(
            ActionClass::Send,
            vec![
                field(F::To, Extract, None, "landlord@mail.test"),
                field(F::Subject, Extract, None, "Re: rent"),
                field(F::Body, Model, None, &"Thank you. ".repeat(100)),
            ],
        ),
        Bound::new(
            ActionClass::Delete,
            vec![field(F::Items, Extract, None, "AAMkAG1")],
        ),
        Bound::new(
            ActionClass::Submit,
            vec![
                field(F::Origin, Const, None, "http://form.test:18090"),
                field(F::Fields, Slot, None, "name, address"),
            ],
        ),
        Bound::new(
            ActionClass::Sign,
            vec![
                field(F::Document, Slot, None, "lease 2027"),
                field(F::Party, Slot, None, "Harbor Homes"),
            ],
        ),
        Bound::new(
            ActionClass::LocalEdit,
            vec![
                field(F::File, Slot, None, "book.xlsx"),
                field(F::Range, Slot, None, "B2:D6"),
            ],
        ),
    ]
    .into_iter()
    .map(Result::unwrap)
    .collect()
}

fn kept(field_name: FieldName, value: &str) -> KeptField {
    KeptField {
        name: field_name,
        value: OwnValue::new(value),
    }
}

fn effect_row(
    seq: u32,
    state: EffectState,
    approval: Option<u8>,
    cause: Option<Cause>,
) -> AgentEffect {
    AgentEffect {
        v: V1,
        workflow: workflow(),
        agent: agent(),
        run: run(),
        effect: effect_id(0x60),
        seq: n(seq),
        day: 20768,
        at: Instant(1_794_339_000_000_000_000 + i64::from(seq)),
        state,
        class: ActionClass::Payment,
        digest: digest('a'),
        kept: vec![
            kept(FieldName::Payee, "Northwind Power"),
            kept(FieldName::Amount, "84.20"),
            kept(FieldName::Instrument, "card 4242"),
            kept(FieldName::Origin, PAY),
        ],
        fields: 4,
        approval: approval.map(|b| ApprovalId::from_random([b; 16])),
        cause,
    }
}

fn checkpoint_row(seq: u32, state: CheckpointState, left: Option<LeftReason>) -> AgentCheckpoint {
    let copy = state != CheckpointState::Planned && left != Some(LeftReason::Changed);
    AgentCheckpoint {
        v: V1,
        workflow: workflow(),
        agent: agent(),
        run: run(),
        seq: n(seq),
        day: 20768,
        at: Instant(1_794_339_000_000_000_000 + i64::from(seq)),
        folder: OwnValue::new("/data/books"),
        workbook: OwnValue::new("/data/books/bills.xlsx"),
        copy: OwnValue::new(&format!("bills.halvo-checkpoint-{}.xlsx", run().as_str())),
        sha256: copy.then(|| digest('b')),
        bytes: copy.then_some(18_432),
        state,
        left,
    }
}

fn checkpoints() -> Vec<AgentCheckpoint> {
    vec![
        checkpoint_row(1, CheckpointState::Planned, None),
        checkpoint_row(2, CheckpointState::Made, None),
        checkpoint_row(3, CheckpointState::Gone, None),
        checkpoint_row(3, CheckpointState::Left, Some(LeftReason::Link)),
        checkpoint_row(2, CheckpointState::Left, Some(LeftReason::Changed)),
    ]
}

fn reply_row(seq: u32, message: &str, state: ReplyState, effect: bool) -> AgentReply {
    AgentReply {
        v: V1,
        workflow: workflow(),
        agent: agent(),
        run: run(),
        seq: n(seq),
        day: 20768,
        at: Instant(1_794_339_000_000_000_000 + i64::from(seq)),
        message: ProviderId::parse(&format!("msg-{message}")).unwrap(),
        draft: (state != ReplyState::Lost)
            .then(|| ProviderId::parse(&format!("draft-{message}")).unwrap()),
        effect: effect.then(|| effect_id(0x60)),
        state,
    }
}

fn replies() -> Vec<AgentReply> {
    vec![
        reply_row(1, "0001", ReplyState::Drafted, false),
        reply_row(2, "0001", ReplyState::Begun, true),
        reply_row(3, "0001", ReplyState::Drafted, true),
        reply_row(4, "0001", ReplyState::Begun, true),
        reply_row(5, "0001", ReplyState::Committed, true),
        reply_row(2, "0002", ReplyState::Begun, true),
        reply_row(3, "0002", ReplyState::Unknown, true),
        reply_row(2, "0003", ReplyState::Lost, false),
        reply_row(3, "0003", ReplyState::Drafted, false),
        reply_row(2, "0004", ReplyState::Withheld, false),
    ]
}

fn listed_checkpoint(state: ListedState, effect: Option<EffectState>) -> ListedCheckpoint {
    let row = checkpoint_row(1, CheckpointState::Planned, None);
    let made = state == ListedState::Made;
    let book = if made { "rent" } else { "bills" };
    ListedCheckpoint {
        workflow: row.workflow,
        agent: row.agent,
        run: row.run,
        folder: row.folder,
        workbook: OwnValue::new(&format!("/data/books/{book}.xlsx")),
        copy: OwnValue::new(&format!("{book}.halvo-checkpoint-{}.xlsx", run().as_str())),
        state,
        sha256: made.then(|| digest('b')),
        bytes: made.then_some(18_432),
        at: row.at,
        ended: if made { YesNo::Yes } else { YesNo::No },
        effect,
        superseded: YesNo::No,
    }
}

fn effects() -> Vec<AgentEffect> {
    let mut send = effect_row(1, EffectState::Held, None, None);
    send.effect = effect_id(0x61);
    send.class = ActionClass::Send;
    send.kept = vec![kept(FieldName::To, "landlord@mail.test")];
    send.fields = 3;
    vec![
        effect_row(1, EffectState::Held, None, None),
        effect_row(2, EffectState::Approved, Some(0x70), None),
        effect_row(3, EffectState::Releasing, Some(0x70), None),
        effect_row(4, EffectState::Committed, Some(0x70), None),
        effect_row(5, EffectState::Rejected, None, Some(Cause::Ended)),
        effect_row(6, EffectState::Unknown, None, None),
        send,
    ]
}

fn log_line() -> AgentLog {
    AgentLog {
        v: V1,
        workflow: workflow(),
        agent: agent(),
        run: run(),
        seq: n(1),
        day: 20768,
        at: Instant(1_794_339_000_000_000_000),
        event: LogEvent::ModelCall,
        step: Some(n(3)),
        cause: None,
        model: Some(name("qwen3_1_7b")),
        ms: Some(812),
        tokens_in: Some(900),
        tokens_out: Some(40),
    }
}

fn slots() -> Vec<Slot> {
    vec![
        Slot {
            name: name("payee"),
            value: OwnValue::new("Northwind Power"),
        },
        Slot {
            name: name("instrument"),
            value: OwnValue::new("card 4242"),
        },
    ]
}

fn session_data() -> SessionData {
    SessionData::new(vec!["eyJjb29raWVzIjpbXX0=".to_owned()]).unwrap()
}

fn runner_bodies() -> Vec<RunnerCall> {
    let all = steps();
    vec![
        RunnerCall::Load(Load {
            agent: agent(),
            run: run(),
        }),
        RunnerCall::LoadSteps(LoadSteps {
            agent: agent(),
            run: run(),
            number: n(1),
            from: n(1),
        }),
        RunnerCall::LoadSession(LoadSession {
            agent: agent(),
            run: run(),
            origin: origin(UTILITY),
            rev: n(1),
            part: n(1),
        }),
        RunnerCall::PutPlaybook(PutPlaybook {
            agent: agent(),
            run: run(),
            number: n(1),
            steps: all[..3].to_vec(),
        }),
        RunnerCall::PutSession(PutSession {
            agent: agent(),
            run: run(),
            origin: origin(UTILITY),
            rev: n(2),
            part: n(1),
            of: n(1),
            data: session_data(),
        }),
        RunnerCall::Effect(PutEffect {
            agent: agent(),
            run: run(),
            record: effect_row(1, EffectState::Held, None, None),
        }),
        RunnerCall::Log(PutLog {
            agent: agent(),
            run: run(),
            records: vec![log_line()],
        }),
        RunnerCall::Finish(Finish {
            agent: agent(),
            run: run(),
            record: runs().remove(0),
        }),
        RunnerCall::Checkpoint(PutCheckpoint {
            agent: agent(),
            run: run(),
            record: checkpoint_row(1, CheckpointState::Planned, None),
        }),
        RunnerCall::Checkpoint(PutCheckpoint {
            agent: agent(),
            run: run(),
            record: checkpoint_row(2, CheckpointState::Made, None),
        }),
        RunnerCall::ReplyRow(PutReplyRow {
            agent: agent(),
            run: run(),
            record: replies().remove(1),
        }),
        RunnerCall::LoadReplies(LoadReplies {
            agent: agent(),
            run: run(),
            messages: ["0001", "0002", "0003", "0004", "0005"]
                .iter()
                .map(|m| ProviderId::parse(&format!("msg-{m}")).unwrap())
                .collect(),
        }),
    ]
}

fn coordinator_bodies() -> Vec<CoordinatorCall> {
    vec![
        CoordinatorCall::CreateAgent(CreateAgent {
            workflow: workflow(),
            agent: agent(),
        }),
        CoordinatorCall::SetBindings(SetBindings {
            workflow: workflow(),
            agent: agent(),
            slots: slots(),
        }),
        CoordinatorCall::SetState(SetState {
            workflow: workflow(),
            agent: agent(),
            state: AgentState::Active,
        }),
        CoordinatorCall::StartRun(StartRun {
            workflow: workflow(),
            agent: agent(),
            run: run(),
            mode: Mode::Compile,
        }),
        CoordinatorCall::EndRun(EndRun {
            workflow: workflow(),
            agent: agent(),
            run: run(),
        }),
        CoordinatorCall::AddOrigin(AddOrigin {
            workflow: workflow(),
            agent: agent(),
            run: run(),
            origin: origin(UTILITY),
            way: Way::SignIn,
        }),
        CoordinatorCall::AddOrigin(AddOrigin {
            workflow: workflow(),
            agent: agent(),
            run: run(),
            origin: origin("https://graph.test:18443"),
            way: Way::AccountLink,
        }),
        CoordinatorCall::AddOrigin(AddOrigin {
            workflow: workflow(),
            agent: agent(),
            run: run(),
            origin: origin(PAY),
            way: Way::NewOrigin,
        }),
        CoordinatorCall::ListCheckpoints(ListCheckpoints {
            workflow: Null,
            agent: Null,
            after: None,
        }),
        CoordinatorCall::ListCheckpoints(ListCheckpoints {
            workflow: Null,
            agent: Null,
            after: Some(run()),
        }),
        CoordinatorCall::MarkCheckpoint(MarkCheckpoint {
            workflow: workflow(),
            agent: agent(),
            run: run(),
            workbook: OwnValue::new("/data/books/bills.xlsx"),
            state: MarkState::Gone,
            left: None,
        }),
        CoordinatorCall::MarkCheckpoint(MarkCheckpoint {
            workflow: workflow(),
            agent: agent(),
            run: run(),
            workbook: OwnValue::new("/data/books/bills.xlsx"),
            state: MarkState::Left,
            left: Some(LeftReason::NotFile),
        }),
    ]
}

fn runner_replies() -> Vec<RunnerReply> {
    vec![
        RunnerReply::Load(LoadReply {
            config: configs().remove(0),
            playbook: Some(LoadedPlaybook {
                number: n(1),
                steps: n(9),
            }),
            bindings: Some(LoadedBindings {
                rev: n(1),
                origins: vec![origin(UTILITY), origin(PAY)],
                slots: slots(),
            }),
            sessions: vec![LoadedSession {
                origin: origin(UTILITY),
                rev: n(1),
                parts: n(1),
            }],
            start: Some(AgentStart {
                v: V1,
                workflow: configs()[0].workflow.clone(),
                agent: configs()[0].agent.clone(),
                day: 20768,
                at: Instant(1_794_339_000_000_000_010),
            }),
        }),
        RunnerReply::Steps(StepsReply {
            steps: steps()[..2].to_vec(),
        }),
        RunnerReply::Session(SessionReply {
            origin: origin(UTILITY),
            rev: n(1),
            part: n(1),
            of: n(1),
            data: session_data(),
        }),
        RunnerReply::Stored(Stored { stored: 1 }),
        RunnerReply::Replies(LoadRepliesReply {
            replies: vec![
                reply_row(5, "0001", ReplyState::Committed, true),
                reply_row(3, "0003", ReplyState::Drafted, false),
            ],
            blocked: vec![ProviderId::parse("msg-0002").unwrap()],
            blocked_withheld: vec![ProviderId::parse("msg-0004").unwrap()],
        }),
    ]
}

fn coordinator_replies() -> Vec<CoordinatorReply> {
    vec![
        CoordinatorReply::Rev(RevReply { rev: n(1) }),
        CoordinatorReply::State(StateReply {
            state: AgentState::Paused,
        }),
        CoordinatorReply::Accepted(Accepted {}),
        CoordinatorReply::Checkpoints(CheckpointsReply {
            checkpoints: vec![
                listed_checkpoint(ListedState::Planned, None),
                listed_checkpoint(ListedState::Made, Some(EffectState::Committed)),
            ],
            next: Some(run()),
        }),
        CoordinatorReply::Checkpoints(CheckpointsReply {
            checkpoints: vec![],
            next: None,
        }),
        CoordinatorReply::Stored(Stored { stored: 1 }),
    ]
}

fn request_id() -> RequestId {
    RequestId::from_random([0x0f; 16])
}

fn confirm_request() -> ConfirmRequest {
    ConfirmRequest {
        v: V1,
        run: run(),
        agent: agent(),
        cut: None,
        effects: vec![ConfirmEffect {
            effect: effect_id(0x60),
            class: ActionClass::Payment,
            digest: digest('a'),
            bound: bounds().remove(0),
            new_origin: Some(origin(PAY)),
        }],
    }
}

fn from_runner() -> Vec<FromRunner> {
    let call = runner_bodies().remove(0);
    let request = RunnerRequest::new(&RunnerRole::claim(), request_id(), call);
    vec![
        FromRunner::Hello(Hello {
            run: run(),
            agent: agent(),
            v: V1,
        }),
        FromRunner::Store(Store {
            request: StoreRequest::new(&request).unwrap(),
        }),
        FromRunner::Confirm(Confirm {
            request: confirm_request(),
        }),
        FromRunner::Handback(Handback {
            request: HandbackRequest {
                v: V1,
                run: run(),
                agent: agent(),
                reason: HandbackReason::SignIn,
                origin: Some(origin(UTILITY)),
                endpoint: Some(Endpoint::parse("127.0.0.1:9222").unwrap()),
            },
        }),
        FromRunner::Secret(Secret {
            secret: name("fixture_token"),
        }),
        FromRunner::Progress(Progress {
            step: n(4),
            of: n(9),
        }),
        FromRunner::Done(Done {
            outcome: Outcome::Completed,
        }),
        FromRunner::Bind(Bind {
            origin: origin(PAY),
        }),
        FromRunner::Browser(Browser {
            pid: n(4242),
            created: CreationTime::parse("1794339000123456").unwrap(),
            group: Some(n(4242)),
        }),
        FromRunner::Browser(Browser {
            pid: n(4242),
            created: CreationTime::parse("1794339000123456").unwrap(),
            group: None,
        }),
    ]
}

fn to_runner() -> Vec<ToRunner> {
    vec![
        ToRunner::StoreReply(StoreReplyFrame {
            reply: StoreReply {
                id: Some(request_id()),
                outcome: Ok(RunnerReply::Stored(Stored { stored: 1 })),
            },
        }),
        ToRunner::StoreReply(StoreReplyFrame {
            reply: StoreReply {
                id: Some(request_id()),
                outcome: Err(ReplyError::Run(RunError::parse("binding").unwrap())),
            },
        }),
        ToRunner::StoreReply(StoreReplyFrame {
            reply: StoreReply {
                id: None,
                outcome: Err(ReplyError::Workflow(ErrorName::Malformed)),
            },
        }),
        ToRunner::ConfirmReply(ConfirmReply {
            decision: ConfirmDecision {
                v: V1,
                approvals: vec![Approval {
                    approval: ApprovalId::from_random([0x70; 16]),
                    effect: effect_id(0x60),
                    digest: digest('a'),
                    answer: Answer::AllowOnce,
                }],
            },
        }),
        ToRunner::HandbackReply(HandbackReply {
            result: HandbackResult {
                v: V1,
                outcome: HandbackOutcome::Done,
                secret: Some(name("fixture_token")),
            },
        }),
        ToRunner::SecretReply(SecretReply::Stored(
            SecretText::parse("tok_fixture").unwrap(),
        )),
        ToRunner::SecretReply(SecretReply::None),
        ToRunner::SecretReply(SecretReply::Unreadable(Unreadable::Locked)),
        ToRunner::SecretReply(SecretReply::Unreadable(Unreadable::Fault)),
        ToRunner::Cancel(Cancel {}),
        ToRunner::BindReply(BindReply { bound: YesNo::Yes }),
        ToRunner::BindReply(BindReply { bound: YesNo::No }),
        ToRunner::BrowserReply(BrowserReply {}),
    ]
}

fn lines<T>(items: &[T], write: impl Fn(&T) -> String) -> String {
    items
        .iter()
        .map(|item| format!("{}\n", write(item)))
        .collect()
}

fn built(file: &str) -> String {
    match file {
        "configs.jsonl" => lines(&configs(), |v| v.to_json().unwrap()),
        "runs.jsonl" => lines(&runs(), |v| v.to_json().unwrap()),
        "playbook.jsonl" => Playbook::new(steps()).unwrap().to_file().unwrap(),
        "bounds.jsonl" => lines(&bounds(), |v| v.to_json().unwrap()),
        "effects.jsonl" => lines(&effects(), |v| v.to_json().unwrap()),
        "checkpoints.jsonl" => lines(&checkpoints(), |v| v.to_json().unwrap()),
        "replies.jsonl" => lines(&replies(), |v| v.to_json().unwrap()),
        "runner-bodies.jsonl" => lines(&runner_bodies(), |v| v.to_json().unwrap()),
        "coordinator-bodies.jsonl" => lines(&coordinator_bodies(), |v| v.to_json().unwrap()),
        "runner-replies.jsonl" => lines(&runner_replies(), |v| v.to_json().unwrap()),
        "coordinator-replies.jsonl" => lines(&coordinator_replies(), |v| v.to_json().unwrap()),
        "from-runner.jsonl" => lines(&from_runner(), |v| v.to_json().unwrap()),
        "to-runner.jsonl" => lines(&to_runner(), |v| v.to_json().unwrap()),
        _ => panic!("{file}"),
    }
}

fn reread(file: &str, line: &str) -> String {
    let b = line.as_bytes();
    match file {
        "configs.jsonl" => AgentConfig::parse(b).and_then(|v| v.to_json()),
        "runs.jsonl" => AgentRun::parse(b).and_then(|v| v.to_json()),
        "playbook.jsonl" => ExecutableStep::parse(b).and_then(|v| v.to_json()),
        "bounds.jsonl" => Bound::parse(b).and_then(|v| v.to_json()),
        "effects.jsonl" => AgentEffect::parse(b).and_then(|v| v.to_json()),
        "checkpoints.jsonl" => AgentCheckpoint::parse(b).and_then(|v| v.to_json()),
        "replies.jsonl" => AgentReply::parse(b).and_then(|v| v.to_json()),
        "runner-bodies.jsonl" => RunnerCall::parse(b).and_then(|v| v.to_json()),
        "coordinator-bodies.jsonl" => CoordinatorCall::parse(b).and_then(|v| v.to_json()),
        "runner-replies.jsonl" => RunnerReply::parse(b).and_then(|v| v.to_json()),
        "coordinator-replies.jsonl" => CoordinatorReply::parse(b).and_then(|v| v.to_json()),
        "from-runner.jsonl" => FromRunner::parse(b).and_then(|v| v.to_json()),
        "to-runner.jsonl" => ToRunner::parse(b).and_then(|v| v.to_json()),
        _ => panic!("{file}"),
    }
    .unwrap_or_else(|e| panic!("{file}: {e}"))
}

fn committed_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/r3")
}

fn committed(file: &str) -> String {
    std::fs::read_to_string(committed_dir().join(file)).unwrap_or_else(|e| panic!("{file}: {e}"))
}

#[test]
fn golden_r3_files_are_built_again_from_values_and_equal_byte_for_byte() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("halvo-schema-golden-r3");
    std::fs::create_dir_all(&out).unwrap();
    for file in FILES {
        std::fs::write(out.join(file), built(file)).unwrap();
    }
    let mut listed: Vec<String> = std::fs::read_dir(committed_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "own" && name != TABLE_MADE_BY_NODE)
        .collect();
    listed.sort();
    let mut expected: Vec<String> = FILES.iter().map(|f| (*f).to_owned()).collect();
    expected.sort();
    assert_eq!(listed, expected);
    for file in FILES {
        assert_eq!(built(file), committed(file), "{file}");
    }
}

#[test]
fn golden_r3_lines_are_read_into_their_type_and_written_back_to_the_same_bytes() {
    for file in FILES {
        let text = committed(file);
        assert!(text.ends_with('\n') && !text.contains('\r'), "{file}");
        for line in text.lines() {
            assert_eq!(reread(file, line), line, "{file}");
        }
    }
    let playbook = committed("playbook.jsonl");
    assert_eq!(
        Playbook::parse_file(playbook.as_bytes())
            .unwrap()
            .to_file()
            .unwrap(),
        playbook
    );
}

fn keys_of(value: &serde_json::Value, keys: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, inner) in map {
                keys.insert(key.clone());
                keys_of(inner, keys);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|i| keys_of(i, keys)),
        _ => {}
    }
}

const LEFT_OUT: [&str; 6] = ["tier", "amount", "period", "week", "month", "role"];

#[test]
fn the_names_list_holds_every_key_and_closed_list_name_of_the_records_and_bodies() {
    let mut keys = BTreeSet::new();
    for file in [
        "configs.jsonl",
        "runs.jsonl",
        "playbook.jsonl",
        "effects.jsonl",
        "checkpoints.jsonl",
        "replies.jsonl",
        "runner-bodies.jsonl",
        "coordinator-bodies.jsonl",
        "runner-replies.jsonl",
        "coordinator-replies.jsonl",
    ] {
        for line in committed(file).lines() {
            keys_of(&serde_json::from_str(line).unwrap(), &mut keys);
        }
    }
    let mut derived: BTreeSet<String> = keys;
    for kind in executable::KINDS {
        derived.insert(kind.name.to_owned());
        derived.insert(kind.family.to_owned());
    }
    derived.extend(["yes", "no", "reply_body"].map(str::to_owned));
    macro_rules! all {
        ($($t:ty),+) => {
            $( derived.extend(<$t>::ALL.iter().map(|v| v.as_str().to_owned())); )+
        };
    }
    all!(
        ActionClass,
        Autonomy,
        ModelTier,
        Reasoning,
        Runtime,
        Permission,
        Period,
        EffectState,
        Cause,
        Outcome,
        Mode,
        LogEvent,
        AgentState,
        StepKind,
        Verb,
        StepEffect,
        LocatorBy,
        Adapter,
        ApiOp,
        Source,
        Check,
        Way,
        CheckpointState,
        ListedState,
        MarkState,
        LeftReason,
        ReplyState
    );
    for class in ActionClass::ALL {
        derived.extend(
            class
                .kept_fields()
                .iter()
                .map(|(f, _)| f.as_str().to_owned()),
        );
    }
    derived.extend(runner_bodies().iter().map(|b| b.op().to_owned()));
    derived.extend(coordinator_bodies().iter().map(|b| b.op().to_owned()));
    for name in LEFT_OUT {
        derived.remove(name);
    }
    let listed: BTreeSet<String> = NAMES.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(listed.len(), NAMES.len(), "a name stands twice in NAMES");
    let extra: Vec<&String> = listed.difference(&derived).collect();
    let missing: Vec<&String> = derived.difference(&listed).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "names that the rules do not give: {extra:?}; names that NAMES lacks: {missing:?}"
    );
    for name in LEFT_OUT {
        assert!(!NAMES.contains(&name), "{name}");
    }
    for name in ["binding", "length"] {
        assert!(!NAMES.contains(&name), "{name}");
    }
    assert_eq!(executable::KINDS.len(), 10);
}

#[test]
fn the_committed_names_file_holds_the_names_of_this_file_and_none_left_out() {
    let file = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/names.txt"),
    )
    .unwrap();
    let lines: BTreeSet<&str> = file.lines().collect();
    for name in NAMES {
        assert!(lines.contains(name), "{name}");
    }
    for name in [
        "tier", "amount", "week", "month", "role", "binding", "length",
    ] {
        assert!(!lines.contains(name), "{name}");
    }
    for name in ERROR_NAMES {
        assert_eq!(lines.contains(name), NAMES.contains(name), "{name}");
    }
}
