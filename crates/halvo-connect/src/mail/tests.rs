use serde_json::{json, Value};

use super::{kept, Draft, DraftToSend, Drafts, Message, Messages, ReplyRule};
use crate::error::{AnswerFault, CallError};
use crate::same_address;

const FORM: Result<Vec<String>, CallError> = Err(CallError::Answer(AnswerFault::Form));

fn rule() -> ReplyRule {
    ReplyRule::new(
        "me@home.test".to_owned(),
        vec!["ann@people.test".to_owned(), "Bob@People.test".to_owned()],
    )
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

fn ids(answer: &Value) -> Result<Vec<String>, CallError> {
    kept(answer, &rule()).map(|list| list.kept.into_iter().map(|m| m.id).collect())
}

#[test]
fn the_rule_takes_unread_mail_to_the_account_from_a_sender_of_the_slot() {
    let r = rule();
    let to = ["x@home.test".to_owned(), "ME@HOME.test".to_owned()];
    assert!(r.selects(false, "ann@people.test", &to));
    assert!(r.selects(false, "bob@people.TEST", &to));
    assert!(!r.selects(true, "ann@people.test", &to));
    assert!(!r.selects(false, "eve@people.test", &to));
    assert!(!r.selects(false, "ann@people.test", &["x@home.test".to_owned()]));
    assert!(!r.selects(false, "ann@people.test", &[]));
    assert!(!r.selects(false, "ann@people.test.x", &to));
    let none = ReplyRule::new("me@home.test".to_owned(), Vec::new());
    assert!(!none.selects(false, "ann@people.test", &to));
}

#[test]
fn two_addresses_are_one_without_regard_to_ascii_case_alone_in_the_rule_too() {
    for (a, b, same) in [
        ("ann@people.test", "ann@people.test", true),
        ("ann@people.test", "ANN@People.TEST", true),
        ("ann@people.test", "ann@people.test ", false),
        ("ann@people.test", "anne@people.test", false),
        ("ann@people.test", "ann@people.tesu", false),
        ("élan@people.test", "éLAN@PEOPLE.test", true),
        ("élan@people.test", "ÉLAN@people.test", false),
        ("strasse@people.test", "straße@people.test", false),
        ("k@people.test", "\u{212a}@people.test", false),
    ] {
        assert_eq!(same_address(a, b), same, "{a} {b}");
        assert_eq!(same_address(b, a), same, "{b} {a}");
        let as_sender = ReplyRule::new("me@home.test".to_owned(), vec![a.to_owned()]);
        assert_eq!(
            as_sender.selects(false, b, &["me@home.test".to_owned()]),
            same,
            "sender {a} {b}"
        );
        let as_account = ReplyRule::new(a.to_owned(), vec!["ann@people.test".to_owned()]);
        assert_eq!(
            as_account.selects(false, "ann@people.test", &[b.to_owned()]),
            same,
            "account {a} {b}"
        );
    }
}

#[test]
fn a_list_keeps_the_selected_messages_in_the_order_of_the_answer() {
    let answer = json!({"value": [
        message("m1", false, "bob@people.test", &["me@home.test"]),
        message("m2", true, "ann@people.test", &["me@home.test"]),
        message("m3", false, "ann@people.test", &["x@home.test"]),
        message("m4", false, "ann@people.test", &["x@home.test", "me@home.test"]),
        message("m5", false, "eve@people.test", &["me@home.test"]),
    ]});
    assert_eq!(ids(&answer), Ok(vec!["m1".to_owned(), "m4".to_owned()]));
    let list = kept(&answer, &rule()).expect("a list");
    assert_eq!(list.listed, ["m1", "m2", "m3", "m4", "m5"]);
    let first = &list.kept[0];
    assert_eq!(
        (
            first.conversation(),
            first.subject(),
            first.sender(),
            first.body()
        ),
        ("c-m1", "s-m1", "bob@people.test", "b-m1")
    );
    assert_eq!(ids(&json!({"value": []})), Ok(Vec::new()));
}

#[test]
fn a_paged_list_is_refused() {
    let answer = json!({
        "value": [message("m1", false, "ann@people.test", &["me@home.test"])],
        "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/mailFolders/inbox/messages?$skip=10",
    });
    assert_eq!(ids(&answer), Err(CallError::Answer(AnswerFault::Paged)));
}

#[test]
fn a_repeated_id_is_refused_also_outside_the_rule() {
    let answer = json!({"value": [
        message("m1", true, "eve@people.test", &["me@home.test"]),
        message("m2", false, "ann@people.test", &["me@home.test"]),
        message("m1", true, "eve@people.test", &["me@home.test"]),
    ]});
    assert_eq!(ids(&answer), Err(CallError::Answer(AnswerFault::Repeated)));
}

#[test]
fn an_id_that_a_path_would_lose_is_refused() {
    for id in ["", ".", ".."] {
        let answer = json!({"value": [message(id, true, "eve@people.test", &[])]});
        assert_eq!(
            ids(&answer),
            Err(CallError::Answer(AnswerFault::Id)),
            "{id}"
        );
    }
    let answer = json!({"value": [message("...", false, "ann@people.test", &["me@home.test"])]});
    assert_eq!(ids(&answer), Ok(vec!["...".to_owned()]));
}

#[test]
fn each_key_of_the_face_is_needed() {
    let whole = message("m1", false, "ann@people.test", &["me@home.test"]);
    for pointer in [
        "/id",
        "/conversationId",
        "/subject",
        "/isRead",
        "/from",
        "/from/emailAddress",
        "/from/emailAddress/address",
        "/toRecipients",
        "/toRecipients/0/emailAddress/address",
        "/body",
        "/body/content",
    ] {
        let mut item = whole.clone();
        let (parent, key) = pointer.rsplit_once('/').expect("a pointer");
        let target = if parent.is_empty() {
            &mut item
        } else {
            item.pointer_mut(parent).expect("the parent")
        };
        target
            .as_object_mut()
            .expect("an object")
            .remove(key)
            .expect("the key");
        assert_eq!(ids(&json!({"value": [item]})), FORM, "{pointer}");
    }
    for (pointer, value) in [
        ("/isRead", json!("false")),
        ("/subject", json!(null)),
        ("/toRecipients", json!({})),
    ] {
        let mut item = whole.clone();
        *item.pointer_mut(pointer).expect("the key") = value;
        assert_eq!(ids(&json!({"value": [item]})), FORM, "{pointer}");
    }
    for answer in [
        json!([]),
        json!({}),
        json!({"value": {}}),
        json!({"value": null}),
    ] {
        assert_eq!(ids(&answer), FORM);
    }
}

#[test]
fn debug_prints_no_value() {
    let message = Message {
        id: "m1".to_owned(),
        conversation: "c1".to_owned(),
        subject: "s1".to_owned(),
        sender: "ann@people.test".to_owned(),
        body: "b1".to_owned(),
    };
    let draft = Draft {
        id: "d1".to_owned(),
        message: "m1".to_owned(),
        to: vec!["ann@people.test".to_owned()],
        subject: "s1".to_owned(),
    };
    assert_eq!(format!("{:?}", rule()), "ReplyRule { senders: 2, .. }");
    assert_eq!(format!("{message:?}"), "Message { .. }");
    assert_eq!(
        format!(
            "{:?}",
            Messages {
                kept: vec![message],
                listed: vec!["m1".to_owned()],
            }
        ),
        "Messages { len: 1 }"
    );
    assert_eq!(format!("{draft:?}"), "Draft { .. }");
    assert_eq!(
        format!("{:?}", DraftToSend(draft.clone())),
        "DraftToSend { .. }"
    );
    assert_eq!(format!("{:?}", Drafts(vec![draft])), "Drafts { len: 1 }");
}
