use halvo_schema::executable::body::{
    CoordinatorCall, CoordinatorReply, LoadReply, RunnerCall, RunnerReply, SessionReply,
    StepsReply, Stored, CHECKPOINTS_REPLY_MAX,
};
use halvo_schema::executable::names::{ActionClass, Cause, FieldName, Way};
use halvo_schema::executable::record::{
    AgentBindings, AgentEffect, AgentLog, AgentPlaybook, AgentRun, AgentSession,
};
use halvo_schema::executable::{Bound, Refusal, RefusalCause};
use halvo_schema::request::{
    self, CoordinatorRole, CoordinatorWrite, Decoded, RunnerRequest, RunnerRole,
};
use halvo_schema::RequestId;

const W: &str = "30000000000000000000000000000001";
const A: &str = "40000000000000000000000000000001";
const R: &str = "50000000000000000000000000000001";
const E: &str = "60000000000000000000000000000001";
const P: &str = "70000000000000000000000000000001";
const U: &str = "http://utility.test:18090";

fn digest() -> String {
    "ab".repeat(32)
}

const CONFIG: &str = r#"{"v":1,"workflow":"30000000000000000000000000000001","agent":"40000000000000000000000000000001","rev":1,"category":"utility_bill_payment","tier":"on_device","reasoning":"medium","autonomy":"draft_and_confirm","allow":[],"caps":{"untrusted":"yes","private":"yes","external":"yes"},"limits":{"steps":20,"wall_s":120,"model_tokens":0},"budget":null,"permissions":[],"runtime":"web","playbook":null}"#;

const PAY_BOUND: &str = r#"{"class":"payment","fields":[{"name":"payee","from":"slot","origin":null,"value":["Northwind Power"]},{"name":"amount","from":"extract","origin":"http://utility.test:18090","value":["84.20"]},{"name":"instrument","from":"slot","origin":null,"value":["card 4242"]},{"name":"origin","from":"const","origin":null,"value":["http://pay.test:18090"]}]}"#;

fn wait(n: u32) -> String {
    format!(
        r#"{{"v":1,"n":{n},"kind":"web","verb":"wait","target":null,"args":[{{"name":"ms","from":"const","value":"1","key":null}}],"pre":[],"post":[],"effect":"none","class":"read"}}"#
    )
}

fn run_record(run: &str, rest: &str) -> String {
    format!(r#"{{"v":1,"workflow":"{W}","agent":"{A}","run":"{run}",{rest}}}"#)
}

const RUN_REST: &str = r#""day":20768,"mode":"replay","outcome":"completed","playbook":1,"steps":9,"prompts":1,"handbacks":0,"confirmations":1,"effects":1,"unknown":0,"model_calls":0,"tokens_in":0,"tokens_out":0,"wall_s":14,"model":null"#;

const PAY_KEPT: &str = r#"[{"name":"payee","value":["Northwind Power"]},{"name":"amount","value":["84.20"]},{"name":"instrument","value":["card 4242"]},{"name":"origin","value":["http://pay.test:18090"]}]"#;

fn effect(
    run: &str,
    state: &str,
    class: &str,
    kept: &str,
    fields: u32,
    approval: Option<&str>,
    cause: Option<&str>,
) -> String {
    let q = |s: Option<&str>| s.map_or("null".to_owned(), |s| format!("\"{s}\""));
    run_record(
        run,
        &format!(
            r#""effect":"{E}","seq":1,"day":20768,"at":1794339000000000000,"state":"{state}","class":"{class}","digest":"{}","kept":{kept},"fields":{fields},"approval":{},"cause":{}"#,
            digest(),
            q(approval),
            q(cause)
        ),
    )
}

fn held() -> String {
    effect(R, "held", "payment", PAY_KEPT, 4, None, None)
}

const LOG_REST: &str = r#""seq":1,"day":20768,"at":1794339000000000000,"event":"model_call","step":3,"cause":null,"model":"qwen3_1_7b","ms":812,"tokens_in":900,"tokens_out":40"#;

fn session(rest: &str) -> String {
    format!(r#"{{"v":1,"workflow":"{W}","agent":"{A}","origin":"{U}",{rest}}}"#)
}

fn bindings(rest: &str) -> String {
    format!(r#"{{"v":1,"workflow":"{W}","agent":"{A}","rev":1,{rest}}}"#)
}

const ORIGINS: &str = r#""origins":["http://utility.test:18090","http://pay.test:18090"]"#;

const SLOTS: &str = r#""slots":[{"name":"payee","value":["Northwind Power"]},{"name":"instrument","value":["card 4242"]}]"#;

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

fn swap(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "{from}");
    text.replacen(from, to, 1)
}

#[test]
fn each_class_has_the_bound_and_kept_fields_of_the_contract() {
    let table: Vec<(&str, Vec<&str>, Vec<&str>)> = vec![
        (
            "payment",
            vec!["payee", "amount", "instrument", "origin"],
            vec!["payee", "amount", "instrument", "origin"],
        ),
        (
            "purchase",
            vec!["seller", "items", "amount", "instrument", "origin"],
            vec!["seller", "amount", "instrument", "origin"],
        ),
        ("send", vec!["to", "subject", "body"], vec!["to"]),
        ("delete", vec!["items"], vec!["items"]),
        ("submit", vec!["origin", "fields"], vec!["origin"]),
        ("sign", vec!["document", "party"], vec!["document", "party"]),
        ("local_edit", vec!["file", "range"], vec!["file", "range"]),
        ("read", vec![], vec![]),
        ("prepare", vec![], vec![]),
        ("credential", vec![], vec![]),
    ];
    for (class, bound, kept) in table {
        let class = ActionClass::parse(class).unwrap();
        let names: Vec<&str> = class.bound_fields().iter().map(|f| f.as_str()).collect();
        assert_eq!(names, bound, "{class}");
        let kept_names: Vec<&str> = class
            .kept_fields()
            .iter()
            .map(|(f, _)| f.as_str())
            .collect();
        assert_eq!(kept_names, kept, "{class}");
        for (field, max) in class.kept_fields() {
            let expected = match field {
                FieldName::Amount => Some(32),
                FieldName::To => Some(320),
                _ => None,
            };
            assert_eq!(max, expected, "{class} {field}");
        }
    }
}

#[test]
fn a_bound_object_holds_exactly_the_fields_of_its_class_in_their_order() {
    let bound = Bound::parse(PAY_BOUND.as_bytes()).unwrap();
    assert_eq!(bound.to_json().unwrap(), PAY_BOUND);
    assert_eq!(bound.class(), ActionClass::Payment);
    assert_eq!(
        bound.field(FieldName::Amount).unwrap().value.joined(),
        "84.20"
    );
    assert!(bound.field(FieldName::Seller).is_none());
    let model_amount = swap(PAY_BOUND, r#""from":"extract""#, r#""from":"model""#);
    let model_amount = swap(
        &model_amount,
        r#""origin":"http://utility.test:18090""#,
        r#""origin":null"#,
    );
    assert!(
        Bound::parse(model_amount.as_bytes()).is_ok(),
        "the source is the rule of admit"
    );
    let swapped = r#"{"class":"payment","fields":[{"name":"amount","from":"extract","origin":"http://utility.test:18090","value":["84.20"]},{"name":"payee","from":"slot","origin":null,"value":["Northwind Power"]},{"name":"instrument","from":"slot","origin":null,"value":["card 4242"]},{"name":"origin","from":"const","origin":null,"value":["http://pay.test:18090"]}]}"#;
    expect(
        Bound::parse(swapped.as_bytes()),
        "Bound",
        "fields_of_class",
        "fields",
    );
    let short = swap(
        PAY_BOUND,
        r#",{"name":"origin","from":"const","origin":null,"value":["http://pay.test:18090"]}"#,
        "",
    );
    expect(
        Bound::parse(short.as_bytes()),
        "Bound",
        "fields_of_class",
        "fields",
    );
    let extra = swap(
        PAY_BOUND,
        "]}]}",
        r#"]},{"name":"seller","from":"slot","origin":null,"value":["x"]}]}"#,
    );
    expect(
        Bound::parse(extra.as_bytes()),
        "Bound",
        "fields_of_class",
        "fields",
    );
    let purchase = swap(PAY_BOUND, r#""class":"payment""#, r#""class":"purchase""#);
    expect(
        Bound::parse(purchase.as_bytes()),
        "Bound",
        "fields_of_class",
        "fields",
    );
    for class in ["read", "prepare", "credential"] {
        let text = format!(r#"{{"class":"{class}","fields":[]}}"#);
        expect(
            Bound::parse(text.as_bytes()),
            "Bound",
            "class_without_effect",
            "class",
        );
    }
    let slot_origin = swap(
        PAY_BOUND,
        r#""from":"slot","origin":null,"value":["Northwind Power"]"#,
        r#""from":"slot","origin":"http://utility.test:18090","value":["Northwind Power"]"#,
    );
    assert_eq!(
        Bound::parse(slot_origin.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        slot_origin,
        "which origin a field admits is the rule of admit"
    );
    let not_cut = swap(PAY_BOUND, r#"["84.20"]"#, r#"["84.","20"]"#);
    expect(
        Bound::parse(not_cut.as_bytes()),
        "OwnValue",
        "not_the_cut",
        "value",
    );
    let empty_piece = swap(PAY_BOUND, r#"["84.20"]"#, r#"[""]"#);
    expect(
        Bound::parse(empty_piece.as_bytes()),
        "OwnValue",
        "not_the_cut",
        "value",
    );
    let long = swap(
        PAY_BOUND,
        r#"["84.20"]"#,
        &format!(r#"["{}"]"#, "9".repeat(1025)),
    );
    expect(
        Bound::parse(long.as_bytes()),
        "OwnText",
        "own_text_length",
        "value",
    );
    let name = swap(PAY_BOUND, r#""name":"payee""#, r#""name":"password""#);
    expect(
        Bound::parse(name.as_bytes()),
        "FieldName",
        "unknown_name",
        "name",
    );
    let from = swap(PAY_BOUND, r#""from":"const""#, r#""from":"page""#);
    expect(
        Bound::parse(from.as_bytes()),
        "Source",
        "unknown_name",
        "from",
    );
    expect(
        Bound::parse(br#"["payment",[]]"#),
        "Bound",
        "not_an_object",
        "-",
    );
    expect(
        Bound::parse(br#"{"class":"payment","class":"payment","fields":[]}"#),
        "Bound",
        "repeated_key",
        "-",
    );
    expect(
        Bound::parse(br#"{"fields":[]}"#),
        "Bound",
        "missing_key",
        "class",
    );
    expect(
        Bound::parse(br#"{"class":"payment","fields":[],"digest":null}"#),
        "Bound",
        "unknown_key",
        "-",
    );
    let value_null = swap(PAY_BOUND, r#"["84.20"]"#, "null");
    expect(
        Bound::parse(value_null.as_bytes()),
        "list",
        "not_a_list",
        "value",
    );
    let value_string = swap(PAY_BOUND, r#"["84.20"]"#, r#""84.20""#);
    expect(
        Bound::parse(value_string.as_bytes()),
        "list",
        "not_a_list",
        "value",
    );
}

#[test]
fn the_mode_of_a_run_record_is_compile_replay_or_null_and_its_key_always_stands() {
    let good = run_record(R, RUN_REST);
    let null_mode = swap(&good, r#""mode":"replay""#, r#""mode":null"#);
    let ended = swap(
        &null_mode,
        r#""outcome":"completed""#,
        r#""outcome":"failed""#,
    );
    assert_eq!(
        AgentRun::parse(ended.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        ended
    );
    let compile = swap(&good, r#""mode":"replay""#, r#""mode":"compile""#);
    assert!(AgentRun::parse(compile.as_bytes()).is_ok());
    expect(
        AgentRun::parse(swap(&good, r#""mode":"replay","#, "").as_bytes()),
        "AgentRun",
        "missing_key",
        "mode",
    );
    expect(
        AgentRun::parse(swap(&ended, r#""mode":null,"#, "").as_bytes()),
        "AgentRun",
        "missing_key",
        "mode",
    );
    for outcome in [
        "completed",
        "nothing_to_do",
        "declined",
        "handed_back",
        "class_mismatch",
        "refused",
        "limit",
        "effect_unknown",
    ] {
        let text = swap(
            &null_mode,
            r#""outcome":"completed""#,
            &format!(r#""outcome":"{outcome}""#),
        );
        expect(
            AgentRun::parse(text.as_bytes()),
            "AgentRun",
            "mode_null_after_finish",
            "mode",
        );
    }
    for (to, type_name, rule) in [
        (r#""mode":"none""#, "Mode", "unknown_name"),
        (r#""mode":"Replay""#, "Mode", "unknown_name"),
        (r#""mode":"""#, "Mode", "unknown_name"),
        (r#""mode":0"#, "Mode", "not_a_string"),
        (r#""mode":[]"#, "Mode", "not_a_string"),
    ] {
        expect(
            AgentRun::parse(swap(&ended, r#""mode":null"#, to).as_bytes()),
            type_name,
            rule,
            "mode",
        );
    }
    let entry = halvo_schema::executable::KINDS
        .iter()
        .find(|k| k.name == "agent_run")
        .unwrap();
    assert_eq!(halvo_schema::kinds::check_record(entry, &ended), Ok(()));
    let finish = format!(r#"{{"op":"finish","agent":"{A}","run":"{R}","record":{ended}}}"#);
    assert_eq!(
        RunnerCall::parse(finish.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        finish
    );
}

#[test]
fn an_agent_run_record_holds_counts_that_agree() {
    let good = run_record(R, RUN_REST);
    assert_eq!(
        AgentRun::parse(good.as_bytes()).unwrap().to_json().unwrap(),
        good
    );
    let failed = swap(&good, r#""outcome":"completed""#, r#""outcome":"failed""#);
    let failed = swap(&failed, r#""unknown":0"#, r#""unknown":1"#);
    assert!(AgentRun::parse(failed.as_bytes()).is_ok());
    let cases = [
        (
            r#""prompts":1"#,
            r#""prompts":2"#,
            "AgentRun",
            "prompts_sum",
            "prompts",
        ),
        (
            r#""prompts":1"#,
            r#""prompts":0"#,
            "AgentRun",
            "prompts_sum",
            "prompts",
        ),
        (
            r#""unknown":0"#,
            r#""unknown":2"#,
            "AgentRun",
            "unknown_above_effects",
            "unknown",
        ),
        (
            r#""unknown":0"#,
            r#""unknown":1"#,
            "AgentRun",
            "completed_with_unknown",
            "unknown",
        ),
        (
            r#""day":20768"#,
            r#""day":-1"#,
            "count",
            "not_a_count",
            "day",
        ),
        (
            r#""day":20768"#,
            r#""day":"20768""#,
            "count",
            "not_a_count",
            "day",
        ),
        (
            r#""day":20768"#,
            r#""day":null"#,
            "count",
            "not_a_count",
            "day",
        ),
        (
            r#""mode":"replay""#,
            r#""mode":"Replay""#,
            "Mode",
            "unknown_name",
            "mode",
        ),
        (
            r#""outcome":"completed""#,
            r#""outcome":"done""#,
            "Outcome",
            "unknown_name",
            "outcome",
        ),
        (
            r#""outcome":"completed""#,
            r#""outcome":"ended""#,
            "Outcome",
            "unknown_name",
            "outcome",
        ),
        (
            r#""model":null"#,
            r#""model":"Qwen3-1.7B""#,
            "Name",
            "name_form",
            "model",
        ),
        (
            r#""playbook":1"#,
            r#""playbook":0"#,
            "number",
            "zero",
            "playbook",
        ),
        (
            r#""wall_s":14"#,
            r#""wall_s":1.4e1"#,
            "count",
            "not_a_count",
            "wall_s",
        ),
        (
            r#""v":1,"#,
            r#""v":1,"tier":"on_device","#,
            "AgentRun",
            "unknown_key",
            "-",
        ),
    ];
    for (from, to, type_name, rule, key) in cases {
        expect(
            AgentRun::parse(swap(&good, from, to).as_bytes()),
            type_name,
            rule,
            key,
        );
    }
}

#[test]
fn an_effect_row_keeps_the_kept_fields_of_its_class_and_its_approval_by_state() {
    for (state, approval, cause) in [
        ("held", None, None),
        ("cleared", None, None),
        ("refused", None, Some("autonomy")),
        ("approved", Some(P), None),
        ("releasing", Some(P), None),
        ("releasing", None, None),
        ("committed", Some(P), None),
        ("unknown", None, None),
        ("rejected", None, Some("ended")),
        ("rejected", Some(P), Some("declined")),
        ("rejected", None, Some("changed")),
    ] {
        let text = effect(R, state, "payment", PAY_KEPT, 4, approval, cause);
        assert_eq!(
            AgentEffect::parse(text.as_bytes())
                .unwrap()
                .to_json()
                .unwrap(),
            text
        );
    }
    for (class, kept, fields) in [
        (
            "purchase",
            r#"[{"name":"seller","value":["Shop"]},{"name":"amount","value":["12.00"]},{"name":"instrument","value":["card"]},{"name":"origin","value":["http://pay.test:18090"]}]"#,
            5,
        ),
        ("send", r#"[{"name":"to","value":["a@mail.test"]}]"#, 3),
        ("delete", r#"[{"name":"items","value":["AAMk1"]}]"#, 1),
        (
            "submit",
            r#"[{"name":"origin","value":["http://form.test:18090"]}]"#,
            2,
        ),
        (
            "sign",
            r#"[{"name":"document","value":["lease"]},{"name":"party","value":["landlord"]}]"#,
            2,
        ),
        (
            "local_edit",
            r#"[{"name":"file","value":["book.xlsx"]},{"name":"range","value":["B2:D6"]}]"#,
            2,
        ),
    ] {
        let text = effect(R, "held", class, kept, fields, None, None);
        assert!(AgentEffect::parse(text.as_bytes()).is_ok(), "{class}");
    }
    let g = held();
    let cases: Vec<(String, &str, &str, &str)> = vec![
        (
            effect(R, "approved", "payment", PAY_KEPT, 4, None, None),
            "AgentEffect",
            "approval_of_state",
            "approval",
        ),
        (
            effect(R, "held", "payment", PAY_KEPT, 4, Some(P), None),
            "AgentEffect",
            "approval_of_state",
            "approval",
        ),
        (
            effect(R, "cleared", "payment", PAY_KEPT, 4, Some(P), None),
            "AgentEffect",
            "approval_of_state",
            "approval",
        ),
        (
            effect(R, "refused", "payment", PAY_KEPT, 4, Some(P), None),
            "AgentEffect",
            "approval_of_state",
            "approval",
        ),
        (
            effect(R, "held", "payment", PAY_KEPT, 4, None, Some("ended")),
            "AgentEffect",
            "ended_not_rejected",
            "cause",
        ),
        (
            effect(
                R,
                "committed",
                "payment",
                PAY_KEPT,
                4,
                Some(P),
                Some("ended"),
            ),
            "AgentEffect",
            "ended_not_rejected",
            "cause",
        ),
        (
            effect(R, "held", "payment", PAY_KEPT, 5, None, None),
            "AgentEffect",
            "fields_of_class",
            "fields",
        ),
        (
            effect(R, "held", "payment", PAY_KEPT, 0, None, None),
            "AgentEffect",
            "fields_of_class",
            "fields",
        ),
        (
            effect(R, "held", "read", "[]", 0, None, None),
            "AgentEffect",
            "class_without_effect",
            "class",
        ),
        (
            effect(R, "held", "credential", "[]", 0, None, None),
            "AgentEffect",
            "class_without_effect",
            "class",
        ),
        (
            effect(R, "held", "purchase", PAY_KEPT, 5, None, None),
            "KeptField",
            "kept_of_class",
            "kept",
        ),
        (
            effect(
                R,
                "held",
                "send",
                r#"[{"name":"to","value":["a@mail.test"]},{"name":"body","value":["hi"]}]"#,
                3,
                None,
                None,
            ),
            "KeptField",
            "kept_of_class",
            "kept",
        ),
        (
            effect(R, "held", "send", r#"[]"#, 3, None, None),
            "KeptField",
            "kept_of_class",
            "kept",
        ),
        (
            swap(&g, r#""payee","value""#, r#""amount","value""#),
            "KeptField",
            "kept_of_class",
            "kept",
        ),
        (
            swap(&g, r#"["84.20"]"#, &format!(r#"["{}"]"#, "9".repeat(33))),
            "KeptField",
            "kept_length",
            "kept",
        ),
        (
            effect(
                R,
                "held",
                "send",
                &format!(r#"[{{"name":"to","value":["{}"]}}]"#, "a".repeat(321)),
                3,
                None,
                None,
            ),
            "KeptField",
            "kept_length",
            "kept",
        ),
        (
            swap(&g, r#""at":1794339000000000000"#, r#""at":-0"#),
            "Instant",
            "minus_zero",
            "at",
        ),
        (
            swap(&g, r#""at":1794339000000000000"#, r#""at":1.5"#),
            "Instant",
            "not_a_count",
            "at",
        ),
        (
            swap(
                &g,
                r#""at":1794339000000000000"#,
                r#""at":9223372036854775808"#,
            ),
            "Instant",
            "range",
            "at",
        ),
        (
            swap(
                &g,
                r#""at":1794339000000000000"#,
                r#""at":"1794339000000000000""#,
            ),
            "Instant",
            "not_a_count",
            "at",
        ),
        (
            swap(&g, r#""seq":1"#, r#""seq":0"#),
            "number",
            "zero",
            "seq",
        ),
        (
            swap(&g, &digest(), &digest().to_uppercase()),
            "Digest",
            "hex64",
            "digest",
        ),
        (
            swap(&g, r#""cause":null"#, r#""cause":"ok""#),
            "Cause",
            "unknown_name",
            "cause",
        ),
        (
            swap(&g, r#""state":"held""#, r#""state":"ended""#),
            "EffectState",
            "unknown_name",
            "state",
        ),
        (
            swap(&g, r#""state":"held""#, r#""state":"Held""#),
            "EffectState",
            "unknown_name",
            "state",
        ),
        (
            swap(&g, r#""approval":null"#, r#""approval":"approved""#),
            "ApprovalId",
            "hex32",
            "approval",
        ),
        (
            swap(&g, r#""v":1,"#, r#""v":1,"bound":null,"#),
            "AgentEffect",
            "unknown_key",
            "-",
        ),
    ];
    for (text, type_name, rule, key) in cases {
        expect(AgentEffect::parse(text.as_bytes()), type_name, rule, key);
    }
    let low = swap(
        &g,
        r#""at":1794339000000000000"#,
        r#""at":-9223372036854775808"#,
    );
    assert!(AgentEffect::parse(low.as_bytes()).is_ok());
}

#[test]
fn a_log_line_holds_figures_and_no_text() {
    let good = run_record(R, LOG_REST);
    assert_eq!(
        AgentLog::parse(good.as_bytes()).unwrap().to_json().unwrap(),
        good
    );
    let bare = run_record(
        R,
        r#""seq":2,"day":20768,"at":0,"event":"stopped","step":null,"cause":"session_size","model":null,"ms":null,"tokens_in":null,"tokens_out":null"#,
    );
    assert!(AgentLog::parse(bare.as_bytes()).is_ok());
    expect(
        AgentLog::parse(swap(&good, r#""event":"model_call""#, r#""event":"prompt""#).as_bytes()),
        "LogEvent",
        "unknown_name",
        "event",
    );
    expect(
        AgentLog::parse(swap(&good, r#""step":3"#, r#""step":0"#).as_bytes()),
        "number",
        "zero",
        "step",
    );
    expect(
        AgentLog::parse(swap(&good, r#""ms":812"#, r#""ms":-0"#).as_bytes()),
        "count",
        "not_a_count",
        "ms",
    );
    expect(
        AgentLog::parse(swap(&good, r#""v":1,"#, r#""v":1,"prompt":null,"#).as_bytes()),
        "AgentLog",
        "unknown_key",
        "-",
    );
    expect(
        AgentLog::parse(swap(&good, r#""cause":null"#, r#""cause":"held""#).as_bytes()),
        "Cause",
        "unknown_name",
        "cause",
    );
}

#[test]
fn a_part_before_the_last_of_48_pieces_with_a_short_last_piece_is_refused() {
    let mut pieces = vec![format!("\"{}\"", "A".repeat(1024)); 47];
    pieces.push("\"QQ==\"".to_owned());
    let data = pieces.join(",");
    let first = session(&format!(
        r#""rev":1,"part":1,"of":2,"day":20768,"data":[{data}]"#
    ));
    expect(
        AgentSession::parse(first.as_bytes()),
        "AgentSession",
        "part_not_full",
        "data",
    );
    let body = format!(
        r#"{{"op":"put_session","agent":"{A}","run":"{R}","origin":"{U}","rev":1,"part":1,"of":2,"data":[{data}]}}"#
    );
    expect(
        RunnerCall::parse(body.as_bytes()),
        "PutSession",
        "part_not_full",
        "data",
    );
    let last = session(&format!(
        r#""rev":1,"part":2,"of":2,"day":20768,"data":[{data}]"#
    ));
    assert!(AgentSession::parse(last.as_bytes()).is_ok());
}

#[test]
fn a_session_part_is_part_of_of_at_most_64_full_but_the_last() {
    let good = session(r#""rev":1,"part":1,"of":1,"day":20768,"data":["QUJD"]"#);
    assert_eq!(
        AgentSession::parse(good.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        good
    );
    let full = format!(
        r#"[{}]"#,
        vec![format!("\"{}\"", "A".repeat(1024)); 48].join(",")
    );
    let first_of_two = session(&format!(
        r#""rev":1,"part":1,"of":2,"day":20768,"data":{full}"#
    ));
    assert!(AgentSession::parse(first_of_two.as_bytes()).is_ok());
    let last_of_64 = session(r#""rev":1,"part":64,"of":64,"day":20768,"data":["QUJD"]"#);
    assert!(AgentSession::parse(last_of_64.as_bytes()).is_ok());
    expect(
        AgentSession::parse(
            session(r#""rev":1,"part":2,"of":1,"day":20768,"data":["QUJD"]"#).as_bytes(),
        ),
        "AgentSession",
        "part_above_of",
        "part",
    );
    expect(
        AgentSession::parse(
            session(r#""rev":1,"part":1,"of":65,"day":20768,"data":["QUJD"]"#).as_bytes(),
        ),
        "AgentSession",
        "parts_max",
        "of",
    );
    expect(
        AgentSession::parse(
            session(r#""rev":1,"part":1,"of":2,"day":20768,"data":["QUJD"]"#).as_bytes(),
        ),
        "AgentSession",
        "part_not_full",
        "data",
    );
    expect(
        AgentSession::parse(
            session(r#""rev":1,"part":1,"of":1,"day":20768,"data":["cookie=1;path"]"#).as_bytes(),
        ),
        "SessionData",
        "base64",
        "data",
    );
    expect(
        AgentSession::parse(session(r#""rev":1,"part":1,"of":1,"day":20768,"data":[]"#).as_bytes()),
        "SessionData",
        "piece_count",
        "data",
    );
    expect(
        AgentSession::parse(
            session(r#""rev":0,"part":1,"of":1,"day":20768,"data":["QUJD"]"#).as_bytes(),
        ),
        "number",
        "zero",
        "rev",
    );
}

#[test]
fn bindings_name_each_origin_and_each_slot_once() {
    let good = bindings(&format!("{ORIGINS},{SLOTS}"));
    assert_eq!(
        AgentBindings::parse(good.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        good
    );
    assert!(AgentBindings::parse(bindings(r#""origins":[],"slots":[]"#).as_bytes()).is_ok());
    expect(
        AgentBindings::parse(
            bindings(r#""origins":["http://a.test:1","http://a.test:1"],"slots":[]"#).as_bytes(),
        ),
        "AgentBindings",
        "repeated_name",
        "origins",
    );
    expect(AgentBindings::parse(bindings(r#""origins":[],"slots":[{"name":"payee","value":[]},{"name":"payee","value":["x"]}]"#).as_bytes()), "AgentBindings", "repeated_name", "slots");
    expect(
        AgentBindings::parse(
            bindings(r#""origins":[],"slots":[{"name":"Payee","value":[]}]"#).as_bytes(),
        ),
        "Name",
        "name_form",
        "name",
    );
    expect(
        AgentBindings::parse(bindings(r#""origins":["https://a.test"],"slots":[]"#).as_bytes()),
        "WebOrigin",
        "origin_form",
        "origins",
    );
    expect(
        AgentBindings::parse(
            bindings(r#""origins":[],"slots":[{"name":"payee","value":[],"from":"slot"}]"#)
                .as_bytes(),
        ),
        "Slot",
        "unknown_key",
        "slots",
    );
}

#[test]
fn a_playbook_row_holds_one_step_with_its_number() {
    let good = format!(
        r#"{{"v":1,"workflow":"{W}","agent":"{A}","number":1,"n":1,"step":{}}}"#,
        wait(1)
    );
    assert_eq!(
        AgentPlaybook::parse(good.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        good
    );
    let other = swap(&good, r#""n":1,"step""#, r#""n":2,"step""#);
    expect(
        AgentPlaybook::parse(other.as_bytes()),
        "AgentPlaybook",
        "n_of_step",
        "n",
    );
    let not_a_step = swap(&good, &wait(1), r#"{"verb":"wait"}"#);
    expect(
        AgentPlaybook::parse(not_a_step.as_bytes()),
        "ExecutableStep",
        "missing_key",
        "v",
    );
    let other_key = swap(
        &good,
        &wait(1),
        &wait(1).replace(r#""v":1,"#, r#""v":1,"op":null,"#),
    );
    expect(
        AgentPlaybook::parse(other_key.as_bytes()),
        "ExecutableStep",
        "unknown_key",
        "step",
    );
}

fn runner_bodies() -> Vec<String> {
    let session_part = format!(r#""origin":"{U}","rev":2,"part":1,"of":1,"data":["QUJD"]"#);
    vec![
        format!(r#"{{"op":"load","agent":"{A}","run":"{R}"}}"#),
        format!(r#"{{"op":"load_steps","agent":"{A}","run":"{R}","number":1,"from":1}}"#),
        format!(
            r#"{{"op":"load_session","agent":"{A}","run":"{R}","origin":"{U}","rev":1,"part":1}}"#
        ),
        format!(
            r#"{{"op":"put_playbook","agent":"{A}","run":"{R}","number":1,"steps":[{},{}]}}"#,
            wait(1),
            wait(2)
        ),
        format!(r#"{{"op":"put_session","agent":"{A}","run":"{R}",{session_part}}}"#),
        format!(
            r#"{{"op":"effect","agent":"{A}","run":"{R}","record":{}}}"#,
            held()
        ),
        format!(
            r#"{{"op":"log","agent":"{A}","run":"{R}","records":[{}]}}"#,
            run_record(R, LOG_REST)
        ),
        format!(
            r#"{{"op":"finish","agent":"{A}","run":"{R}","record":{}}}"#,
            run_record(R, RUN_REST)
        ),
    ]
}

#[test]
fn every_runner_body_reads_writes_back_and_travels_in_a_runner_request() {
    let ops = [
        "load",
        "load_steps",
        "load_session",
        "put_playbook",
        "put_session",
        "effect",
        "log",
        "finish",
    ];
    for (text, op) in runner_bodies().iter().zip(ops) {
        let call = RunnerCall::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(call.op(), op);
        assert_eq!(call.to_json().unwrap(), *text);
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
    }
}

#[test]
fn a_runner_body_is_refused_for_each_rule() {
    let bodies = runner_bodies();
    let body = |op: &str| {
        bodies
            .iter()
            .find(|b| b.starts_with(&format!(r#"{{"op":"{op}""#)))
            .unwrap()
            .clone()
    };
    let cases: Vec<(String, &str, &str, &str)> = vec![
        (
            format!(r#"{{"op":"list_agents","agent":"{A}","run":"{R}"}}"#),
            "RunnerCall",
            "unknown_op",
            "op",
        ),
        (
            format!(r#"{{"op":"create_agent","workflow":"{W}","agent":"{A}"}}"#),
            "RunnerCall",
            "unknown_op",
            "op",
        ),
        (
            format!(r#"{{"op":"Load","agent":"{A}","run":"{R}"}}"#),
            "RunnerCall",
            "unknown_op",
            "op",
        ),
        (
            format!(r#"{{"op":1,"agent":"{A}","run":"{R}"}}"#),
            "RunnerCall",
            "not_a_string",
            "op",
        ),
        (
            format!(r#"{{"agent":"{A}","run":"{R}"}}"#),
            "RunnerCall",
            "missing_key",
            "op",
        ),
        (
            format!(r#"["load","{A}","{R}"]"#),
            "RunnerCall",
            "not_an_object",
            "-",
        ),
        (
            format!(r#"{{"op":"load","agent":"{A}","run":"{R}","number":1}}"#),
            "Load",
            "unknown_key",
            "-",
        ),
        (
            format!(r#"{{"op":"load","op":"load","agent":"{A}","run":"{R}"}}"#),
            "RunnerCall",
            "repeated_key",
            "-",
        ),
        (
            format!(r#"{{"op":"load","agent":"{A}","run":null}}"#),
            "RunId",
            "not_a_string",
            "run",
        ),
        (
            format!(r#"{{"op":"load","agent":"{A}"}}"#),
            "Load",
            "missing_key",
            "run",
        ),
        (
            format!(r#"{{"op":"load_steps","agent":"{A}","run":"{R}","number":1,"from":0}}"#),
            "number",
            "zero",
            "from",
        ),
        (
            format!(
                r#"{{"op":"load_session","agent":"{A}","run":"{R}","origin":"{U}","rev":1,"part":65}}"#
            ),
            "LoadSession",
            "parts_max",
            "part",
        ),
        (
            format!(r#"{{"op":"put_playbook","agent":"{A}","run":"{R}","number":1,"steps":[]}}"#),
            "PutPlaybook",
            "no_step",
            "steps",
        ),
        (
            format!(
                r#"{{"op":"put_playbook","agent":"{A}","run":"{R}","number":1,"steps":[{}]}}"#,
                (1..=17).map(wait).collect::<Vec<_>>().join(",")
            ),
            "PutPlaybook",
            "steps_per_request",
            "steps",
        ),
        (
            format!(
                r#"{{"op":"put_playbook","agent":"{A}","run":"{R}","number":1,"steps":[{},{}]}}"#,
                wait(1),
                wait(3)
            ),
            "PutPlaybook",
            "number_order",
            "steps",
        ),
        (
            format!(
                r#"{{"op":"put_playbook","agent":"{A}","run":"{R}","number":1,"steps":[{},{}]}}"#,
                wait(2),
                wait(1)
            ),
            "PutPlaybook",
            "number_order",
            "steps",
        ),
        (
            swap(
                &body("put_session"),
                r#""part":1,"of":1"#,
                r#""part":1,"of":2"#,
            ),
            "PutSession",
            "part_not_full",
            "data",
        ),
        (
            swap(
                &body("effect"),
                &format!(r#""run":"{R}","record""#),
                &format!(r#""run":"{E}","record""#),
            ),
            "PutEffect",
            "record_of_another_run",
            "record",
        ),
        (
            swap(
                &body("effect"),
                &format!(r#""workflow":"{W}","agent":"{A}""#),
                &format!(r#""workflow":"{W}","agent":"{P}""#),
            ),
            "PutEffect",
            "record_of_another_run",
            "record",
        ),
        (
            swap(
                &body("finish"),
                &format!(r#""run":"{R}","record""#),
                &format!(r#""run":"{E}","record""#),
            ),
            "Finish",
            "record_of_another_run",
            "record",
        ),
        (
            swap(
                &body("log"),
                &format!(r#""run":"{R}","records""#),
                &format!(r#""run":"{E}","records""#),
            ),
            "PutLog",
            "record_of_another_run",
            "record",
        ),
        (
            format!(r#"{{"op":"log","agent":"{A}","run":"{R}","records":[]}}"#),
            "PutLog",
            "no_record",
            "records",
        ),
        (
            swap(&body("effect"), r#""state":"held""#, r#""state":"done""#),
            "EffectState",
            "unknown_name",
            "state",
        ),
    ];
    for (text, type_name, rule, key) in cases {
        expect(RunnerCall::parse(text.as_bytes()), type_name, rule, key);
    }
    let error = serde_json::from_str::<RunnerCall>(r#"{"op":"zq_secret_4711","agent":"x"}"#)
        .unwrap_err()
        .to_string();
    assert!(!error.contains("4711") && !error.contains("zq_"), "{error}");
    let broken = serde_json::from_str::<RunnerCall>(r#"{"op":"zq_secret_4711""#)
        .unwrap_err()
        .to_string();
    assert!(!broken.contains("4711"), "{broken}");
}

#[test]
fn a_step_or_a_first_step_numbered_above_64_is_refused_with_length() {
    let load_steps = |from: u32| {
        format!(r#"{{"op":"load_steps","agent":"{A}","run":"{R}","number":1,"from":{from}}}"#)
    };
    assert!(RunnerCall::parse(load_steps(64).as_bytes()).is_ok());
    let refusal = RunnerCall::parse(load_steps(65).as_bytes()).unwrap_err();
    assert_eq!(
        (
            refusal.cause(),
            refusal.type_name(),
            refusal.rule(),
            refusal.key()
        ),
        (
            RefusalCause::Length,
            "LoadSteps",
            "step_number",
            Some("from")
        )
    );
    let put = format!(
        r#"{{"op":"put_playbook","agent":"{A}","run":"{R}","number":1,"steps":[{}]}}"#,
        wait(65)
    );
    let refusal = RunnerCall::parse(put.as_bytes()).unwrap_err();
    assert_eq!(
        (refusal.cause(), refusal.rule()),
        (RefusalCause::Length, "step_number")
    );
    let row = format!(
        r#"{{"v":1,"workflow":"{W}","agent":"{A}","number":1,"n":65,"step":{}}}"#,
        wait(65)
    );
    assert_eq!(
        AgentPlaybook::parse(row.as_bytes()).unwrap_err().cause(),
        RefusalCause::Length
    );
}

fn coordinator_bodies() -> Vec<String> {
    vec![
        format!(r#"{{"op":"create_agent","workflow":"{W}","agent":"{A}"}}"#),
        format!(r#"{{"op":"set_bindings","workflow":"{W}","agent":"{A}",{SLOTS}}}"#),
        format!(r#"{{"op":"set_state","workflow":"{W}","agent":"{A}","state":"active"}}"#),
        format!(
            r#"{{"op":"start_run","workflow":"{W}","agent":"{A}","run":"{R}","mode":"compile"}}"#
        ),
        format!(r#"{{"op":"end_run","workflow":"{W}","agent":"{A}","run":"{R}"}}"#),
        add_origin("sign_in"),
    ]
}

fn add_origin(way: &str) -> String {
    format!(
        r#"{{"op":"add_origin","workflow":"{W}","agent":"{A}","run":"{R}","origin":"http://pay.test:18090","way":"{way}"}}"#
    )
}

#[test]
fn every_coordinator_body_reads_writes_back_and_travels_in_a_coordinator_write() {
    let ops = [
        "create_agent",
        "set_bindings",
        "set_state",
        "start_run",
        "end_run",
        "add_origin",
    ];
    assert_eq!(coordinator_bodies().len(), ops.len());
    for (text, op) in coordinator_bodies().iter().zip(ops) {
        let call =
            CoordinatorCall::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(call.op(), op);
        assert_eq!(call.to_json().unwrap(), *text);
        let write = CoordinatorWrite::new(
            &CoordinatorRole::claim(),
            RequestId::from_random([9; 16]),
            call.clone(),
        );
        let Ok(Decoded::Raw(raw)) = request::decode(&write.to_bytes().unwrap()) else {
            panic!("{op}")
        };
        assert_eq!(
            CoordinatorCall::parse(raw.body().get().as_bytes()).unwrap(),
            call
        );
        assert_eq!(serde_json::from_str::<CoordinatorCall>(text).unwrap(), call);
    }
    let cases: Vec<(String, &str, &str, &str)> = vec![
        (
            format!(r#"{{"op":"list_agents","workflow":"{W}","agent":"{A}"}}"#),
            "CoordinatorCall",
            "unknown_op",
            "op",
        ),
        (
            format!(r#"{{"op":"load","agent":"{A}","run":"{R}"}}"#),
            "CoordinatorCall",
            "unknown_op",
            "op",
        ),
        (
            format!(r#"{{"op":"set_state","workflow":"{W}","agent":"{A}","state":"proposed"}}"#),
            "AgentState",
            "unknown_name",
            "state",
        ),
        (
            format!(r#"{{"op":"set_state","workflow":"{W}","agent":"{A}","state":"dismissed"}}"#),
            "AgentState",
            "unknown_name",
            "state",
        ),
        (
            format!(
                r#"{{"op":"start_run","workflow":"{W}","agent":"{A}","run":"{R}","mode":"Replay"}}"#
            ),
            "Mode",
            "unknown_name",
            "mode",
        ),
        (
            format!(r#"{{"op":"create_agent","workflow":"{W}","agent":"{A}","run":"{R}"}}"#),
            "CreateAgent",
            "unknown_key",
            "-",
        ),
        (
            format!(r#"{{"op":"create_agent","workflow":"{W}","agent":"{A}","category":null}}"#),
            "CreateAgent",
            "unknown_key",
            "-",
        ),
        (
            format!(r#"{{"op":"end_run","workflow":"{W}","agent":"{A}"}}"#),
            "EndRun",
            "missing_key",
            "run",
        ),
        (
            format!(
                r#"{{"op":"set_bindings","workflow":"{W}","agent":"{A}","slots":[{{"name":"payee","value":[]}},{{"name":"payee","value":["x"]}}]}}"#
            ),
            "SetBindings",
            "repeated_name",
            "slots",
        ),
        (
            format!(r#"{{"workflow":"{W}","agent":"{A}"}}"#),
            "CoordinatorCall",
            "missing_key",
            "op",
        ),
    ];
    for (text, type_name, rule, key) in cases {
        expect(
            CoordinatorCall::parse(text.as_bytes()),
            type_name,
            rule,
            key,
        );
    }
}

#[test]
fn each_reply_is_told_apart_by_its_keys_and_answers_its_own_call() {
    let load = format!(
        r#"{{"config":{CONFIG},"playbook":{{"number":1,"steps":9}},"bindings":{{"rev":1,{ORIGINS},{SLOTS}}},"sessions":[{{"origin":"{U}","rev":1,"parts":1}}]}}"#
    );
    let empty_load =
        format!(r#"{{"config":{CONFIG},"playbook":null,"bindings":null,"sessions":[]}}"#);
    let steps = format!(r#"{{"steps":[{},{}]}}"#, wait(3), wait(4));
    let session_reply = format!(r#"{{"origin":"{U}","rev":1,"part":1,"of":1,"data":["QUJD"]}}"#);
    for text in [
        load.as_str(),
        empty_load.as_str(),
        steps.as_str(),
        r#"{"steps":[]}"#,
        session_reply.as_str(),
        r#"{"stored":3}"#,
        r#"{"stored":0}"#,
    ] {
        let reply = RunnerReply::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(reply.to_json().unwrap(), text);
        assert_eq!(serde_json::from_str::<RunnerReply>(text).unwrap(), reply);
    }
    let calls: Vec<RunnerCall> = runner_bodies()
        .iter()
        .map(|b| RunnerCall::parse(b.as_bytes()).unwrap())
        .collect();
    let answered = |text: &str| -> Vec<&str> {
        let reply = RunnerReply::parse(text.as_bytes()).unwrap();
        calls
            .iter()
            .filter(|c| reply.answers(c))
            .map(|c| c.op())
            .collect()
    };
    assert_eq!(answered(&load), ["load"]);
    assert_eq!(answered(&steps), ["load_steps"]);
    assert_eq!(answered(&session_reply), ["load_session"]);
    assert_eq!(
        answered(r#"{"stored":1}"#),
        ["put_playbook", "put_session", "effect", "log", "finish"]
    );
    expect(
        RunnerReply::parse(br#"{"stored":1,"steps":[]}"#),
        "StepsReply",
        "unknown_key",
        "-",
    );
    expect(
        RunnerReply::parse(br#"{}"#),
        "RunnerReply",
        "reply_form",
        "-",
    );
    expect(
        RunnerReply::parse(br#"[]"#),
        "RunnerReply",
        "not_an_object",
        "-",
    );
    expect(
        RunnerReply::parse(br#"{"stored":-1}"#),
        "count",
        "not_a_count",
        "stored",
    );
    expect(
        RunnerReply::parse(format!(r#"{{"steps":[{},{}]}}"#, wait(3), wait(5)).as_bytes()),
        "StepsReply",
        "number_order",
        "steps",
    );
    expect(
        RunnerReply::parse(
            format!(
                r#"{{"steps":[{}]}}"#,
                (1..=17).map(wait).collect::<Vec<_>>().join(",")
            )
            .as_bytes(),
        ),
        "StepsReply",
        "steps_per_request",
        "steps",
    );
    let long_playbook = swap(&load, r#""steps":9"#, r#""steps":65"#);
    let refusal = RunnerReply::parse(long_playbook.as_bytes()).unwrap_err();
    assert_eq!(
        (refusal.cause(), refusal.rule()),
        (RefusalCause::Length, "steps")
    );
    let twice = swap(
        &load,
        r#""sessions":[{"origin":"http://utility.test:18090","rev":1,"parts":1}]"#,
        r#""sessions":[{"origin":"http://utility.test:18090","rev":1,"parts":1},{"origin":"http://utility.test:18090","rev":2,"parts":1}]"#,
    );
    expect(
        RunnerReply::parse(twice.as_bytes()),
        "LoadReply",
        "repeated_name",
        "sessions",
    );
    let parts = swap(&load, r#""parts":1"#, r#""parts":65"#);
    expect(
        RunnerReply::parse(parts.as_bytes()),
        "LoadedSession",
        "parts_max",
        "parts",
    );
    let money = swap(&load, r#""allow":[]"#, r#""allow":["payment"]"#);
    expect(
        RunnerReply::parse(money.as_bytes()),
        "AllowedClass",
        "unknown_name",
        "allow",
    );

    let bodies: Vec<CoordinatorCall> = coordinator_bodies()
        .iter()
        .map(|b| CoordinatorCall::parse(b.as_bytes()).unwrap())
        .collect();
    for (text, ops) in [
        (
            r#"{"rev":1}"#,
            vec!["create_agent", "set_bindings", "add_origin"],
        ),
        (r#"{"state":"paused"}"#, vec!["set_state"]),
        (r#"{"ok":"yes"}"#, vec!["start_run", "end_run"]),
    ] {
        let reply = CoordinatorReply::parse(text.as_bytes()).unwrap();
        assert_eq!(reply.to_json().unwrap(), text);
        assert_eq!(
            serde_json::from_str::<CoordinatorReply>(text).unwrap(),
            reply
        );
        let answered: Vec<&str> = bodies
            .iter()
            .filter(|b| reply.answers(b))
            .map(|b| b.op())
            .collect();
        assert_eq!(answered, ops);
    }
    expect(
        CoordinatorReply::parse(br#"{"ok":"no"}"#),
        "Accepted",
        "tag",
        "ok",
    );
    expect(
        CoordinatorReply::parse(br#"{"ok":true}"#),
        "Accepted",
        "not_a_string",
        "ok",
    );
    expect(
        CoordinatorReply::parse(br#"{"rev":0}"#),
        "number",
        "zero",
        "rev",
    );
    expect(
        CoordinatorReply::parse(br#"{"state":"proposed"}"#),
        "AgentState",
        "unknown_name",
        "state",
    );
    expect(
        CoordinatorReply::parse(br#"{"ok":"yes","rev":1}"#),
        "RevReply",
        "unknown_key",
        "-",
    );
    expect(
        CoordinatorReply::parse(br#"{"workflows":[],"next":null}"#),
        "CoordinatorReply",
        "reply_form",
        "-",
    );
}

#[test]
fn a_set_bindings_body_that_holds_origins_is_refused() {
    let slots_alone = format!(r#"{{"op":"set_bindings","workflow":"{W}","agent":"{A}",{SLOTS}}}"#);
    let Ok(CoordinatorCall::SetBindings(body)) = CoordinatorCall::parse(slots_alone.as_bytes())
    else {
        panic!("slots alone are taken")
    };
    assert_eq!(body.slots.len(), 2);
    assert_eq!(body.to_json().unwrap(), slots_alone);
    let no_slot = format!(r#"{{"op":"set_bindings","workflow":"{W}","agent":"{A}","slots":[]}}"#);
    assert!(CoordinatorCall::parse(no_slot.as_bytes()).is_ok());
    let with =
        |keys: &str| format!(r#"{{"op":"set_bindings","workflow":"{W}","agent":"{A}",{keys}}}"#);
    for text in [
        with(&format!("{ORIGINS},{SLOTS}")),
        with(&format!("{SLOTS},{ORIGINS}")),
        with(&format!(r#""origins":[],{SLOTS}"#)),
        with(&format!(r#""origins":null,{SLOTS}"#)),
        with(&format!(r#""origins":"http://pay.test:18090",{SLOTS}"#)),
    ] {
        expect(
            CoordinatorCall::parse(text.as_bytes()),
            "SetBindings",
            "unknown_key",
            "-",
        );
    }
    let cases: Vec<(String, &str, &str, &str)> = vec![
        (
            format!(r#"["set_bindings","{W}","{A}",[]]"#),
            "CoordinatorCall",
            "not_an_object",
            "-",
        ),
        (
            with(r#""slots":[["payee",["x"]]]"#),
            "Slot",
            "not_an_object",
            "slots",
        ),
        (
            with(r#""slots":[],"slots":[]"#),
            "CoordinatorCall",
            "repeated_key",
            "-",
        ),
        (
            with(r#""slots":[{"name":"payee","name":"payee","value":[]}]"#),
            "Slot",
            "repeated_key",
            "slots",
        ),
        (with(r#""slots":null"#), "list", "not_a_list", "slots"),
        (with(r#""slots":"[]""#), "list", "not_a_list", "slots"),
        (
            with(r#""slots":[{"name":"payee","value":["x"]}],"rev":1"#),
            "SetBindings",
            "unknown_key",
            "-",
        ),
        (
            with(&format!(r#""run":"{R}",{SLOTS}"#)),
            "SetBindings",
            "unknown_key",
            "-",
        ),
        (
            with(&format!(r#""way":"sign_in",{SLOTS}"#)),
            "SetBindings",
            "unknown_key",
            "-",
        ),
        (
            with(r#""slots":[{"name":"payee","value":["x"],"origin":null}]"#),
            "Slot",
            "unknown_key",
            "slots",
        ),
        (
            format!(r#"{{"op":"set_bindings","workflow":"{W}","agent":"{A}"}}"#),
            "SetBindings",
            "missing_key",
            "slots",
        ),
        (
            with(r#""slots":[{"name":"payee","value":"x"}]"#),
            "list",
            "not_a_list",
            "value",
        ),
    ];
    for (text, type_name, rule, key) in cases {
        expect(
            CoordinatorCall::parse(text.as_bytes()),
            type_name,
            rule,
            key,
        );
    }
}

#[test]
fn an_add_origin_body_with_a_fourth_way_is_refused() {
    let names: Vec<&str> = Way::ALL.iter().map(|w| w.as_str()).collect();
    assert_eq!(names, ["sign_in", "account_link", "new_origin"]);
    for way in Way::ALL {
        assert_eq!(Way::parse(way.as_str()), Ok(*way));
        let text = add_origin(way.as_str());
        let call =
            CoordinatorCall::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
        let CoordinatorCall::AddOrigin(body) = &call else {
            panic!("{text}")
        };
        assert_eq!(
            (body.way, body.origin.as_str(), body.run.as_str()),
            (*way, "http://pay.test:18090", R)
        );
        assert_eq!(call.to_json().unwrap(), text);
        assert!(CoordinatorReply::parse(br#"{"rev":7}"#)
            .unwrap()
            .answers(&call));
        assert!(!CoordinatorReply::parse(br#"{"ok":"yes"}"#)
            .unwrap()
            .answers(&call));
        assert!(!CoordinatorReply::parse(br#"{"state":"active"}"#)
            .unwrap()
            .answers(&call));
    }
    for fourth in [
        "captcha",
        "second_factor",
        "handback",
        "Sign_in",
        "sign_in ",
        "",
        "new_origins",
        "account-link",
    ] {
        expect(
            CoordinatorCall::parse(add_origin(fourth).as_bytes()),
            "Way",
            "unknown_name",
            "way",
        );
    }
    let good = add_origin("new_origin");
    let way = r#""way":"new_origin""#;
    let at = r#""origin":"http://pay.test:18090""#;
    let run = format!(r#""run":"{R}""#);
    let cases: Vec<(String, &str, &str, &str)> = vec![
        (
            format!(r#"["add_origin","{W}","{A}","{R}","http://pay.test:18090","sign_in"]"#),
            "CoordinatorCall",
            "not_an_object",
            "-",
        ),
        (
            swap(&good, way, &format!(r#"{way},"way":"sign_in""#)),
            "CoordinatorCall",
            "repeated_key",
            "-",
        ),
        (
            swap(&good, way, r#""way":["new_origin"]"#),
            "Way",
            "not_a_string",
            "way",
        ),
        (
            swap(&good, way, r#""way":null"#),
            "Way",
            "not_a_string",
            "way",
        ),
        (
            swap(&good, &format!(",{way}"), ""),
            "AddOrigin",
            "missing_key",
            "way",
        ),
        (
            swap(&good, at, r#""origin":null"#),
            "WebOrigin",
            "not_a_string",
            "origin",
        ),
        (
            swap(&good, at, r#""origin":"https://pay.test""#),
            "WebOrigin",
            "origin_form",
            "origin",
        ),
        (
            swap(&good, at, r#""origin":"http://Pay.test:18090""#),
            "WebOrigin",
            "origin_form",
            "origin",
        ),
        (
            swap(&good, at, r#""origin":["http://pay.test:18090"]"#),
            "WebOrigin",
            "not_a_string",
            "origin",
        ),
        (
            swap(&good, &run, r#""run":50000000000000000000000000000001"#),
            "RunId",
            "not_a_string",
            "run",
        ),
        (
            swap(&good, &format!("{run},"), ""),
            "AddOrigin",
            "missing_key",
            "run",
        ),
        (
            swap(&good, &run, r#""run":null"#),
            "RunId",
            "not_a_string",
            "run",
        ),
        (
            swap(&good, way, &format!(r#"{way},"slots":[]"#)),
            "AddOrigin",
            "unknown_key",
            "-",
        ),
        (
            swap(
                &good,
                way,
                &format!(r#"{way},"origins":["http://pay.test:18090"]"#),
            ),
            "AddOrigin",
            "unknown_key",
            "-",
        ),
        (
            swap(&good, way, &format!(r#"{way},"mode":"compile""#)),
            "AddOrigin",
            "unknown_key",
            "-",
        ),
        (
            swap(&good, way, &format!(r#"{way},"answer":null"#)),
            "AddOrigin",
            "unknown_key",
            "-",
        ),
        (
            swap(&good, r#""op":"add_origin""#, r#""op":"add_origins""#),
            "CoordinatorCall",
            "unknown_op",
            "op",
        ),
    ];
    for (text, type_name, rule, key) in cases {
        expect(
            CoordinatorCall::parse(text.as_bytes()),
            type_name,
            rule,
            key,
        );
    }
    expect(
        RunnerCall::parse(good.as_bytes()),
        "RunnerCall",
        "unknown_op",
        "op",
    );
}

#[test]
fn a_log_line_ends_a_run_with_the_cause_secret_and_no_other_spelling() {
    let event = r#""event":"model_call","step":3,"cause":null"#;
    let stopped = run_record(
        R,
        &swap(
            LOG_REST,
            event,
            r#""event":"stopped","step":3,"cause":"secret""#,
        ),
    );
    let line = AgentLog::parse(stopped.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(line.cause, Some(Cause::Secret));
    assert_eq!(line.to_json().unwrap(), stopped);
    assert_eq!(Cause::parse("secret"), Ok(Cause::Secret));
    assert_eq!(
        Cause::ALL.iter().filter(|c| c.as_str() == "secret").count(),
        1
    );
    for other in ["Secret", "secrets", "unreadable", "locked", "fault", "none"] {
        let text = run_record(
            R,
            &swap(
                LOG_REST,
                event,
                &format!(r#""event":"stopped","step":3,"cause":"{other}""#),
            ),
        );
        expect(
            AgentLog::parse(text.as_bytes()),
            "Cause",
            "unknown_name",
            "cause",
        );
    }
}

#[test]
fn the_reply_of_create_agent_is_revision_1_and_set_bindings_and_add_origin_take_any_revision() {
    let bodies: Vec<CoordinatorCall> = coordinator_bodies()
        .iter()
        .map(|b| CoordinatorCall::parse(b.as_bytes()).unwrap())
        .collect();
    let answered = |text: &str| -> Vec<&str> {
        let reply = CoordinatorReply::parse(text.as_bytes()).unwrap();
        bodies
            .iter()
            .filter(|b| reply.answers(b))
            .map(|b| b.op())
            .collect()
    };
    assert_eq!(
        answered(r#"{"rev":1}"#),
        ["create_agent", "set_bindings", "add_origin"]
    );
    for rev in [2, 7, 4_294_967_295_u32] {
        assert_eq!(
            answered(&format!(r#"{{"rev":{rev}}}"#)),
            ["set_bindings", "add_origin"],
            "{rev}"
        );
    }
}

#[test]
fn a_load_reply_whose_bindings_name_an_origin_twice_is_refused() {
    let load = |origins: &str| {
        format!(
            r#"{{"config":{CONFIG},"playbook":null,"bindings":{{"rev":3,{origins},{SLOTS}}},"sessions":[]}}"#
        )
    };
    assert!(RunnerReply::parse(load(ORIGINS).as_bytes()).is_ok());
    expect(
        RunnerReply::parse(
            load(r#""origins":["http://pay.test:18090","http://pay.test:18090"]"#).as_bytes(),
        ),
        "LoadedBindings",
        "repeated_name",
        "origins",
    );
    expect(
        RunnerReply::parse(
            load(r#""origins":["http://pay.test:18090","http://utility.test:18090","http://pay.test:18090"]"#)
                .as_bytes(),
        ),
        "LoadedBindings",
        "repeated_name",
        "origins",
    );
}

#[test]
fn every_reply_of_a_runner_body_is_counted_on_the_bytes_given_by_its_own_parse_and_its_readers() {
    type Parse = fn(&[u8]) -> Result<(), Refusal>;
    let load = format!(r#"{{"config":{CONFIG},"playbook":null,"bindings":null,"sessions":[]}}"#);
    let steps = format!(r#"{{"steps":[{}]}}"#, wait(3));
    let session = format!(r#"{{"origin":"{U}","rev":1,"part":1,"of":1,"data":["QUJD"]}}"#);
    let cases: [(&str, &str, Parse); 4] = [
        (&load, "LoadReply", |b| LoadReply::parse(b).map(drop)),
        (&steps, "StepsReply", |b| StepsReply::parse(b).map(drop)),
        (&session, "SessionReply", |b| {
            SessionReply::parse(b).map(drop)
        }),
        (r#"{"stored":1}"#, "Stored", |b| Stored::parse(b).map(drop)),
    ];
    for (text, type_name, parse) in cases {
        let room = CHECKPOINTS_REPLY_MAX - text.len();
        let inside = |n: usize| format!("{{{}{}", " ".repeat(n), &text[1..]);
        let at = inside(room);
        assert_eq!(at.len(), CHECKPOINTS_REPLY_MAX);
        assert!(parse(at.as_bytes()).is_ok(), "{type_name}");
        assert!(RunnerReply::parse(at.as_bytes()).is_ok(), "{type_name}");
        assert!(
            serde_json::from_str::<RunnerReply>(&at).is_ok(),
            "{type_name}"
        );
        for over in [
            format!("{}{text}", " ".repeat(room + 1)),
            format!("{text}{}", "\n".repeat(room + 1)),
            inside(room + 1),
        ] {
            for (refusal, name) in [
                (parse(over.as_bytes()).expect_err(type_name), type_name),
                (
                    RunnerReply::parse(over.as_bytes()).expect_err(type_name),
                    "RunnerReply",
                ),
            ] {
                assert_eq!(
                    (refusal.cause(), refusal.type_name(), refusal.rule()),
                    (RefusalCause::Length, name, "reply_bytes"),
                    "{type_name}"
                );
            }
        }
        let through_serde = serde_json::from_str::<RunnerReply>(&inside(room + 1))
            .expect_err(type_name)
            .to_string();
        assert!(
            through_serde.contains("length: RunnerReply: -: reply_bytes"),
            "{type_name}: {through_serde}"
        );
    }
}

fn refused_with(
    result: Result<impl std::fmt::Debug, Refusal>,
    cause: RefusalCause,
    type_name: &str,
    rule: &str,
) {
    let refusal = result.expect_err(type_name);
    assert_eq!(
        (refusal.cause(), refusal.type_name(), refusal.rule()),
        (cause, type_name, rule)
    );
}

#[test]
fn a_runner_reply_one_byte_over_its_bound_is_refused_before_any_byte_of_it_is_read() {
    let text = r#"{"stored":1}"#;
    let fill = |len: usize| " ".repeat(len - text.len() - 1);
    for len in [CHECKPOINTS_REPLY_MAX, CHECKPOINTS_REPLY_MAX + 1] {
        let not_json_at_first = format!("x{}{text}", fill(len));
        let broken_at_last = format!("{text}{}x", fill(len));
        for input in [not_json_at_first, broken_at_last] {
            assert_eq!(input.len(), len);
            let parsed = RunnerReply::parse(input.as_bytes());
            if len > CHECKPOINTS_REPLY_MAX {
                refused_with(parsed, RefusalCause::Length, "RunnerReply", "reply_bytes");
            } else {
                refused_with(parsed, RefusalCause::Malformed, "RunnerReply", "json");
            }
        }
    }
}
