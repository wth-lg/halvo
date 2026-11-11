use halvo_coord::types::{
    Action, AgentState, ArgType, Autonomy, BoundFrom, CandidateFrom, Category, Cause, Choice,
    Class, Control, EffectAnswer, Failure, FieldFrom, HandbackReason, HandbackResult, HoldSetCause,
    ItemKind, LapseCause, LineKey, LivePhase, Mode, Outcome, Param, Period, Permission, Presenter,
    Reasoning, RunEnd, Runtime, Status, Tier, Trigger, WorkflowState, WorkflowTrigger, YesNo,
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
