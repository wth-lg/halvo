use halvo_schema::executable::names::{Cause, LogEvent, ReplyState};
use halvo_schema::executable::record::{AgentEffect, AgentLog, AgentReply};
use halvo_schema::executable::text::ProviderId;
use halvo_schema::executable::{self, Refusal, RefusalCause};
use halvo_schema::kinds::{
    check_entries, check_record, FieldType, KindEntry, Origin, RetentionClass,
};

const W: &str = "30000000000000000000000000000001";
const A: &str = "40000000000000000000000000000001";
const R: &str = "50000000000000000000000000000001";
const E: &str = "60000000000000000000000000000001";

fn of_run(rest: &str) -> String {
    format!(r#"{{"v":1,"workflow":"{W}","agent":"{A}","run":"{R}",{rest}}}"#)
}

fn swap(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "{from}");
    text.replacen(from, to, 1)
}

fn expect(result: Result<impl std::fmt::Debug, Refusal>, type_name: &str, rule: &str, key: &str) {
    let refusal = result.expect_err("refused");
    assert_eq!(refusal.cause(), RefusalCause::Malformed, "{refusal}");
    assert_eq!(
        (
            refusal.type_name(),
            refusal.rule(),
            refusal.key().unwrap_or("-")
        ),
        (type_name, rule, key)
    );
}

fn line(seq: u32, event: &str, cause: Option<&str>) -> String {
    let cause = cause.map_or("null".to_owned(), |c| format!("\"{c}\""));
    of_run(&format!(
        r#""seq":{seq},"day":20768,"at":1794339000000000000,"event":"{event}","step":null,"cause":{cause},"model":null,"ms":null,"tokens_in":null,"tokens_out":null"#
    ))
}

#[test]
fn the_cause_list_holds_unauthorized_not_selected_and_no_start_in_the_order_of_the_contract() {
    let names: Vec<&str> = Cause::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(
        names,
        [
            "approval",
            "digest",
            "used",
            "unknown",
            "autonomy",
            "checkpoint",
            "class",
            "class_mismatch",
            "origin",
            "provenance",
            "declined",
            "timeout",
            "session_size",
            "step_failed",
            "steps",
            "wall",
            "tokens",
            "sheet",
            "workbook",
            "adapter",
            "effect",
            "ended",
            "changed",
            "secret",
            "unauthorized",
            "not_selected",
            "no_start",
        ]
    );
    for cause in Cause::ALL {
        assert_eq!(Cause::parse(cause.as_str()), Ok(*cause));
    }
}

#[test]
fn a_start_accepted_line_is_line_1_with_no_step_cause_model_or_figures() {
    assert_eq!(LogEvent::ALL[0].as_str(), "start_accepted");
    assert_eq!(
        LogEvent::parse("start_accepted"),
        Ok(LogEvent::StartAccepted)
    );
    let good = line(1, "start_accepted", None);
    let read = AgentLog::parse(good.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read.event, LogEvent::StartAccepted);
    assert_eq!(read.to_json().unwrap(), good);
    expect(
        AgentLog::parse(swap(&good, r#""seq":1,"#, r#""seq":2,"#).as_bytes()),
        "AgentLog",
        "start_accepted_first",
        "seq",
    );
    for (key, from, to) in [
        ("step", r#""step":null"#, r#""step":1"#),
        ("cause", r#""cause":null"#, r#""cause":"secret""#),
        ("model", r#""model":null"#, r#""model":"qwen3_1_7b""#),
        ("ms", r#""ms":null"#, r#""ms":0"#),
        ("tokens_in", r#""tokens_in":null"#, r#""tokens_in":0"#),
        ("tokens_out", r#""tokens_out":null"#, r#""tokens_out":0"#),
    ] {
        expect(
            AgentLog::parse(swap(&good, from, to).as_bytes()),
            "AgentLog",
            "start_accepted_form",
            key,
        );
    }
}

#[test]
fn unauthorized_stands_in_a_step_failed_line_and_not_selected_and_no_start_in_a_stopped_line() {
    for (cause, event) in [
        ("unauthorized", "step_failed"),
        ("not_selected", "stopped"),
        ("no_start", "stopped"),
    ] {
        for other in LogEvent::ALL {
            if *other == LogEvent::StartAccepted {
                continue;
            }
            let text = line(2, other.as_str(), Some(cause));
            if other.as_str() == event {
                let read = AgentLog::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
                assert_eq!(read.cause.map(|c| c.as_str()), Some(cause));
                assert_eq!(read.to_json().unwrap(), text);
            } else {
                expect(
                    AgentLog::parse(text.as_bytes()),
                    "AgentLog",
                    "cause_of_event",
                    "cause",
                );
            }
        }
    }
    for other in ["secret", "session_size", "adapter"] {
        assert!(AgentLog::parse(line(2, "step_ok", Some(other)).as_bytes()).is_ok());
    }
}

const PAY_KEPT: &str = r#"[{"name":"payee","value":["Northwind Power"]},{"name":"amount","value":["84.20"]},{"name":"instrument","value":["card 4242"]},{"name":"origin","value":["http://pay.test:18090"]}]"#;

fn effect_row(state: &str, approval: Option<&str>, cause: &str) -> String {
    let approval = approval.map_or("null".to_owned(), |a| format!("\"{a}\""));
    of_run(&format!(
        r#""effect":"{E}","seq":2,"day":20768,"at":1794339000000000000,"state":"{state}","class":"payment","digest":"{}","kept":{PAY_KEPT},"fields":4,"approval":{approval},"cause":{cause}"#,
        "a".repeat(64)
    ))
}

#[test]
fn no_effect_row_holds_unauthorized_not_selected_or_no_start_while_the_states_have_no_not_made() {
    let approval = "70000000000000000000000000000001";
    for (state, approval) in [
        ("held", None),
        ("rejected", None),
        ("releasing", Some(approval)),
        ("unknown", Some(approval)),
    ] {
        assert!(AgentEffect::parse(effect_row(state, approval, "null").as_bytes()).is_ok());
        for cause in ["unauthorized", "not_selected", "no_start"] {
            expect(
                AgentEffect::parse(effect_row(state, approval, &format!("\"{cause}\"")).as_bytes()),
                "AgentEffect",
                "cause_of_state",
                "cause",
            );
        }
    }
}

fn kind(name: &str) -> &'static KindEntry {
    executable::KINDS
        .iter()
        .find(|k| k.name == name)
        .unwrap_or_else(|| panic!("no kind {name}"))
}

const REPLY_EXAMPLE: &str = r#"{"v":1,"workflow":"30303030303030303030303030303030","agent":"40404040404040404040404040404040","run":"50505050505050505050505050505050","seq":1,"day":20768,"at":1794339000000000003,"message":"msg-utility-0001","draft":"draft-utility-0001","effect":null,"state":"drafted"}"#;

fn reply_row(state: &str, draft: Option<&str>, effect: Option<&str>) -> String {
    let q = |s: Option<&str>| s.map_or("null".to_owned(), |s| format!("\"{s}\""));
    of_run(&format!(
        r#""seq":2,"day":20768,"at":1794339000000000000,"message":"msg-1","draft":{},"effect":{},"state":"{state}""#,
        q(draft),
        q(effect)
    ))
}

#[test]
fn agent_reply_is_a_kept_kind_of_origin_own_with_the_keys_of_a7_4_and_its_example() {
    use FieldType::*;
    let entry = kind("agent_reply");
    assert_eq!(entry.class, RetentionClass::Kept);
    assert_eq!(entry.origin, Origin::Own);
    assert_eq!(entry.family, "agent");
    assert_eq!(entry.workflow_key, Some("workflow"));
    assert_eq!(
        entry.fields,
        [
            ("v", Version),
            ("workflow", Id),
            ("agent", Id),
            ("run", Id),
            ("seq", Count),
            ("day", Day),
            ("at", Instant),
            ("message", OwnText),
            ("draft", OwnText),
            ("effect", Id),
            ("state", Name),
        ]
    );
    assert_eq!(entry.example, REPLY_EXAMPLE);
    assert_eq!(check_entries(&[entry]), Vec::<String>::new());
    assert_eq!(check_record(entry, entry.example), Ok(()));
    let read = AgentReply::parse(entry.example.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read.state, ReplyState::Drafted);
    assert_eq!(read.message.as_str(), "msg-utility-0001");
    assert_eq!(read.to_json().unwrap(), entry.example);
}

#[test]
fn a_reply_row_holds_one_of_six_states_with_the_draft_and_the_effect_that_its_state_allows() {
    let names: Vec<&str> = ReplyState::ALL.iter().map(|s| s.as_str()).collect();
    assert_eq!(
        names,
        [
            "drafted",
            "begun",
            "committed",
            "unknown",
            "lost",
            "withheld"
        ]
    );
    let d = Some("draft-1");
    let e = Some(E);
    let taken = [
        ("drafted", d, None),
        ("drafted", d, e),
        ("begun", d, e),
        ("committed", d, e),
        ("unknown", d, e),
        ("lost", None, None),
        ("withheld", d, None),
    ];
    for (state, draft, effect) in taken {
        let text = reply_row(state, draft, effect);
        let read = AgentReply::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{state}: {e}"));
        assert_eq!(read.state.as_str(), state);
        assert_eq!(read.draft.as_ref().map(ProviderId::as_str), draft);
        assert_eq!(read.effect.as_ref().map(|e| e.as_str()), effect);
        assert_eq!(read.to_json().unwrap(), text);
    }
    let refused = [
        ("drafted", None, None, "draft_of_state", "draft"),
        ("begun", None, e, "draft_of_state", "draft"),
        ("committed", None, e, "draft_of_state", "draft"),
        ("unknown", None, e, "draft_of_state", "draft"),
        ("withheld", None, None, "draft_of_state", "draft"),
        ("lost", d, None, "draft_of_state", "draft"),
        ("begun", d, None, "effect_of_state", "effect"),
        ("committed", d, None, "effect_of_state", "effect"),
        ("unknown", d, None, "effect_of_state", "effect"),
        ("lost", None, e, "effect_of_state", "effect"),
        ("withheld", d, e, "effect_of_state", "effect"),
    ];
    for (state, draft, effect, rule, key) in refused {
        expect(
            AgentReply::parse(reply_row(state, draft, effect).as_bytes()),
            "AgentReply",
            rule,
            key,
        );
    }
    for other in ["sent", "Drafted", "held", "rejected", "not_made"] {
        expect(
            AgentReply::parse(reply_row(other, d, None).as_bytes()),
            "ReplyState",
            "unknown_name",
            "state",
        );
    }
}

#[test]
fn a_reply_row_that_holds_a_key_beyond_a7_4_or_text_in_place_of_an_id_is_refused() {
    let entry = kind("agent_reply");
    let good = reply_row("drafted", Some("draft-1"), None);
    for key in [
        "subject", "to", "from", "address", "body", "text", "preview", "sender", "note",
    ] {
        let text = swap(
            &good,
            r#""state":"drafted""#,
            &format!(r#""state":"drafted","{key}":"Your bill""#),
        );
        expect(
            AgentReply::parse(text.as_bytes()),
            "AgentReply",
            "unknown_key",
            "-",
        );
        assert_eq!(
            check_record(entry, &text),
            Err("agent_reply: a key that the fields do not list: unknown_key".to_owned())
        );
    }
    let at_most = "m".repeat(1024);
    let at = swap(&good, r#""msg-1""#, &format!("\"{at_most}\""));
    assert!(AgentReply::parse(at.as_bytes()).is_ok());
    for (from, to, type_name, rule, key) in [
        (
            r#""msg-1""#,
            "\"\"".to_owned(),
            "ProviderId",
            "empty_id",
            "message",
        ),
        (
            r#""msg-1""#,
            format!("\"{at_most}m\""),
            "ProviderId",
            "own_text_length",
            "message",
        ),
        (
            r#""msg-1""#,
            r#"["msg-1"]"#.to_owned(),
            "ProviderId",
            "not_a_string",
            "message",
        ),
        (
            r#""msg-1""#,
            "null".to_owned(),
            "ProviderId",
            "not_a_string",
            "message",
        ),
        (
            r#""draft-1""#,
            "\"\"".to_owned(),
            "ProviderId",
            "empty_id",
            "draft",
        ),
        (
            r#""draft-1""#,
            r#"["draft-1"]"#.to_owned(),
            "ProviderId",
            "not_a_string",
            "draft",
        ),
        (
            r#""seq":2"#,
            r#""seq":0"#.to_owned(),
            "number",
            "zero",
            "seq",
        ),
    ] {
        expect(
            AgentReply::parse(swap(&good, from, &to).as_bytes()),
            type_name,
            rule,
            key,
        );
    }
    let list = swap(&good, r#""msg-1""#, r#"["msg-1"]"#);
    assert_eq!(check_record(entry, &list), Ok(()));
    let dropped = swap(&good, r#","effect":null"#, "");
    expect(
        AgentReply::parse(dropped.as_bytes()),
        "AgentReply",
        "missing_key",
        "effect",
    );
}

#[test]
fn the_names_of_agent_reply_stand_in_names_and_in_the_names_file() {
    let file = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/names.txt"),
    )
    .unwrap();
    let lines: Vec<&str> = file.lines().collect();
    for name in [
        "agent_reply",
        "message",
        "draft",
        "drafted",
        "begun",
        "committed",
        "unknown",
        "lost",
        "withheld",
    ] {
        assert!(executable::NAMES.contains(&name), "{name}");
        assert!(lines.contains(&name), "{name}");
    }
}
