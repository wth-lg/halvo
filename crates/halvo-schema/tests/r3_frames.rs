use halvo_schema::executable::body::RunnerCall;
use halvo_schema::executable::frame::{
    BindReply, BrowserReply, ConfirmDecision, ConfirmReply, ConfirmRequest, FromRunner,
    HandbackReply, HandbackRequest, HandbackResult, ReplyError, RunError, SecretReply,
    StoreReplyFrame, StoreRequest, ToRunner, FRAME_JSON_MAX,
};
use halvo_schema::executable::names::{SecretAnswer, Unreadable, YesNo};
use halvo_schema::executable::{Refusal, RefusalCause, ERROR_NAMES, NAMES};
use halvo_schema::request::{
    CoordinatorRole, CoordinatorWrite, ErrorName, RunnerRequest, RunnerRole,
};
use halvo_schema::RequestId;

const A: &str = "40000000000000000000000000000001";
const R: &str = "50000000000000000000000000000001";
const E: &str = "60000000000000000000000000000001";
const E2: &str = "60000000000000000000000000000002";
const P: &str = "70000000000000000000000000000001";
const P2: &str = "70000000000000000000000000000002";
const ID: &str = "0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f";

const PAY_BOUND: &str = r#"{"class":"payment","fields":[{"name":"payee","from":"slot","origin":null,"value":["Northwind Power"]},{"name":"amount","from":"extract","origin":"http://utility.test:18090","value":["84.20"]},{"name":"instrument","from":"slot","origin":null,"value":["card 4242"]},{"name":"origin","from":"const","origin":null,"value":["http://pay.test:18090"]}]}"#;
const SEND_BOUND: &str = r#"{"class":"send","fields":[{"name":"to","from":"extract","origin":null,"value":["a@mail.test"]},{"name":"subject","from":"extract","origin":null,"value":["Re: rent"]},{"name":"body","from":"model","origin":null,"value":["Thanks, noted."]}]}"#;

fn d(c: char) -> String {
    c.to_string().repeat(64)
}

fn confirm_effect(effect: &str, class: &str, bound: &str, new_origin: Option<&str>) -> String {
    let origin = new_origin.map_or("null".to_owned(), |o| format!("\"{o}\""));
    format!(
        r#"{{"effect":"{effect}","class":"{class}","digest":"{}","bound":{bound},"new_origin":{origin}}}"#,
        d('a')
    )
}

fn confirm(effects: &[String]) -> String {
    format!(
        r#"{{"v":1,"run":"{R}","agent":"{A}","cut":null,"effects":[{}]}}"#,
        effects.join(",")
    )
}

fn approval(id: &str, effect: &str, digest: &str, answer: &str) -> String {
    format!(r#"{{"approval":"{id}","effect":"{effect}","digest":"{digest}","answer":"{answer}"}}"#)
}

fn decision(approvals: &[String]) -> String {
    format!(r#"{{"v":1,"approvals":[{}]}}"#, approvals.join(","))
}

const HANDBACK: &str = r#"{"v":1,"run":"50000000000000000000000000000001","agent":"40000000000000000000000000000001","reason":"sign_in","origin":"http://utility.test:18090","endpoint":"127.0.0.1:9222"}"#;
const RESULT: &str = r#"{"v":1,"outcome":"done","secret":"fixture_token"}"#;

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

fn load_envelope() -> String {
    let call =
        RunnerCall::parse(format!(r#"{{"op":"load","agent":"{A}","run":"{R}"}}"#).as_bytes())
            .unwrap();
    let request = RunnerRequest::new(&RunnerRole::claim(), RequestId::parse(ID).unwrap(), call);
    String::from_utf8(request.to_bytes().unwrap()).unwrap()
}

fn from_runner() -> Vec<String> {
    vec![
        format!(r#"{{"t":"hello","run":"{R}","agent":"{A}","v":1}}"#),
        format!(r#"{{"t":"store","request":{}}}"#, load_envelope()),
        format!(
            r#"{{"t":"confirm","request":{}}}"#,
            confirm(&[confirm_effect(
                E,
                "payment",
                PAY_BOUND,
                Some("http://pay.test:18090")
            )])
        ),
        format!(r#"{{"t":"handback","request":{HANDBACK}}}"#),
        r#"{"t":"secret","ref":"fixture_token"}"#.to_owned(),
        r#"{"t":"progress","step":1,"of":9}"#.to_owned(),
        r#"{"t":"progress","step":9,"of":9}"#.to_owned(),
        r#"{"t":"done","outcome":"completed"}"#.to_owned(),
        r#"{"t":"bind","origin":"https://pay.test:443"}"#.to_owned(),
        BROWSER.to_owned(),
        r#"{"t":"browser","pid":4242,"created":"1794339000123456","group":null}"#.to_owned(),
    ]
}

const BROWSER: &str = r#"{"t":"browser","pid":4242,"created":"1794339000123456","group":4242}"#;

fn to_runner() -> Vec<String> {
    vec![
        format!(r#"{{"t":"store_reply","reply":{{"v":1,"id":"{ID}","ok":{{"stored":1}}}}}}"#),
        format!(r#"{{"t":"store_reply","reply":{{"v":1,"id":"{ID}","error":"binding"}}}}"#),
        r#"{"t":"store_reply","reply":{"v":1,"id":null,"error":"malformed"}}"#.to_owned(),
        format!(
            r#"{{"t":"confirm_reply","decision":{}}}"#,
            decision(&[approval(P, E, &d('a'), "allow_once")])
        ),
        format!(r#"{{"t":"handback_reply","result":{RESULT}}}"#),
        r#"{"t":"handback_reply","result":{"v":1,"outcome":"timeout","secret":null}}"#.to_owned(),
        r#"{"t":"secret_reply","answer":"stored","value":"tok_fixture"}"#.to_owned(),
        r#"{"t":"secret_reply","answer":"stored","value":"t"}"#.to_owned(),
        r#"{"t":"secret_reply","answer":"none"}"#.to_owned(),
        r#"{"t":"secret_reply","answer":"unreadable","member":"locked"}"#.to_owned(),
        r#"{"t":"secret_reply","answer":"unreadable","member":"fault"}"#.to_owned(),
        r#"{"t":"cancel"}"#.to_owned(),
        r#"{"t":"bind_reply","bound":"yes"}"#.to_owned(),
        r#"{"t":"bind_reply","bound":"no"}"#.to_owned(),
        r#"{"t":"browser_reply"}"#.to_owned(),
    ]
}

#[test]
fn every_frame_reads_and_writes_back_to_the_same_bytes() {
    for text in from_runner() {
        let frame = FromRunner::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(frame.to_json().unwrap(), text);
        expect(
            ToRunner::parse(text.as_bytes()),
            "ToRunner",
            "unknown_frame",
            "t",
        );
    }
    for text in to_runner() {
        let frame = ToRunner::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(frame.to_json().unwrap(), text);
        expect(
            FromRunner::parse(text.as_bytes()),
            "FromRunner",
            "unknown_frame",
            "t",
        );
    }
}

#[test]
fn a_frame_is_refused_by_its_tag_its_keys_and_its_length() {
    expect(
        FromRunner::parse(br#"{"t":"ping"}"#),
        "FromRunner",
        "unknown_frame",
        "t",
    );
    expect(
        FromRunner::parse(br#"{"t":"Hello"}"#),
        "FromRunner",
        "unknown_frame",
        "t",
    );
    expect(
        FromRunner::parse(br#"{"run":"x"}"#),
        "FromRunner",
        "missing_key",
        "t",
    );
    expect(
        FromRunner::parse(br#"{"t":1}"#),
        "FromRunner",
        "not_a_string",
        "t",
    );
    expect(
        FromRunner::parse(br#"["done","completed"]"#),
        "FromRunner",
        "not_an_object",
        "-",
    );
    expect(
        FromRunner::parse(br#"{"t":"done","t":"done","outcome":"completed"}"#),
        "FromRunner",
        "repeated_key",
        "-",
    );
    expect(
        FromRunner::parse(br#"{"t":"done","outcome":"completed","run":null}"#),
        "Done",
        "unknown_key",
        "-",
    );
    expect(
        FromRunner::parse(br#"{"t":"done","outcome":"Completed"}"#),
        "Outcome",
        "unknown_name",
        "outcome",
    );
    expect(
        FromRunner::parse(br#"{"t":"done","outcome":"held"}"#),
        "Outcome",
        "unknown_name",
        "outcome",
    );
    expect(
        FromRunner::parse(br#"{"t":"progress","step":10,"of":9}"#),
        "Progress",
        "step_above_of",
        "step",
    );
    expect(
        FromRunner::parse(br#"{"t":"progress","step":0,"of":9}"#),
        "number",
        "zero",
        "step",
    );
    expect(
        FromRunner::parse(br#"{"t":"progress","step":"1","of":9}"#),
        "number",
        "not_a_count",
        "step",
    );
    expect(
        FromRunner::parse(br#"{"t":"secret","ref":"Fixture-Token"}"#),
        "Name",
        "name_form",
        "ref",
    );
    expect(
        FromRunner::parse(br#"{"t":"secret","value":"x"}"#),
        "Secret",
        "unknown_key",
        "-",
    );
    let hello2 = format!(r#"{{"t":"hello","run":"{R}","agent":"{A}","v":2}}"#);
    expect(FromRunner::parse(hello2.as_bytes()), "V1", "version", "v");
    let reordered = format!(r#"{{"v":1,"agent":"{A}","run":"{R}","t":"hello"}}"#);
    assert_eq!(
        FromRunner::parse(reordered.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        from_runner()[0]
    );
    expect(
        ToRunner::parse(br#"{"t":"cancel","run":null}"#),
        "Cancel",
        "unknown_key",
        "-",
    );
    expect(
        ToRunner::parse(br#"{"t":"secret_reply","answer":"stored","value":1}"#),
        "SecretText",
        "not_a_string",
        "value",
    );
    expect(
        ToRunner::parse(br#"{"t":"secret_reply"}"#),
        "SecretReply",
        "missing_key",
        "answer",
    );

    assert_eq!(FRAME_JSON_MAX, 65_536);
    let shell = r#"{"t":"secret_reply","answer":"stored","value":""}"#.len();
    let exact = format!(
        r#"{{"t":"secret_reply","answer":"stored","value":"{}"}}"#,
        "s".repeat(65_536 - shell)
    );
    assert_eq!(exact.len(), 65_536);
    assert_eq!(
        ToRunner::parse(exact.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        exact
    );
    let over = format!(
        r#"{{"t":"secret_reply","answer":"stored","value":"{}"}}"#,
        "s".repeat(65_536 - shell + 1)
    );
    let refusal = ToRunner::parse(over.as_bytes()).unwrap_err();
    assert_eq!(
        (refusal.cause(), refusal.rule()),
        (RefusalCause::Length, "frame_length")
    );
    let spaced = exact.replacen(',', ", ", 1);
    assert_eq!(
        ToRunner::parse(spaced.as_bytes()).unwrap_err().cause(),
        RefusalCause::Length
    );
    let ToRunner::SecretReply(SecretReply::Stored(_)) = ToRunner::parse(exact.as_bytes()).unwrap()
    else {
        panic!()
    };
    let frame = SecretReply::Stored(
        halvo_schema::executable::text::SecretText::parse(&"s".repeat(65_536 - shell + 1)).unwrap(),
    );
    let refusal = ToRunner::SecretReply(frame).to_json().unwrap_err();
    assert_eq!(
        (refusal.cause(), refusal.rule()),
        (RefusalCause::Length, "frame_length")
    );
}

#[test]
fn a_confirmation_request_holds_confirm_classes_whose_bound_object_fits() {
    let pay = confirm_effect(E, "payment", PAY_BOUND, Some("http://pay.test:18090"));
    let send = confirm_effect(E, "send", SEND_BOUND, None);
    let send2 = confirm_effect(E2, "send", SEND_BOUND, None);
    for text in [
        confirm(std::slice::from_ref(&pay)),
        confirm(&[send.clone(), send2.clone()]),
        confirm(&[confirm_effect(E, "payment", PAY_BOUND, None)]),
    ] {
        assert_eq!(
            ConfirmRequest::parse(text.as_bytes())
                .unwrap()
                .to_json()
                .unwrap(),
            text
        );
    }
    let cases: Vec<(String, &str, &str, &str)> = vec![
        (confirm(&[]), "ConfirmRequest", "no_effect", "effects"),
        (
            confirm(&[send.clone(), send.clone()]),
            "ConfirmRequest",
            "repeated_name",
            "effects",
        ),
        (
            confirm(&[pay.clone(), send2.clone()]),
            "ConfirmRequest",
            "browser_effect_not_alone",
            "effects",
        ),
        (
            confirm(&[send.clone(), confirm_effect(E2, "payment", PAY_BOUND, None)]),
            "ConfirmRequest",
            "browser_effect_not_alone",
            "effects",
        ),
        (
            confirm(&[confirm_effect(
                E,
                "send",
                SEND_BOUND,
                Some("http://mail.test:18090"),
            )]),
            "ConfirmEffect",
            "new_origin_of_adapter",
            "new_origin",
        ),
        (
            confirm(&[confirm_effect(
                E,
                "local_edit",
                r#"{"class":"local_edit","fields":[{"name":"file","from":"slot","origin":null,"value":["book.xlsx"]},{"name":"range","from":"slot","origin":null,"value":["B2:D6"]}]}"#,
                None,
            )]),
            "ConfirmEffect",
            "not_a_confirm_class",
            "class",
        ),
        (
            confirm(&[confirm_effect(E, "read", PAY_BOUND, None)]),
            "ConfirmEffect",
            "not_a_confirm_class",
            "class",
        ),
        (
            confirm(&[confirm_effect(E, "credential", PAY_BOUND, None)]),
            "ConfirmEffect",
            "not_a_confirm_class",
            "class",
        ),
        (
            confirm(&[confirm_effect(E, "payment", SEND_BOUND, None)]),
            "ConfirmEffect",
            "class_of_bound",
            "bound",
        ),
        (
            confirm(&[confirm_effect(E, "purchase", PAY_BOUND, None)]),
            "ConfirmEffect",
            "class_of_bound",
            "bound",
        ),
        (
            confirm(&[confirm_effect(
                E,
                "payment",
                PAY_BOUND,
                Some("https://pay.test"),
            )]),
            "WebOrigin",
            "origin_form",
            "new_origin",
        ),
        (
            confirm(&[pay.replace(&d('a'), &d('A'))]),
            "Digest",
            "hex64",
            "digest",
        ),
        (
            confirm(&[pay.replace(r#""new_origin""#, r#""answer":null,"new_origin""#)]),
            "ConfirmEffect",
            "unknown_key",
            "effects",
        ),
        (
            confirm(&[pay.replace(
                r#""class":"payment","digest""#,
                r#""class":"Payment","digest""#,
            )]),
            "ActionClass",
            "unknown_name",
            "class",
        ),
        (
            confirm(std::slice::from_ref(&pay)).replace(r#""v":1"#, r#""v":1,"approvals":[]"#),
            "ConfirmRequest",
            "unknown_key",
            "-",
        ),
        (
            format!(r#"{{"v":1,"run":"{R}","agent":"{A}","cut":null,"effects":{pay}}}"#),
            "list",
            "not_a_list",
            "effects",
        ),
    ];
    for (text, type_name, rule, key) in cases {
        expect(ConfirmRequest::parse(text.as_bytes()), type_name, rule, key);
    }
}

const DELETE_BOUND: &str = r#"{"class":"delete","fields":[{"name":"items","from":"extract","origin":null,"value":["AAMk1"]}]}"#;
const SUBMIT_BOUND: &str = r#"{"class":"submit","fields":[{"name":"origin","from":"const","origin":null,"value":["http://form.test:18090"]},{"name":"fields","from":"slot","origin":null,"value":["name, address"]}]}"#;
const PURCHASE_BOUND: &str = r#"{"class":"purchase","fields":[{"name":"seller","from":"slot","origin":null,"value":["Corner Shop"]},{"name":"items","from":"extract","origin":"http://shop.test:18090","value":["tea"]},{"name":"amount","from":"extract","origin":"http://shop.test:18090","value":["12.40"]},{"name":"instrument","from":"slot","origin":null,"value":["card 4242"]},{"name":"origin","from":"const","origin":null,"value":["http://pay.test:18090"]}]}"#;
const SIGN_BOUND: &str = r#"{"class":"sign","fields":[{"name":"document","from":"slot","origin":null,"value":["lease"]},{"name":"party","from":"slot","origin":null,"value":["landlord"]}]}"#;

#[test]
fn each_confirm_class_refuses_the_bound_object_of_another_class() {
    let own = [
        ("send", SEND_BOUND),
        ("delete", DELETE_BOUND),
        ("submit", SUBMIT_BOUND),
        ("purchase", PURCHASE_BOUND),
        ("payment", PAY_BOUND),
        ("sign", SIGN_BOUND),
    ];
    for (class, bound) in own {
        let text = confirm(&[confirm_effect(E, class, bound, None)]);
        assert!(ConfirmRequest::parse(text.as_bytes()).is_ok(), "{class}");
    }
    for (class, other) in [
        ("send", DELETE_BOUND),
        ("delete", SEND_BOUND),
        ("submit", PAY_BOUND),
        ("purchase", SUBMIT_BOUND),
        ("payment", PURCHASE_BOUND),
        ("sign", PAY_BOUND),
    ] {
        let text = confirm(&[confirm_effect(E, class, other, None)]);
        expect(
            ConfirmRequest::parse(text.as_bytes()),
            "ConfirmEffect",
            "class_of_bound",
            "bound",
        );
    }
}

#[test]
fn a_submit_effect_is_a_request_of_the_browser_and_stands_alone() {
    let submit = SUBMIT_BOUND;
    let alone = confirm(&[confirm_effect(
        E,
        "submit",
        submit,
        Some("http://form.test:18090"),
    )]);
    assert_eq!(
        ConfirmRequest::parse(alone.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        alone
    );
    let with_send = confirm(&[
        confirm_effect(E, "submit", submit, None),
        confirm_effect(E2, "send", SEND_BOUND, None),
    ]);
    expect(
        ConfirmRequest::parse(with_send.as_bytes()),
        "ConfirmRequest",
        "browser_effect_not_alone",
        "effects",
    );
}

#[test]
fn a_decision_holds_one_approval_per_effect_of_its_request() {
    let request = ConfirmRequest::parse(
        confirm(&[
            confirm_effect(E, "send", SEND_BOUND, None),
            confirm_effect(E2, "send", SEND_BOUND, None),
        ])
        .as_bytes(),
    )
    .unwrap();
    let good = decision(&[
        approval(P, E, &d('a'), "allow_once"),
        approval(P2, E2, &d('a'), "reject_once"),
    ]);
    let read = ConfirmDecision::parse(good.as_bytes()).unwrap();
    assert_eq!(read.to_json().unwrap(), good);
    assert!(read.answers(&request).is_ok());
    let swapped = decision(&[
        approval(P2, E2, &d('a'), "reject_once"),
        approval(P, E, &d('a'), "allow_once"),
    ]);
    assert!(ConfirmDecision::parse(swapped.as_bytes())
        .unwrap()
        .answers(&request)
        .is_ok());
    for text in [
        decision(&[approval(P, E, &d('a'), "allow_once")]),
        decision(&[
            approval(P, E, &d('a'), "allow_once"),
            approval(P2, E2, &d('b'), "allow_once"),
        ]),
        decision(&[
            approval(P, E, &d('a'), "allow_once"),
            approval(
                P2,
                "60000000000000000000000000000003",
                &d('a'),
                "allow_once",
            ),
        ]),
    ] {
        let short = ConfirmDecision::parse(text.as_bytes()).unwrap();
        expect(
            short.answers(&request),
            "ConfirmDecision",
            "not_the_request",
            "approvals",
        );
    }
    let cases: Vec<(String, &str, &str, &str)> = vec![
        (decision(&[]), "ConfirmDecision", "no_approval", "approvals"),
        (
            decision(&[
                approval(P, E, &d('a'), "allow_once"),
                approval(P2, E, &d('a'), "allow_once"),
            ]),
            "ConfirmDecision",
            "repeated_name",
            "approvals",
        ),
        (
            decision(&[
                approval(P, E, &d('a'), "allow_once"),
                approval(P, E2, &d('a'), "allow_once"),
            ]),
            "ConfirmDecision",
            "repeated_name",
            "approvals",
        ),
        (
            decision(&[approval(P, E, &d('a'), "allow_always")]),
            "Answer",
            "unknown_name",
            "answer",
        ),
        (
            decision(&[approval(P, E, &d('a'), "allow")]),
            "Answer",
            "unknown_name",
            "answer",
        ),
        (
            decision(&[approval(P, E, &d('a'), "Allow_once")]),
            "Answer",
            "unknown_name",
            "answer",
        ),
        (
            decision(&[approval("approval-1", E, &d('a'), "allow_once")]),
            "ApprovalId",
            "hex32",
            "approval",
        ),
        (
            decision(&[approval(P, E, &d('a'), "allow_once")
                .replace(r#""answer""#, r#""lasting":null,"answer""#)]),
            "Approval",
            "unknown_key",
            "approvals",
        ),
    ];
    for (text, type_name, rule, key) in cases {
        expect(
            ConfirmDecision::parse(text.as_bytes()),
            type_name,
            rule,
            key,
        );
    }
}

#[test]
fn a_handback_names_its_reason_and_its_result_names_a_secret_only_when_done() {
    assert_eq!(
        HandbackRequest::parse(HANDBACK.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        HANDBACK
    );
    assert_eq!(
        HandbackResult::parse(RESULT.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        RESULT
    );
    let product = HANDBACK
        .replace(r#""127.0.0.1:9222""#, "null")
        .replace(r#""http://utility.test:18090""#, "null");
    assert!(HandbackRequest::parse(product.as_bytes()).is_ok());
    expect(
        HandbackRequest::parse(
            HANDBACK
                .replace("127.0.0.1:9222", "localhost:9222")
                .as_bytes(),
        ),
        "Endpoint",
        "endpoint_form",
        "endpoint",
    );
    expect(
        HandbackRequest::parse(HANDBACK.replace("sign_in", "password").as_bytes()),
        "HandbackReason",
        "unknown_name",
        "reason",
    );
    expect(
        HandbackRequest::parse(HANDBACK.replace("sign_in", "Sign_in").as_bytes()),
        "HandbackReason",
        "unknown_name",
        "reason",
    );
    expect(
        HandbackRequest::parse(
            HANDBACK
                .replace(r#""v":1,"#, r#""v":1,"secret":null,"#)
                .as_bytes(),
        ),
        "HandbackRequest",
        "unknown_key",
        "-",
    );
    expect(
        HandbackResult::parse(br#"{"v":1,"outcome":"declined","secret":"fixture_token"}"#),
        "HandbackResult",
        "secret_without_done",
        "secret",
    );
    expect(
        HandbackResult::parse(br#"{"v":1,"outcome":"timeout","secret":"fixture_token"}"#),
        "HandbackResult",
        "secret_without_done",
        "secret",
    );
    expect(
        HandbackResult::parse(br#"{"v":1,"outcome":"done","secret":"Fixture Token"}"#),
        "Name",
        "name_form",
        "secret",
    );
    expect(
        HandbackResult::parse(br#"{"v":1,"outcome":"sign_in","secret":null}"#),
        "HandbackOutcome",
        "unknown_name",
        "outcome",
    );
    expect(
        HandbackResult::parse(br#"{"v":1,"outcome":"done","secret":null,"value":"x"}"#),
        "HandbackResult",
        "unknown_key",
        "-",
    );
}

#[test]
fn a_store_frame_holds_a_runner_request_whose_body_is_a_runner_body() {
    let call =
        RunnerCall::parse(format!(r#"{{"op":"load","agent":"{A}","run":"{R}"}}"#).as_bytes())
            .unwrap();
    let request = RunnerRequest::new(
        &RunnerRole::claim(),
        RequestId::parse(ID).unwrap(),
        call.clone(),
    );
    let store = StoreRequest::new(&request).unwrap();
    assert_eq!(store.envelope(), load_envelope());
    assert_eq!(store.id().as_str(), ID);
    assert_eq!(store.body(), &call);
    let probe =
        r#"{"v":1,"id":"0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f","request":"runner","body":{"probe":1}}"#;
    expect(
        FromRunner::parse(format!(r#"{{"t":"store","request":{probe}}}"#).as_bytes()),
        "RunnerCall",
        "missing_key",
        "op",
    );
    let write = CoordinatorWrite::new(
        &CoordinatorRole::claim(),
        RequestId::parse(ID).unwrap(),
        halvo_schema::executable::CoordinatorCall::parse(format!(r#"{{"op":"end_run","workflow":"30000000000000000000000000000001","agent":"{A}","run":"{R}"}}"#).as_bytes()).unwrap(),
    );
    let coordinator = String::from_utf8(write.to_bytes().unwrap()).unwrap();
    for envelope in [
        coordinator,
        load_envelope().replace(r#""v":1"#, r#""v":2"#),
        load_envelope().replace(r#""request":"runner""#, r#""request":"runners""#),
        load_envelope().replace(ID, "0F0F0F0F0F0F0F0F0F0F0F0F0F0F0F0F"),
        r#"[1,"0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f","runner",{}]"#.to_owned(),
    ] {
        let text = format!(r#"{{"t":"store","request":{envelope}}}"#);
        let refusal = FromRunner::parse(text.as_bytes()).unwrap_err();
        assert_eq!(refusal.type_name(), "StoreRequest", "{envelope}");
        assert!(
            ["not_a_runner_request", "not_an_object"].contains(&refusal.rule()),
            "{refusal}"
        );
    }
    let other_run = load_envelope().replace(R, "50000000000000000000000000000002");
    let text = format!(r#"{{"t":"store","request":{other_run}}}"#);
    let FromRunner::Store(frame) = FromRunner::parse(text.as_bytes()).unwrap() else {
        panic!()
    };
    assert_eq!(
        frame.request.body().run().as_str(),
        "50000000000000000000000000000002"
    );
}

#[test]
fn a_store_reply_takes_the_nine_error_names_and_the_six_of_this_run() {
    assert_eq!(
        ERROR_NAMES,
        ["binding", "playbook", "config", "class", "length", "effect"]
    );
    for name in ERROR_NAMES {
        assert_eq!(RunError::parse(name).unwrap().as_str(), *name);
        let text =
            format!(r#"{{"t":"store_reply","reply":{{"v":1,"id":"{ID}","error":"{name}"}}}}"#);
        let ToRunner::StoreReply(frame) = ToRunner::parse(text.as_bytes()).unwrap() else {
            panic!()
        };
        assert_eq!(
            frame.reply.outcome,
            Err(ReplyError::Run(RunError::parse(name).unwrap()))
        );
    }
    for name in ErrorName::ALL {
        let text = format!(
            r#"{{"t":"store_reply","reply":{{"v":1,"id":"{ID}","error":"{}"}}}}"#,
            name.as_str()
        );
        let ToRunner::StoreReply(frame) = ToRunner::parse(text.as_bytes()).unwrap() else {
            panic!()
        };
        assert_eq!(frame.reply.outcome, Err(ReplyError::Workflow(*name)));
        assert_eq!(ToRunner::StoreReply(frame).to_json().unwrap(), text);
    }
    let reply = |inner: &str| format!(r#"{{"t":"store_reply","reply":{inner}}}"#);
    let cases: Vec<(String, &str, &str, &str)> = vec![
        (
            reply(&format!(r#"{{"v":1,"id":"{ID}","error":"nope"}}"#)),
            "RunError",
            "unknown_name",
            "error",
        ),
        (
            reply(&format!(r#"{{"v":1,"id":"{ID}","error":"Binding"}}"#)),
            "RunError",
            "unknown_name",
            "error",
        ),
        (
            reply(&format!(r#"{{"v":1,"id":"{ID}","error":"used"}}"#)),
            "RunError",
            "unknown_name",
            "error",
        ),
        (
            reply(&format!(r#"{{"v":1,"id":"{ID}","ok":{{"rev":1}}}}"#)),
            "RunnerReply",
            "reply_form",
            "ok",
        ),
        (
            reply(&format!(r#"{{"v":1,"id":"{ID}","ok":null}}"#)),
            "RunnerReply",
            "not_an_object",
            "ok",
        ),
        (
            reply(&format!(
                r#"{{"v":1,"id":"{ID}","ok":{{"workflows":[],"next":null}}}}"#
            )),
            "RunnerReply",
            "reply_form",
            "ok",
        ),
        (
            reply(&format!(
                r#"{{"v":1,"id":"{ID}","ok":{{"stored":1}},"error":"binding"}}"#
            )),
            "StoreReply",
            "unknown_key",
            "reply",
        ),
        (
            reply(&format!(r#"{{"v":1,"id":"{ID}"}}"#)),
            "StoreReply",
            "missing_key",
            "error",
        ),
        (
            reply(&format!(r#"{{"v":2,"id":"{ID}","error":"binding"}}"#)),
            "V1",
            "version",
            "v",
        ),
        (
            reply(r#"{"v":1,"id":"x","error":"binding"}"#),
            "RequestId",
            "length",
            "id",
        ),
        (
            reply(&format!(r#"{{"v":1,"id":"{ID}","error":7}}"#)),
            "StoreReply",
            "not_a_string",
            "error",
        ),
        (
            reply(r#"[1,null,"binding"]"#),
            "StoreReply",
            "not_an_object",
            "reply",
        ),
    ];
    for (text, type_name, rule, key) in cases {
        expect(ToRunner::parse(text.as_bytes()), type_name, rule, key);
    }
}

#[test]
fn debug_of_a_frame_prints_no_secret_and_no_value_of_the_user() {
    let secret =
        ToRunner::parse(br#"{"t":"secret_reply","answer":"stored","value":"tok_4711"}"#).unwrap();
    let confirm_frame = FromRunner::parse(from_runner()[2].as_bytes()).unwrap();
    let put_session = RunnerCall::parse(format!(r#"{{"op":"put_session","agent":"{A}","run":"{R}","origin":"http://utility.test:18090","rev":1,"part":1,"of":1,"data":["c2VjcmV0NDcxMQ=="]}}"#).as_bytes()).unwrap();
    let request = RunnerRequest::new(
        &RunnerRole::claim(),
        RequestId::parse(ID).unwrap(),
        put_session,
    );
    let store = StoreRequest::new(&request).unwrap();
    let shown = format!("{secret:?} {confirm_frame:?} {store:?}");
    for value in ["4711", "Northwind", "card", "c2VjcmV0"] {
        assert!(!shown.contains(value), "{value}: {shown}");
    }
}

#[test]
fn the_frames_bind_and_browser_and_their_replies_hold_the_keys_of_the_contract() {
    let FromRunner::Bind(bind) =
        FromRunner::parse(br#"{"t":"bind","origin":"http://pay.test:18090"}"#).unwrap()
    else {
        panic!("bind")
    };
    assert_eq!(bind.origin.as_str(), "http://pay.test:18090");
    let FromRunner::Browser(browser) = FromRunner::parse(BROWSER.as_bytes()).unwrap() else {
        panic!("browser")
    };
    assert_eq!(
        (
            browser.pid.get(),
            browser.created.micros().get(),
            browser.created.to_string(),
            browser.group.map(|g| g.get())
        ),
        (
            4242,
            1_794_339_000_123_456,
            "1794339000123456".to_owned(),
            Some(4242)
        )
    );
    let windows = r#"{"t":"browser","pid":4242,"created":"1794339000123456","group":null}"#;
    let FromRunner::Browser(windows) = FromRunner::parse(windows.as_bytes()).unwrap() else {
        panic!("browser")
    };
    assert_eq!(windows.group, None);
    for (text, yes) in [
        (r#"{"t":"bind_reply","bound":"yes"}"#, YesNo::Yes),
        (r#"{"t":"bind_reply","bound":"no"}"#, YesNo::No),
    ] {
        let ToRunner::BindReply(reply) = ToRunner::parse(text.as_bytes()).unwrap() else {
            panic!("bind_reply")
        };
        assert_eq!(reply.bound, yes);
    }
    assert!(matches!(
        ToRunner::parse(br#"{"t":"browser_reply"}"#),
        Ok(ToRunner::BrowserReply(_))
    ));
    let largest =
        r#"{"t":"browser","pid":4294967295,"created":"18446744073709551615","group":4294967295}"#;
    assert_eq!(
        FromRunner::parse(largest.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        largest
    );

    let from: Vec<(&str, &str, &str, &str)> = vec![
        (
            r#"["bind","http://pay.test:18090"]"#,
            "FromRunner",
            "not_an_object",
            "-",
        ),
        (
            r#"{"t":"bind","origin":"http://pay.test:18090","origin":"http://pay.test:18090"}"#,
            "FromRunner",
            "repeated_key",
            "-",
        ),
        (
            r#"{"t":"bind","origin":null}"#,
            "WebOrigin",
            "not_a_string",
            "origin",
        ),
        (r#"{"t":"bind"}"#, "Bind", "missing_key", "origin"),
        (
            r#"{"t":"bind","origin":"https://pay.test"}"#,
            "WebOrigin",
            "origin_form",
            "origin",
        ),
        (
            r#"{"t":"bind","origin":"http://pay.test:18090/"}"#,
            "WebOrigin",
            "origin_form",
            "origin",
        ),
        (
            r#"{"t":"bind","origin":"http://PAY.test:18090"}"#,
            "WebOrigin",
            "origin_form",
            "origin",
        ),
        (
            r#"{"t":"bind","origin":["http://pay.test:18090"]}"#,
            "WebOrigin",
            "not_a_string",
            "origin",
        ),
        (
            r#"{"t":"bind","origin":"http://pay.test:18090","way":"sign_in"}"#,
            "Bind",
            "unknown_key",
            "-",
        ),
        (
            r#"{"t":"bind","origin":"http://pay.test:18090","bound":"yes"}"#,
            "Bind",
            "unknown_key",
            "-",
        ),
        (
            r#"{"t":"bind","origins":["http://pay.test:18090"]}"#,
            "Bind",
            "unknown_key",
            "-",
        ),
        (
            r#"[4242,"1794339000123456",4242]"#,
            "FromRunner",
            "not_an_object",
            "-",
        ),
        (
            r#"{"t":"browser","pid":4242,"pid":4243,"created":"1794339000123456","group":null}"#,
            "FromRunner",
            "repeated_key",
            "-",
        ),
        (
            r#"{"t":"browser","pid":"4242","created":"1794339000123456","group":null}"#,
            "number",
            "not_a_count",
            "pid",
        ),
        (
            r#"{"t":"browser","pid":0,"created":"1794339000123456","group":null}"#,
            "number",
            "zero",
            "pid",
        ),
        (
            r#"{"t":"browser","pid":-1,"created":"1794339000123456","group":null}"#,
            "number",
            "not_a_count",
            "pid",
        ),
        (
            r#"{"t":"browser","pid":4242.0,"created":"1794339000123456","group":null}"#,
            "number",
            "not_a_count",
            "pid",
        ),
        (
            r#"{"t":"browser","pid":4294967296,"created":"1794339000123456","group":null}"#,
            "number",
            "range",
            "pid",
        ),
        (
            r#"{"t":"browser","pid":null,"created":"1794339000123456","group":null}"#,
            "number",
            "not_a_count",
            "pid",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":1794339000123456,"group":null}"#,
            "CreationTime",
            "not_a_string",
            "created",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":"0","group":null}"#,
            "CreationTime",
            "creation_time",
            "created",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":"01794339000123456","group":null}"#,
            "CreationTime",
            "creation_time",
            "created",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":"","group":null}"#,
            "CreationTime",
            "creation_time",
            "created",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":"+1794339000123456","group":null}"#,
            "CreationTime",
            "creation_time",
            "created",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":" 1794339000123456","group":null}"#,
            "CreationTime",
            "creation_time",
            "created",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":"-1","group":null}"#,
            "CreationTime",
            "creation_time",
            "created",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":"1794339000 123456","group":null}"#,
            "CreationTime",
            "creation_time",
            "created",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":"18446744073709551616","group":null}"#,
            "CreationTime",
            "creation_time",
            "created",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":"2026-11-12T05:00:00Z","group":null}"#,
            "CreationTime",
            "creation_time",
            "created",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":null,"group":null}"#,
            "CreationTime",
            "not_a_string",
            "created",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":"1794339000123456","group":0}"#,
            "number",
            "zero",
            "group",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":"1794339000123456","group":"job"}"#,
            "number",
            "not_a_count",
            "group",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":"1794339000123456"}"#,
            "Browser",
            "missing_key",
            "group",
        ),
        (
            r#"{"t":"browser","created":"1794339000123456","group":null}"#,
            "Browser",
            "missing_key",
            "pid",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":"1794339000123456","group":null,"run":null}"#,
            "Browser",
            "unknown_key",
            "-",
        ),
        (
            r#"{"t":"browser","pid":4242,"created":"1794339000123456","group":null,"origin":"http://pay.test:18090"}"#,
            "Browser",
            "unknown_key",
            "-",
        ),
        (
            r#"{"t":"bind_reply","bound":"yes"}"#,
            "FromRunner",
            "unknown_frame",
            "t",
        ),
        (
            r#"{"t":"browser_reply"}"#,
            "FromRunner",
            "unknown_frame",
            "t",
        ),
    ];
    for (text, type_name, rule, key) in from {
        expect(FromRunner::parse(text.as_bytes()), type_name, rule, key);
    }

    let to: Vec<(&str, &str, &str, &str)> = vec![
        (r#"["bind_reply","yes"]"#, "ToRunner", "not_an_object", "-"),
        (
            r#"{"t":"bind_reply","bound":"yes","bound":"no"}"#,
            "ToRunner",
            "repeated_key",
            "-",
        ),
        (
            r#"{"t":"bind_reply","bound":null}"#,
            "YesNo",
            "not_a_string",
            "bound",
        ),
        (
            r#"{"t":"bind_reply","bound":true}"#,
            "YesNo",
            "not_a_string",
            "bound",
        ),
        (
            r#"{"t":"bind_reply","bound":"Yes"}"#,
            "YesNo",
            "unknown_name",
            "bound",
        ),
        (
            r#"{"t":"bind_reply","bound":"true"}"#,
            "YesNo",
            "unknown_name",
            "bound",
        ),
        (
            r#"{"t":"bind_reply","bound":"allow"}"#,
            "YesNo",
            "unknown_name",
            "bound",
        ),
        (
            r#"{"t":"bind_reply","bound":"declined"}"#,
            "YesNo",
            "unknown_name",
            "bound",
        ),
        (r#"{"t":"bind_reply"}"#, "BindReply", "missing_key", "bound"),
        (
            r#"{"t":"bind_reply","bound":"yes","origin":"http://pay.test:18090"}"#,
            "BindReply",
            "unknown_key",
            "-",
        ),
        (
            r#"{"t":"bind_reply","bound":"yes","rev":2}"#,
            "BindReply",
            "unknown_key",
            "-",
        ),
        (
            r#"{"t":"browser_reply","pid":4242}"#,
            "BrowserReply",
            "unknown_key",
            "-",
        ),
        (
            r#"{"t":"browser_reply","ok":"yes"}"#,
            "BrowserReply",
            "unknown_key",
            "-",
        ),
        (
            r#"{"t":"browser_reply","group":null}"#,
            "BrowserReply",
            "unknown_key",
            "-",
        ),
        (
            r#"{"t":"bind","origin":"http://pay.test:18090"}"#,
            "ToRunner",
            "unknown_frame",
            "t",
        ),
        (BROWSER, "ToRunner", "unknown_frame", "t"),
    ];
    for (text, type_name, rule, key) in to {
        expect(ToRunner::parse(text.as_bytes()), type_name, rule, key);
    }
}

fn secret_reply(keys: &str) -> String {
    format!(r#"{{"t":"secret_reply",{keys}}}"#)
}

#[test]
fn a_secret_reply_tells_the_secret_none_stored_and_a_store_that_could_not_be_read_apart() {
    let answers: Vec<&str> = SecretAnswer::ALL.iter().map(|a| a.as_str()).collect();
    assert_eq!(answers, ["stored", "none", "unreadable"]);
    let members: Vec<&str> = Unreadable::ALL.iter().map(|m| m.as_str()).collect();
    assert_eq!(members, ["locked", "fault"]);
    let read = |keys: &str| match ToRunner::parse(secret_reply(keys).as_bytes()) {
        Ok(ToRunner::SecretReply(reply)) => reply,
        other => panic!("{keys}: {other:?}"),
    };
    let stored = read(r#""answer":"stored","value":"tok_fixture""#);
    let SecretReply::Stored(value) = &stored else {
        panic!("stored")
    };
    assert_eq!(value.expose(), "tok_fixture");
    assert_eq!(stored.answer(), SecretAnswer::Stored);
    assert_eq!(read(r#""answer":"none""#), SecretReply::None);
    assert_eq!(read(r#""answer":"none""#).answer(), SecretAnswer::None);
    for member in Unreadable::ALL {
        let keys = format!(r#""answer":"unreadable","member":"{}""#, member.as_str());
        let reply = read(&keys);
        assert_eq!(reply, SecretReply::Unreadable(*member));
        assert_eq!(reply.answer(), SecretAnswer::Unreadable);
        assert_ne!(reply, SecretReply::None);
        assert_eq!(
            ToRunner::SecretReply(reply).to_json().unwrap(),
            secret_reply(&keys)
        );
    }
    let reordered = r#"{"value":"tok_fixture","answer":"stored","t":"secret_reply"}"#;
    assert_eq!(
        ToRunner::parse(reordered.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        secret_reply(r#""answer":"stored","value":"tok_fixture""#)
    );
    let written: Vec<String> = [
        SecretReply::Stored(
            halvo_schema::executable::text::SecretText::parse("tok_fixture").unwrap(),
        ),
        SecretReply::None,
        SecretReply::Unreadable(Unreadable::Locked),
        SecretReply::Unreadable(Unreadable::Fault),
    ]
    .into_iter()
    .map(|r| ToRunner::SecretReply(r).to_json().unwrap())
    .collect();
    assert_eq!(
        written,
        [
            secret_reply(r#""answer":"stored","value":"tok_fixture""#),
            secret_reply(r#""answer":"none""#),
            secret_reply(r#""answer":"unreadable","member":"locked""#),
            secret_reply(r#""answer":"unreadable","member":"fault""#),
        ]
    );
}

#[test]
fn a_secret_reply_with_a_value_and_an_answer_other_than_stored_is_refused() {
    for keys in [
        r#""answer":"none","value":"tok_fixture""#,
        r#""answer":"none","value":"""#,
        r#""answer":"none","value":null"#,
        r#""value":null,"answer":"none""#,
        r#""answer":"unreadable","member":"locked","value":"tok_fixture""#,
        r#""answer":"unreadable","member":"fault","value":null"#,
        r#""answer":"unreadable","value":"tok_fixture""#,
    ] {
        expect(
            ToRunner::parse(secret_reply(keys).as_bytes()),
            "SecretReply",
            "value_without_stored",
            "value",
        );
    }
}

#[test]
fn a_secret_reply_with_a_member_and_an_answer_other_than_unreadable_is_refused() {
    for keys in [
        r#""answer":"none","member":"locked""#,
        r#""answer":"none","member":"fault""#,
        r#""answer":"none","member":null"#,
        r#""answer":"stored","value":"tok_fixture","member":"locked""#,
        r#""answer":"stored","value":"tok_fixture","member":null"#,
        r#""answer":"stored","member":"fault""#,
    ] {
        expect(
            ToRunner::parse(secret_reply(keys).as_bytes()),
            "SecretReply",
            "member_without_unreadable",
            "member",
        );
    }
}

#[test]
fn a_secret_reply_whose_stored_value_is_empty_is_refused_and_one_byte_is_taken() {
    let empty = secret_reply(r#""answer":"stored","value":"""#);
    expect(
        ToRunner::parse(empty.as_bytes()),
        "SecretText",
        "empty_secret",
        "value",
    );
    expect(
        SecretReply::parse(empty.as_bytes()),
        "SecretText",
        "empty_secret",
        "value",
    );
    let one = secret_reply(r#""answer":"stored","value":"t""#);
    assert_eq!(
        ToRunner::parse(one.as_bytes()).unwrap().to_json().unwrap(),
        one
    );
    let ToRunner::SecretReply(SecretReply::Stored(value)) =
        ToRunner::parse(one.as_bytes()).unwrap()
    else {
        panic!("{one}")
    };
    assert_eq!(value.expose(), "t");
}

#[test]
fn a_secret_reply_with_the_answer_stored_and_no_value_is_refused() {
    expect(
        ToRunner::parse(secret_reply(r#""answer":"stored""#).as_bytes()),
        "SecretReply",
        "stored_without_value",
        "value",
    );
    expect(
        ToRunner::parse(secret_reply(r#""answer":"stored","value":null"#).as_bytes()),
        "SecretText",
        "not_a_string",
        "value",
    );
    expect(
        ToRunner::parse(secret_reply(r#""answer":"unreadable""#).as_bytes()),
        "SecretReply",
        "unreadable_without_member",
        "member",
    );
}

#[test]
fn a_secret_reply_is_refused_for_each_form_that_the_contract_does_not_give() {
    let cases: Vec<(String, &str, &str, &str)> = vec![
        (
            r#"["secret_reply","stored","tok_fixture"]"#.to_owned(),
            "ToRunner",
            "not_an_object",
            "-",
        ),
        (
            secret_reply(r#""answer":"stored","answer":"none""#),
            "ToRunner",
            "repeated_key",
            "-",
        ),
        (
            secret_reply(r#""answer":"stored","value":"a","value":"b""#),
            "ToRunner",
            "repeated_key",
            "-",
        ),
        (
            secret_reply(r#""answer":null"#),
            "SecretAnswer",
            "not_a_string",
            "answer",
        ),
        (
            secret_reply(r#""answer":1"#),
            "SecretAnswer",
            "not_a_string",
            "answer",
        ),
        (
            secret_reply(r#""answer":"1""#),
            "SecretAnswer",
            "unknown_name",
            "answer",
        ),
        (
            secret_reply(r#""answer":"Stored","value":"x""#),
            "SecretAnswer",
            "unknown_name",
            "answer",
        ),
        (
            secret_reply(r#""answer":"locked""#),
            "SecretAnswer",
            "unknown_name",
            "answer",
        ),
        (
            secret_reply(r#""answer":"not_stored""#),
            "SecretAnswer",
            "unknown_name",
            "answer",
        ),
        (
            secret_reply(r#""answer":["none"]"#),
            "SecretAnswer",
            "not_a_string",
            "answer",
        ),
        (
            secret_reply(r#""value":"tok_fixture""#),
            "SecretReply",
            "missing_key",
            "answer",
        ),
        (
            secret_reply(r#""answer":"unreadable","member":"Locked""#),
            "Unreadable",
            "unknown_name",
            "member",
        ),
        (
            secret_reply(r#""answer":"unreadable","member":"none""#),
            "Unreadable",
            "unknown_name",
            "member",
        ),
        (
            secret_reply(r#""answer":"unreadable","member":"failed""#),
            "Unreadable",
            "unknown_name",
            "member",
        ),
        (
            secret_reply(r#""answer":"unreadable","member":null"#),
            "Unreadable",
            "not_a_string",
            "member",
        ),
        (
            secret_reply(r#""answer":"unreadable","member":0"#),
            "Unreadable",
            "not_a_string",
            "member",
        ),
        (
            secret_reply(r#""answer":"stored","value":["tok"]"#),
            "SecretText",
            "not_a_string",
            "value",
        ),
        (
            secret_reply(r#""answer":"stored","value":12345"#),
            "SecretText",
            "not_a_string",
            "value",
        ),
        (
            secret_reply(r#""answer":"none","secret":null"#),
            "SecretReply",
            "unknown_key",
            "-",
        ),
        (
            secret_reply(r#""answer":"none","ref":"fixture_token""#),
            "SecretReply",
            "unknown_key",
            "-",
        ),
        (
            secret_reply(r#""answer":"none","outcome":"done""#),
            "SecretReply",
            "unknown_key",
            "-",
        ),
        (
            secret_reply(r#""answer":"stored","value":"tok","name":"x""#),
            "SecretReply",
            "unknown_key",
            "-",
        ),
        (
            secret_reply(r#""value":null"#),
            "SecretReply",
            "missing_key",
            "answer",
        ),
        (
            r#"{"t":"secret_reply","answer":"none","t":"secret_reply"}"#.to_owned(),
            "ToRunner",
            "repeated_key",
            "-",
        ),
    ];
    for (text, type_name, rule, key) in cases {
        expect(ToRunner::parse(text.as_bytes()), type_name, rule, key);
    }
    expect(
        FromRunner::parse(secret_reply(r#""answer":"none""#).as_bytes()),
        "FromRunner",
        "unknown_frame",
        "t",
    );
    let error =
        ToRunner::parse(secret_reply(r#""answer":"none","value":"zq_secret_4711""#).as_bytes())
            .unwrap_err()
            .to_string();
    assert!(!error.contains("4711"), "{error}");
}

#[test]
fn a_secret_reply_read_alone_checks_its_own_tag() {
    assert_eq!(
        SecretReply::parse(br#"{"t":"secret_reply","answer":"none"}"#),
        Ok(SecretReply::None)
    );
    for text in [
        r#"{"t":"secret","answer":"none"}"#,
        r#"{"t":"bind_reply","answer":"none"}"#,
        r#"{"t":"Secret_reply","answer":"none"}"#,
    ] {
        expect(
            SecretReply::parse(text.as_bytes()),
            "SecretReply",
            "tag",
            "t",
        );
    }
    expect(
        SecretReply::parse(br#"{"t":1,"answer":"none"}"#),
        "SecretReply",
        "not_a_string",
        "t",
    );
    expect(
        SecretReply::parse(br#"{"answer":"none"}"#),
        "SecretReply",
        "missing_key",
        "t",
    );
}

#[test]
fn a_secret_reply_with_a_key_that_the_contract_does_not_name_is_refused_for_each_answer() {
    for keys in [
        r#""answer":"stored","value":"tok_fixture","x":1"#,
        r#""answer":"none","origin":"http://pay.test:18090""#,
        r#""answer":"unreadable","member":"locked","bound":"yes""#,
    ] {
        expect(
            ToRunner::parse(secret_reply(keys).as_bytes()),
            "SecretReply",
            "unknown_key",
            "-",
        );
    }
    let mut names: Vec<&str> = NAMES.to_vec();
    names.extend([
        "x", "", "T", "Answer", "Value", "Member", "answers", "values", "members", "t2", "bound",
        "pid", "created", "group", "ref", "secret", "request", "result", "decision", "reply",
        "endpoint", "reason", "outcome", "origin",
    ]);
    names.retain(|name| !["t", "answer", "value", "member"].contains(name));
    for answer in [
        r#""answer":"stored","value":"tok_fixture""#,
        r#""answer":"none""#,
        r#""answer":"unreadable","member":"fault""#,
    ] {
        for name in &names {
            let after = secret_reply(&format!(r#"{answer},"{name}":null"#));
            expect(
                SecretReply::parse(after.as_bytes()),
                "SecretReply",
                "unknown_key",
                "-",
            );
            let before = secret_reply(&format!(r#""{name}":"x",{answer}"#));
            expect(
                ToRunner::parse(before.as_bytes()),
                "SecretReply",
                "unknown_key",
                "-",
            );
        }
    }
}

#[test]
fn every_reply_frame_is_counted_on_the_bytes_given_by_its_own_parse() {
    type Parse = fn(&[u8]) -> Result<(), Refusal>;
    let parses: [(&str, &str, Parse); 6] = [
        ("store_reply", "StoreReplyFrame", |b| {
            StoreReplyFrame::parse(b).map(drop)
        }),
        ("confirm_reply", "ConfirmReply", |b| {
            ConfirmReply::parse(b).map(drop)
        }),
        ("handback_reply", "HandbackReply", |b| {
            HandbackReply::parse(b).map(drop)
        }),
        ("secret_reply", "SecretReply", |b| {
            SecretReply::parse(b).map(drop)
        }),
        ("bind_reply", "BindReply", |b| BindReply::parse(b).map(drop)),
        ("browser_reply", "BrowserReply", |b| {
            BrowserReply::parse(b).map(drop)
        }),
    ];
    let mut read = 0;
    for text in to_runner() {
        let Some((_, type_name, parse)) = parses
            .iter()
            .find(|(t, _, _)| text.starts_with(&format!(r#"{{"t":"{t}""#)))
        else {
            continue;
        };
        read += 1;
        let room = FRAME_JSON_MAX - text.len();
        let at = format!("{{{}{}", " ".repeat(room), &text[1..]);
        assert_eq!(at.len(), FRAME_JSON_MAX);
        assert!(parse(at.as_bytes()).is_ok(), "{text}");
        assert!(ToRunner::parse(at.as_bytes()).is_ok(), "{text}");
        for over in [
            format!("{}{text}", " ".repeat(room + 1)),
            format!("{text}{}", "\n".repeat(room + 1)),
            format!("{{{}{}", "\t".repeat(room + 1), &text[1..]),
        ] {
            let refusal = parse(over.as_bytes()).expect_err(&text);
            assert_eq!(
                (refusal.cause(), refusal.type_name(), refusal.rule()),
                (RefusalCause::Length, *type_name, "frame_length"),
                "{text}"
            );
        }
    }
    assert_eq!(read, 14);
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
fn a_secret_reply_one_byte_over_the_frame_bound_is_refused_before_any_byte_of_it_is_read() {
    let text = r#"{"t":"secret_reply","answer":"none"}"#;
    let fill = |len: usize| " ".repeat(len - text.len() - 1);
    for len in [FRAME_JSON_MAX, FRAME_JSON_MAX + 1] {
        let not_json_at_first = format!("x{}{text}", fill(len));
        let broken_at_last = format!("{text}{}x", fill(len));
        for input in [not_json_at_first, broken_at_last] {
            assert_eq!(input.len(), len);
            let parsed = SecretReply::parse(input.as_bytes());
            if len > FRAME_JSON_MAX {
                refused_with(parsed, RefusalCause::Length, "SecretReply", "frame_length");
            } else {
                refused_with(parsed, RefusalCause::Malformed, "SecretReply", "json");
            }
        }
    }
}

#[test]
fn a_reply_frame_is_counted_in_bytes_and_every_carriage_return_counts() {
    let head = r#"{"t":"secret_reply","answer":"stored","value":""#;
    let wide = |len: usize| {
        let room = len - head.len() - 2;
        format!(
            "{head}{}{}\"}}",
            "c".repeat(room % 2),
            "\u{e9}".repeat(room / 2)
        )
    };
    let at = wide(FRAME_JSON_MAX);
    assert_eq!(at.len(), FRAME_JSON_MAX);
    assert!(SecretReply::parse(at.as_bytes()).is_ok());
    let over = wide(FRAME_JSON_MAX + 1);
    assert_eq!(over.len(), FRAME_JSON_MAX + 1);
    assert!(over.chars().count() <= FRAME_JSON_MAX / 2 + head.len());
    refused_with(
        SecretReply::parse(over.as_bytes()),
        RefusalCause::Length,
        "SecretReply",
        "frame_length",
    );
    refused_with(
        ToRunner::parse(over.as_bytes()),
        RefusalCause::Length,
        "ToRunner",
        "frame_length",
    );
    let browser = r#"{"t":"browser_reply"}"#;
    let returns = |len: usize| format!("{browser}{}", "\r".repeat(len - browser.len()));
    assert!(BrowserReply::parse(returns(FRAME_JSON_MAX).as_bytes()).is_ok());
    assert!(ToRunner::parse(returns(FRAME_JSON_MAX).as_bytes()).is_ok());
    let over = returns(FRAME_JSON_MAX + 1);
    assert_eq!(over.len(), FRAME_JSON_MAX + 1);
    refused_with(
        BrowserReply::parse(over.as_bytes()),
        RefusalCause::Length,
        "BrowserReply",
        "frame_length",
    );
    refused_with(
        ToRunner::parse(over.as_bytes()),
        RefusalCause::Length,
        "ToRunner",
        "frame_length",
    );
    let secret = r#"{"t":"secret_reply","answer":"none"}"#;
    let over = format!("{secret}{}", "\r".repeat(FRAME_JSON_MAX + 1 - secret.len()));
    refused_with(
        SecretReply::parse(over.as_bytes()),
        RefusalCause::Length,
        "SecretReply",
        "frame_length",
    );
}
