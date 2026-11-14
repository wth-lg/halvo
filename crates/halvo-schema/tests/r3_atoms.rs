use halvo_schema::executable::names::{
    ActionClass, AgentState, AllowedClass, Answer, ApiOp, Autonomy, Cause, Check, EffectState,
    FieldName, HandbackOutcome, HandbackReason, LocatorBy, LogEvent, Mode, ModelOutput, ModelTier,
    Outcome, Period, Permission, Reasoning, Runtime, Source, StepEffect, StepKind, Verb, YesNo,
};
use halvo_schema::executable::text::{
    ApprovalId, Digest, EffectId, Endpoint, Name, OwnText, OwnValue, RunId, SecretText,
    SessionData, WebOrigin,
};
use halvo_schema::executable::{Refusal, RefusalCause};

fn malformed(result: Result<impl std::fmt::Debug, Refusal>, type_name: &str, rule: &str) {
    let refusal = result.expect_err("refused");
    assert_eq!(refusal.cause(), RefusalCause::Malformed);
    assert_eq!((refusal.type_name(), refusal.rule()), (type_name, rule));
}

macro_rules! names_of {
    ($t:ty) => {
        <$t>::ALL.iter().map(|n| n.as_str()).collect::<Vec<_>>()
    };
}

#[test]
fn every_closed_list_holds_the_names_of_the_contract_in_its_order() {
    assert_eq!(
        names_of!(ActionClass),
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
            "credential"
        ]
    );
    assert_eq!(names_of!(AllowedClass), ["send", "delete", "submit"]);
    assert_eq!(
        names_of!(Autonomy),
        ["observe", "draft_and_confirm", "unattended"]
    );
    assert_eq!(
        names_of!(ModelTier),
        ["on_device", "low_cost_hosted", "frontier"]
    );
    assert_eq!(names_of!(Reasoning), ["low", "medium", "high"]);
    assert_eq!(
        names_of!(Runtime),
        ["web", "graph_mail", "graph_calendar", "file", "none"]
    );
    assert_eq!(names_of!(Permission), ["user_files"]);
    assert_eq!(names_of!(Period), ["week", "month"]);
    assert_eq!(names_of!(YesNo), ["yes", "no"]);
    assert_eq!(
        names_of!(EffectState),
        [
            "refused",
            "cleared",
            "held",
            "approved",
            "rejected",
            "releasing",
            "committed",
            "unknown"
        ]
    );
    assert_eq!(
        names_of!(Cause),
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
            "secret"
        ]
    );
    assert_eq!(
        names_of!(Outcome),
        [
            "completed",
            "nothing_to_do",
            "declined",
            "handed_back",
            "class_mismatch",
            "refused",
            "limit",
            "effect_unknown",
            "failed"
        ]
    );
    assert_eq!(names_of!(Mode), ["compile", "replay"]);
    assert_eq!(
        names_of!(LogEvent),
        [
            "step_ok",
            "step_failed",
            "recovered",
            "handback",
            "confirm",
            "model_call",
            "checkpoint",
            "stopped"
        ]
    );
    assert_eq!(
        names_of!(HandbackReason),
        [
            "sign_in",
            "second_factor",
            "captcha",
            "account_link",
            "step_failed",
            "effect_unknown"
        ]
    );
    assert_eq!(names_of!(HandbackOutcome), ["done", "declined", "timeout"]);
    assert_eq!(names_of!(Answer), ["allow_once", "reject_once"]);
    assert_eq!(names_of!(AgentState), ["approved", "active", "paused"]);
    assert_eq!(names_of!(StepKind), ["web", "api", "file"]);
    assert_eq!(
        names_of!(Verb),
        [
            "navigate", "click", "fill", "select", "press", "upload", "extract", "wait",
            "handback", "call", "read", "write"
        ]
    );
    assert_eq!(names_of!(StepEffect), ["none", "request"]);
    assert_eq!(
        names_of!(LocatorBy),
        ["role", "label", "text", "test_id", "css"]
    );
    assert_eq!(
        names_of!(ApiOp),
        [
            "mail_list",
            "mail_draft_reply",
            "mail_send",
            "calendar_list",
            "calendar_delete"
        ]
    );
    assert_eq!(names_of!(Source), ["const", "slot", "extract", "model"]);
    assert_eq!(
        names_of!(Check),
        ["url_is", "present", "enabled", "text_is", "value_is", "count_is"]
    );
    assert_eq!(names_of!(ModelOutput), ["reply_body"]);
    assert_eq!(
        names_of!(FieldName),
        [
            "payee",
            "amount",
            "instrument",
            "origin",
            "seller",
            "items",
            "to",
            "subject",
            "body",
            "fields",
            "document",
            "party",
            "file",
            "range"
        ]
    );
}

#[test]
fn a_closed_list_reads_its_own_names_and_refuses_a_sibling_lists_names() {
    for class in ActionClass::ALL {
        assert_eq!(ActionClass::parse(class.as_str()), Ok(*class));
    }
    for state in EffectState::ALL {
        assert_eq!(EffectState::parse(state.as_str()), Ok(*state));
        if Cause::parse(state.as_str()).is_err() {
            malformed(Cause::parse(state.as_str()), "Cause", "unknown_name");
        }
    }
    for name in ["held", "cleared", "approved", "releasing", "committed"] {
        malformed(Cause::parse(name), "Cause", "unknown_name");
    }
    for name in ["ended", "changed", "used", "digest"] {
        malformed(EffectState::parse(name), "EffectState", "unknown_name");
    }
    for name in ["compile", "replay", "observe"] {
        malformed(Outcome::parse(name), "Outcome", "unknown_name");
    }
    for name in ["approved", "active"] {
        malformed(Mode::parse(name), "Mode", "unknown_name");
    }
    malformed(AgentState::parse("proposed"), "AgentState", "unknown_name");
    malformed(AgentState::parse("dismissed"), "AgentState", "unknown_name");
    malformed(ModelTier::parse("observe"), "ModelTier", "unknown_name");
    malformed(Autonomy::parse("on_device"), "Autonomy", "unknown_name");
    malformed(Runtime::parse("graph"), "Runtime", "unknown_name");
    malformed(Source::parse("page"), "Source", "unknown_name");
    malformed(
        HandbackReason::parse("password"),
        "HandbackReason",
        "unknown_name",
    );
    malformed(Answer::parse("allow_always"), "Answer", "unknown_name");
    malformed(Answer::parse("allow"), "Answer", "unknown_name");
    malformed(ApiOp::parse("mail_delete"), "ApiOp", "unknown_name");
    malformed(Verb::parse("type"), "Verb", "unknown_name");
    malformed(LocatorBy::parse("xpath"), "LocatorBy", "unknown_name");
    malformed(ModelOutput::parse("amount"), "ModelOutput", "unknown_name");
    malformed(FieldName::parse("password"), "FieldName", "unknown_name");
    malformed(Period::parse("day"), "Period", "unknown_name");
    malformed(Permission::parse("network"), "Permission", "unknown_name");
}

#[test]
fn a_name_with_an_upper_case_letter_or_a_space_is_no_name_of_a_list() {
    for (word, refused) in [
        ("Send", AllowedClass::parse("Send").is_err()),
        ("SEND", AllowedClass::parse("SEND").is_err()),
        ("send ", AllowedClass::parse("send ").is_err()),
        (" send", AllowedClass::parse(" send").is_err()),
        ("Payment", ActionClass::parse("Payment").is_err()),
        ("Held", EffectState::parse("Held").is_err()),
        ("Ended", Cause::parse("Ended").is_err()),
        (
            "Draft_And_Confirm",
            Autonomy::parse("Draft_And_Confirm").is_err(),
        ),
        ("", YesNo::parse("").is_err()),
        ("YES", YesNo::parse("YES").is_err()),
        ("true", YesNo::parse("true").is_err()),
    ] {
        assert!(refused, "{word}");
    }
}

#[test]
fn the_classes_have_the_money_and_confirm_sets_of_the_contract() {
    let money: Vec<&str> = ActionClass::ALL
        .iter()
        .filter(|c| c.is_money())
        .map(|c| c.as_str())
        .collect();
    assert_eq!(money, ["purchase", "payment"]);
    let confirm: Vec<&str> = ActionClass::ALL
        .iter()
        .filter(|c| c.is_confirm())
        .map(|c| c.as_str())
        .collect();
    assert_eq!(
        confirm,
        ["send", "delete", "submit", "purchase", "payment", "sign"]
    );
    for allowed in AllowedClass::ALL {
        let class = ActionClass::from(*allowed);
        assert_eq!(class.as_str(), allowed.as_str());
        assert!(!class.is_money() && class != ActionClass::Sign);
    }
    for state in EffectState::ALL {
        assert_eq!(
            state.is_final(),
            ["refused", "rejected", "committed", "unknown"].contains(&state.as_str())
        );
    }
}

#[test]
fn the_classes_of_a_request_of_the_browser_are_submit_purchase_payment_and_sign() {
    let browser: Vec<&str> = ActionClass::ALL
        .iter()
        .filter(|c| c.is_browser_effect())
        .map(|c| c.as_str())
        .collect();
    assert_eq!(browser, ["submit", "purchase", "payment", "sign"]);
}

#[test]
fn own_text_holds_at_most_1024_bytes() {
    assert_eq!(
        OwnText::parse(&"a".repeat(1024)).unwrap().as_str().len(),
        1024
    );
    assert_eq!(OwnText::parse("").unwrap().as_str(), "");
    malformed(
        OwnText::parse(&"a".repeat(1025)),
        "OwnText",
        "own_text_length",
    );
    malformed(
        OwnText::parse(&"\u{e9}".repeat(513)),
        "OwnText",
        "own_text_length",
    );
    assert!(OwnText::parse(&"\u{e9}".repeat(512)).is_ok());
}

#[test]
fn an_own_value_is_cut_into_the_longest_pieces_at_character_bounds() {
    assert!(OwnValue::new("").pieces().is_empty());
    assert_eq!(OwnValue::new("abc").pieces(), ["abc"]);
    let exact = "a".repeat(1024);
    assert_eq!(OwnValue::new(&exact).pieces(), [exact.as_str()]);
    let long = format!("{}{}", "a".repeat(1023), "\u{e9}bc");
    let value = OwnValue::new(&long);
    assert_eq!(value.pieces().len(), 2);
    assert_eq!(value.pieces()[0].len(), 1023);
    assert_eq!(value.pieces()[1], "\u{e9}bc");
    assert_eq!(value.joined(), long);
    assert_eq!(value.len(), long.len());
    let big = "x".repeat(3000);
    let pieces: Vec<usize> = OwnValue::new(&big)
        .pieces()
        .iter()
        .map(String::len)
        .collect();
    assert_eq!(pieces, [1024, 1024, 952]);
}

#[test]
fn debug_of_a_text_type_prints_no_value() {
    let shown = format!(
        "{:?} {:?} {:?} {:?}",
        OwnText::parse("Northwind 4711").unwrap(),
        OwnValue::new("Northwind 4711"),
        SecretText::parse("tok_4711").unwrap(),
        SessionData::new(vec!["QUJD".to_owned()]).unwrap()
    );
    assert!(
        !shown.contains("4711") && !shown.contains("QUJD"),
        "{shown}"
    );
}

#[test]
fn a_name_is_1_to_32_bytes_of_lower_case_letters_digits_and_underscore() {
    for good in ["a", "reply_senders", "amount_due", "a1", &"z".repeat(32)] {
        assert_eq!(Name::parse(good).unwrap().as_str(), good);
    }
    for bad in [
        "",
        "A",
        "Amount",
        "a-b",
        "a.b",
        "a b",
        "\u{e9}",
        &"z".repeat(33),
    ] {
        malformed(Name::parse(bad), "Name", "name_form");
    }
}

#[test]
fn a_secret_holds_at_least_one_byte() {
    malformed(SecretText::parse(""), "SecretText", "empty_secret");
    assert_eq!(SecretText::parse("t").unwrap().expose(), "t");
    assert_eq!(SecretText::parse(" ").unwrap().expose(), " ");
}

#[test]
fn a_web_origin_is_scheme_host_and_port_in_lower_case() {
    for good in [
        "http://127.0.0.1:18090",
        "https://utility.test:443",
        "http://pay.utility.test:1",
        "https://a-b.example:65535",
    ] {
        assert_eq!(WebOrigin::parse(good).unwrap().as_str(), good);
    }
    for bad in [
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
    ] {
        malformed(WebOrigin::parse(bad), "WebOrigin", "origin_form");
    }
    let label = "a".repeat(64);
    malformed(
        WebOrigin::parse(&format!("http://{label}.test:80")),
        "WebOrigin",
        "origin_form",
    );
}

#[test]
fn a_web_origin_whose_host_is_a_number_form_of_an_ipv4_address_is_refused() {
    for bad in [
        "http://127.1:80",
        "http://0x7f000001:80",
        "http://2130706433:80",
        "http://0177.0.0.1:80",
        "http://1.2.3.04:80",
        "http://127.0.0.1.1:80",
        "http://256.0.0.1:80",
        "http://0x7f.0.0.1:80",
        "https://pay.123:443",
        "https://pay.0x:443",
        "https://pay.0xff:443",
        "http://1:80",
        "http://00.0.0.0:80",
    ] {
        malformed(WebOrigin::parse(bad), "WebOrigin", "origin_ipv4");
        malformed(WebOrigin::of_address(bad), "WebOrigin", "address_ipv4");
    }
    for good in [
        "http://127.0.0.1:18090",
        "http://0.0.0.0:80",
        "http://255.255.255.255:65535",
        "http://1.2.3.a:80",
        "https://pay.0xg:443",
        "https://0x7f000001.test:443",
        "https://127.1.test:443",
        "http://utility.agent.test:18090",
        "http://pay.agent.test:18090",
        "http://control.agent.test:18090",
    ] {
        assert_eq!(WebOrigin::parse(good).unwrap().as_str(), good);
        assert_eq!(WebOrigin::of_address(good).unwrap().as_str(), good);
    }
}

#[test]
fn a_host_holds_at_most_253_bytes() {
    let labels = |last: usize| {
        format!(
            "{}.{}.{}.{}",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(last)
        )
    };
    assert_eq!(labels(61).len(), 253);
    let taken = format!("http://{}:80", labels(61));
    assert_eq!(WebOrigin::parse(&taken).unwrap().as_str(), taken);
    malformed(
        WebOrigin::parse(&format!("http://{}:80", labels(62))),
        "WebOrigin",
        "origin_form",
    );
}

#[test]
fn a_digest_is_64_lower_case_hex_digits() {
    let hex = "0123456789abcdef".repeat(4);
    assert_eq!(Digest::parse(&hex).unwrap().as_str(), hex);
    malformed(Digest::parse(&hex.to_uppercase()), "Digest", "hex64");
    malformed(Digest::parse(&hex[..63]), "Digest", "hex64");
    malformed(Digest::parse(&format!("{hex}0")), "Digest", "hex64");
    malformed(Digest::parse(&hex.replace('a', "g")), "Digest", "hex64");
}

#[test]
fn the_three_ids_of_this_run_are_32_hex_digits_from_16_random_bytes() {
    let bytes = [0xab_u8; 16];
    assert_eq!(RunId::from_random(bytes).as_str(), "ab".repeat(16));
    assert_eq!(EffectId::from_random([0; 16]).as_str(), "0".repeat(32));
    assert_eq!(
        ApprovalId::from_random([0x0f; 16]).to_string(),
        "0f".repeat(16)
    );
    let hex = "0123456789abcdef".repeat(2);
    assert_eq!(RunId::parse(&hex).unwrap().as_str(), hex);
    malformed(RunId::parse(&hex.to_uppercase()), "RunId", "hex32");
    malformed(EffectId::parse(&hex[..31]), "EffectId", "hex32");
    malformed(ApprovalId::parse(&format!("{hex}a")), "ApprovalId", "hex32");
    malformed(RunId::parse(""), "RunId", "hex32");
}

#[test]
fn an_endpoint_is_the_loopback_address_and_a_port() {
    assert_eq!(
        Endpoint::parse("127.0.0.1:9222").unwrap().as_str(),
        "127.0.0.1:9222"
    );
    for bad in [
        "localhost:9222",
        "127.0.0.1",
        "127.0.0.1:",
        "127.0.0.1:0",
        "127.0.0.1:65536",
        "127.0.0.1:09222",
        "127.0.0.2:9222",
        "http://127.0.0.1:9222",
        "[::1]:9222",
    ] {
        malformed(Endpoint::parse(bad), "Endpoint", "endpoint_form");
    }
}

#[test]
fn session_data_is_1_to_48_pieces_of_base64_full_but_the_last() {
    let full = "A".repeat(1024);
    assert!(SessionData::new(vec!["QUJD".to_owned()]).is_ok());
    assert!(SessionData::new(vec![full.clone(), "QQ==".to_owned()]).is_ok());
    let all = SessionData::new(vec![full.clone(); 48]).unwrap();
    assert!(all.is_full());
    assert!(!SessionData::new(vec![full.clone(), "QQ==".to_owned()])
        .unwrap()
        .is_full());
    let mut short_last = vec![full.clone(); 47];
    short_last.push("QQ==".to_owned());
    assert!(!SessionData::new(short_last).unwrap().is_full());
    malformed(SessionData::new(Vec::new()), "SessionData", "piece_count");
    malformed(
        SessionData::new(vec![full.clone(); 49]),
        "SessionData",
        "piece_count",
    );
    malformed(
        SessionData::new(vec!["QQ==".to_owned(), full.clone()]),
        "SessionData",
        "piece_length",
    );
    malformed(
        SessionData::new(vec![String::new()]),
        "SessionData",
        "piece_length",
    );
    malformed(
        SessionData::new(vec!["A".repeat(1025)]),
        "SessionData",
        "piece_length",
    );
    for bad in ["QUJD\n", "QU JD", "QUJD-", "QUJD_", "\u{e9}"] {
        malformed(
            SessionData::new(vec![bad.to_owned()]),
            "SessionData",
            "base64",
        );
    }
}

#[test]
fn a_refusal_prints_its_cause_type_key_and_rule_and_never_the_input() {
    let refusal = Name::parse("Secret Payee 4711").unwrap_err();
    let shown = format!("{refusal} {refusal:?}");
    assert_eq!(refusal.to_string(), "malformed: Name: -: name_form");
    assert_eq!(refusal.key(), None);
    assert!(
        !shown.contains("4711") && !shown.contains("Secret"),
        "{shown}"
    );
    assert_eq!(RefusalCause::Malformed.as_str(), "malformed");
    assert_eq!(RefusalCause::Length.as_str(), "length");
}
