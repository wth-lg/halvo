mod common;

use common::stub::Stub;
use halvo_connect::{
    reply_senders, AnswerFault, CallError, DraftFailure, Drafted, Mail, Marked, ReplyRule, Token,
    Unsent,
};
use halvo_schema::record::Instant;
use serde_json::{json, Value};

const LIST: &str = "/v1.0/me/mailFolders/inbox/messages?$filter=isRead%20eq%20false%20and%20from%2FemailAddress%2Faddress%20eq%20%27ann%40people.test%27%20and%20receivedDateTime%20ge%201970-01-01T00%3A00%3A00Z&$top=25&$select=id%2CisRead%2Cfrom%2CtoRecipients%2CreplyTo%2CreceivedDateTime%2CconversationId%2Csubject%2Cbody%2CisReadReceiptRequested";

fn token() -> Token {
    Token::new("stub-token-1".to_owned()).expect("a token")
}

fn message(id: &str) -> Value {
    json!({
        "isReadReceiptRequested": id.ends_with('r'),
        "id": id,
        "conversationId": format!("c-{id}"),
        "subject": format!("s-{id}"),
        "isRead": false,
        "from": {"emailAddress": {"address": "ann@people.test"}},
        "toRecipients": [{"emailAddress": {"address": "me@home.test"}}],
        "replyTo": [],
        "receivedDateTime": "2026-11-18T10:00:00Z",
        "body": {"content": format!("b-{id}")},
    })
}

fn draft(id: &str, message: &str) -> Value {
    json!({
        "id": id,
        "conversationId": format!("c-{message}"),
        "subject": format!("RE: s-{message}"),
        "isDraft": true,
        "toRecipients": [{"emailAddress": {"address": "ann@people.test"}}],
        "ccRecipients": [],
        "bccRecipients": [],
    })
}

fn created(id: &str) -> String {
    json!({"id": id, "isDraft": true}).to_string()
}

fn routes(messages: &[&str], answers: Vec<(String, u16, String)>) -> Stub {
    let inbox = json!({"value": messages.iter().map(|id| message(id)).collect::<Vec<_>>()});
    let mut all = vec![(format!("GET {LIST}"), 200, inbox.to_string())];
    all.extend(answers);
    Stub::start(all)
}

fn reply(stub: &Stub) -> Result<Drafted, Box<DraftFailure>> {
    let graph = stub.graph("mail");
    let rule = ReplyRule::new(
        "me@home.test".to_owned(),
        reply_senders("ann@people.test").expect("a sender"),
        Instant(0),
    );
    let messages = graph.mail_list(&token(), &rule).expect("mail_list answers");
    graph.mail_draft_reply(&token(), messages, "Thanks.")
}

fn create(message: &str, status: u16, answer: String) -> (String, u16, String) {
    (
        format!("POST /v1.0/me/messages/{message}/createReply"),
        status,
        answer,
    )
}

fn patch(draft: &str, status: u16, answer: Value) -> (String, u16, String) {
    (
        format!("PATCH /v1.0/me/messages/{draft}"),
        status,
        answer.to_string(),
    )
}

fn methods(stub: &Stub) -> Vec<String> {
    stub.seen()
        .into_iter()
        .map(|seen| {
            format!(
                "{} {}",
                seen.method,
                seen.path.split('?').next().unwrap_or("")
            )
        })
        .collect()
}

#[test]
fn a_create_reply_answer_that_is_no_draft_fails_the_call_before_its_patch() {
    for answer in [
        json!({"id": "d1", "isDraft": false}),
        json!({"id": "d1"}),
        json!({"id": "d1", "isDraft": "true"}),
    ] {
        let stub = routes(
            &["m1", "m2"],
            vec![
                create("m1", 201, answer.to_string()),
                patch("d1", 200, draft("d1", "m1")),
            ],
        );
        let failure = reply(&stub).expect_err("the call fails");
        assert_eq!(
            failure.error(),
            CallError::Answer(AnswerFault::Draft),
            "{answer}"
        );
        assert!(failure.finished().is_empty() && failure.unsent().is_empty());
        assert!(
            failure.unfinished().is_none(),
            "no PATCH is sent to that id"
        );
        assert_eq!(failure.unreached(), ["m1", "m2"]);
        assert_eq!(
            methods(&stub),
            [
                "GET /v1.0/me/mailFolders/inbox/messages",
                "POST /v1.0/me/messages/m1/createReply"
            ],
            "{answer}"
        );
    }
}

#[test]
fn each_check_after_the_patch_holds_the_draft_back_with_its_message_and_its_reason() {
    let with = |key: &str, value: Value| {
        let mut answer = draft("d1", "m1");
        answer[key] = value;
        answer
    };
    let other = |address: &str| json!([{"emailAddress": {"address": address}}]);
    let rows = [
        (with("toRecipients", json!([])), Unsent::Recipients),
        (
            with(
                "toRecipients",
                json!([
                    {"emailAddress": {"address": "ann@people.test"}},
                    {"emailAddress": {"address": "bob@people.test"}}
                ]),
            ),
            Unsent::Recipients,
        ),
        (
            with("toRecipients", other("Ann@people.test")),
            Unsent::Recipient,
        ),
        (
            with("toRecipients", other("ann@people.test ")),
            Unsent::Recipient,
        ),
        (
            with("toRecipients", other("eve@people.test")),
            Unsent::Recipient,
        ),
        (with("ccRecipients", other("eve@people.test")), Unsent::Copy),
        (
            with("bccRecipients", other("eve@people.test")),
            Unsent::Copy,
        ),
        (
            with("conversationId", json!("c-other")),
            Unsent::Conversation,
        ),
        (with("isDraft", json!(false)), Unsent::NotDraft),
    ];
    for (answer, why) in rows {
        let stub = routes(
            &["m1"],
            vec![
                create("m1", 201, created("d1")),
                patch("d1", 200, answer.clone()),
            ],
        );
        let drafted = reply(&stub).expect("the call answers");
        assert!(drafted.drafts().is_empty(), "{answer}: no send handle");
        let unsent: Vec<_> = drafted
            .unsent()
            .iter()
            .map(|u| (u.message().to_owned(), u.draft().to_owned(), u.why()))
            .collect();
        assert_eq!(
            unsent,
            [("m1".to_owned(), "d1".to_owned(), why)],
            "{answer}"
        );
        assert_eq!(
            drafted.held_recipient(),
            u32::from(why != Unsent::NotDraft),
            "{answer}"
        );
    }
}

#[test]
fn a_cause_of_the_call_or_not_a_draft_wins_over_a_reason_of_the_closed_list() {
    let mut not_draft = draft("d1", "m1");
    not_draft["isDraft"] = json!(false);
    not_draft["toRecipients"] = json!([
        {"emailAddress": {"address": "ann@people.test"}},
        {"emailAddress": {"address": "bob@people.test"}}
    ]);
    let stub = routes(
        &["m1"],
        vec![
            create("m1", 201, created("d1")),
            patch("d1", 200, not_draft),
        ],
    );
    let drafted = reply(&stub).expect("the call answers");
    assert_eq!(drafted.unsent()[0].why(), Unsent::NotDraft);
    assert_eq!(drafted.held_recipient(), 0);
    let mut lacking = draft("d1", "m1");
    lacking["ccRecipients"] = json!(null);
    lacking["toRecipients"] = json!([]);
    let stub = routes(
        &["m1"],
        vec![create("m1", 201, created("d1")), patch("d1", 200, lacking)],
    );
    assert_eq!(
        reply(&stub).expect_err("the call fails").error(),
        CallError::Answer(AnswerFault::Form)
    );
}

#[test]
fn a_draft_with_one_recipient_that_is_the_sender_and_no_conversation_key_gets_a_handle() {
    let mut answer = draft("d1", "m1");
    answer
        .as_object_mut()
        .expect("an object")
        .remove("conversationId");
    answer["subject"] = json!(null);
    let stub = routes(
        &["m1"],
        vec![create("m1", 201, created("d1")), patch("d1", 200, answer)],
    );
    let drafted = reply(&stub).expect("the call answers");
    assert!(drafted.unsent().is_empty());
    let kept: Vec<_> = drafted
        .drafts()
        .iter()
        .map(|d| (d.id(), d.message(), d.to().to_vec(), d.subject()))
        .collect();
    assert_eq!(kept, [("d1", "m1", vec!["ann@people.test".to_owned()], "")]);
}

#[test]
fn an_error_after_the_patch_fails_the_call_and_returns_what_the_call_made() {
    let mut no_address = draft("d2", "m2");
    no_address["toRecipients"] = json!([{"emailAddress": {}}]);
    let mut no_flag = draft("d2", "m2");
    no_flag
        .as_object_mut()
        .expect("an object")
        .remove("isDraft");
    let mut no_copy = draft("d2", "m2");
    no_copy
        .as_object_mut()
        .expect("an object")
        .remove("bccRecipients");
    let mut odd_conversation = draft("d2", "m2");
    odd_conversation["conversationId"] = json!(7);
    let rows = [
        (
            200,
            draft("other", "m2"),
            CallError::Answer(AnswerFault::Item),
        ),
        (200, no_address, CallError::Answer(AnswerFault::Form)),
        (200, no_flag, CallError::Answer(AnswerFault::Form)),
        (200, no_copy, CallError::Answer(AnswerFault::Form)),
        (200, odd_conversation, CallError::Answer(AnswerFault::Form)),
        (503, json!({}), CallError::Status(503, None)),
    ];
    let mut held = draft("d1", "m1");
    held["ccRecipients"] = json!([{"emailAddress": {"address": "eve@people.test"}}]);
    for (status, answer, error) in rows {
        let stub = routes(
            &["m0", "m1", "m2", "m3"],
            vec![
                create("m0", 201, created("d0")),
                patch("d0", 200, draft("d0", "m0")),
                create("m1", 201, created("d1")),
                patch("d1", 200, held.clone()),
                create("m2", 201, created("d2")),
                patch("d2", status, answer.clone()),
                create("m3", 201, created("d3")),
            ],
        );
        let failure = reply(&stub).expect_err("the call fails");
        assert_eq!(failure.error(), error, "{answer}");
        let finished: Vec<_> = failure.finished().iter().map(|d| d.id()).collect();
        assert_eq!(finished, ["d0"], "{answer}");
        let unsent: Vec<_> = failure
            .unsent()
            .iter()
            .map(|u| (u.draft(), u.why()))
            .collect();
        assert_eq!(unsent, [("d1", Unsent::Copy)], "{answer}");
        let unfinished = failure.unfinished().map(|u| (u.message(), u.draft()));
        assert_eq!(unfinished, Some(("m2", "d2")), "{answer}");
        assert_eq!(failure.unreached(), ["m3"], "{answer}");
        assert!(
            !methods(&stub).contains(&"POST /v1.0/me/messages/m3/createReply".to_owned()),
            "{answer}: it stops"
        );
    }
}

#[test]
fn a_failed_create_reply_leaves_its_message_unreached() {
    let stub = routes(&["m1", "m2"], vec![create("m1", 503, "{}".to_owned())]);
    let failure = reply(&stub).expect_err("the call fails");
    assert_eq!(failure.error(), CallError::Status(503, None));
    assert!(failure.unfinished().is_none());
    assert_eq!(failure.unreached(), ["m1", "m2"]);
    assert_eq!(format!("{failure:?}"), "DraftFailure { error: Status(503, None), finished: 0, unsent: 0, unfinished: false, unreached: 2 }");
    assert_eq!(failure.to_string(), "the provider answered the status 503");
}

fn send(draft: &str) -> (String, u16, String) {
    (
        format!("POST /v1.0/me/messages/{draft}/send"),
        202,
        String::new(),
    )
}

fn marked(message: &str, read: bool) -> (String, u16, String) {
    (
        format!("PATCH /v1.0/me/messages/{message}"),
        200,
        json!({"id": message, "isRead": read}).to_string(),
    )
}

fn send_and_mark(stub: &Stub) -> Vec<Result<Marked, CallError>> {
    let graph = stub.graph("mail");
    let drafted = reply(stub).expect("the call answers");
    drafted
        .into_drafts()
        .into_sends()
        .map(|handle| {
            let sent = graph
                .mail_send(&token(), handle)
                .expect("mail_send answers");
            graph.mail_mark_read(&token(), sent)
        })
        .collect()
}

#[test]
fn after_a_send_one_patch_of_is_read_marks_its_message_and_none_for_a_read_receipt() {
    let stub = routes(
        &["m1", "m2r"],
        vec![
            create("m1", 201, created("d1")),
            patch("d1", 200, draft("d1", "m1")),
            create("m2r", 201, created("d2")),
            patch("d2", 200, draft("d2", "m2r")),
            send("d1"),
            send("d2"),
            marked("m1", true),
            marked("m2r", true),
        ],
    );
    assert_eq!(
        send_and_mark(&stub),
        [Ok(Marked::Read), Ok(Marked::ReceiptAsked)]
    );
    let seen = stub.seen();
    let tail: Vec<_> = seen[seen.len() - 3..]
        .iter()
        .map(|seen| (seen.method.as_str(), seen.path.as_str(), seen.body.clone()))
        .collect();
    assert_eq!(
        tail,
        [
            ("POST", "/v1.0/me/messages/d1/send", None),
            (
                "PATCH",
                "/v1.0/me/messages/m1",
                Some(json!({"isRead": true}))
            ),
            ("POST", "/v1.0/me/messages/d2/send", None),
        ]
    );
}

#[test]
fn a_mark_whose_answer_names_another_message_or_no_read_flag_fails() {
    for (answer, error) in [
        (
            json!({"id": "m9", "isRead": true}),
            CallError::Answer(AnswerFault::Item),
        ),
        (
            json!({"id": "m1", "isRead": false}),
            CallError::Answer(AnswerFault::Form),
        ),
        (json!({"id": "m1"}), CallError::Answer(AnswerFault::Form)),
    ] {
        let stub = routes(
            &["m1"],
            vec![
                create("m1", 201, created("d1")),
                patch("d1", 200, draft("d1", "m1")),
                send("d1"),
                (
                    "PATCH /v1.0/me/messages/m1".to_owned(),
                    200,
                    answer.to_string(),
                ),
            ],
        );
        assert_eq!(send_and_mark(&stub), [Err(error)], "{answer}");
    }
    let stub = routes(
        &["m1"],
        vec![
            create("m1", 201, created("d1")),
            patch("d1", 200, draft("d1", "m1")),
            send("d1"),
            ("PATCH /v1.0/me/messages/m1".to_owned(), 429, String::new()),
        ],
    );
    assert_eq!(send_and_mark(&stub), [Err(CallError::Status(429, None))]);
}
