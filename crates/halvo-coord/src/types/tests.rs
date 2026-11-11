use super::*;

#[test]
fn ids_parse_32_lower_hex_and_nothing_else() {
    let good = "0123456789abcdef0123456789abcdef";
    assert_eq!(AgentId::parse(good).unwrap().as_str(), good);
    assert_eq!(RunId::parse(good).unwrap().to_string(), good);
    for bad in [
        "",
        "0123456789abcdef0123456789abcde",
        "0123456789abcdef0123456789abcdef0",
        "0123456789ABCDEF0123456789abcdef",
        "0123456789abcdef0123456789abcdeg",
        " 123456789abcdef0123456789abcdef",
    ] {
        assert_eq!(AgentId::parse(bad), Err(Cause::Malformed), "{bad:?}");
        assert_eq!(WorkflowId::parse(bad), Err(Cause::Malformed), "{bad:?}");
        assert_eq!(RunId::parse(bad), Err(Cause::Malformed), "{bad:?}");
        assert_eq!(EffectId::parse(bad), Err(Cause::Malformed), "{bad:?}");
        assert_eq!(ApprovalId::parse(bad), Err(Cause::Malformed), "{bad:?}");
        assert_eq!(ItemId::parse(bad), Err(Cause::Malformed), "{bad:?}");
    }
}

#[test]
fn hex_of_counted_bytes_gives_the_first_fixture_id() {
    let mut one = [0u8; 16];
    one[15] = 1;
    assert_eq!(hex(one), "00000000000000000000000000000001");
    let mut big = [0u8; 16];
    big[0] = 0xab;
    big[15] = 0x14;
    assert_eq!(hex(big), "ab000000000000000000000000000014");
    assert!(ItemId::parse(&hex(big)).is_ok());
}

#[test]
fn ids_serialize_as_their_string() {
    let id = ItemId::parse("00000000000000000000000000000001").unwrap();
    assert_eq!(
        serde_json::to_string(&id).unwrap(),
        "\"00000000000000000000000000000001\""
    );
}

#[test]
fn digest_is_64_lower_hex() {
    let good = "a".repeat(64);
    assert_eq!(Digest::parse(&good).unwrap().as_str(), good);
    assert_eq!(Digest::parse(&"a".repeat(63)), Err(Cause::Malformed));
    assert_eq!(Digest::parse(&"A".repeat(64)), Err(Cause::Malformed));
}

#[test]
fn names_follow_i0_6() {
    assert!(Name::parse("a").is_ok());
    assert!(Name::parse("reply_senders").is_ok());
    assert!(Name::parse(&"z".repeat(32)).is_ok());
    for bad in ["", "A", "a-b", "a b", "é", &"z".repeat(33)] {
        assert_eq!(Name::parse(bad), Err(Cause::Malformed), "{bad:?}");
    }
}

#[test]
fn origins_are_lower_case_with_the_port_written() {
    assert!(Origin::parse("https://pay.site1.example:443").is_ok());
    assert!(Origin::parse("http://127.0.0.1:8080").is_ok());
    for bad in [
        "https://pay.site1.example",
        "HTTPS://pay.site1.example:443",
        "https://Pay.site1.example:443",
        "https://pay.site1.example:443/",
        "https://:443",
        "pay.site1.example:443",
        "https://pay.site1.example:",
        "https://pay.site1.example:x1",
        "https://pay.site1.example:65536",
    ] {
        assert_eq!(Origin::parse(bad), Err(Cause::Malformed), "{bad:?}");
    }
}

#[test]
fn closed_lists_have_the_contract_names_in_order() {
    let names = |all: &[&str]| all.join(",");
    assert_eq!(
        names(&Cause::ALL.iter().map(|c| c.name()).collect::<Vec<_>>()),
        "stale,presenter,early,hold,state,budget,queue_full,busy,no_service,unknown_item,unknown_agent,malformed"
    );
    assert_eq!(
        names(&Status::ALL.iter().map(|c| c.name()).collect::<Vec<_>>()),
        "needs_you,problem,working,waiting_turn,all_set,off,not_yet"
    );
    assert_eq!(
        names(&ItemKind::ALL.iter().map(|c| c.name()).collect::<Vec<_>>()),
        "confirm,sign_in,link_account,stuck,check,setup,ready,show"
    );
    assert_eq!(
        names(&RunEnd::ALL.iter().map(|c| c.name()).collect::<Vec<_>>()),
        "completed,nothing_to_do,declined,handed_back,class_mismatch,refused,limit,effect_unknown,failed,cancelled,lapsed,interrupted,deleted,budget,kind"
    );
    assert_eq!(
        names(&Outcome::ALL.iter().map(|c| c.name()).collect::<Vec<_>>()),
        "completed,nothing_to_do,declined,handed_back,class_mismatch,refused,limit,effect_unknown,failed"
    );
    assert_eq!(
        names(&Category::ALL.iter().map(|c| c.name()).collect::<Vec<_>>()),
        "utility_bill_payment,tenant_rent,lease_renewal_or_change,tenant_application_review,email,spreadsheet_fill,shop_order,calendar_cleanup,ride_order,daily_chart_check"
    );
    assert_eq!(
        names(&Class::ALL.iter().map(|c| c.name()).collect::<Vec<_>>()),
        "read,prepare,local_edit,send,delete,submit,purchase,payment,sign,credential"
    );
    assert_eq!(
        names(
            &WorkflowState::ALL
                .iter()
                .map(|c| c.name())
                .collect::<Vec<_>>()
        ),
        "proposed,approved,active,paused"
    );
    assert_eq!(
        names(&Param::ALL.iter().map(|c| c.name()).collect::<Vec<_>>()),
        "start,payee,instrument,reply_senders,workbook,sheet,range,source,seller"
    );
    assert_eq!(
        names(&LapseCause::ALL.iter().map(|c| c.name()).collect::<Vec<_>>()),
        "digest,run_ended,wait_limit,cancelled,no_record,app_ended"
    );
}

#[test]
fn every_closed_list_parses_its_names_and_refuses_others() {
    fn round<T: Copy + PartialEq + std::fmt::Debug>(
        all: &[T],
        name: fn(T) -> &'static str,
        parse: fn(&str) -> Result<T, Cause>,
    ) {
        for &v in all {
            assert_eq!(parse(name(v)), Ok(v));
            assert!(
                Name::parse(name(v)).is_ok(),
                "{} is no name of I0.6",
                name(v)
            );
        }
        assert_eq!(parse("nope_not_a_value"), Err(Cause::Malformed));
        assert_eq!(parse(""), Err(Cause::Malformed));
    }
    round(Cause::ALL, Cause::name, Cause::parse);
    round(Status::ALL, Status::name, Status::parse);
    round(ItemKind::ALL, ItemKind::name, ItemKind::parse);
    round(Choice::ALL, Choice::name, Choice::parse);
    round(Control::ALL, Control::name, Control::parse);
    round(Action::ALL, Action::name, Action::parse);
    round(Tier::ALL, Tier::name, Tier::parse);
    round(Reasoning::ALL, Reasoning::name, Reasoning::parse);
    round(Autonomy::ALL, Autonomy::name, Autonomy::parse);
    round(Runtime::ALL, Runtime::name, Runtime::parse);
    round(Permission::ALL, Permission::name, Permission::parse);
    round(Period::ALL, Period::name, Period::parse);
    round(Category::ALL, Category::name, Category::parse);
    round(Class::ALL, Class::name, Class::parse);
    round(BoundFrom::ALL, BoundFrom::name, BoundFrom::parse);
    round(FieldFrom::ALL, FieldFrom::name, FieldFrom::parse);
    round(
        CandidateFrom::ALL,
        CandidateFrom::name,
        CandidateFrom::parse,
    );
    round(Presenter::ALL, Presenter::name, Presenter::parse);
    round(YesNo::ALL, YesNo::name, YesNo::parse);
    round(
        HandbackReason::ALL,
        HandbackReason::name,
        HandbackReason::parse,
    );
    round(
        HandbackResult::ALL,
        HandbackResult::name,
        HandbackResult::parse,
    );
    round(Outcome::ALL, Outcome::name, Outcome::parse);
    round(RunEnd::ALL, RunEnd::name, RunEnd::parse);
    round(Failure::ALL, Failure::name, Failure::parse);
    round(LivePhase::ALL, LivePhase::name, LivePhase::parse);
    round(Mode::ALL, Mode::name, Mode::parse);
    round(Trigger::ALL, Trigger::name, Trigger::parse);
    round(
        WorkflowTrigger::ALL,
        WorkflowTrigger::name,
        WorkflowTrigger::parse,
    );
    round(
        WorkflowState::ALL,
        WorkflowState::name,
        WorkflowState::parse,
    );
    round(AgentState::ALL, AgentState::name, AgentState::parse);
    round(EffectAnswer::ALL, EffectAnswer::name, EffectAnswer::parse);
    round(LapseCause::ALL, LapseCause::name, LapseCause::parse);
    round(HoldSetCause::ALL, HoldSetCause::name, HoldSetCause::parse);
    round(LineKey::ALL, LineKey::name, LineKey::parse);
    round(ArgType::ALL, ArgType::name, ArgType::parse);
    round(Param::ALL, Param::name, Param::parse);
}

#[test]
fn the_failure_class_is_the_six_ends_of_i5_6() {
    let failures: Vec<&str> = RunEnd::ALL
        .iter()
        .filter_map(|e| e.failure())
        .map(Failure::name)
        .collect();
    assert_eq!(
        failures.join(","),
        "class_mismatch,refused,limit,failed,budget,kind"
    );
    for f in Failure::ALL {
        assert_eq!(RunEnd::from(*f).failure(), Some(*f));
    }
    for o in Outcome::ALL {
        assert_eq!(RunEnd::from(*o).name(), o.name());
    }
}

#[test]
fn an_answered_row_pairs_each_kind_with_its_own_choices_of_i3_1() {
    let rows = [
        Answered::SignIn(DoneAnswer::Decline),
        Answered::SignIn(DoneAnswer::Done),
        Answered::LinkAccount(DoneAnswer::Decline),
        Answered::LinkAccount(DoneAnswer::Done),
        Answered::Stuck(DoneAnswer::Decline),
        Answered::Stuck(DoneAnswer::Done),
        Answered::Check(CheckAnswer::TryAgain),
        Answered::Check(CheckAnswer::WentThrough),
        Answered::Setup(SetupAnswer::None),
        Answered::Setup(SetupAnswer::Choose),
        Answered::Ready(ReadyAnswer::Skip),
        Answered::Ready(ReadyAnswer::Start),
        Answered::Show(DoneAnswer::Decline),
        Answered::Show(DoneAnswer::Done),
    ];
    let got: Vec<String> = rows
        .iter()
        .map(|r| format!("{}.{}", r.kind().name(), r.choice().unwrap().name()))
        .collect();
    assert_eq!(
        got,
        [
            "sign_in.decline",
            "sign_in.done",
            "link_account.decline",
            "link_account.done",
            "stuck.decline",
            "stuck.done",
            "check.try_again",
            "check.went_through",
            "setup.none",
            "setup.choose",
            "ready.skip",
            "ready.start",
            "show.decline",
            "show.done",
        ]
    );
    let entry = DecisionEntry {
        approval: ApprovalId::parse(&"9".repeat(32)).unwrap(),
        effect: EffectId::parse(&"8".repeat(32)).unwrap(),
        digest: Digest::parse(&"a".repeat(64)).unwrap(),
        answer: EffectAnswer::AllowOnce,
    };
    assert_eq!(Answered::Confirm(entry.clone()).kind(), ItemKind::Confirm);
    assert_eq!(Answered::Confirm(entry).choice(), None);
}

#[test]
fn names_serialize_as_strings() {
    assert_eq!(
        serde_json::to_string(&Status::NeedsYou).unwrap(),
        "\"needs_you\""
    );
    assert_eq!(serde_json::to_string(&YesNo::Yes).unwrap(), "\"yes\"");
    assert_eq!(
        serde_json::to_string(&Cause::QueueFull).unwrap(),
        "\"queue_full\""
    );
}

#[test]
fn product_limits_are_the_values_of_i0_10() {
    let l = Limits::PRODUCT;
    assert_eq!(l.max_live_runs, 1);
    assert_eq!(l.queue_max, 4);
    assert_eq!(l.wait_limit_s, 900);
    assert_eq!(l.arm_ms, 500);
    assert_eq!(l.cancel_grace_s, 5);
    assert_eq!(l.starts_per_day, 6);
    assert_eq!(l.items_per_run, 8);
    assert_eq!(l.fails_to_pause, 3);
    assert_eq!(l.catch_up_days, 7);
    assert_eq!(l.next_wait_s, 25);
    assert_eq!(l.coalesce_ms, 250);
    assert_eq!(l.owner_wait_s, 5);
    assert_eq!(l.roster_max, 256);
    assert_eq!(l.request_records, 8);
    assert_eq!(l.request_bytes, 4096);
    assert_eq!(l.history_runs, 5);
    assert_eq!(l.setup_params, 8);
    assert_eq!(l.setup_candidates, 8);
}

const PRODUCT_JSON: &str = "{\"max_live_runs\":1,\"queue_max\":4,\"wait_limit_s\":900,\"arm_ms\":500,\"cancel_grace_s\":5,\"starts_per_day\":6,\"items_per_run\":8,\"fails_to_pause\":3,\"catch_up_days\":7,\"next_wait_s\":25,\"coalesce_ms\":250,\"owner_wait_s\":5,\"roster_max\":256,\"request_records\":8,\"request_bytes\":4096,\"history_runs\":5,\"setup_params\":8,\"setup_candidates\":8}";

#[test]
fn limits_write_and_read_the_field_names_of_i0_10() {
    assert_eq!(
        serde_json::to_string(&Limits::PRODUCT).unwrap(),
        PRODUCT_JSON
    );
    assert_eq!(Limits::from_json(PRODUCT_JSON), Ok(Limits::PRODUCT));
    let model = "{\"max_live_runs\":2,\"queue_max\":2,\"wait_limit_s\":900,\"arm_ms\":500,\"cancel_grace_s\":5,\"starts_per_day\":2,\"items_per_run\":8,\"fails_to_pause\":2,\"catch_up_days\":0,\"next_wait_s\":25,\"coalesce_ms\":250,\"owner_wait_s\":5,\"roster_max\":256,\"request_records\":8,\"request_bytes\":4096,\"history_runs\":5,\"setup_params\":8,\"setup_candidates\":8}";
    let l = Limits::from_json(model).unwrap();
    assert_eq!((l.max_live_runs, l.queue_max, l.catch_up_days), (2, 2, 0));
}

#[test]
fn limits_refuse_a_doubled_key() {
    for bad in [
        PRODUCT_JSON.replace("}", ",\"queue_max\":9}"),
        PRODUCT_JSON.replace("}", ",\"queue_max\":4}"),
        PRODUCT_JSON.replace("}", ",\"queue\\u005fmax\":4}"),
    ] {
        assert_eq!(Limits::from_json(&bad), Err(Cause::Malformed), "{bad}");
    }
}

#[test]
fn limits_refuse_an_unknown_key() {
    let bad = PRODUCT_JSON.replace("\"arm_ms\":500", "\"arm_ms\":500,\"arm_s\":1");
    assert_eq!(Limits::from_json(&bad), Err(Cause::Malformed));
    let renamed = PRODUCT_JSON.replace("\"arm_ms\"", "\"arm_s\"");
    assert_eq!(Limits::from_json(&renamed), Err(Cause::Malformed));
}

#[test]
fn limits_refuse_a_missing_key() {
    for field in [
        "\"max_live_runs\":1,",
        ",\"arm_ms\":500",
        ",\"setup_candidates\":8",
    ] {
        let bad = PRODUCT_JSON.replace(field, "");
        assert_eq!(Limits::from_json(&bad), Err(Cause::Malformed), "{bad}");
    }
}

#[test]
fn limits_refuse_every_other_shape() {
    let missing = PRODUCT_JSON.replace(",\"setup_candidates\":8", "");
    let extra = PRODUCT_JSON.replace("}", ",\"other\":1}");
    let negative = PRODUCT_JSON.replace("\"arm_ms\":500", "\"arm_ms\":-1");
    let fraction = PRODUCT_JSON.replace("\"arm_ms\":500", "\"arm_ms\":0.5");
    let text = PRODUCT_JSON.replace("\"arm_ms\":500", "\"arm_ms\":\"500\"");
    let wide = PRODUCT_JSON.replace("\"queue_max\":4", "\"queue_max\":4294967296");
    for bad in [
        "",
        "[]",
        "[1,4,900,500,5,6,8,3,7,25,250,5,256,8,4096,5,8,8]",
        "null",
        missing.as_str(),
        extra.as_str(),
        negative.as_str(),
        fraction.as_str(),
        text.as_str(),
        wide.as_str(),
    ] {
        assert_eq!(Limits::from_json(bad), Err(Cause::Malformed), "{bad}");
    }
}
