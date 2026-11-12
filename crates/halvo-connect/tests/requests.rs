mod common;

use common::stub::{Seen, Stub};
use halvo_connect::{AnswerFault, CallError, Mail, ReplyRule, Token};
use serde_json::{json, Value};

const INBOX: &str = "/v1.0/me/mailFolders/inbox/messages";
const BEARER: &str = "Bearer stub-token-1";

fn token() -> Token {
    Token::new("stub-token-1".to_owned()).expect("a token")
}

fn message(id: &str, is_read: bool, from: &str, to: &[&str]) -> Value {
    json!({
        "id": id,
        "conversationId": format!("c-{id}"),
        "subject": format!("s-{id}"),
        "isRead": is_read,
        "from": {"emailAddress": {"address": from}},
        "toRecipients": to.iter().map(|a| json!({"emailAddress": {"address": a}})).collect::<Vec<_>>(),
        "body": {"content": format!("b-{id}")},
    })
}

fn draft(id: &str, to: &str, subject: &str) -> String {
    json!({"id": id, "subject": subject, "toRecipients": [{"emailAddress": {"address": to}}]})
        .to_string()
}

fn seen(method: &str, path: &str, body: Option<Value>) -> Seen {
    Seen {
        method: method.to_owned(),
        path: path.to_owned(),
        authorization: Some(BEARER.to_owned()),
        body,
    }
}

#[test]
fn the_reply_task_makes_exactly_its_requests() {
    let inbox = json!({"value": [
        message("m1", false, "ann@people.test", &["me@home.test"]),
        message("m2", true, "ann@people.test", &["me@home.test"]),
        message("m/3", false, "BOB@people.test", &["x@home.test", "ME@home.test"]),
        message("m4", false, "ann@people.test", &["x@home.test"]),
        message("m5", false, "eve@people.test", &["me@home.test"]),
    ]});
    let stub = Stub::start(vec![
        (format!("GET {INBOX}"), 200, inbox.to_string()),
        (
            "POST /v1.0/me/messages/m1/createReply".to_owned(),
            201,
            json!({"id": "d1"}).to_string(),
        ),
        (
            "POST /v1.0/me/messages/m%2F3/createReply".to_owned(),
            201,
            json!({"id": "d/3"}).to_string(),
        ),
        (
            "PATCH /v1.0/me/messages/d1".to_owned(),
            200,
            draft("d1", "ann@people.test", "RE: s-m1"),
        ),
        (
            "PATCH /v1.0/me/messages/d%2F3".to_owned(),
            200,
            draft("d/3", "BOB@people.test", "RE: s-m/3"),
        ),
        (
            "POST /v1.0/me/messages/d1/send".to_owned(),
            202,
            String::new(),
        ),
        (
            "POST /v1.0/me/messages/d%2F3/send".to_owned(),
            202,
            String::new(),
        ),
    ]);
    let graph = stub.graph("mail");
    let rule = ReplyRule::new(
        "me@home.test".to_owned(),
        vec!["ann@people.test".to_owned(), "bob@people.test".to_owned()],
    );
    let messages = graph.mail_list(&token(), &rule).expect("mail_list answers");
    let drafts = graph
        .mail_draft_reply(&token(), messages, "Thanks.")
        .expect("mail_draft_reply answers");
    let kept: Vec<_> = drafts
        .iter()
        .map(|d| (d.id(), d.message(), d.to().to_vec(), d.subject()))
        .collect();
    assert_eq!(
        kept,
        [
            ("d1", "m1", vec!["ann@people.test".to_owned()], "RE: s-m1"),
            (
                "d/3",
                "m/3",
                vec!["BOB@people.test".to_owned()],
                "RE: s-m/3"
            ),
        ]
    );
    for handle in drafts.into_sends() {
        graph
            .mail_send(&token(), handle)
            .expect("mail_send answers");
    }
    let patch = json!({"body": {"contentType": "text", "content": "Thanks."}});
    assert_eq!(
        stub.seen(),
        [
            seen("GET", INBOX, None),
            seen("POST", "/v1.0/me/messages/m1/createReply", Some(json!({}))),
            seen("PATCH", "/v1.0/me/messages/d1", Some(patch.clone())),
            seen(
                "POST",
                "/v1.0/me/messages/m%2F3/createReply",
                Some(json!({}))
            ),
            seen("PATCH", "/v1.0/me/messages/d%2F3", Some(patch)),
            seen("POST", "/v1.0/me/messages/d1/send", None),
            seen("POST", "/v1.0/me/messages/d%2F3/send", None),
        ]
    );
}

#[test]
fn a_paged_inbox_is_refused_after_one_request() {
    let inbox = json!({
        "value": [message("m1", false, "ann@people.test", &["me@home.test"])],
        "@odata.nextLink": "http://mail.agent.test/v1.0/me/mailFolders/inbox/messages?$skip=1",
    });
    let stub = Stub::start(vec![(format!("GET {INBOX}"), 200, inbox.to_string())]);
    let rule = ReplyRule::new(
        "me@home.test".to_owned(),
        vec!["ann@people.test".to_owned()],
    );
    assert_eq!(
        stub.graph("mail").mail_list(&token(), &rule).err(),
        Some(CallError::Answer(AnswerFault::Paged))
    );
    assert_eq!(stub.seen(), [seen("GET", INBOX, None)]);
}

#[test]
fn draft_reply_stops_at_the_first_error() {
    let inbox = json!({"value": [
        message("m1", false, "ann@people.test", &["me@home.test"]),
        message("m2", false, "ann@people.test", &["me@home.test"]),
    ]});
    let stub = Stub::start(vec![
        (format!("GET {INBOX}"), 200, inbox.to_string()),
        (
            "POST /v1.0/me/messages/m1/createReply".to_owned(),
            201,
            json!({"id": "d1"}).to_string(),
        ),
        (
            "PATCH /v1.0/me/messages/d1".to_owned(),
            503,
            "{}".to_owned(),
        ),
    ]);
    let graph = stub.graph("mail");
    let rule = ReplyRule::new(
        "me@home.test".to_owned(),
        vec!["ann@people.test".to_owned()],
    );
    let messages = graph.mail_list(&token(), &rule).expect("mail_list answers");
    assert_eq!(
        graph.mail_draft_reply(&token(), messages, "Thanks.").err(),
        Some(CallError::Status(503))
    );
    let patch = json!({"body": {"contentType": "text", "content": "Thanks."}});
    assert_eq!(
        stub.seen(),
        [
            seen("GET", INBOX, None),
            seen("POST", "/v1.0/me/messages/m1/createReply", Some(json!({}))),
            seen("PATCH", "/v1.0/me/messages/d1", Some(patch)),
        ]
    );
}
