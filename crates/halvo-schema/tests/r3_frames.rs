use halvo_schema::executable::body::RunnerCall;
use halvo_schema::executable::frame::{
    ConfirmDecision, ConfirmRequest, FromRunner, HandbackRequest, HandbackResult, ReplyError,
    RunError, StoreRequest, ToRunner, FRAME_JSON_MAX,
};
use halvo_schema::executable::{Refusal, RefusalCause, ERROR_NAMES};
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
        r#"{{"v":1,"run":"{R}","agent":"{A}","effects":[{}]}}"#,
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
    ]
}

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
        r#"{"t":"secret_reply","value":"tok_fixture"}"#.to_owned(),
        r#"{"t":"secret_reply","value":null}"#.to_owned(),
        r#"{"t":"cancel"}"#.to_owned(),
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
        ToRunner::parse(br#"{"t":"secret_reply","value":1}"#),
        "SecretText",
        "not_a_string",
        "value",
    );
    expect(
        ToRunner::parse(br#"{"t":"secret_reply"}"#),
        "SecretReply",
        "missing_key",
        "value",
    );

    let shell = r#"{"t":"secret_reply","value":""}"#.len();
    let exact = format!(
        r#"{{"t":"secret_reply","value":"{}"}}"#,
        "s".repeat(FRAME_JSON_MAX - shell)
    );
    assert_eq!(exact.len(), FRAME_JSON_MAX);
    assert_eq!(
        ToRunner::parse(exact.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        exact
    );
    let over = format!(
        r#"{{"t":"secret_reply","value":"{}"}}"#,
        "s".repeat(FRAME_JSON_MAX - shell + 1)
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
    let ToRunner::SecretReply(mut frame) = ToRunner::parse(exact.as_bytes()).unwrap() else {
        panic!()
    };
    frame.value = Some(halvo_schema::executable::text::SecretText::new(
        &"s".repeat(FRAME_JSON_MAX),
    ));
    assert_eq!(
        ToRunner::SecretReply(frame).to_json().unwrap_err().cause(),
        RefusalCause::Length
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
            format!(r#"{{"v":1,"run":"{R}","agent":"{A}","effects":{pay}}}"#),
            "list",
            "not_a_list",
            "effects",
        ),
    ];
    for (text, type_name, rule, key) in cases {
        expect(ConfirmRequest::parse(text.as_bytes()), type_name, rule, key);
    }
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
    let secret = ToRunner::parse(br#"{"t":"secret_reply","value":"tok_4711"}"#).unwrap();
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
