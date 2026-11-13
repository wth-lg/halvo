use std::num::NonZeroU32;

use halvo_schema::coordinator::names::{
    BindResult, CheckAnswer, ConfirmLapse, DoneAnswer, EffectAnswer, Event, HoldSetCause, ItemKind,
    OtherKind, OtherLapse, ReadyAnswer, RunEnd, Trigger,
};
use halvo_schema::coordinator::record::{
    Answered, BindAnswer, Cleared, CoordEvent, Entry, ItemId, Lapsed, LinkAnswer, Raised,
    SignInAnswer, What,
};
use halvo_schema::executable::{ApprovalId, Digest, EffectId, RunId, WebOrigin};
use halvo_schema::kinds::check_record;
use halvo_schema::record::Instant;
use halvo_schema::{AgentId, WorkflowId};

const WORKFLOW: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";
const AGENT: &str = "1a2b3c4d5e6f708192a3b4c5d6e7f809";
const RUN: &str = "9f8e7d6c5b4a39281706f5e4d3c2b1a0";
const ITEM: &str = "7e6d5c4b3a2918070f1e2d3c4b5a6978";
const ORIGIN: &str = "https://northwind-power.test:443";

const HEAD: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","seq":7,"day":20767,"at":1794339000000000000,"#;

const KEYS: [&str; 18] = [
    "v", "workflow", "agent", "seq", "day", "at", "event", "run", "slot", "item", "kind", "effect",
    "approval", "digest", "answer", "cause", "trigger", "origin",
];

fn run() -> RunId {
    RunId::parse(RUN).unwrap()
}

fn item() -> ItemId {
    ItemId::parse(ITEM).unwrap()
}

fn origin() -> WebOrigin {
    WebOrigin::parse(ORIGIN).unwrap()
}

fn entry(answer: EffectAnswer) -> Entry {
    Entry {
        effect: EffectId::parse("5c4b3a29180f7e6d5c4b3a29180f7e6d").unwrap(),
        approval: ApprovalId::parse("6d5c4b3a29180f7e6d5c4b3a29180f7e").unwrap(),
        digest: Digest::parse(&"ab".repeat(32)).unwrap(),
        answer,
    }
}

fn event(what: What) -> CoordEvent {
    CoordEvent {
        workflow: WorkflowId::parse(WORKFLOW).unwrap(),
        agent: AgentId::parse(AGENT).unwrap(),
        seq: NonZeroU32::new(7).unwrap(),
        day: 20767,
        at: Instant(1_794_339_000_000_000_000),
        what,
    }
}

fn line(event_name: &str, own: &[(&str, &str)]) -> String {
    let mut text = format!(r#"{HEAD}"event":"{event_name}""#);
    for key in &KEYS[7..] {
        let value = own
            .iter()
            .find(|(k, _)| k == key)
            .map_or("null", |(_, v)| *v);
        text.push_str(&format!(r#","{key}":{value}"#));
    }
    text.push('}');
    text
}

fn q(s: &str) -> String {
    format!("\"{s}\"")
}

fn every_shape() -> Vec<(What, String)> {
    let mut shapes = vec![
        (
            What::RunStarted {
                run: run(),
                trigger: Trigger::Schedule,
                slot: Some(20767),
            },
            line(
                "run_started",
                &[
                    ("run", &q(RUN)),
                    ("slot", "20767"),
                    ("trigger", &q("schedule")),
                ],
            ),
        ),
        (
            What::RunStarted {
                run: run(),
                trigger: Trigger::Person,
                slot: None,
            },
            line(
                "run_started",
                &[("run", &q(RUN)), ("trigger", &q("person"))],
            ),
        ),
        (
            What::HoldSet {
                cause: HoldSetCause::Person,
            },
            line("hold_set", &[("cause", &q("person"))]),
        ),
        (
            What::HoldSet {
                cause: HoldSetCause::Failures,
            },
            line("hold_set", &[("cause", &q("failures"))]),
        ),
        (
            What::HoldCleared {
                cleared: Cleared::Person,
            },
            line("hold_cleared", &[("cause", &q("person"))]),
        ),
        (
            What::HoldCleared {
                cleared: Cleared::Failures,
            },
            line("hold_cleared", &[("cause", &q("failures"))]),
        ),
        (
            What::SlotSkipped { slot: Some(20766) },
            line("slot_skipped", &[("slot", "20766")]),
        ),
        (What::SlotSkipped { slot: None }, line("slot_skipped", &[])),
    ];
    for answer in CheckAnswer::ALL {
        shapes.push((
            What::HoldCleared {
                cleared: Cleared::Check {
                    run: run(),
                    answer: *answer,
                },
            },
            line(
                "hold_cleared",
                &[
                    ("run", &q(RUN)),
                    ("answer", &q(answer.as_str())),
                    ("cause", &q("check")),
                ],
            ),
        ));
    }
    for cause in RunEnd::ALL {
        shapes.push((
            What::RunEnded {
                run: run(),
                cause: *cause,
            },
            line(
                "run_ended",
                &[("run", &q(RUN)), ("cause", &q(cause.as_str()))],
            ),
        ));
    }
    let raised = [
        (Raised::Confirm(run()), "confirm", true),
        (Raised::SignIn(run()), "sign_in", true),
        (Raised::LinkAccount(run()), "link_account", true),
        (Raised::Stuck(run()), "stuck", true),
        (Raised::Check(Some(run())), "check", true),
        (Raised::Check(None), "check", false),
        (Raised::Setup, "setup", false),
        (Raised::Ready, "ready", false),
        (Raised::Show(run()), "show", true),
    ];
    for (raised, kind, with_run) in raised {
        let mut own = vec![("item", q(ITEM)), ("kind", q(kind))];
        if with_run {
            own.push(("run", q(RUN)));
        }
        let own: Vec<(&str, &str)> = own.iter().map(|(k, v)| (*k, v.as_str())).collect();
        shapes.push((
            What::ItemRaised {
                item: item(),
                raised,
            },
            line("item_raised", &own),
        ));
    }
    for answer in EffectAnswer::ALL {
        let own = [
            ("item", q(ITEM)),
            ("kind", q("confirm")),
            ("effect", q("5c4b3a29180f7e6d5c4b3a29180f7e6d")),
            ("approval", q("6d5c4b3a29180f7e6d5c4b3a29180f7e")),
            ("digest", q(&"ab".repeat(32))),
            ("answer", q(answer.as_str())),
        ];
        let own: Vec<(&str, &str)> = own.iter().map(|(k, v)| (*k, v.as_str())).collect();
        shapes.push((
            What::ItemAnswered {
                item: item(),
                answered: Answered::Confirm(entry(*answer)),
            },
            line("item_answered", &own),
        ));
        shapes.push((
            What::ItemReleased {
                item: item(),
                entry: entry(*answer),
            },
            line("item_released", &own),
        ));
    }
    let answered: Vec<(Answered, &str, &str, bool)> = vec![
        (
            Answered::SignIn(SignInAnswer::First(DoneAnswer::Done)),
            "sign_in",
            "done",
            false,
        ),
        (
            Answered::SignIn(SignInAnswer::First(DoneAnswer::Decline)),
            "sign_in",
            "decline",
            false,
        ),
        (
            Answered::SignIn(SignInAnswer::Second(BindAnswer {
                answer: BindResult::Allow,
                origin: origin(),
            })),
            "sign_in",
            "allow",
            true,
        ),
        (
            Answered::SignIn(SignInAnswer::Second(BindAnswer {
                answer: BindResult::Decline,
                origin: origin(),
            })),
            "sign_in",
            "decline",
            true,
        ),
        (
            Answered::LinkAccount(LinkAnswer {
                answer: DoneAnswer::Done,
                origin: origin(),
            }),
            "link_account",
            "done",
            true,
        ),
        (
            Answered::LinkAccount(LinkAnswer {
                answer: DoneAnswer::Decline,
                origin: origin(),
            }),
            "link_account",
            "decline",
            true,
        ),
        (Answered::Stuck(DoneAnswer::Done), "stuck", "done", false),
        (
            Answered::Stuck(DoneAnswer::Decline),
            "stuck",
            "decline",
            false,
        ),
        (
            Answered::Check(CheckAnswer::TryAgain),
            "check",
            "try_again",
            false,
        ),
        (
            Answered::Check(CheckAnswer::WentThrough),
            "check",
            "went_through",
            false,
        ),
        (Answered::Setup, "setup", "choose", false),
        (Answered::Ready(ReadyAnswer::Skip), "ready", "skip", false),
        (Answered::Ready(ReadyAnswer::Start), "ready", "start", false),
        (Answered::Show(DoneAnswer::Done), "show", "done", false),
        (
            Answered::Show(DoneAnswer::Decline),
            "show",
            "decline",
            false,
        ),
    ];
    for (answered, kind, answer, with_origin) in answered {
        let mut own = vec![("item", q(ITEM)), ("kind", q(kind)), ("answer", q(answer))];
        if with_origin {
            own.push(("origin", q(ORIGIN)));
        }
        let own: Vec<(&str, &str)> = own.iter().map(|(k, v)| (*k, v.as_str())).collect();
        shapes.push((
            What::ItemAnswered {
                item: item(),
                answered,
            },
            line("item_answered", &own),
        ));
    }
    for cause in ConfirmLapse::ALL {
        shapes.push((
            What::ItemLapsed {
                item: item(),
                lapsed: Lapsed::Confirm(*cause),
            },
            line(
                "item_lapsed",
                &[
                    ("item", &q(ITEM)),
                    ("kind", &q("confirm")),
                    ("cause", &q(cause.as_str())),
                ],
            ),
        ));
    }
    for kind in OtherKind::ALL {
        for cause in OtherLapse::ALL {
            if *cause == OtherLapse::Check && *kind != OtherKind::Ready {
                continue;
            }
            shapes.push((
                What::ItemLapsed {
                    item: item(),
                    lapsed: Lapsed::Other {
                        kind: *kind,
                        cause: *cause,
                    },
                },
                line(
                    "item_lapsed",
                    &[
                        ("item", &q(ITEM)),
                        ("kind", &q(kind.as_str())),
                        ("cause", &q(cause.as_str())),
                    ],
                ),
            ));
        }
    }
    shapes
}

fn coord_event_entry() -> &'static halvo_schema::KindEntry {
    &halvo_schema::coordinator::KINDS[0]
}

#[test]
fn r4_record_writes_and_reads_every_shape_of_the_nine_events() {
    let shapes = every_shape();
    let mut events_seen = Vec::new();
    for (what, text) in shapes {
        let value = event(what);
        assert_eq!(value.to_json().unwrap(), text);
        assert_eq!(CoordEvent::parse(text.as_bytes()).unwrap(), value, "{text}");
        assert_eq!(serde_json::to_string(&value).unwrap(), text);
        assert_eq!(serde_json::from_str::<CoordEvent>(&text).unwrap(), value);
        assert_eq!(check_record(coord_event_entry(), &text), Ok(()), "{text}");
        let name = value.what.event();
        if !events_seen.contains(&name) {
            events_seen.push(name);
        }
    }
    assert_eq!(events_seen.len(), Event::ALL.len());
}

#[test]
fn r4_record_writes_the_keys_of_the_kind_entry_in_their_order() {
    let keys: Vec<&str> = coord_event_entry().fields.iter().map(|(k, _)| *k).collect();
    assert_eq!(keys, KEYS);
    let entry_example = CoordEvent::parse(coord_event_entry().example.as_bytes()).unwrap();
    assert_eq!(
        entry_example.to_json().unwrap(),
        coord_event_entry().example
    );
}

#[test]
fn r4_record_the_closed_lists_hold_the_names_of_the_contract() {
    let names = |all: Vec<&'static str>| all;
    assert_eq!(
        names(Event::ALL.iter().map(|v| v.as_str()).collect()),
        [
            "run_started",
            "run_ended",
            "item_raised",
            "item_answered",
            "item_lapsed",
            "item_released",
            "hold_set",
            "hold_cleared",
            "slot_skipped",
        ]
    );
    assert_eq!(
        names(ItemKind::ALL.iter().map(|v| v.as_str()).collect()),
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
    assert_eq!(
        names(OtherKind::ALL.iter().map(|v| v.as_str()).collect()),
        ItemKind::ALL[1..]
            .iter()
            .map(|v| v.as_str())
            .collect::<Vec<_>>()
    );
    for kind in OtherKind::ALL {
        assert_eq!(kind.kind().as_str(), kind.as_str());
    }
    assert_eq!(
        names(RunEnd::ALL.iter().map(|v| v.as_str()).collect()),
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
            "budget",
            "kind",
        ]
    );
    assert_eq!(
        names(ConfirmLapse::ALL.iter().map(|v| v.as_str()).collect()),
        [
            "digest",
            "run_ended",
            "wait_limit",
            "cancelled",
            "no_record",
            "app_ended"
        ]
    );
    assert_eq!(
        names(Trigger::ALL.iter().map(|v| v.as_str()).collect()),
        ["person", "schedule"]
    );
    assert_eq!(
        names(HoldSetCause::ALL.iter().map(|v| v.as_str()).collect()),
        ["person", "failures"]
    );
    assert_eq!(
        names(EffectAnswer::ALL.iter().map(|v| v.as_str()).collect()),
        ["allow_once", "reject_once"]
    );
    assert_eq!(
        names(DoneAnswer::ALL.iter().map(|v| v.as_str()).collect()),
        ["decline", "done"]
    );
    assert_eq!(
        names(BindResult::ALL.iter().map(|v| v.as_str()).collect()),
        ["decline", "allow"]
    );
    assert_eq!(
        names(CheckAnswer::ALL.iter().map(|v| v.as_str()).collect()),
        ["try_again", "went_through"]
    );
    assert_eq!(
        names(ReadyAnswer::ALL.iter().map(|v| v.as_str()).collect()),
        ["skip", "start"]
    );
}

fn refused(text: &str, key: Option<&str>, rule: &str) {
    let refusal = CoordEvent::parse(text.as_bytes()).unwrap_err();
    assert_eq!((refusal.key(), refusal.rule()), (key, rule), "{text}");
    assert!(serde_json::from_str::<CoordEvent>(text).is_err(), "{text}");
}

fn with(own: &[(&str, &str)], event_name: &str) -> String {
    line(event_name, own)
}

#[test]
fn r4_record_refuses_an_answer_that_its_kind_does_not_have_and_an_origin_out_of_place() {
    let i = q(ITEM);
    let o = q(ORIGIN);
    let answered = |kind: &str, answer: &str, origin: Option<&str>| {
        let k = q(kind);
        let a = q(answer);
        let mut own = vec![
            ("item", i.as_str()),
            ("kind", k.as_str()),
            ("answer", a.as_str()),
        ];
        if let Some(origin) = origin {
            own.push(("origin", origin));
        }
        with(&own, "item_answered")
    };
    refused(
        &answered("setup", "none", None),
        Some("answer"),
        "unknown_name",
    );
    refused(
        &answered("sign_in", "done", Some(&o)),
        Some("origin"),
        "not_null",
    );
    refused(
        &answered("sign_in", "allow", None),
        Some("origin"),
        "no_origin",
    );
    refused(
        &answered("sign_in", "skip", Some(&o)),
        Some("answer"),
        "unknown_name",
    );
    refused(
        &answered("sign_in", "skip", None),
        Some("answer"),
        "unknown_name",
    );
    refused(
        &answered("link_account", "done", None),
        Some("origin"),
        "not_a_string",
    );
    refused(
        &answered("stuck", "done", Some(&o)),
        Some("origin"),
        "not_null",
    );
    refused(
        &answered("check", "skip", None),
        Some("answer"),
        "unknown_name",
    );
    refused(
        &answered("ready", "try_again", None),
        Some("answer"),
        "unknown_name",
    );
    refused(
        &answered("show", "allow", None),
        Some("answer"),
        "unknown_name",
    );
    refused(
        &answered("confirm", "allow", None),
        Some("effect"),
        "not_a_string",
    );
    refused(
        &answered("sign_in", "allow", Some(r#""Amount due""#)),
        Some("origin"),
        "origin_form",
    );
    refused(
        &answered(
            "sign_in",
            "allow",
            Some(r#""https://northwind-power.test""#),
        ),
        Some("origin"),
        "origin_form",
    );
    refused(
        &with(
            &[("run", &q(RUN)), ("trigger", &q("person")), ("origin", &o)],
            "run_started",
        ),
        Some("origin"),
        "not_null",
    );
}

#[test]
fn r4_record_refuses_a_key_that_its_event_does_not_set_and_a_key_that_it_lacks() {
    let i = q(ITEM);
    let r = q(RUN);
    refused(
        &with(&[("run", &r)], "run_started"),
        Some("trigger"),
        "not_a_string",
    );
    refused(
        &with(
            &[
                ("run", &r),
                ("trigger", &q("person")),
                ("cause", &q("failed")),
            ],
            "run_started",
        ),
        Some("cause"),
        "not_null",
    );
    refused(
        &with(&[("run", &r)], "run_ended"),
        Some("cause"),
        "not_a_string",
    );
    refused(
        &with(
            &[("run", &r), ("cause", &q("failed")), ("slot", "1")],
            "run_ended",
        ),
        Some("slot"),
        "not_null",
    );
    refused(
        &with(&[("item", &i), ("kind", &q("confirm"))], "item_raised"),
        Some("run"),
        "not_a_string",
    );
    refused(
        &with(
            &[("item", &i), ("kind", &q("ready")), ("run", &r)],
            "item_raised",
        ),
        Some("run"),
        "not_null",
    );
    refused(
        &with(
            &[("item", &i), ("kind", &q("setup")), ("run", &r)],
            "item_raised",
        ),
        Some("run"),
        "not_null",
    );
    refused(
        &with(
            &[
                ("item", &i),
                ("kind", &q("stuck")),
                ("cause", &q("run_ended")),
                ("run", &r),
            ],
            "item_lapsed",
        ),
        Some("run"),
        "not_null",
    );
    refused(
        &with(
            &[
                ("item", &i),
                ("kind", &q("stuck")),
                ("effect", &q("5c4b3a29180f7e6d5c4b3a29180f7e6d")),
                ("approval", &q("6d5c4b3a29180f7e6d5c4b3a29180f7e")),
                ("digest", &q(&"ab".repeat(32))),
                ("answer", &q("allow_once")),
            ],
            "item_released",
        ),
        Some("kind"),
        "not_confirm",
    );
    refused(
        &with(&[("cause", &q("check"))], "hold_set"),
        Some("cause"),
        "unknown_name",
    );
    refused(
        &with(&[("cause", &q("check"))], "hold_cleared"),
        Some("run"),
        "not_a_string",
    );
    refused(
        &with(&[("cause", &q("person")), ("run", &r)], "hold_cleared"),
        Some("run"),
        "not_null",
    );
    refused(
        &with(
            &[("cause", &q("check")), ("run", &r), ("answer", &q("skip"))],
            "hold_cleared",
        ),
        Some("answer"),
        "unknown_name",
    );
    refused(
        &with(&[("slot", "1"), ("trigger", &q("person"))], "slot_skipped"),
        Some("trigger"),
        "not_null",
    );
}

#[test]
fn r4_record_hold_cleared_sets_run_and_answer_for_check_alone() {
    let r = q(RUN);
    let a = q("try_again");
    for cause in ["person", "failures"] {
        let c = q(cause);
        for (key, value) in [("run", r.as_str()), ("answer", a.as_str())] {
            let text = with(&[("cause", &c), (key, value)], "hold_cleared");
            let refusal = CoordEvent::parse(text.as_bytes()).unwrap_err();
            assert_eq!(
                (refusal.key(), refusal.rule()),
                (Some(key), "not_null"),
                "I7.1: hold_cleared sets run and answer for check alone; {text}"
            );
        }
    }
}

#[test]
fn r4_record_lapse_causes_are_the_six_for_a_confirmation_and_others_for_the_rest() {
    let i = q(ITEM);
    let lapsed = |kind: &str, cause: &str| {
        with(
            &[("item", &i), ("kind", &q(kind)), ("cause", &q(cause))],
            "item_lapsed",
        )
    };
    refused(&lapsed("confirm", "check"), Some("cause"), "unknown_name");
    for cause in ["digest", "no_record"] {
        for kind in OtherKind::ALL {
            refused(&lapsed(kind.as_str(), cause), Some("cause"), "unknown_name");
        }
    }
    refused(&lapsed("confirm", "failed"), Some("cause"), "unknown_name");
    refused(&lapsed("other", "run_ended"), Some("kind"), "unknown_name");
}

#[test]
fn r4_record_refuses_every_other_form_of_the_first_seven_keys_and_of_the_whole() {
    let good = line("slot_skipped", &[("slot", "20766")]);
    assert!(CoordEvent::parse(good.as_bytes()).is_ok());
    let cases: [(&str, &str, Option<&str>, &str); 14] = [
        (r#""v":1"#, r#""v":2"#, Some("v"), "version"),
        (r#""seq":7"#, r#""seq":0"#, Some("seq"), "zero"),
        (r#""seq":7"#, r#""seq":-1"#, Some("seq"), "not_a_count"),
        (r#""seq":7"#, r#""seq":4294967296"#, Some("seq"), "range"),
        (
            r#""day":20767"#,
            r#""day":20767.0"#,
            Some("day"),
            "not_a_count",
        ),
        (
            r#""at":1794339000000000000"#,
            r#""at":-0"#,
            Some("at"),
            "minus_zero",
        ),
        (
            r#""at":1794339000000000000"#,
            r#""at":9223372036854775808"#,
            Some("at"),
            "range",
        ),
        (
            r#""at":1794339000000000000"#,
            r#""at":null"#,
            Some("at"),
            "not_an_instant",
        ),
        (
            r#""event":"slot_skipped""#,
            r#""event":"slot_moved""#,
            Some("event"),
            "unknown_name",
        ),
        (
            r#""workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0""#,
            r#""workflow":"0F1E2D3C4B5A69788796A5B4C3D2E1F0""#,
            Some("workflow"),
            "hex_digit",
        ),
        (
            r#""agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809""#,
            r#""agent":null"#,
            Some("agent"),
            "not_a_string",
        ),
        (
            r#""slot":20766"#,
            r#""slot":"20766""#,
            Some("slot"),
            "not_a_count",
        ),
        (
            r#""origin":null"#,
            r#""origin":null,"text":"Amount due""#,
            None,
            "unknown_key",
        ),
        (r#","origin":null"#, "", Some("origin"), "missing_key"),
    ];
    for (from, to, key, rule) in cases {
        let text = good.replacen(from, to, 1);
        assert_ne!(text, good, "{from}");
        refused(&text, key, rule);
    }
    refused(
        &good.replacen(r#""v":1,"#, r#""v":1,"v":1,"#, 1),
        None,
        "repeated_key",
    );
    refused("[]", None, "not_an_object");
    refused("{", None, "json");
    let item_at = with(
        &[
            ("item", r#""7E6D5C4B3A2918070F1E2D3C4B5A6978""#),
            ("kind", &q("ready")),
            ("answer", &q("skip")),
        ],
        "item_answered",
    );
    refused(&item_at, Some("item"), "hex32");
}

#[test]
fn r4_record_a_refusal_holds_no_value_of_the_input() {
    let secret = "Northwind account 7 due 42.17";
    let text = line("slot_skipped", &[("slot", "20766")]).replacen(
        r#""origin":null"#,
        &format!(r#""origin":"{secret}""#),
        1,
    );
    let refusal = CoordEvent::parse(text.as_bytes()).unwrap_err();
    let shown = format!("{refusal} {refusal:?}");
    assert!(!shown.contains("Northwind"), "{shown}");
    assert!(!shown.contains("42.17"), "{shown}");
    assert_eq!(refusal.to_string(), "CoordEvent: origin: not_null");
}

#[test]
fn r4_record_item_id_takes_32_lower_case_hex_digits_alone() {
    assert_eq!(ItemId::parse(ITEM).unwrap().as_str(), ITEM);
    assert_eq!(ItemId::from_random([0xab; 16]).as_str(), "ab".repeat(16));
    for bad in [
        "",
        "7e6d",
        &ITEM.to_uppercase(),
        &format!("{ITEM}0"),
        &ITEM.replacen('7', "g", 1),
    ] {
        assert_eq!(ItemId::parse(bad).unwrap_err().rule(), "hex32", "{bad}");
    }
}
