mod common;

use common::fixture::Fixture;
use common::seed::Seed;
use halvo_connect::Mail;
use serde_json::{json, Value};

const BODY: &str = "Thank you, noted.";

fn requests(fixture: &Fixture) -> Value {
    fixture.state()["requests"].clone()
}

fn counts(total: u64, state_changing: u64) -> Value {
    json!({"total": total, "state_changing": state_changing, "evil": 0, "unauthorized": 0})
}

fn reply_rows(seed: &Seed) -> Vec<(String, String, String)> {
    seed.end("reply")
        .as_array()
        .expect("end.reply")
        .iter()
        .map(|row| {
            (
                row["to"].as_str().expect("to").to_owned(),
                row["conversation"]
                    .as_str()
                    .expect("conversation")
                    .to_owned(),
                row["in_reply_to"].as_str().expect("in_reply_to").to_owned(),
            )
        })
        .collect()
}

#[test]
fn mail_list() {
    let fixture = Fixture::start();
    let graph = fixture.graph("mail");
    for n in 1..=3 {
        fixture.reset(n, &[]);
        let seed = Seed::load(n);
        let messages = graph
            .mail_list(&seed.token(), &seed.rule())
            .expect("mail_list answers");
        let got: Vec<_> = messages
            .iter()
            .map(|m| {
                (
                    m.sender().to_owned(),
                    m.conversation().to_owned(),
                    m.id().to_owned(),
                )
            })
            .collect();
        assert!(
            got == reply_rows(&seed),
            "seed {n}: the messages that the reply rule selects"
        );
        assert!(
            messages
                .iter()
                .all(|m| seed.subject(m.id()) == Some(m.subject())),
            "seed {n}: each subject"
        );
        assert!(
            messages
                .iter()
                .next()
                .is_some_and(|m| m.body().contains(seed.page_marker())),
            "seed {n}: the body of the first message"
        );
        assert!(
            requests(&fixture) == counts(1, 0),
            "seed {n}: one GET and no state change"
        );
    }
}

#[test]
fn mail_draft_reply() {
    let fixture = Fixture::start();
    let graph = fixture.graph("mail");
    for n in 1..=3 {
        fixture.reset(n, &[]);
        let seed = Seed::load(n);
        let token = seed.token();
        let messages = graph
            .mail_list(&token, &seed.rule())
            .expect("mail_list answers");
        let want: Vec<_> = messages
            .iter()
            .map(|m| {
                (
                    m.id().to_owned(),
                    vec![m.sender().to_owned()],
                    format!("RE: {}", m.subject()),
                )
            })
            .collect();
        let drafts = graph
            .mail_draft_reply(&token, messages, BODY)
            .expect("mail_draft_reply answers");
        let got: Vec<_> = drafts
            .iter()
            .map(|d| {
                (
                    d.message().to_owned(),
                    d.to().to_vec(),
                    d.subject().to_owned(),
                )
            })
            .collect();
        assert!(
            got == want,
            "seed {n}: one draft per message, to its sender, with its subject"
        );
        assert!(
            drafts.iter().all(|d| d.id().starts_with("draft-")),
            "seed {n}: the ids of the drafts"
        );
        let state = fixture.state();
        assert!(
            state["mail"]["drafts"] == 2,
            "seed {n}: two drafts at the provider"
        );
        assert!(state["mail"]["sent"] == json!([]), "seed {n}: nothing sent");
        assert!(
            state["requests"] == counts(5, 4),
            "seed {n}: createReply and PATCH once per message"
        );
    }
}

#[test]
fn mail_send() {
    let fixture = Fixture::start();
    let graph = fixture.graph("mail");
    for n in 1..=3 {
        fixture.reset(n, &[]);
        let seed = Seed::load(n);
        let token = seed.token();
        let messages = graph
            .mail_list(&token, &seed.rule())
            .expect("mail_list answers");
        let drafts = graph
            .mail_draft_reply(&token, messages, BODY)
            .expect("mail_draft_reply answers");
        for draft in drafts.into_sends() {
            graph.mail_send(&token, draft).expect("mail_send answers");
        }
        let state = fixture.state();
        assert!(
            state["mail"]["sent"] == *seed.end("reply"),
            "seed {n}: mail.sent equals end.reply"
        );
        assert!(state["mail"]["drafts"] == 0, "seed {n}: no draft left");
        assert!(
            state["requests"] == counts(7, 6),
            "seed {n}: one send per draft"
        );
        let again = graph
            .mail_list(&token, &seed.rule())
            .expect("mail_list answers");
        assert!(again.is_empty(), "seed {n}: the answered messages are read");
        fixture.advance();
        let later = graph
            .mail_list(&token, &seed.rule())
            .expect("mail_list answers");
        let ids: Vec<_> = later.iter().map(|m| m.id()).collect();
        assert!(
            ids == [seed.later("message/id")],
            "seed {n}: the later message alone"
        );
    }
}
