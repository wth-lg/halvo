use halvo_coord::types::{
    Action, AgentState, ArgType, Autonomy, BoundFrom, CandidateFrom, Category, Cause, Choice,
    Class, Control, EffectAnswer, Failure, FieldFrom, HandbackReason, HandbackResult, HoldSetCause,
    ItemKind, LapseCause, LineKey, LivePhase, Mode, Outcome, Param, Period, Permission, Presenter,
    Reasoning, RunEnd, Runtime, Status, Tier, Trigger, WorkflowState, WorkflowTrigger, YesNo,
};

use halvo_coord::types::{
    Answered, ApprovalId, CheckAnswer, DecisionEntry, Digest, Doing, DoneAnswer, EffectId, ItemId,
    Lapsed, LinkAnswer, Live, Origin, OtherLapse, OwnItem, OwnItemKind, Raised, ReadyAnswer, RunId,
    RunItem, RunItemKind, SetupAnswer, SignInAnswer,
};

fn names<T: Copy>(all: &[T], name: fn(T) -> &'static str) -> Vec<&'static str> {
    all.iter().map(|v| name(*v)).collect()
}

#[test]
fn cause_names_are_the_twelve_of_i8_1() {
    assert_eq!(
        names(Cause::ALL, Cause::name),
        [
            "stale",
            "presenter",
            "early",
            "hold",
            "state",
            "budget",
            "queue_full",
            "busy",
            "no_service",
            "unknown_item",
            "unknown_agent",
            "malformed",
        ]
    );
}

#[test]
fn status_names_are_the_seven_of_i1_2() {
    assert_eq!(
        names(Status::ALL, Status::name),
        [
            "needs_you",
            "problem",
            "working",
            "waiting_turn",
            "all_set",
            "off",
            "not_yet",
        ]
    );
}

#[test]
fn item_kind_names_are_the_eight_of_i3_1() {
    assert_eq!(
        names(ItemKind::ALL, ItemKind::name),
        [
            "confirm",
            "sign_in",
            "link_account",
            "stuck",
            "check",
            "setup",
            "ready",
            "show",
        ]
    );
}

#[test]
fn choice_names_are_those_of_i3_1_and_present_of_i3_4() {
    assert_eq!(
        names(Choice::ALL, Choice::name),
        [
            "reject",
            "allow",
            "decline",
            "done",
            "try_again",
            "went_through",
            "none",
            "choose",
            "skip",
            "start",
            "present",
        ]
    );
}

#[test]
fn control_names_are_the_five_of_i2_5() {
    assert_eq!(
        names(Control::ALL, Control::name),
        ["run_now", "stop", "switch_off", "switch_on", "delete"]
    );
}

#[test]
fn action_names_are_the_four_of_agents_control_in_i8_1() {
    assert_eq!(
        names(Action::ALL, Action::name),
        ["run_now", "stop", "switch_off", "switch_on"]
    );
}

#[test]
fn tier_names_are_those_of_i2_1() {
    assert_eq!(
        names(Tier::ALL, Tier::name),
        ["on_device", "low_cost_hosted", "frontier"]
    );
}

#[test]
fn reasoning_names_are_those_of_i2_1() {
    assert_eq!(
        names(Reasoning::ALL, Reasoning::name),
        ["low", "medium", "high"]
    );
}

#[test]
fn autonomy_names_are_those_of_i2_1() {
    assert_eq!(
        names(Autonomy::ALL, Autonomy::name),
        ["observe", "draft_and_confirm", "unattended"]
    );
}

#[test]
fn runtime_names_are_those_of_i2_1() {
    assert_eq!(
        names(Runtime::ALL, Runtime::name),
        ["web", "graph_mail", "graph_calendar", "file", "none"]
    );
}

#[test]
fn permission_names_are_those_of_i2_1() {
    assert_eq!(names(Permission::ALL, Permission::name), ["user_files"]);
}

#[test]
fn period_names_are_those_of_a_a2_1() {
    assert_eq!(names(Period::ALL, Period::name), ["week", "month"]);
}

#[test]
fn category_names_are_the_ten_of_i2_3() {
    assert_eq!(
        names(Category::ALL, Category::name),
        [
            "utility_bill_payment",
            "tenant_rent",
            "lease_renewal_or_change",
            "tenant_application_review",
            "email",
            "spreadsheet_fill",
            "shop_order",
            "calendar_cleanup",
            "ride_order",
            "daily_chart_check",
        ]
    );
}

#[test]
fn class_names_are_the_ten_of_i4_1() {
    assert_eq!(
        names(Class::ALL, Class::name),
        [
            "read",
            "prepare",
            "local_edit",
            "send",
            "delete",
            "submit",
            "purchase",
            "payment",
            "sign",
            "credential",
        ]
    );
}

#[test]
fn bound_from_names_are_the_sources_of_i5_10() {
    assert_eq!(
        names(BoundFrom::ALL, BoundFrom::name),
        ["const", "slot", "extract", "model"]
    );
}

#[test]
fn field_from_names_are_those_of_i3_4() {
    assert_eq!(
        names(FieldFrom::ALL, FieldFrom::name),
        ["you", "site", "agent", "halvo"]
    );
}

#[test]
fn candidate_from_names_are_those_of_i3_5() {
    assert_eq!(
        names(CandidateFrom::ALL, CandidateFrom::name),
        ["records", "site", "files"]
    );
}

#[test]
fn presenter_names_are_those_of_i3_4() {
    assert_eq!(names(Presenter::ALL, Presenter::name), ["page", "native"]);
}

#[test]
fn yes_no_names_are_those_of_i0_4() {
    assert_eq!(names(YesNo::ALL, YesNo::name), ["yes", "no"]);
}

#[test]
fn handback_reason_names_are_the_six_of_i3_1() {
    assert_eq!(
        names(HandbackReason::ALL, HandbackReason::name),
        [
            "sign_in",
            "second_factor",
            "captcha",
            "account_link",
            "step_failed",
            "effect_unknown",
        ]
    );
}

#[test]
fn handback_result_names_are_the_three_of_i3_1() {
    assert_eq!(
        names(HandbackResult::ALL, HandbackResult::name),
        ["done", "declined", "timeout"]
    );
}

#[test]
fn outcome_names_are_the_nine_of_i5_6() {
    assert_eq!(
        names(Outcome::ALL, Outcome::name),
        [
            "completed",
            "nothing_to_do",
            "declined",
            "handed_back",
            "class_mismatch",
            "refused",
            "limit",
            "effect_unknown",
            "failed",
        ]
    );
}

#[test]
fn run_end_names_are_the_fifteen_of_i5_6() {
    assert_eq!(
        names(RunEnd::ALL, RunEnd::name),
        [
            "completed",
            "nothing_to_do",
            "declined",
            "handed_back",
            "class_mismatch",
            "refused",
            "limit",
            "effect_unknown",
            "failed",
            "cancelled",
            "lapsed",
            "interrupted",
            "deleted",
            "budget",
            "kind",
        ]
    );
}

#[test]
fn failure_names_are_the_failure_class_of_i5_6() {
    assert_eq!(
        names(Failure::ALL, Failure::name),
        [
            "failed",
            "limit",
            "class_mismatch",
            "refused",
            "budget",
            "kind",
        ]
    );
}

#[test]
fn live_phase_names_are_the_live_phases_of_i5_6() {
    assert_eq!(
        names(LivePhase::ALL, LivePhase::name),
        ["starting", "working", "waiting", "stopping"]
    );
}

#[test]
fn mode_names_are_those_of_i5_10() {
    assert_eq!(names(Mode::ALL, Mode::name), ["compile", "replay"]);
}

#[test]
fn trigger_names_are_those_of_i7_1() {
    assert_eq!(names(Trigger::ALL, Trigger::name), ["person", "schedule"]);
}

#[test]
fn workflow_trigger_names_are_those_of_i5_2() {
    assert_eq!(
        names(WorkflowTrigger::ALL, WorkflowTrigger::name),
        ["app_context", "schedule", "manual"]
    );
}

#[test]
fn workflow_state_names_are_the_four_of_i5_10() {
    assert_eq!(
        names(WorkflowState::ALL, WorkflowState::name),
        ["proposed", "approved", "active", "paused"]
    );
}

#[test]
fn agent_state_names_are_those_of_i6_1() {
    assert_eq!(
        names(AgentState::ALL, AgentState::name),
        ["approved", "active", "paused"]
    );
}

#[test]
fn effect_answer_names_are_those_of_i5_10() {
    assert_eq!(
        names(EffectAnswer::ALL, EffectAnswer::name),
        ["allow_once", "reject_once"]
    );
}

#[test]
fn lapse_cause_names_are_the_six_of_i4_2() {
    assert_eq!(
        names(LapseCause::ALL, LapseCause::name),
        [
            "digest",
            "run_ended",
            "wait_limit",
            "cancelled",
            "no_record",
            "app_ended",
        ]
    );
}

#[test]
fn hold_set_cause_names_are_those_of_i7_1() {
    assert_eq!(
        names(HoldSetCause::ALL, HoldSetCause::name),
        ["person", "failures"]
    );
}

#[test]
fn line_key_names_are_the_six_of_i2_2() {
    assert_eq!(
        names(LineKey::ALL, LineKey::name),
        ["level", "runs_on", "asks", "time", "files", "spend"]
    );
}

#[test]
fn arg_type_names_are_those_of_i8_3() {
    assert_eq!(
        names(ArgType::ALL, ArgType::name),
        ["count", "id", "name", "text"]
    );
}

#[test]
fn param_names_are_the_nine_of_i3_5() {
    assert_eq!(
        names(Param::ALL, Param::name),
        [
            "start",
            "payee",
            "instrument",
            "reply_senders",
            "workbook",
            "sheet",
            "range",
            "source",
            "seller",
        ]
    );
}

#[test]
fn own_item_kind_names_are_the_kinds_that_no_run_raises_of_i3_1() {
    assert_eq!(
        names(OwnItemKind::ALL, OwnItemKind::name),
        ["check", "setup", "ready"]
    );
    for k in OwnItemKind::ALL {
        assert_eq!(k.kind().name(), k.name());
    }
}

#[test]
fn run_item_kind_names_are_the_kinds_that_a_run_raises_of_i3_1() {
    assert_eq!(
        names(RunItemKind::ALL, RunItemKind::name),
        [
            "confirm",
            "sign_in",
            "link_account",
            "stuck",
            "check",
            "show"
        ]
    );
    for k in RunItemKind::ALL {
        assert_eq!(k.kind().name(), k.name());
    }
}

#[test]
fn other_lapse_names_are_the_causes_of_an_item_of_another_kind_of_i7_1() {
    assert_eq!(
        names(OtherLapse::ALL, OtherLapse::name),
        ["run_ended", "app_ended", "wait_limit", "cancelled"]
    );
    for c in OtherLapse::ALL {
        assert_eq!(c.cause().name(), c.name());
    }
}

#[test]
fn done_answer_names_are_the_choices_of_sign_in_link_account_stuck_and_show_of_i3_1() {
    assert_eq!(
        names(DoneAnswer::ALL, DoneAnswer::name),
        ["decline", "done"]
    );
    for a in DoneAnswer::ALL {
        assert_eq!(a.choice().name(), a.name());
    }
}

#[test]
fn check_answer_names_are_the_choices_of_check_of_i3_1() {
    assert_eq!(
        names(CheckAnswer::ALL, CheckAnswer::name),
        ["try_again", "went_through"]
    );
    for a in CheckAnswer::ALL {
        assert_eq!(a.choice().name(), a.name());
    }
}

#[test]
fn setup_answer_names_are_the_choices_of_setup_of_i3_1() {
    assert_eq!(
        names(SetupAnswer::ALL, SetupAnswer::name),
        ["none", "choose"]
    );
    for a in SetupAnswer::ALL {
        assert_eq!(a.choice().name(), a.name());
    }
}

#[test]
fn ready_answer_names_are_the_choices_of_ready_of_i3_1() {
    assert_eq!(
        names(ReadyAnswer::ALL, ReadyAnswer::name),
        ["skip", "start"]
    );
    for a in ReadyAnswer::ALL {
        assert_eq!(a.choice().name(), a.name());
    }
}

const EIGHT_KINDS_OF_I3_1: [&str; 8] = [
    "confirm",
    "sign_in",
    "link_account",
    "stuck",
    "check",
    "setup",
    "ready",
    "show",
];

fn run() -> RunId {
    RunId::parse(&"4".repeat(32)).unwrap()
}

fn answered_kind(a: &Answered) -> &'static str {
    match a {
        Answered::Confirm(_) => "confirm",
        Answered::SignIn(_) => "sign_in",
        Answered::LinkAccount(_) => "link_account",
        Answered::Stuck(_) => "stuck",
        Answered::Check(_) => "check",
        Answered::Setup(_) => "setup",
        Answered::Ready(_) => "ready",
        Answered::Show(_) => "show",
    }
}

#[test]
fn answered_has_one_member_for_each_of_the_eight_kinds_of_i3_1() {
    let entry = DecisionEntry {
        approval: ApprovalId::parse(&"9".repeat(32)).unwrap(),
        effect: EffectId::parse(&"8".repeat(32)).unwrap(),
        digest: Digest::parse(&"a".repeat(64)).unwrap(),
        answer: EffectAnswer::AllowOnce,
    };
    let all = [
        Answered::Confirm(entry),
        Answered::SignIn(SignInAnswer::First(DoneAnswer::Done)),
        Answered::LinkAccount(LinkAnswer {
            answer: DoneAnswer::Done,
            origin: Origin::parse("https://site2.example:443").unwrap(),
        }),
        Answered::Stuck(DoneAnswer::Done),
        Answered::Check(CheckAnswer::TryAgain),
        Answered::Setup(SetupAnswer::Choose),
        Answered::Ready(ReadyAnswer::Start),
        Answered::Show(DoneAnswer::Done),
    ];
    let got: Vec<&str> = all.iter().map(answered_kind).collect();
    assert_eq!(got, EIGHT_KINDS_OF_I3_1);
    for a in &all {
        assert_eq!(a.kind().name(), answered_kind(a));
    }
}

fn raised_kind(r: &Raised) -> &'static str {
    match r {
        Raised::Confirm(_) => "confirm",
        Raised::SignIn(_) => "sign_in",
        Raised::LinkAccount(_) => "link_account",
        Raised::Stuck(_) => "stuck",
        Raised::Check(_) => "check",
        Raised::Setup => "setup",
        Raised::Ready => "ready",
        Raised::Show(_) => "show",
    }
}

#[test]
fn raised_has_one_member_for_each_of_the_eight_kinds_of_i3_1() {
    let all = [
        Raised::Confirm(run()),
        Raised::SignIn(run()),
        Raised::LinkAccount(run()),
        Raised::Stuck(run()),
        Raised::Check(None),
        Raised::Setup,
        Raised::Ready,
        Raised::Show(run()),
    ];
    let got: Vec<&str> = all.iter().map(raised_kind).collect();
    assert_eq!(got, EIGHT_KINDS_OF_I3_1);
    for r in &all {
        assert_eq!(r.kind().name(), raised_kind(r));
    }
}

fn lapsed_kind(l: &Lapsed) -> &'static str {
    match l {
        Lapsed::Confirm(_) => "confirm",
        Lapsed::SignIn(_) => "sign_in",
        Lapsed::LinkAccount(_) => "link_account",
        Lapsed::Stuck(_) => "stuck",
        Lapsed::Check(_) => "check",
        Lapsed::Setup(_) => "setup",
        Lapsed::Ready(_) => "ready",
        Lapsed::Show(_) => "show",
    }
}

#[test]
fn lapsed_has_one_member_for_each_of_the_eight_kinds_of_i3_1() {
    let all = [
        Lapsed::Confirm(LapseCause::Digest),
        Lapsed::SignIn(OtherLapse::RunEnded),
        Lapsed::LinkAccount(OtherLapse::RunEnded),
        Lapsed::Stuck(OtherLapse::RunEnded),
        Lapsed::Check(OtherLapse::RunEnded),
        Lapsed::Setup(OtherLapse::RunEnded),
        Lapsed::Ready(OtherLapse::RunEnded),
        Lapsed::Show(OtherLapse::RunEnded),
    ];
    let got: Vec<&str> = all.iter().map(lapsed_kind).collect();
    assert_eq!(got, EIGHT_KINDS_OF_I3_1);
    for l in &all {
        assert_eq!(l.kind().name(), lapsed_kind(l));
    }
}

fn live_phase(l: &Live) -> &'static str {
    match l {
        Live::Starting => "starting",
        Live::Working => "working",
        Live::Waiting(_) => "waiting",
        Live::Stopping => "stopping",
    }
}

#[test]
fn live_has_the_four_phases_of_a_live_run_of_i5_6() {
    let item = RunItem {
        item: ItemId::parse(&"3".repeat(32)).unwrap(),
        kind: RunItemKind::Confirm,
    };
    let all = [
        Live::Starting,
        Live::Working,
        Live::Waiting(item),
        Live::Stopping,
    ];
    let got: Vec<&str> = all.iter().map(live_phase).collect();
    assert_eq!(got, ["starting", "working", "waiting", "stopping"]);
    for l in &all {
        assert_eq!(l.phase().name(), live_phase(l));
    }
}

fn doing_state(d: &Doing) -> &'static str {
    match d {
        Doing::Idle(_) => "idle",
        Doing::Queued(_) => "queued",
        Doing::Live(_) => "live",
    }
}

#[test]
fn doing_is_idle_queued_or_live_in_a_phase_of_i5_1() {
    let ready = OwnItem {
        item: ItemId::parse(&"3".repeat(32)).unwrap(),
        kind: OwnItemKind::Ready,
    };
    let all = [
        Doing::Idle(None),
        Doing::Queued(Some(ready)),
        Doing::Live(Live::Working),
    ];
    let got: Vec<&str> = all.iter().map(doing_state).collect();
    assert_eq!(got, ["idle", "queued", "live"]);
}
