use std::num::NonZeroUsize;

use halvo_schema::executable::names::YesNo;
use halvo_schema::record::Instant;
use serde_json::{json, Value};

use super::{
    verdict, Draft, DraftToSend, Drafts, MailCounts, Message, Messages, ReplyRule, Senders, Verdict,
};
use crate::{reply_senders, same_address, SendersError};

const NOV_18_2026_10_00: i64 = 1_794_996_000;

fn senders(slot: &str) -> Senders {
    reply_senders(slot).expect("senders that the reader takes")
}

fn rule() -> ReplyRule {
    ReplyRule::new(
        "me@home.test".to_owned(),
        senders("ann@people.test, Bob@People.test"),
        Instant(NOV_18_2026_10_00 * 1_000_000_000 + 999_999_999),
    )
}

fn kind(item: &Value) -> &'static str {
    match verdict(item, &rule()) {
        Verdict::Kept(_) => "kept",
        Verdict::Other => "other",
        Verdict::NoKey => "no_key",
        Verdict::BeforeStart => "before_start",
        Verdict::NotToAccount => "not_to_account",
        Verdict::Incomplete => "incomplete",
        Verdict::ReplyToOther => "reply_to_other",
    }
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
        "replyTo": [],
        "receivedDateTime": "2026-11-18T10:00:00Z",
        "isReadReceiptRequested": false,
    })
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
}

#[test]
fn a_sender_that_holds_a_space_makes_no_rule() {
    let second = NonZeroUsize::MIN.saturating_add(1);
    let rows = [
        ("ann berg@people.test", NonZeroUsize::MIN),
        ("me@home.test, ann berg@people.test", second),
        ("me@home.test, ann@people.test bob@people.test", second),
    ];
    for (slot, piece) in rows {
        assert_eq!(
            reply_senders(slot).err(),
            Some(SendersError::NotAnAddress(piece)),
            "{slot}"
        );
    }
}

#[test]
fn two_addresses_are_one_without_regard_to_ascii_case_alone_in_the_rule_too() {
    let mut as_sender = 0;
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
        match reply_senders(a) {
            Ok(listed) => {
                as_sender += 1;
                let rule = ReplyRule::new("me@home.test".to_owned(), listed, Instant(0));
                assert_eq!(
                    rule.selects(false, b, &["me@home.test".to_owned()]),
                    same,
                    "sender {a} {b}"
                );
            }
            Err(fault) => assert_eq!(fault, SendersError::NotAscii(NonZeroUsize::MIN), "{a}"),
        }
        let as_account = ReplyRule::new(a.to_owned(), senders("ann@people.test"), Instant(0));
        assert_eq!(
            as_account.selects(false, "ann@people.test", &[b.to_owned()]),
            same,
            "account {a} {b}"
        );
    }
    assert_eq!(as_sender, 7);
}

#[test]
fn the_filter_names_the_sender_quoted_and_the_start_cut_to_its_whole_second() {
    let rule = rule();
    assert_eq!(
        rule.chain("o'neil@people.test"),
        "/v1.0/me/mailFolders/inbox/messages?$filter=isRead%20eq%20false%20and%20from%2FemailAddress%2Faddress%20eq%20%27o%27%27neil%40people.test%27%20and%20receivedDateTime%20ge%202026-11-18T10%3A00%3A00Z&$top=25&$select=id%2CisRead%2Cfrom%2CtoRecipients%2CreplyTo%2CreceivedDateTime%2CconversationId%2Csubject%2Cbody%2CisReadReceiptRequested"
    );
    let before_1970 = ReplyRule::new("me@home.test".to_owned(), senders("a@b"), Instant(-1));
    assert!(before_1970
        .chain("a@b")
        .contains("ge%201969-12-31T23%3A59%3A59Z"));
}

#[test]
fn every_byte_that_the_rule_for_a_sender_takes_is_quoted_and_encoded() {
    let rule = rule();
    for byte in 0x21u8..0x7f {
        let sender = format!("a{}b@people.test", char::from(byte));
        if reply_senders(&sender).is_err() {
            continue;
        }
        let chain = rule.chain(&sender);
        let query = chain.split_once('?').expect("a query").1;
        assert_eq!(query.matches('&').count(), 2, "{sender}");
        assert!(
            query
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"$=&%-._~".contains(&b)),
            "{sender}"
        );
    }
}

#[test]
fn the_rule_selects_from_the_start_second_on_and_counts_an_earlier_or_unreadable_time() {
    let at = |time: &str| {
        let mut item = message("m1", false, "ann@people.test", &["me@home.test"]);
        item["receivedDateTime"] = json!(time);
        kind(&item)
    };
    assert_eq!(at("2026-11-18T10:00:00Z"), "kept");
    assert_eq!(at("2026-11-18T10:00:00.9999999Z"), "kept");
    assert_eq!(at("2026-11-18T10:00:01Z"), "kept");
    assert_eq!(at("2026-11-18T09:59:59.9999999Z"), "before_start");
    assert_eq!(at("2025-11-18T10:00:00Z"), "before_start");
    for unreadable in [
        "2026-11-18T10:00:00",
        "2026-11-18T10:00:00+00:00",
        "2026-11-18 10:00:00Z",
        "2026-11-18T10:00:60Z",
        "2026-11-18T10:00:00ZZ",
        "",
    ] {
        assert_eq!(at(unreadable), "no_key", "{unreadable}");
    }
    let mut none = message("m1", false, "ann@people.test", &["me@home.test"]);
    none.as_object_mut()
        .expect("an object")
        .remove("receivedDateTime");
    assert_eq!(kind(&none), "no_key");
    none["receivedDateTime"] = json!(null);
    assert_eq!(kind(&none), "no_key");
}

#[test]
fn a_message_stands_in_the_first_count_that_fits() {
    let plain = || message("m1", false, "ann@people.test", &["me@home.test"]);
    assert_eq!(kind(&plain()), "kept");
    assert_eq!(
        kind(&message("m1", true, "ann@people.test", &["me@home.test"])),
        "other"
    );
    assert_eq!(
        kind(&message("m1", false, "eve@people.test", &["me@home.test"])),
        "other"
    );
    assert_eq!(
        kind(&message("m1", false, "ann@people.test", &["x@home.test"])),
        "not_to_account"
    );
    let mut early_elsewhere = message("m1", false, "ann@people.test", &["x@home.test"]);
    early_elsewhere["receivedDateTime"] = json!("2026-01-01T00:00:00Z");
    assert_eq!(kind(&early_elsewhere), "before_start");
    let mut early_unread_other = message("m1", true, "ann@people.test", &["me@home.test"]);
    early_unread_other["receivedDateTime"] = json!("2026-01-01T00:00:00Z");
    assert_eq!(kind(&early_unread_other), "other");
    for pointer in [
        "/conversationId",
        "/body/content",
        "/isReadReceiptRequested",
    ] {
        let mut item = plain();
        let (parent, key) = pointer.rsplit_once('/').expect("a pointer");
        let target = if parent.is_empty() {
            &mut item
        } else {
            item.pointer_mut(parent).expect("the parent")
        };
        target.as_object_mut().expect("an object").remove(key);
        assert_eq!(kind(&item), "incomplete", "{pointer}");
        item["replyTo"] = json!([{"emailAddress": {"address": "eve@people.test"}}]);
        assert_eq!(kind(&item), "incomplete", "{pointer} before reply_to_other");
    }
    let mut odd_subject = plain();
    odd_subject["subject"] = json!(["s"]);
    assert_eq!(kind(&odd_subject), "incomplete");
    let mut elsewhere_and_other = message("m1", false, "ann@people.test", &["x@home.test"]);
    elsewhere_and_other["replyTo"] = json!([{"emailAddress": {"address": "eve@people.test"}}]);
    assert_eq!(kind(&elsewhere_and_other), "not_to_account");
}

#[test]
fn an_item_without_an_id_or_a_key_that_the_rule_reads_counts_in_no_key() {
    let whole = message("m1", false, "ann@people.test", &["me@home.test"]);
    for pointer in [
        "/id",
        "/isRead",
        "/from",
        "/from/emailAddress",
        "/from/emailAddress/address",
        "/toRecipients",
        "/toRecipients/0/emailAddress/address",
        "/replyTo",
        "/receivedDateTime",
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
        assert_eq!(kind(&item), "no_key", "{pointer}");
    }
    for (pointer, value) in [
        ("/id", json!(1)),
        ("/isRead", json!("false")),
        ("/toRecipients", json!({})),
        ("/replyTo", json!(null)),
        ("/replyTo", json!([{"emailAddress": {}}])),
    ] {
        let mut item = whole.clone();
        *item.pointer_mut(pointer).expect("the key") = value;
        assert_eq!(kind(&item), "no_key", "{pointer}");
    }
    let mut read = message("m1", true, "eve@people.test", &["me@home.test"]);
    read.as_object_mut().expect("an object").remove("replyTo");
    assert_eq!(kind(&read), "no_key", "the keys count before the rule");
}

#[test]
fn a_subject_that_is_null_or_missing_reads_as_empty() {
    let mut item = message("m1", false, "ann@people.test", &["me@home.test"]);
    item["subject"] = json!(null);
    let Verdict::Kept(kept) = verdict(&item, &rule()) else {
        panic!("kept");
    };
    assert_eq!(kept.subject(), "");
    item.as_object_mut().expect("an object").remove("subject");
    assert_eq!(kind(&item), "kept");
}

#[test]
fn a_reply_to_is_held_against_the_sender_byte_for_byte() {
    let rows = [
        (json!([]), "kept"),
        (
            json!([{"emailAddress": {"address": "Ann@people.test"}}]),
            "kept",
        ),
        (
            json!([{"emailAddress": {"address": "ann@people.test"}}]),
            "reply_to_other",
        ),
        (
            json!([{"emailAddress": {"address": "Ann@People.test"}}]),
            "reply_to_other",
        ),
        (
            json!([{"emailAddress": {"address": "Ann@people.test "}}]),
            "reply_to_other",
        ),
        (
            json!([{"emailAddress": {"address": "eve@people.test"}}]),
            "reply_to_other",
        ),
        (
            json!([
                {"emailAddress": {"address": "Ann@people.test"}},
                {"emailAddress": {"address": "eve@people.test"}}
            ]),
            "reply_to_other",
        ),
        (
            json!([{"emailAddress": {"address": "Ann@people.test", "name": "Eve"}}]),
            "kept",
        ),
    ];
    for (reply_to, want) in rows {
        let mut item = message("m1", false, "Ann@people.test", &["me@home.test"]);
        item["replyTo"] = reply_to.clone();
        assert_eq!(kind(&item), want, "{reply_to}");
    }
}

#[test]
fn a_kept_message_holds_what_the_provider_gave() {
    let mut item = message(
        "m1",
        false,
        "Bob@people.test",
        &["x@home.test", "ME@HOME.test"],
    );
    item["isReadReceiptRequested"] = json!(true);
    let Verdict::Kept(kept) = verdict(&item, &rule()) else {
        panic!("kept");
    };
    assert_eq!(
        (
            kept.id(),
            kept.conversation(),
            kept.subject(),
            kept.sender(),
            kept.body(),
            kept.asks_receipt()
        ),
        ("m1", "c-m1", "s-m1", "Bob@people.test", "b-m1", true)
    );
}

#[test]
fn debug_prints_no_value() {
    let message = Message {
        id: "m1".to_owned(),
        conversation: "c1".to_owned(),
        subject: "s1".to_owned(),
        sender: "ann@people.test".to_owned(),
        body: "b1".to_owned(),
        receipt: false,
        earlier: None,
    };
    let draft = Draft {
        id: "d1".to_owned(),
        message: "m1".to_owned(),
        to: vec!["ann@people.test".to_owned()],
        subject: "s1".to_owned(),
        receipt: false,
    };
    assert_eq!(format!("{:?}", rule()), "ReplyRule { senders: 2, .. }");
    assert_eq!(format!("{message:?}"), "Message { .. }");
    assert_eq!(
        format!(
            "{:?}",
            Messages {
                kept: vec![message],
                listed: vec!["m1".to_owned()],
                counts: MailCounts {
                    before_start: 0,
                    cut: YesNo::No,
                    no_key: 0,
                    not_to_account: 0,
                    incomplete: 0,
                    reply_to_other: 0,
                    more_pages: YesNo::No,
                    earlier_page: 0,
                },
                left: Vec::new(),
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
