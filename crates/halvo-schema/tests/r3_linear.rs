use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use halvo_schema::executable::{AgentBindings, AgentConfig, Refusal, RefusalCause, RunnerCall};

const LIMIT: Duration = Duration::from_secs(60);
const A: &str = "40000000000000000000000000000001";
const W: &str = "30000000000000000000000000000001";

fn within<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let _ = send.send(work());
    });
    receive
        .recv_timeout(LIMIT)
        .expect("the parser did not answer within 60 s")
}

fn keys(count: usize) -> String {
    let mut text = String::with_capacity(count * 12);
    for i in 0..count {
        text.push_str(&format!(",\"k{i}\":0"));
    }
    text
}

fn origins(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| format!("\"http://h{i}.test:80\""))
        .collect()
}

fn refusal(
    result: Result<impl std::fmt::Debug, Refusal>,
) -> (RefusalCause, &'static str, &'static str) {
    let refusal = result.expect_err("refused");
    (refusal.cause(), refusal.type_name(), refusal.rule())
}

#[test]
fn an_object_of_200000_distinct_unknown_keys_is_refused_in_time_that_grows_with_its_size() {
    let config = format!("{{\"v\":1{}}}", keys(200_000));
    let body = format!("{{\"op\":\"load\"{}}}", keys(200_000));
    let twice = format!("{{\"v\":1{},\"k7\":1}}", keys(200_000));
    let (config, body, twice) = within(move || {
        (
            refusal(AgentConfig::parse(config.as_bytes())),
            refusal(RunnerCall::parse(body.as_bytes())),
            refusal(AgentConfig::parse(twice.as_bytes())),
        )
    });
    assert_eq!(
        config,
        (RefusalCause::Malformed, "AgentConfig", "unknown_key")
    );
    assert_eq!(body, (RefusalCause::Malformed, "Load", "unknown_key"));
    assert_eq!(
        twice,
        (RefusalCause::Malformed, "AgentConfig", "repeated_key")
    );
}

#[test]
fn an_agent_bindings_record_of_50000_distinct_origins_is_taken_in_time_that_grows_with_its_size() {
    let list = origins(50_000);
    let record = format!(
        r#"{{"v":1,"workflow":"{W}","agent":"{A}","rev":1,"origins":[{}],"slots":[]}}"#,
        list.join(",")
    );
    let twice = format!(
        r#"{{"v":1,"workflow":"{W}","agent":"{A}","rev":1,"origins":[{},{}],"slots":[]}}"#,
        list.join(","),
        list[49_999]
    );
    let (taken, refused) = within(move || {
        let taken = AgentBindings::parse(record.as_bytes())
            .map(|b| b.origins.len())
            .unwrap_or_else(|e| panic!("not taken: {e}"));
        (taken, refusal(AgentBindings::parse(twice.as_bytes())))
    });
    assert_eq!(taken, 50_000);
    assert_eq!(
        refused,
        (RefusalCause::Malformed, "AgentBindings", "repeated_name")
    );
}
