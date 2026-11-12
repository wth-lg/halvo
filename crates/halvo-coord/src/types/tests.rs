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

/// The inputs of the two test lists of `WebOrigin::parse` of the agent-setup run at b8e367e
/// (`crates/halvo-schema/tests/r3_atoms.rs`, `a_web_origin_is_scheme_host_and_port_in_lower_case`
/// and `a_host_holds_at_most_253_bytes`), each with that function's answer: taken or refused.
fn web_origin_cases() -> Vec<(String, bool)> {
    let taken = [
        "http://127.0.0.1:18090",
        "https://utility.test:443",
        "http://pay.utility.test:1",
        "https://a-b.example:65535",
    ];
    let refused = [
        "http://utility.test",
        "https://utility.test/",
        "http://utility.test:80/",
        "HTTP://utility.test:80",
        "http://Utility.test:80",
        "ftp://utility.test:21",
        "ws://utility.test:80",
        "http://utility.test:0",
        "http://utility.test:65536",
        "http://utility.test:080",
        "http://utility.test:+80",
        "http://-utility.test:80",
        "http://utility-.test:80",
        "http://utility..test:80",
        "http://.test:80",
        "http://[::1]:80",
        "http://user@utility.test:80",
        "http://utility.test:80 ",
        "http://utility.test:80?x",
        "http://:80",
        "utility.test:80",
        "",
    ];
    let labels = |last: usize| {
        format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(last)
        )
    };
    let mut cases: Vec<(String, bool)> = taken.iter().map(|s| (s.to_string(), true)).collect();
    cases.extend(refused.iter().map(|s| (s.to_string(), false)));
    cases.push((format!("http://{}.test:80", "a".repeat(64)), false));
    cases.push((format!("http://{}:80", labels(61)), true));
    cases.push((format!("http://{}:80", labels(62)), false));
    cases
}

#[test]
fn origin_parse_answers_every_input_as_web_origin_parse_of_the_agent_setup_run() {
    let cases = web_origin_cases();
    assert_eq!(cases.len(), 29);
    let mut differ = Vec::new();
    for (input, taken) in &cases {
        let got = Origin::parse(input);
        let agrees = match got {
            Ok(o) => *taken && o.as_str() == input,
            Err(cause) => !*taken && cause == Cause::Malformed,
        };
        if !agrees {
            differ.push(input.clone());
        }
    }
    assert!(differ.is_empty(), "Origin::parse differs on {differ:?}");
}

#[test]
fn an_origin_has_one_written_form() {
    for bad in [
        "https://pay.site1.example:0443",
        "https://pay.site1.example:00443",
        "http://127.0.0.1:08080",
        "https://pay.site1.example:00",
        "https://.:443",
        "https://-:443",
        "http://localhost:0",
    ] {
        assert_eq!(Origin::parse(bad), Err(Cause::Malformed), "{bad:?}");
    }
    for good in [
        "https://pay.site1.example:443",
        "http://127.0.0.1:8080",
        "http://localhost:1",
        "https://a-b.example:65535",
    ] {
        assert_eq!(Origin::parse(good).unwrap().as_str(), good);
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
    let linked = |answer| {
        Answered::LinkAccount(LinkAnswer {
            answer,
            origin: Origin::parse("https://mail.site2.example:443").unwrap(),
        })
    };
    let bound = |answer| {
        Answered::SignIn(SignInAnswer::Second(BindAnswer {
            answer,
            origin: Origin::parse("https://pay.site1.example:8443").unwrap(),
        }))
    };
    let rows = [
        Answered::SignIn(SignInAnswer::First(DoneAnswer::Decline)),
        Answered::SignIn(SignInAnswer::First(DoneAnswer::Done)),
        bound(BindResult::Decline),
        bound(BindResult::Allow),
        linked(DoneAnswer::Decline),
        linked(DoneAnswer::Done),
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
            "sign_in.decline",
            "sign_in.allow",
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
fn a_decision_is_never_empty_and_holds_one_entry_for_an_effect() {
    let entry = |effect: &str, approval: &str| DecisionEntry {
        approval: ApprovalId::parse(&approval.repeat(32)).unwrap(),
        effect: EffectId::parse(&effect.repeat(32)).unwrap(),
        digest: Digest::parse(&"a".repeat(64)).unwrap(),
        answer: EffectAnswer::AllowOnce,
    };
    assert_eq!(Decision::new(vec![]), Err(Cause::Malformed));
    assert_eq!(
        Decision::new(vec![entry("8", "1"), entry("8", "2")]),
        Err(Cause::Malformed)
    );
    let two = Decision::new(vec![entry("8", "1"), entry("7", "2")]).unwrap();
    assert_eq!(two.approvals().len(), 2);
}

#[test]
fn a_confirm_request_is_never_empty_and_holds_one_entry_for_an_effect() {
    let effect = |e: &str| AskedEffect {
        effect: EffectId::parse(&e.repeat(32)).unwrap(),
        class: Class::Send,
        digest: Digest::parse(&"a".repeat(64)).unwrap(),
        digest_ok: true,
        fields: vec![],
        new_origin: None,
    };
    let run = RunId::parse(&"4".repeat(32)).unwrap();
    let agent = AgentId::parse(&"2".repeat(32)).unwrap();
    let ask = |effects| ConfirmAsk::new(run.clone(), agent.clone(), effects);
    assert_eq!(ask(vec![]), Err(Cause::Malformed));
    assert_eq!(ask(vec![effect("8"), effect("8")]), Err(Cause::Malformed));
    let two = ask(vec![effect("8"), effect("7")]).unwrap();
    assert_eq!(two.effects().len(), 2);
    assert_eq!((two.run(), two.agent()), (&run, &agent));
}

#[test]
fn an_item_that_a_run_raises_names_its_run() {
    let run = RunId::parse(&"4".repeat(32)).unwrap();
    let rows = [
        Raised::Confirm(run.clone()),
        Raised::SignIn(run.clone()),
        Raised::LinkAccount(run.clone()),
        Raised::Stuck(run.clone()),
        Raised::Check(Some(run.clone())),
        Raised::Check(None),
        Raised::Setup,
        Raised::Ready,
        Raised::Show(run.clone()),
    ];
    let got: Vec<(&str, bool)> = rows
        .iter()
        .map(|r| (r.kind().name(), r.run() == Some(&run)))
        .collect();
    assert_eq!(
        got,
        [
            ("confirm", true),
            ("sign_in", true),
            ("link_account", true),
            ("stuck", true),
            ("check", true),
            ("check", false),
            ("setup", false),
            ("ready", false),
            ("show", true),
        ]
    );
}

#[test]
fn a_progress_has_its_step_at_most_its_total() {
    assert_eq!(Progress::new(4, 3), Err(Cause::Malformed));
    assert_eq!(Progress::new(u32::MAX, 0), Err(Cause::Malformed));
    let p = Progress::new(3, 3).unwrap();
    assert_eq!((p.step(), p.of()), (3, 3));
    assert!(Progress::new(1, 3).is_ok());
}

fn empty_field() -> Field {
    Field {
        label: String::new(),
        value: vec![],
        from: FieldFrom::Halvo,
        from_text: String::new(),
    }
}

#[test]
fn an_item_is_made_by_one_constructor_per_kind() {
    let id = || ItemId::parse(&"3".repeat(32)).unwrap();
    let agent = || AgentId::parse(&"2".repeat(32)).unwrap();
    let s = String::new;
    let no = YesNo::No;
    let items = [
        Item::confirm(id(), agent(), Presenter::Native, no, s(), vec![], vec![]),
        Item::sign_in(id(), agent(), s(), s(), vec![]),
        Item::link_account(id(), agent(), s(), s(), empty_field(), vec![]),
        Item::stuck(id(), agent(), s(), s(), vec![]),
        Item::check(id(), agent(), s(), s(), vec![], vec![]),
        Item::setup(id(), agent(), s(), s(), vec![], vec![]),
        Item::ready(id(), agent(), s(), s(), vec![]),
        Item::show(id(), agent(), s(), s(), vec![]),
    ];
    let got: Vec<(&str, Option<Presenter>)> = items
        .iter()
        .map(|i| (i.kind().name(), i.presenter()))
        .collect();
    assert_eq!(
        got,
        [
            ("confirm", Some(Presenter::Native)),
            ("sign_in", None),
            ("link_account", None),
            ("stuck", None),
            ("check", None),
            ("setup", None),
            ("ready", None),
            ("show", None),
        ]
    );
}

#[test]
fn the_reply_ok_holds_nothing_but_ok() {
    assert_eq!(serde_json::to_string(&OkReply).unwrap(), "{\"ok\":\"yes\"}");
}

#[test]
fn the_facts_hold_one_run_state_and_at_most_one_item_of_the_right_source() {
    let id = || ItemId::parse(&"3".repeat(32)).unwrap();
    let own = |kind| OwnItem { item: id(), kind };
    let run = |kind| RunItem { item: id(), kind };
    let all = [
        Doing::Idle(None),
        Doing::Idle(Some(own(OwnItemKind::Check))),
        Doing::Idle(Some(own(OwnItemKind::Setup))),
        Doing::Queued(None),
        Doing::Queued(Some(own(OwnItemKind::Ready))),
        Doing::Live(Live::Starting),
        Doing::Live(Live::Working),
        Doing::Live(Live::Waiting(run(RunItemKind::Confirm))),
        Doing::Live(Live::Waiting(run(RunItemKind::Check))),
        Doing::Live(Live::Stopping),
    ];
    let got: Vec<(Option<&str>, Option<&str>)> = all
        .iter()
        .map(|d| {
            (
                d.phase().map(LivePhase::name),
                d.item().map(|i| i.kind.name()),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (None, None),
            (None, Some("check")),
            (None, Some("setup")),
            (None, None),
            (None, Some("ready")),
            (Some("starting"), None),
            (Some("working"), None),
            (Some("waiting"), Some("confirm")),
            (Some("waiting"), Some("check")),
            (Some("stopping"), None),
        ]
    );
    let raised: Vec<&str> = [
        RunItemKind::Confirm,
        RunItemKind::SignIn,
        RunItemKind::LinkAccount,
        RunItemKind::Stuck,
        RunItemKind::Check,
        RunItemKind::Show,
    ]
    .iter()
    .map(|k| k.kind().name())
    .collect();
    assert_eq!(
        raised,
        [
            "confirm",
            "sign_in",
            "link_account",
            "stuck",
            "check",
            "show"
        ],
        "the kinds that a run raises, I3.1"
    );
    let own_kinds: Vec<&str> = [OwnItemKind::Check, OwnItemKind::Setup, OwnItemKind::Ready]
        .iter()
        .map(|k| k.kind().name())
        .collect();
    assert_eq!(
        own_kinds,
        ["check", "setup", "ready"],
        "the kinds that a hold or the scheduler raises, I3.1"
    );
}

#[test]
fn an_item_that_is_no_confirmation_is_not_armed() {
    let id = || ItemId::parse(&"3".repeat(32)).unwrap();
    let agent = || AgentId::parse(&"2".repeat(32)).unwrap();
    let s = String::new;
    let others = [
        Item::sign_in(id(), agent(), s(), s(), vec![]),
        Item::link_account(id(), agent(), s(), s(), empty_field(), vec![]),
        Item::stuck(id(), agent(), s(), s(), vec![]),
        Item::check(id(), agent(), s(), s(), vec![], vec![]),
        Item::setup(id(), agent(), s(), s(), vec![], vec![]),
        Item::ready(id(), agent(), s(), s(), vec![]),
        Item::show(id(), agent(), s(), s(), vec![]),
    ];
    assert!(others.iter().all(|i| i.armed() == YesNo::No));
    let page = Item::confirm(
        id(),
        agent(),
        Presenter::Page,
        YesNo::Yes,
        s(),
        vec![],
        vec![],
    );
    assert_eq!(page.armed(), YesNo::Yes);
}

#[test]
fn an_item_lapses_with_the_causes_of_its_kind() {
    let mut confirm: Vec<&str> = LapseCause::ALL
        .iter()
        .map(|c| Lapsed::Confirm(*c).cause().name())
        .collect();
    confirm.sort_unstable();
    assert_eq!(
        confirm,
        [
            "app_ended",
            "cancelled",
            "digest",
            "no_record",
            "run_ended",
            "wait_limit"
        ],
        "a confirmation lapses with the six causes of I4.2"
    );
    let other: Vec<&str> = [
        OtherLapse::RunEnded,
        OtherLapse::AppEnded,
        OtherLapse::WaitLimit,
    ]
    .iter()
    .map(|o| Lapsed::Ready(*o).cause().name())
    .collect();
    assert_eq!(
        other,
        ["run_ended", "app_ended", "wait_limit"],
        "an item of another kind lapses with these three, I7.1"
    );
    let kinds: Vec<&str> = [
        Lapsed::Confirm(LapseCause::Digest),
        Lapsed::SignIn(OtherLapse::RunEnded),
        Lapsed::LinkAccount(OtherLapse::RunEnded),
        Lapsed::Stuck(OtherLapse::RunEnded),
        Lapsed::Check(OtherLapse::RunEnded),
        Lapsed::Setup(OtherLapse::RunEnded),
        Lapsed::Ready(OtherLapse::RunEnded),
        Lapsed::Show(OtherLapse::RunEnded),
    ]
    .iter()
    .map(|l| l.kind().name())
    .collect();
    assert_eq!(
        kinds,
        [
            "confirm",
            "sign_in",
            "link_account",
            "stuck",
            "check",
            "setup",
            "ready",
            "show"
        ]
    );
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
