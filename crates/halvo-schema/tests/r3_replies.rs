use halvo_schema::executable::body::{
    LoadReplies, LoadRepliesReply, LoadReply, PutReplyRow, RunnerCall, RunnerReply,
    CHECKPOINTS_REPLY_MAX, RUNNER_REPLY_VALUE_MAX,
};
use halvo_schema::executable::frame::{
    ConfirmRequest, FromRunner, StoreReply, StoreReplyFrame, ToRunner, FRAME_JSON_MAX,
};
use halvo_schema::executable::names::{Cause, LogEvent, ReplyState, YesNo};
use halvo_schema::executable::record::{
    AgentEffect, AgentLog, AgentReply, AgentRun, AgentStart, RUN_LIST_KEYS,
};
use halvo_schema::executable::text::ProviderId;
use halvo_schema::executable::{self, Refusal, RefusalCause};
use halvo_schema::kinds::{
    check_entries, check_record, FieldType, KindEntry, Origin, RetentionClass,
};
use halvo_schema::request::{self, Decoded, RunnerRequest, RunnerRole};
use halvo_schema::RequestId;

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

const START_EXAMPLE: &str = r#"{"v":1,"workflow":"30303030303030303030303030303030","agent":"40404040404040404040404040404040","day":20768,"at":1794339000000000010}"#;

#[test]
fn agent_start_is_a_kept_kind_of_origin_own_with_its_day_and_its_instant_and_no_other_key() {
    use FieldType::*;
    let entry = kind("agent_start");
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
            ("day", Day),
            ("at", Instant),
        ]
    );
    assert_eq!(entry.example, START_EXAMPLE);
    assert_eq!(check_entries(&[entry]), Vec::<String>::new());
    assert_eq!(check_record(entry, entry.example), Ok(()));
    let read = AgentStart::parse(entry.example.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read.day, 20768);
    assert_eq!(read.to_json().unwrap(), entry.example);
    for key in ["run", "seq", "rev", "mode", "note"] {
        let text = swap(
            START_EXAMPLE,
            r#""day":20768"#,
            &format!(r#""{key}":1,"day":20768"#),
        );
        expect(
            AgentStart::parse(text.as_bytes()),
            "AgentStart",
            "unknown_key",
            "-",
        );
    }
    let dropped = swap(START_EXAMPLE, r#","at":1794339000000000010"#, "");
    expect(
        AgentStart::parse(dropped.as_bytes()),
        "AgentStart",
        "missing_key",
        "at",
    );
    expect(
        AgentStart::parse(swap(START_EXAMPLE, "1794339000000000010", "\"now\"").as_bytes()),
        "Instant",
        "not_a_count",
        "at",
    );
}

#[test]
fn the_entry_of_agent_start_under_origin_capture_is_still_refused_for_its_instant() {
    let own = kind("agent_start");
    let capture = KindEntry {
        origin: Origin::Capture,
        ..*own
    };
    assert_eq!(
        check_entries(&[&capture]),
        [
            "kind \"agent_start\": a bounded or kept kind of origin capture with the key at of the type Instant"
        ]
    );
    let config = kind("agent_config");
    assert!(!config.fields.iter().any(|(_, t)| *t == FieldType::Instant));
}

const LOAD_CONFIG: &str = r#"{"v":1,"workflow":"30000000000000000000000000000001","agent":"40000000000000000000000000000001","rev":1,"category":"utility_bill_payment","tier":"on_device","reasoning":"medium","autonomy":"draft_and_confirm","allow":[],"caps":{"untrusted":"yes","private":"yes","external":"yes"},"limits":{"steps":20,"wall_s":120,"model_tokens":0},"budget":null,"permissions":[],"runtime":"web","playbook":null}"#;

fn load_reply(start: &str) -> String {
    format!(
        r#"{{"config":{LOAD_CONFIG},"playbook":null,"bindings":null,"sessions":[],"start":{start}}}"#
    )
}

#[test]
fn the_reply_of_load_holds_the_agent_s_start_row_or_null_under_the_key_start() {
    let row =
        format!(r#"{{"v":1,"workflow":"{W}","agent":"{A}","day":20768,"at":1794339000000000010}}"#);
    for start in ["null", row.as_str()] {
        let text = load_reply(start);
        let read = LoadReply::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read.start.is_some(), start != "null");
        assert_eq!(read.to_json().unwrap(), text);
        let RunnerReply::Load(through) = RunnerReply::parse(text.as_bytes()).unwrap() else {
            panic!("not a load reply")
        };
        assert_eq!(through, read);
    }
    let without =
        format!(r#"{{"config":{LOAD_CONFIG},"playbook":null,"bindings":null,"sessions":[]}}"#);
    expect(
        LoadReply::parse(without.as_bytes()),
        "LoadReply",
        "missing_key",
        "start",
    );
    let other_agent = swap(&row, A, "40000000000000000000000000000002");
    let other_workflow = swap(&row, W, "30000000000000000000000000000002");
    for other in [other_agent, other_workflow] {
        expect(
            LoadReply::parse(load_reply(&other).as_bytes()),
            "LoadReply",
            "start_of_another_agent",
            "start",
        );
    }
    expect(
        LoadReply::parse(load_reply(r#"{"v":1,"day":20768}"#).as_bytes()),
        "AgentStart",
        "missing_key",
        "workflow",
    );
}

#[test]
fn the_names_of_agent_start_stand_in_names_and_in_the_names_file() {
    let file = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/names.txt"),
    )
    .unwrap();
    let lines: Vec<&str> = file.lines().collect();
    for name in ["agent_start", "start", "day", "at"] {
        assert!(executable::NAMES.contains(&name), "{name}");
        assert!(lines.contains(&name), "{name}");
    }
}

const A2: &str = "40000000000000000000000000000002";

fn q(s: Option<&str>) -> String {
    s.map_or("null".to_owned(), |s| format!("\"{s}\""))
}

fn row_of(workflow: &str, agent: &str, message: &str, state: &str, draft: Option<&str>) -> String {
    let effect = matches!(state, "begun" | "committed" | "unknown").then_some(E);
    format!(
        r#"{{"v":1,"workflow":"{workflow}","agent":"{agent}","run":"{R}","seq":2,"day":20768,"at":1794339000000000000,"message":"{message}","draft":{},"effect":{},"state":"{state}"}}"#,
        q(draft),
        q(effect)
    )
}

fn mine(message: &str) -> String {
    row_of(W, A, message, "drafted", Some(&format!("d-{message}")))
}

fn ids(list: &[&str]) -> String {
    list.iter()
        .map(|id| format!("\"{id}\""))
        .collect::<Vec<_>>()
        .join(",")
}

fn replies_text(rows: &[String], blocked: &[&str], withheld: &[&str]) -> String {
    format!(
        r#"{{"replies":[{}],"blocked":[{}],"blocked_withheld":[{}]}}"#,
        rows.join(","),
        ids(blocked),
        ids(withheld)
    )
}

fn load_replies_body(messages: &[&str]) -> String {
    format!(
        r#"{{"op":"load_replies","agent":"{A}","run":"{R}","message_ids":[{}]}}"#,
        ids(messages)
    )
}

fn travels(text: &str, op: &str) -> RunnerCall {
    let call = RunnerCall::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {op}"));
    assert_eq!(call.op(), op);
    assert_eq!(call.to_json().unwrap(), text);
    assert_eq!((call.agent().as_str(), call.run().as_str()), (A, R));
    let request = RunnerRequest::new(
        &RunnerRole::claim(),
        RequestId::from_random([7; 16]),
        call.clone(),
    );
    let bytes = request.to_bytes().unwrap();
    let Ok(Decoded::Raw(raw)) = request::decode(&bytes) else {
        panic!("{op}")
    };
    assert_eq!(
        RunnerCall::parse(raw.body().get().as_bytes()).unwrap(),
        call
    );
    let read: RunnerCall = serde_json::from_str(text).unwrap();
    assert_eq!(read, call);
    call
}

#[test]
fn the_body_load_replies_names_1_to_25_message_ids_each_once_and_travels_in_a_runner_request() {
    let names: Vec<String> = (1..=26).map(|i| format!("m{i}")).collect();
    let all: Vec<&str> = names.iter().map(String::as_str).collect();
    for count in [1, 2, 25] {
        let text = load_replies_body(&all[..count]);
        let RunnerCall::LoadReplies(body) = travels(&text, "load_replies") else {
            panic!("not load_replies")
        };
        assert_eq!(body.message_ids.len(), count);
        assert_eq!(LoadReplies::parse(text.as_bytes()).unwrap(), body);
    }
    let one = load_replies_body(&["m1"]);
    for (text, type_name, rule, key) in [
        (
            load_replies_body(&[]),
            "LoadReplies",
            "no_message",
            "message_ids",
        ),
        (
            load_replies_body(&all),
            "LoadReplies",
            "messages_max",
            "message_ids",
        ),
        (
            load_replies_body(&["m1", "m2", "m1"]),
            "LoadReplies",
            "repeated_name",
            "message_ids",
        ),
        (
            load_replies_body(&[""]),
            "ProviderId",
            "empty_id",
            "message_ids",
        ),
        (
            swap(&one, r#"["m1"]"#, r#"[["m1"]]"#),
            "ProviderId",
            "not_a_string",
            "message_ids",
        ),
        (
            swap(&one, r#""run""#, &format!(r#""workflow":"{W}","run""#)),
            "LoadReplies",
            "unknown_key",
            "-",
        ),
        (
            swap(
                &one,
                r#""message_ids""#,
                r#""subjects":["Bill"],"message_ids""#,
            ),
            "LoadReplies",
            "unknown_key",
            "-",
        ),
    ] {
        expect(RunnerCall::parse(text.as_bytes()), type_name, rule, key);
    }
}

#[test]
fn the_body_reply_row_holds_one_reply_row_of_its_own_agent_and_run() {
    let body = |record: &str| {
        format!(r#"{{"op":"reply_row","agent":"{A}","run":"{R}","record":{record}}}"#)
    };
    let good = body(&mine("m1"));
    let RunnerCall::ReplyRow(read) = travels(&good, "reply_row") else {
        panic!("not reply_row")
    };
    assert_eq!(read.record.message.as_str(), "m1");
    assert_eq!(PutReplyRow::parse(good.as_bytes()).unwrap(), read);
    let call = RunnerCall::ReplyRow(read);
    let stored = RunnerReply::parse(br#"{"stored":1}"#).unwrap();
    assert!(stored.answers(&call));
    let replies = RunnerReply::parse(replies_text(&[], &[], &[]).as_bytes()).unwrap();
    assert!(!replies.answers(&call));
    let other_run = mine("m1").replacen(R, "50000000000000000000000000000002", 1);
    for (text, type_name, rule, key) in [
        (
            body(&row_of(W, A2, "m1", "drafted", Some("d-1"))),
            "PutReplyRow",
            "record_of_another_run",
            "record",
        ),
        (
            body(&other_run),
            "PutReplyRow",
            "record_of_another_run",
            "record",
        ),
        (
            body(&swap(
                &mine("m1"),
                r#""state""#,
                r#""to":"a@b.test","state""#,
            )),
            "AgentReply",
            "unknown_key",
            "record",
        ),
        (
            swap(&good, r#""record""#, r#""subject":"Bill","record""#),
            "PutReplyRow",
            "unknown_key",
            "-",
        ),
    ] {
        expect(RunnerCall::parse(text.as_bytes()), type_name, rule, key);
    }
}

#[test]
fn a_load_replies_body_that_names_its_ids_under_the_key_messages_is_refused() {
    let text = format!(r#"{{"op":"load_replies","agent":"{A}","run":"{R}","messages":["m1"]}}"#);
    expect(
        RunnerCall::parse(text.as_bytes()),
        "LoadReplies",
        "unknown_key",
        "-",
    );
}

#[test]
fn the_reply_of_load_replies_holds_the_asking_agent_s_rows_and_two_lists_of_bare_ids() {
    let good = replies_text(&[mine("m1"), mine("m2")], &["m3"], &["m4"]);
    let read = LoadRepliesReply::parse(good.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read.replies.len(), 2);
    assert_eq!(read.to_json().unwrap(), good);
    let RunnerReply::Replies(through) = RunnerReply::parse(good.as_bytes()).unwrap() else {
        panic!("not a replies reply")
    };
    assert_eq!(through, read);
    let serde: RunnerReply = serde_json::from_str(&good).unwrap();
    assert_eq!(serde, RunnerReply::Replies(read.clone()));
    let call = |messages: &[&str]| {
        let RunnerCall::LoadReplies(b) =
            RunnerCall::parse(load_replies_body(messages).as_bytes()).unwrap()
        else {
            panic!("not load_replies")
        };
        b
    };
    let asked = call(&["m1", "m2", "m3", "m4", "m5"]);
    assert_eq!(read.answers(&asked), Ok(()));
    assert!(RunnerReply::Replies(read.clone()).answers(&RunnerCall::LoadReplies(asked.clone())));
    let empty = LoadRepliesReply::parse(replies_text(&[], &[], &[]).as_bytes()).unwrap();
    assert_eq!(empty.answers(&asked), Ok(()));

    let names: Vec<String> = (1..=26).map(|i| format!("m{i}")).collect();
    let rows: Vec<String> = names[..13].iter().map(|m| mine(m)).collect();
    let rest: Vec<&str> = names[13..].iter().map(String::as_str).collect();
    let at_25 = replies_text(&rows, &rest[..12], &[]);
    assert!(LoadRepliesReply::parse(at_25.as_bytes()).is_ok());
    for (text, type_name, rule, key) in [
        (
            replies_text(
                &[mine("m1"), row_of(W, A2, "m2", "begun", Some("d-2"))],
                &[],
                &[],
            ),
            "LoadRepliesReply",
            "row_of_another_agent",
            "replies",
        ),
        (
            replies_text(
                &[
                    mine("m1"),
                    row_of("30000000000000000000000000000002", A, "m2", "lost", None),
                ],
                &[],
                &[],
            ),
            "LoadRepliesReply",
            "row_of_another_agent",
            "replies",
        ),
        (
            replies_text(&[mine("m1"), mine("m1")], &[], &[]),
            "LoadRepliesReply",
            "repeated_name",
            "replies",
        ),
        (
            replies_text(&[], &["m3", "m3"], &[]),
            "LoadRepliesReply",
            "repeated_name",
            "blocked",
        ),
        (
            replies_text(&[], &[], &["m4", "m4"]),
            "LoadRepliesReply",
            "repeated_name",
            "blocked_withheld",
        ),
        (
            replies_text(&[], &["m3"], &["m3"]),
            "LoadRepliesReply",
            "blocked_twice",
            "blocked_withheld",
        ),
        (
            replies_text(&rows, &rest, &[]),
            "LoadRepliesReply",
            "ids_max",
            "replies",
        ),
        (
            swap(&good, r#""blocked":"#, r#""agents":[],"blocked":"#),
            "LoadRepliesReply",
            "unknown_key",
            "-",
        ),
        (
            replies_text(
                &[swap(
                    &mine("m1"),
                    r#""state""#,
                    r#""subject":"Bill","state""#,
                )],
                &[],
                &[],
            ),
            "AgentReply",
            "unknown_key",
            "replies",
        ),
        (
            replies_text(&[], &[], &[]).replacen(
                r#""blocked":[]"#,
                &format!(r#""blocked":[{}]"#, mine("m3")),
                1,
            ),
            "ProviderId",
            "not_a_string",
            "blocked",
        ),
        (
            replies_text(&[], &[""], &[]),
            "ProviderId",
            "empty_id",
            "blocked",
        ),
    ] {
        expect(
            LoadRepliesReply::parse(text.as_bytes()),
            type_name,
            rule,
            key,
        );
        expect(RunnerReply::parse(text.as_bytes()), type_name, rule, key);
    }

    let of_another = LoadRepliesReply::parse(
        replies_text(&[row_of(W, A2, "m1", "committed", Some("d-1"))], &[], &[]).as_bytes(),
    )
    .unwrap();
    let not_asked =
        LoadRepliesReply::parse(replies_text(&[mine("m9")], &[], &[]).as_bytes()).unwrap();
    let out_of_order =
        LoadRepliesReply::parse(replies_text(&[mine("m2"), mine("m1")], &[], &[]).as_bytes())
            .unwrap();
    let blocked_not_asked =
        LoadRepliesReply::parse(replies_text(&[], &["m9"], &[]).as_bytes()).unwrap();
    let withheld_not_asked =
        LoadRepliesReply::parse(replies_text(&[], &[], &["m9"]).as_bytes()).unwrap();
    for (reply, rule, key) in [
        (&of_another, "row_of_another_agent", "replies"),
        (&not_asked, "not_asked", "replies"),
        (&out_of_order, "order_asked", "replies"),
        (&blocked_not_asked, "not_asked", "blocked"),
        (&withheld_not_asked, "not_asked", "blocked_withheld"),
    ] {
        expect(reply.answers(&asked), "LoadRepliesReply", rule, key);
        assert!(
            !RunnerReply::Replies(reply.clone()).answers(&RunnerCall::LoadReplies(asked.clone()))
        );
    }
    let load =
        RunnerCall::parse(format!(r#"{{"op":"load","agent":"{A}","run":"{R}"}}"#).as_bytes())
            .unwrap();
    assert!(!RunnerReply::Replies(read).answers(&load));
}

fn plain_reply(drafts: &[usize; 25], blocked: usize) -> String {
    let names: Vec<String> = (0..25)
        .map(|i| format!("{i:02}{}", "m".repeat(1022)))
        .collect();
    let rows: Vec<String> = names
        .iter()
        .zip(drafts)
        .map(|(m, d)| row_of(W, A, m, "drafted", Some(&"d".repeat(*d))))
        .collect();
    let listed: Vec<&str> = names[..blocked].iter().map(String::as_str).collect();
    replies_text(&rows, &listed, &[])
}

fn reply_of_length(len: usize) -> String {
    let mut drafts = [1; 25];
    let mut blocked = 0;
    while blocked < 25 && plain_reply(&drafts, blocked + 1).len() <= len {
        blocked += 1;
    }
    let mut gap = len - plain_reply(&drafts, blocked).len();
    for draft in drafts.iter_mut() {
        let add = gap.min(1024 - *draft);
        *draft += add;
        gap -= add;
    }
    assert_eq!(gap, 0);
    let text = plain_reply(&drafts, blocked);
    assert_eq!(text.len(), len);
    text
}

fn value_of(text: &str) -> LoadRepliesReply {
    let v: serde_json::Value = serde_json::from_str(text).unwrap();
    let list = |key: &str| -> Vec<ProviderId> {
        v[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| ProviderId::parse(id.as_str().unwrap()).unwrap())
            .collect()
    };
    LoadRepliesReply {
        replies: v["replies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| AgentReply::parse(row.to_string().as_bytes()).unwrap())
            .collect(),
        blocked: list("blocked"),
        blocked_withheld: list("blocked_withheld"),
    }
}

fn store_reply_frame(reply: LoadRepliesReply) -> ToRunner {
    ToRunner::StoreReply(StoreReplyFrame {
        reply: StoreReply {
            id: Some(RequestId::from_random([0x40; 16])),
            outcome: Ok(RunnerReply::Replies(reply)),
        },
    })
}

fn length(result: Result<impl std::fmt::Debug, Refusal>, type_name: &str, rule: &str) {
    let refusal = result.expect_err("refused");
    assert_eq!(
        (refusal.cause(), refusal.type_name(), refusal.rule()),
        (RefusalCause::Length, type_name, rule),
        "{refusal}"
    );
}

#[test]
fn the_constant_of_a_runner_reply_s_value_is_what_fills_a_store_reply_frame_exactly() {
    assert_eq!(RUNNER_REPLY_VALUE_MAX, 65_455);
    assert_eq!(FRAME_JSON_MAX - RUNNER_REPLY_VALUE_MAX, 28 + 53);
    assert_eq!(CHECKPOINTS_REPLY_MAX - RUNNER_REPLY_VALUE_MAX, 195_612);
    let exact = reply_of_length(RUNNER_REPLY_VALUE_MAX);
    let read = LoadRepliesReply::parse(exact.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read.to_json().unwrap(), exact);
    assert!(RunnerReply::parse(exact.as_bytes()).is_ok());
    let frame = store_reply_frame(read);
    let text = frame.to_json().unwrap();
    assert_eq!(text.len(), FRAME_JSON_MAX);
    assert_eq!(ToRunner::parse(text.as_bytes()).unwrap(), frame);

    let over = reply_of_length(RUNNER_REPLY_VALUE_MAX + 1);
    length(
        LoadRepliesReply::parse(over.as_bytes()),
        "LoadRepliesReply",
        "reply_bytes",
    );
    length(
        RunnerReply::parse(over.as_bytes()),
        "RunnerReply",
        "reply_bytes",
    );
    let through_serde = serde_json::from_str::<RunnerReply>(&over)
        .unwrap_err()
        .to_string();
    assert!(
        through_serde.contains("length: RunnerReply: -: reply_bytes"),
        "{through_serde}"
    );
    let value = value_of(&over);
    length(value.to_json(), "LoadRepliesReply", "json_length");
    length(
        store_reply_frame(value).to_json(),
        "LoadRepliesReply",
        "json_length",
    );
    let raw_frame = format!(
        r#"{{"t":"store_reply","reply":{{"v":1,"id":"{}","ok":{over}}}}}"#,
        "40".repeat(16)
    );
    assert_eq!(raw_frame.len(), FRAME_JSON_MAX + 1);
    length(
        ToRunner::parse(raw_frame.as_bytes()),
        "ToRunner",
        "frame_length",
    );

    let padded = exact.replacen(r#","blocked""#, r#", "blocked""#, 1);
    assert_eq!(padded.len(), RUNNER_REPLY_VALUE_MAX + 1);
    length(
        LoadRepliesReply::parse(padded.as_bytes()),
        "LoadRepliesReply",
        "reply_bytes",
    );
    length(
        RunnerReply::parse(padded.as_bytes()),
        "RunnerReply",
        "reply_bytes",
    );
    for outside in [format!(" {exact}"), format!("{exact}\n")] {
        length(
            LoadRepliesReply::parse(outside.as_bytes()),
            "LoadRepliesReply",
            "reply_bytes",
        );
        length(
            RunnerReply::parse(outside.as_bytes()),
            "RunnerReply",
            "reply_bytes",
        );
    }
}

fn largest_row(message: &str) -> String {
    format!(
        r#"{{"v":1,"workflow":"{W}","agent":"{A}","run":"{R}","seq":4294967295,"day":4294967295,"at":-9223372036854775808,"message":"{message}","draft":"{}","effect":"{E}","state":"committed"}}"#,
        "d".repeat(1024)
    )
}

fn page_of_25_blocked_with_ids_of(len: usize) -> String {
    let names: Vec<String> = (0..25)
        .map(|i| format!("{i:02}{}", "m".repeat(len - 2)))
        .collect();
    let rows: Vec<String> = names
        .iter()
        .map(|m| {
            largest_row(m).replacen(
                &format!(r#""draft":"{}""#, "d".repeat(1024)),
                &format!(r#""draft":"{}""#, "d".repeat(len)),
                1,
            )
        })
        .collect();
    let listed: Vec<&str> = names.iter().map(String::as_str).collect();
    replies_text(&rows, &listed, &[])
}

#[test]
fn a_page_of_25_rows_with_their_25_ids_blocked_fits_with_ids_of_775_bytes_and_not_with_776() {
    let fits = page_of_25_blocked_with_ids_of(775);
    assert_eq!(fits.len(), 65_397);
    let read = LoadRepliesReply::parse(fits.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read.replies.len(), 25);
    assert_eq!(read.blocked.len(), 25);
    assert!(read.replies.iter().all(
        |r| r.message.as_str().len() == 775 && r.draft.as_ref().unwrap().as_str().len() == 775
    ));
    assert_eq!(read.to_json().unwrap(), fits);
    let over = page_of_25_blocked_with_ids_of(776);
    assert_eq!(over.len(), 65_472);
    length(
        LoadRepliesReply::parse(over.as_bytes()),
        "LoadRepliesReply",
        "reply_bytes",
    );
    length(value_of(&over).to_json(), "LoadRepliesReply", "json_length");
}

fn page_of_25_blocked_with_escaped_ids_written_in(written: usize) -> String {
    let escapes = (written - 3) / 2;
    assert_eq!(2 + 2 * escapes + 1, written);
    let names: Vec<String> = (0..25)
        .map(|i| format!("{i:02}{}m", "\\\\".repeat(escapes)))
        .collect();
    let rows: Vec<String> = names
        .iter()
        .map(|m| {
            largest_row(m).replacen(
                &format!(r#""draft":"{}""#, "d".repeat(1024)),
                &format!(r#""draft":"{m}""#),
                1,
            )
        })
        .collect();
    let listed: Vec<&str> = names.iter().map(String::as_str).collect();
    replies_text(&rows, &listed, &[])
}

#[test]
fn the_775_bytes_of_an_id_are_the_bytes_that_compact_json_writes_backslashes_counted_twice() {
    let fits = page_of_25_blocked_with_escaped_ids_written_in(775);
    assert_eq!(fits.len(), 65_397);
    let read = LoadRepliesReply::parse(fits.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    for row in &read.replies {
        assert_eq!(row.message.as_str().len(), 389);
        assert_eq!(row.draft.as_ref().unwrap().as_str().len(), 389);
    }
    assert!(read.blocked.iter().all(|id| id.as_str().len() == 389));
    assert_eq!(read.to_json().unwrap(), fits);
    let over = page_of_25_blocked_with_escaped_ids_written_in(777);
    assert_eq!(over.len(), 65_547);
    length(
        LoadRepliesReply::parse(over.as_bytes()),
        "LoadRepliesReply",
        "reply_bytes",
    );
    let value = value_of(&over);
    assert!(value
        .replies
        .iter()
        .all(|r| r.message.as_str().len() == 390));
    assert!(value.replies.iter().all(|r| r.message.as_str().len() < 400));
    length(value.to_json(), "LoadRepliesReply", "json_length");
}

#[test]
fn twenty_five_rows_at_their_largest_fit_a_frame_and_with_25_bare_ids_beside_them_do_not() {
    let names: Vec<String> = (0..25)
        .map(|i| format!("{i:02}{}", "m".repeat(1022)))
        .collect();
    let row = largest_row(&names[0]);
    assert!(AgentReply::parse(row.as_bytes()).is_ok());
    assert_eq!(row.len(), 2_333);
    let rows: Vec<String> = names.iter().map(|m| largest_row(m)).collect();
    let listed: Vec<&str> = names.iter().map(String::as_str).collect();
    let rows_alone = replies_text(&rows, &[], &[]);
    assert_eq!(rows_alone.len(), 58_398);
    assert_eq!(rows_alone.len() + 28 + 53, 58_479);
    assert!(LoadRepliesReply::parse(rows_alone.as_bytes()).is_ok());
    let with_ids = replies_text(&rows, &listed, &[]);
    assert_eq!(with_ids.len(), 84_072);
    length(
        LoadRepliesReply::parse(with_ids.as_bytes()),
        "LoadRepliesReply",
        "reply_bytes",
    );
    let split = replies_text(&rows, &listed[..12], &listed[12..]);
    assert_eq!(split.len(), with_ids.len() - 1);
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Form {
    Count,
    YesNo,
}

fn list_keys() -> Vec<(&'static str, Form)> {
    let entry = kind("agent_run");
    RUN_LIST_KEYS
        .iter()
        .map(|key| {
            let types: Vec<FieldType> = entry
                .fields
                .iter()
                .filter(|(k, _)| k == key)
                .map(|(_, t)| *t)
                .collect();
            let form = match types[..] {
                [FieldType::Count] => Form::Count,
                [FieldType::Name] => Form::YesNo,
                _ => panic!("{key}: {types:?}"),
            };
            (*key, form)
        })
        .collect()
}

#[test]
fn the_keys_of_the_run_s_list_are_the_twelve_of_a7_4_in_their_order_and_two_are_yes_or_no() {
    assert_eq!(
        RUN_LIST_KEYS,
        [
            "before_start",
            "cut",
            "no_key",
            "not_to_account",
            "incomplete",
            "reply_to_other",
            "more_pages",
            "held_recipient",
            "organized",
            "other_task",
            "held_earlier",
            "earlier_page",
        ]
    );
    let yes_no: Vec<&str> = list_keys()
        .into_iter()
        .filter(|(_, form)| *form == Form::YesNo)
        .map(|(key, _)| key)
        .collect();
    assert_eq!(yes_no, ["cut", "more_pages"]);
}

const RUN_HEAD: &str = r#""day":20768,"mode":"replay","outcome":"completed","playbook":1,"steps":9,"prompts":1,"handbacks":0,"confirmations":1,"effects":1,"unknown":0,"model_calls":0,"tokens_in":0,"tokens_out":0,"wall_s":14,"model":null"#;

fn run_with(value: impl Fn(&str, Form) -> String) -> String {
    let list: Vec<String> = list_keys()
        .into_iter()
        .map(|(key, form)| format!(r#""{key}":{}"#, value(key, form)))
        .collect();
    of_run(&format!("{RUN_HEAD},{}", list.join(",")))
}

fn list_values(run: &AgentRun) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(&run.to_json().unwrap()).unwrap();
    RUN_LIST_KEYS
        .iter()
        .map(|key| v[*key].to_string())
        .collect()
}

#[test]
fn a_run_record_holds_each_key_of_the_run_s_list_in_its_order_as_a_count_a_yes_or_no_or_null() {
    let nulls = run_with(|_, _| "null".to_owned());
    let read = AgentRun::parse(nulls.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read.to_json().unwrap(), nulls);
    assert!(list_values(&read).iter().all(|v| v == "null"));
    for (count, yes_no) in [("0", "no"), ("7", "yes"), ("4294967295", "no")] {
        let text = run_with(|_, form| match form {
            Form::Count => count.to_owned(),
            Form::YesNo => format!("\"{yes_no}\""),
        });
        let read = AgentRun::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read.to_json().unwrap(), text);
    }
    let keys: Vec<String> =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&nulls)
            .unwrap()
            .keys()
            .cloned()
            .collect();
    assert_eq!(keys.len(), 19 + RUN_LIST_KEYS.len());
    for (key, form) in list_keys() {
        let one = |text: &str| {
            run_with(|k, _| {
                if k == key {
                    text.to_owned()
                } else {
                    "null".to_owned()
                }
            })
        };
        let dropped = nulls.replacen(&format!(r#","{key}":null"#), "", 1);
        assert_eq!(dropped.len() + key.len() + 8, nulls.len(), "{key}");
        expect(
            AgentRun::parse(dropped.as_bytes()),
            "AgentRun",
            "missing_key",
            key,
        );
        let refused: &[(&str, &str, &str)] = match form {
            Form::Count => &[
                ("\"yes\"", "count", "not_a_count"),
                ("\"3\"", "count", "not_a_count"),
                ("-1", "count", "not_a_count"),
                ("1.5", "count", "not_a_count"),
                ("true", "count", "not_a_count"),
                ("[]", "count", "not_a_count"),
                ("4294967296", "count", "range"),
            ],
            Form::YesNo => &[
                ("\"maybe\"", "YesNo", "unknown_name"),
                ("\"Yes\"", "YesNo", "unknown_name"),
                ("0", "YesNo", "not_a_string"),
                ("1", "YesNo", "not_a_string"),
                ("true", "YesNo", "not_a_string"),
                ("[\"yes\"]", "YesNo", "not_a_string"),
            ],
        };
        for (value, type_name, rule) in refused {
            expect(AgentRun::parse(one(value).as_bytes()), type_name, rule, key);
        }
    }
}

#[test]
fn the_entry_of_agent_run_lists_the_keys_of_the_run_s_list_after_model_with_their_types() {
    let entry = kind("agent_run");
    let at = entry
        .fields
        .iter()
        .position(|(key, _)| *key == "model")
        .unwrap();
    let after: Vec<&str> = entry.fields[at + 1..].iter().map(|(key, _)| *key).collect();
    assert_eq!(after, RUN_LIST_KEYS);
    assert_eq!(list_keys().len(), RUN_LIST_KEYS.len());
    assert_eq!(check_entries(&[entry]), Vec::<String>::new());
    let read = AgentRun::parse(entry.example.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read.to_json().unwrap(), entry.example);
    let nulls = run_with(|_, _| "null".to_owned());
    assert_eq!(check_record(entry, &nulls), Ok(()));
    for key in RUN_LIST_KEYS {
        let dropped = nulls.replacen(&format!(r#","{key}":null"#), "", 1);
        assert_eq!(
            check_record(entry, &dropped),
            Err(format!("agent_run: {key}: missing_key"))
        );
    }
}

#[test]
fn the_names_of_the_run_s_list_stand_in_names_and_in_the_names_file() {
    let file = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/names.txt"),
    )
    .unwrap();
    let lines: Vec<&str> = file.lines().collect();
    for key in RUN_LIST_KEYS {
        assert!(executable::NAMES.contains(key), "{key}");
        assert!(lines.contains(key), "{key}");
    }
}

#[test]
fn a_run_row_with_every_key_at_its_largest_is_measured_in_bytes() {
    let largest = format!(
        r#"{{"v":1,"workflow":"{W}","agent":"{A}","run":"{R}","day":4294967295,"mode":"compile","outcome":"effect_unknown","playbook":4294967295,"steps":4294967295,"prompts":4294967295,"handbacks":2147483648,"confirmations":2147483647,"effects":4294967295,"unknown":4294967295,"model_calls":4294967295,"tokens_in":4294967295,"tokens_out":4294967295,"wall_s":4294967295,"model":"{}",{}}}"#,
        "m".repeat(32),
        list_keys()
            .into_iter()
            .map(|(key, form)| match form {
                Form::Count => format!(r#""{key}":4294967295"#),
                Form::YesNo => format!(r#""{key}":"yes""#),
            })
            .collect::<Vec<_>>()
            .join(",")
    );
    let read = AgentRun::parse(largest.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read.to_json().unwrap(), largest);
    assert_eq!(largest.len(), 771);
}

const SEND_BOUND: &str = r#"{"class":"send","fields":[{"name":"to","from":"extract","origin":null,"value":["a@mail.test"]},{"name":"subject","from":"extract","origin":null,"value":["Re: rent"]},{"name":"body","from":"model","origin":null,"value":["Thanks, noted."]}]}"#;

fn confirm_with(cut: &str) -> String {
    format!(
        r#"{{"v":1,"run":"{R}","agent":"{A}","cut":{cut},"effects":[{{"effect":"{E}","class":"send","digest":"{}","bound":{SEND_BOUND},"new_origin":null}}]}}"#,
        "a".repeat(64)
    )
}

#[test]
fn a_confirmation_request_holds_cut_after_agent_as_yes_no_or_null() {
    for (cut, value) in [
        ("null", None),
        ("\"yes\"", Some(YesNo::Yes)),
        ("\"no\"", Some(YesNo::No)),
    ] {
        let text = confirm_with(cut);
        let read = ConfirmRequest::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read.cut, value);
        assert_eq!(read.to_json().unwrap(), text);
        let frame = format!(r#"{{"t":"confirm","request":{text}}}"#);
        assert_eq!(
            FromRunner::parse(frame.as_bytes())
                .unwrap()
                .to_json()
                .unwrap(),
            frame
        );
    }
    let good = confirm_with("null");
    expect(
        ConfirmRequest::parse(swap(&good, r#""cut":null,"#, "").as_bytes()),
        "ConfirmRequest",
        "missing_key",
        "cut",
    );
    for (cut, type_name, rule) in [
        ("\"maybe\"", "YesNo", "unknown_name"),
        ("0", "YesNo", "not_a_string"),
        ("true", "YesNo", "not_a_string"),
        ("3", "YesNo", "not_a_string"),
    ] {
        expect(
            ConfirmRequest::parse(confirm_with(cut).as_bytes()),
            type_name,
            rule,
            "cut",
        );
    }
}

#[test]
fn a_load_reply_whose_start_row_is_of_another_agent_does_not_answer_the_asking_agent_s_load() {
    let load = |agent: &str| {
        RunnerCall::parse(format!(r#"{{"op":"load","agent":"{agent}","run":"{R}"}}"#).as_bytes())
            .unwrap()
    };
    let start_of = |agent: &str| {
        format!(
            r#"{{"v":1,"workflow":"{W}","agent":"{agent}","day":20768,"at":1794339000000000010}}"#
        )
    };
    let own = RunnerReply::parse(load_reply(&start_of(A)).as_bytes()).unwrap();
    assert!(own.answers(&load(A)));
    let none = RunnerReply::parse(load_reply("null").as_bytes()).unwrap();
    assert!(none.answers(&load(A)));
    let text = swap(
        &load_reply(&start_of(A2)),
        &format!(r#""agent":"{A}""#),
        &format!(r#""agent":"{A2}""#),
    );
    let other = RunnerReply::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert!(other.answers(&load(A2)));
    assert!(!other.answers(&load(A)));
}

#[test]
fn a_load_reply_with_no_start_row_answers_only_the_load_of_its_config_s_agent() {
    let load = |agent: &str| {
        RunnerCall::parse(format!(r#"{{"op":"load","agent":"{agent}","run":"{R}"}}"#).as_bytes())
            .unwrap()
    };
    let own = RunnerReply::parse(load_reply("null").as_bytes()).unwrap();
    assert!(own.answers(&load(A)));
    assert!(!own.answers(&load(A2)));
    let text = swap(
        &load_reply("null"),
        &format!(r#""agent":"{A}""#),
        &format!(r#""agent":"{A2}""#),
    );
    let other = RunnerReply::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert!(other.answers(&load(A2)));
    assert!(!other.answers(&load(A)));
}

fn golden_lines(file: &str) -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/r3")
        .join(file);
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn no_reply_of_another_form_answers_load_replies_and_its_reply_answers_no_other_call() {
    let calls: Vec<RunnerCall> = golden_lines("runner-bodies.jsonl")
        .iter()
        .map(|b| RunnerCall::parse(b.as_bytes()).unwrap())
        .collect();
    let replies: Vec<RunnerReply> = golden_lines("runner-replies.jsonl")
        .iter()
        .map(|r| RunnerReply::parse(r.as_bytes()).unwrap())
        .collect();
    let ops: std::collections::BTreeSet<&str> = calls.iter().map(|c| c.op()).collect();
    assert_eq!(ops.len(), 11, "{ops:?}");
    let load_replies = calls
        .iter()
        .find(|c| matches!(c, RunnerCall::LoadReplies(_)))
        .unwrap();
    let mut forms = Vec::new();
    for reply in &replies {
        let own = matches!(reply, RunnerReply::Replies(_));
        assert_eq!(reply.answers(load_replies), own, "{reply:?}");
        if own {
            for call in calls.iter().filter(|c| c.op() != "load_replies") {
                assert!(!reply.answers(call), "{}", call.op());
            }
        }
        forms.push(match reply {
            RunnerReply::Load(_) => "load",
            RunnerReply::Steps(_) => "steps",
            RunnerReply::Session(_) => "session",
            RunnerReply::Stored(_) => "stored",
            RunnerReply::Replies(_) => "replies",
        });
    }
    assert_eq!(forms, ["load", "steps", "session", "stored", "replies"]);
}

#[test]
fn the_debug_form_of_a_provider_id_is_its_type_and_length_and_holds_no_part_of_the_id() {
    let message = "msg-Zq7Northwind-0042";
    let draft = "draft-Xw9Southwind-0042";
    let blocked = "msg-Yv8Eastwind-0043";
    let id = ProviderId::parse(message).unwrap();
    assert_eq!(
        format!("{id:?}"),
        format!("ProviderId({} bytes)", message.len())
    );
    let row = row_of(W, A, message, "drafted", Some(draft));
    let read = AgentReply::parse(row.as_bytes()).unwrap();
    let reply = LoadRepliesReply::parse(replies_text(&[row], &[blocked], &[]).as_bytes()).unwrap();
    for (what, shown) in [
        ("id", format!("{id:?}")),
        ("row", format!("{read:?}")),
        ("reply", format!("{reply:?}")),
        ("pretty", format!("{reply:#?}")),
    ] {
        for secret in [
            "Zq7",
            "Northwind",
            "Xw9",
            "Southwind",
            "Yv8",
            "Eastwind",
            "0042",
            "0043",
        ] {
            assert!(!shown.contains(secret), "{what}: {secret}");
        }
    }
    assert!(format!("{read:?}").contains(&format!("ProviderId({} bytes)", draft.len())));
    assert!(format!("{reply:?}").contains(&format!("ProviderId({} bytes)", blocked.len())));
}

#[test]
fn the_bound_of_a_load_replies_reply_decides_before_a_row_is_read_and_after_the_json_read() {
    let misspell = |text: &str| text.replacen(r#""message":"#, r#""messagx":"#, 1);
    let at = misspell(&reply_of_length(RUNNER_REPLY_VALUE_MAX));
    assert_eq!(at.len(), RUNNER_REPLY_VALUE_MAX);
    expect(
        LoadRepliesReply::parse(at.as_bytes()),
        "AgentReply",
        "unknown_key",
        "replies",
    );
    expect(
        RunnerReply::parse(at.as_bytes()),
        "AgentReply",
        "unknown_key",
        "replies",
    );
    let over = misspell(&reply_of_length(RUNNER_REPLY_VALUE_MAX + 1));
    assert_eq!(over.len(), RUNNER_REPLY_VALUE_MAX + 1);
    length(
        LoadRepliesReply::parse(over.as_bytes()),
        "LoadRepliesReply",
        "reply_bytes",
    );
    length(
        RunnerReply::parse(over.as_bytes()),
        "RunnerReply",
        "reply_bytes",
    );
    let not_json = format!("x{}", &reply_of_length(RUNNER_REPLY_VALUE_MAX + 1)[1..]);
    assert_eq!(not_json.len(), RUNNER_REPLY_VALUE_MAX + 1);
    length(
        LoadRepliesReply::parse(not_json.as_bytes()),
        "LoadRepliesReply",
        "reply_bytes",
    );
    expect(
        RunnerReply::parse(not_json.as_bytes()),
        "RunnerReply",
        "json",
        "-",
    );
}
