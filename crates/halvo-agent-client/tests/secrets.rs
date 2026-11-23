mod common;

use std::fs;
use std::time::Duration;

use common::{id16, Rig};
use halvo_agent_client::wire::FRAME_MAX;
use halvo_agent_client::{Hosted, RunSpec};
use halvo_schema::executable::frame::SecretReply;
use halvo_schema::executable::names::Unreadable;
use halvo_schema::executable::text::{Name, SecretText};
use halvo_schema::AgentId;

const SHORT: Duration = Duration::from_secs(10);

const ASK: &str = r#"ask {"t":"secret","ref":"fixture_token"}"#;

fn name(text: &str) -> Name {
    Name::parse(text).expect("the fixture name parses")
}

fn reply_for(answer: SecretReply) -> String {
    let rig = Rig::new("secret");
    *rig.vault.other.lock().unwrap() = answer;
    let script = rig.script(&["hello", ASK, "log asked", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    let agent = spec.agent.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("asked", SHORT), "{}", script.log());
    rig.sup.kill(&run).unwrap();
    assert_eq!(
        *rig.vault.calls.lock().unwrap(),
        vec![(agent, name("fixture_token"))]
    );
    assert_eq!(rig.vault.reads(), 1);
    let replies = script.replies();
    assert_eq!(replies.len(), 1, "{}", script.log());
    replies[0].clone()
}

fn stored(value: &str) -> SecretReply {
    SecretReply::Stored(SecretText::parse(value).expect("the fixture secret parses"))
}

#[test]
fn a_stored_secret_reaches_the_runner_as_stored_with_its_value() {
    assert_eq!(
        reply_for(stored("tok_fixture")),
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
fn an_empty_secret_cannot_reach_the_runner_because_its_parse_is_refused_with_empty_secret() {
    let refusal = SecretText::parse("").expect_err("an empty secret parses");
    assert_eq!(
        (refusal.type_name(), refusal.rule()),
        ("SecretText", "empty_secret")
    );
}

#[test]
fn a_secret_longer_than_1024_bytes_passes_and_one_that_no_frame_can_carry_is_unreadable_with_fault()
{
    let long = "t".repeat(4096);
    assert_eq!(
        reply_for(stored(&long)),
        format!(r#"{{"t":"secret_reply","answer":"stored","value":"{long}"}}"#)
    );
    let over = "t".repeat(FRAME_MAX);
    assert_eq!(
        reply_for(stored(&over)),
        r#"{"t":"secret_reply","answer":"unreadable","member":"fault"}"#
    );
}

#[test]
fn two_live_runs_that_ask_two_names_get_on_every_read_in_order_what_their_own_agent_holds() {
    let rig = Rig::new("secret-agents");
    let go = rig.dir.join("go");
    let wait = format!("wait-for {}", go.display());
    let other = r#"ask {"t":"secret","ref":"other_token"}"#;
    let lines = ["hello", "log live", &wait, ASK, other, "log asked", "idle"];
    let (first, second) = (rig.script(&lines), rig.script(&lines));
    let (a, b) = (rig.spec(&first), rig.spec(&second));
    let (a_run, b_run) = (a.run.clone(), b.run.clone());
    let (a_agent, b_agent) = (a.agent.clone(), b.agent.clone());
    rig.vault.held.lock().unwrap().extend([
        (
            a_agent.clone(),
            name("fixture_token"),
            stored("tok_a_fixture"),
        ),
        (a_agent.clone(), name("other_token"), stored("tok_a_other")),
        (b_agent.clone(), name("other_token"), stored("tok_b_other")),
    ]);
    rig.sup.start(a).unwrap();
    rig.sup.start(b).unwrap();
    assert!(first.wait_log("live", SHORT), "{}", first.log());
    assert!(second.wait_log("live", SHORT), "{}", second.log());
    fs::write(&go, b"go").unwrap();
    assert!(first.wait_log("asked", SHORT), "{}", first.log());
    assert!(second.wait_log("asked", SHORT), "{}", second.log());
    assert!(rig.mark(&a_run).is_some() && rig.mark(&b_run).is_some());
    rig.sup.kill(&a_run).unwrap();
    rig.sup.kill(&b_run).unwrap();
    let value = |v: &str| format!(r#"{{"t":"secret_reply","answer":"stored","value":"{v}"}}"#);
    let none = r#"{"t":"secret_reply","answer":"none"}"#.to_owned();
    assert_eq!(
        first.replies(),
        vec![value("tok_a_fixture"), value("tok_a_other")]
    );
    assert_eq!(second.replies(), vec![none, value("tok_b_other")]);
    let calls = rig.vault.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 4, "{calls:?}");
    for agent in [a_agent, b_agent] {
        let asked: Vec<&Name> = calls
            .iter()
            .filter(|(a, _)| a == &agent)
            .map(|(_, n)| n)
            .collect();
        assert_eq!(
            asked,
            [&name("fixture_token"), &name("other_token")],
            "{calls:?}"
        );
    }
}

#[test]
fn a_secret_frame_with_an_agent_key_is_refused_and_its_run_ends_with_no_read_made() {
    let rig = Rig::new("secret-frame-agent");
    let other = AgentId::from_random(id16());
    rig.vault.held.lock().unwrap().push((
        other.clone(),
        name("fixture_token"),
        stored("tok_other_agent"),
    ));
    let ask = format!(
        r#"ask {{"t":"secret","ref":"fixture_token","agent":"{}"}}"#,
        other.as_str()
    );
    let script = rig.script(&["hello", &ask, "log asked", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(rig.rec.wait_end(&run, SHORT).is_some(), "{}", script.log());
    let calls = rig.vault.calls.lock().unwrap().clone();
    assert!(calls.is_empty(), "{calls:?}");
    assert!(script.replies().is_empty(), "{}", script.log());
    assert!(
        !script.log().contains("tok_other_agent"),
        "{}",
        script.log()
    );
}

const MODEL_KEY_ASK: &str = r#"ask {"t":"secret","ref":"model_key"}"#;

/// The person's key that the secret store holds in the model key's test: a made-up text that names itself as a test
/// value.
const TEST_MODEL_KEY: &str = "test-value-not-a-model-key";

/// In a run whose start names `hosted`, with a secret store that holds the test's key for every agent and name: the
/// reply that the runner gets to the frame `secret` of the reserved name `model_key`, the reads that the store got,
/// and the run's agent.
fn model_key_reply(hosted: Option<Hosted>) -> (String, Vec<(AgentId, Name)>, AgentId) {
    let rig = Rig::new("model-key");
    *rig.vault.other.lock().unwrap() = stored(TEST_MODEL_KEY);
    let script = rig.script(&["hello", MODEL_KEY_ASK, "log asked", "idle"]);
    let spec = RunSpec {
        hosted,
        ..rig.spec(&script)
    };
    let run = spec.run.clone();
    let agent = spec.agent.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("asked", SHORT), "{}", script.log());
    rig.sup.kill(&run).unwrap();
    let replies = script.replies();
    assert_eq!(replies.len(), 1, "{}", script.log());
    let calls = rig.vault.calls.lock().unwrap().clone();
    (replies[0].clone(), calls, agent)
}

/// r3 U7.6, sentence (4) (the coordinator's log lines 16422, point 4, and 16823, its point (2)): a run whose start
/// names no hosted tier gets no model key: the library answers the frame `secret` of the reserved name `model_key`
/// with none stored and reads nothing from the secret store, which holds a key. A run whose start names a hosted tier
/// gets the store's answer, read once for the run's agent and that name.
#[test]
fn a_run_whose_start_names_no_hosted_row_gets_no_model_key() {
    let (refused, reads, _) = model_key_reply(None);
    assert_eq!(
        (refused.as_str(), reads),
        (r#"{"t":"secret_reply","answer":"none"}"#, Vec::new()),
        "a run whose start names no hosted tier"
    );
    let (given, reads, agent) = model_key_reply(Some(Hosted::LowCostHosted));
    assert_eq!(
        (given, reads),
        (
            format!(r#"{{"t":"secret_reply","answer":"stored","value":"{TEST_MODEL_KEY}"}}"#),
            vec![(agent, name("model_key"))]
        ),
        "a run whose start names a hosted tier"
    );
}
