mod common;

use std::time::Duration;

use common::stub::{Seen, Stub, HOLD};
use halvo_connect::{AnswerFault, Calendar, CallError, Effect, Mail, ReplyRule, RunDay, Token};
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

fn event(id: &str, start: &str, cancelled: bool, response: &str) -> Value {
    json!({
        "id": id,
        "subject": format!("s-{id}"),
        "start": {"dateTime": start, "timeZone": "UTC"},
        "end": {"dateTime": start, "timeZone": "UTC"},
        "isCancelled": cancelled,
        "responseStatus": {"response": response},
    })
}

#[test]
fn the_cleanup_task_makes_exactly_its_requests() {
    let window =
        "/v1.0/me/calendarView?startDateTime=2026-10-13T00:00:00Z&endDateTime=2026-11-13T00:00:00Z";
    let view = json!({"value": [
        event("e1", "2026-10-12T23:59:59.9999999", true, "organizer"),
        event("e2", "2026-10-13T00:00:00.0000000", true, "organizer"),
        event("e/3", "2026-11-12T23:59:59.9999999", false, "declined"),
        event("e4", "2026-11-13T00:00:00.0000000", true, "organizer"),
        event("e5", "2026-11-01T10:00:00.0000000", false, "accepted"),
    ]});
    let stub = Stub::start(vec![
        (format!("GET {window}"), 200, view.to_string()),
        ("DELETE /v1.0/me/events/e2".to_owned(), 204, String::new()),
        (
            "DELETE /v1.0/me/events/e%2F3".to_owned(),
            204,
            String::new(),
        ),
    ]);
    let graph = stub.graph("calendar");
    let events = graph
        .calendar_list(&token(), RunDay::from_unix_seconds(1_794_441_600 + 86_399))
        .expect("calendar_list answers");
    let kept: Vec<_> = events
        .iter()
        .map(|e| (e.id(), e.subject(), e.start()))
        .collect();
    assert_eq!(
        kept,
        [
            ("e2", "s-e2", "2026-10-13T00:00:00.0000000"),
            ("e/3", "s-e/3", "2026-11-12T23:59:59.9999999"),
        ]
    );
    for handle in events.into_deletes() {
        graph
            .calendar_delete(&token(), handle)
            .expect("calendar_delete answers");
    }
    assert_eq!(
        stub.seen(),
        [
            seen("GET", window, None),
            seen("DELETE", "/v1.0/me/events/e2", None),
            seen("DELETE", "/v1.0/me/events/e%2F3", None),
        ]
    );
}

#[test]
fn an_effect_without_an_answer_is_unknown_and_not_sent_again() {
    let window =
        "/v1.0/me/calendarView?startDateTime=2026-10-13T00:00:00Z&endDateTime=2026-11-13T00:00:00Z";
    let view = json!({"value": [
        event("e1", "2026-11-01T10:00:00.0000000", true, "organizer"),
        event("e2", "2026-11-02T10:00:00.0000000", true, "organizer"),
    ]});
    let stub = Stub::start(vec![
        (format!("GET {window}"), 200, view.to_string()),
        ("DELETE /v1.0/me/events/e1".to_owned(), HOLD, String::new()),
        ("DELETE /v1.0/me/events/e2".to_owned(), 204, String::new()),
    ]);
    let graph = stub.graph_within("calendar", Duration::from_millis(500));
    let events = graph
        .calendar_list(&token(), RunDay::from_unix_seconds(1_794_441_600))
        .expect("calendar_list answers");
    let mut deletes = events.into_deletes();
    let error = graph
        .calendar_delete(&token(), deletes.next().expect("an event"))
        .expect_err("no answer");
    assert_eq!(error, CallError::NoAnswer);
    assert_eq!(error.effect(), Effect::Unknown);
    graph
        .calendar_delete(&token(), deletes.next().expect("an event"))
        .expect("calendar_delete answers");
    assert_eq!(
        stub.seen(),
        [
            seen("GET", window, None),
            seen("DELETE", "/v1.0/me/events/e1", None),
            seen("DELETE", "/v1.0/me/events/e2", None),
        ]
    );
}
