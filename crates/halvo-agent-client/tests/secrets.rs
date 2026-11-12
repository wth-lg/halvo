mod common;

use std::time::Duration;

use common::Rig;
use halvo_agent_client::wire::FRAME_MAX;
use halvo_schema::executable::frame::SecretReply;
use halvo_schema::executable::names::Unreadable;
use halvo_schema::executable::text::SecretText;

const SHORT: Duration = Duration::from_secs(10);

fn reply_for(answer: SecretReply) -> String {
    let rig = Rig::new("secret");
    *rig.vault.0.lock().unwrap() = answer;
    let script = rig.script(&[
        "hello",
        r#"ask {"t":"secret","ref":"fixture_token"}"#,
        "log asked",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("asked", SHORT), "{}", script.log());
    rig.sup.kill(&run).unwrap();
    let replies = script.replies();
    assert_eq!(replies.len(), 1, "{}", script.log());
    replies[0].clone()
}

#[test]
fn a_stored_secret_reaches_the_runner_as_stored_with_its_value() {
    assert_eq!(
        reply_for(SecretReply::Stored(SecretText::new("tok_fixture"))),
        r#"{"t":"secret_reply","answer":"stored","value":"tok_fixture"}"#
    );
}

#[test]
fn none_stored_reaches_the_runner_as_none() {
    assert_eq!(
        reply_for(SecretReply::None),
        r#"{"t":"secret_reply","answer":"none"}"#
    );
}

#[test]
fn a_secrets_that_answers_unreadable_reaches_the_runner_as_unreadable_and_never_as_none() {
    assert_eq!(
        reply_for(SecretReply::Unreadable(Unreadable::Locked)),
        r#"{"t":"secret_reply","answer":"unreadable","member":"locked"}"#
    );
    assert_eq!(
        reply_for(SecretReply::Unreadable(Unreadable::Fault)),
        r#"{"t":"secret_reply","answer":"unreadable","member":"fault"}"#
    );
}

#[test]
fn an_empty_secret_reaches_the_runner_as_unreadable_with_fault() {
    assert_eq!(
        reply_for(SecretReply::Stored(SecretText::new(""))),
        r#"{"t":"secret_reply","answer":"unreadable","member":"fault"}"#
    );
}

#[test]
fn a_secret_longer_than_1024_bytes_passes_and_one_that_no_frame_can_carry_is_unreadable_with_fault()
{
    let long = "t".repeat(4096);
    assert_eq!(
        reply_for(SecretReply::Stored(SecretText::new(&long))),
        format!(r#"{{"t":"secret_reply","answer":"stored","value":"{long}"}}"#)
    );
    let over = "t".repeat(FRAME_MAX);
    assert_eq!(
        reply_for(SecretReply::Stored(SecretText::new(&over))),
        r#"{"t":"secret_reply","answer":"unreadable","member":"fault"}"#
    );
}
