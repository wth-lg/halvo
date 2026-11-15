mod common;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{ask_store, effect, id, wait_until, Rig, Script};
use halvo_agent_client::{CallError, EndRun};
use halvo_schema::executable::names::{EffectState, Outcome};
use halvo_schema::executable::text::EffectId;

const SHORT: Duration = Duration::from_secs(10);

const RUN_RECORD: &str = r#"{"v":1,"workflow":"{workflow}","agent":"{agent}","run":"{run}","day":20768,"mode":"replay","outcome":"completed","playbook":1,"steps":9,"prompts":2,"handbacks":1,"confirmations":1,"effects":1,"unknown":0,"model_calls":0,"tokens_in":0,"tokens_out":0,"wall_s":41,"model":null,"before_start":null,"cut":null,"no_key":null,"not_to_account":null,"incomplete":null,"reply_to_other":null,"more_pages":null,"held_recipient":null,"organized":null,"other_task":null,"held_earlier":null,"earlier_page":null}"#;

fn send_store(id: &str, body: &str) -> String {
    ask_store(id, body).replacen("ask ", "send ", 1)
}

fn load() -> String {
    r#"{"op":"load","agent":"{agent}","run":"{run}"}"#.to_owned()
}

fn finish() -> String {
    r#"{"op":"finish","agent":"{agent}","run":"{run}","record":RECORD}"#
        .replace("RECORD", RUN_RECORD)
}

fn sent_by_runner(script: &Script, rig: &Rig) -> Vec<String> {
    let _ = script;
    rig.store
        .bodies()
        .into_iter()
        .filter(|b| b.contains(r#""request":"runner""#))
        .collect()
}

#[test]
fn a_store_request_is_relayed_as_the_runner_wrote_it_and_its_reply_reaches_the_runner() {
    let rig = Rig::new("relay");
    let script = rig.script(&[
        "hello",
        &ask_store(&id(1), &load()),
        &ask_store(&id(2), &effect(&id(0x60), 1, "held")),
        "log asked",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec.clone()).unwrap();
    assert!(script.wait_log("asked", SHORT), "{}", script.log());
    let sent = sent_by_runner(&script, &rig);
    assert_eq!(sent.len(), 2);
    let fill = |s: &str| {
        s.replace("{run}", spec.run.as_str())
            .replace("{agent}", spec.agent.as_str())
            .replace("{workflow}", spec.workflow.as_str())
    };
    let first = r#"{"v":1,"id":"ID","request":"runner","body":BODY}"#
        .replace("ID", &id(1))
        .replace("BODY", &fill(&load()));
    assert_eq!(sent[0], first);
    let replies = script.replies();
    assert_eq!(replies.len(), 2, "{}", script.log());
    assert!(replies[0].starts_with(&format!(
        r#"{{"t":"store_reply","reply":{{"v":1,"id":"{}","ok":{{"config":"#,
        id(1)
    )));
    assert_eq!(
        replies[1],
        format!(
            r#"{{"t":"store_reply","reply":{{"v":1,"id":"{}","ok":{{"stored":1}}}}}}"#,
            id(2)
        )
    );
    let held = rig.store.states(&run, &EffectId::parse(&id(0x60)).unwrap());
    assert_eq!(held, vec![EffectState::Held]);
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_store_request_that_names_another_run_or_agent_is_answered_binding_and_passes_nothing_on() {
    let rig = Rig::new("relay-binding");
    let other_run = load().replace("{run}", &"f".repeat(32));
    let other_agent = load().replace("{agent}", &"e".repeat(32));
    let script = rig.script(&[
        "hello",
        &ask_store(&id(3), &other_run),
        &ask_store(&id(4), &other_agent),
        "log asked",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("asked", SHORT), "{}", script.log());
    assert!(sent_by_runner(&script, &rig).is_empty());
    for (n, reply) in [3u8, 4].iter().zip(script.replies()) {
        assert_eq!(
            reply,
            format!(
                r#"{{"t":"store_reply","reply":{{"v":1,"id":"{}","error":"binding"}}}}"#,
                id(*n)
            )
        );
    }
    rig.sup.kill(&run).unwrap();
}

#[test]
fn the_owner_gets_a_store_request_only_while_the_mark_is_complete() {
    let rig = Rig::new("relay-mark");
    let seen: Arc<Mutex<Vec<bool>>> = Arc::default();
    let data = rig.data();
    let seen_in = Arc::clone(&seen);
    *rig.store.check.lock().unwrap() = Some(Box::new(move |run| {
        let complete = common::mark_of(&data, run).is_some_and(|m| m.is_complete());
        seen_in.lock().unwrap().push(complete);
    }));
    let script = rig.script(&["hello", &ask_store(&id(5), &load()), "log asked", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("asked", SHORT), "{}", script.log());
    assert_eq!(seen.lock().unwrap().as_slice(), &[true]);
    rig.sup.kill(&run).unwrap();

    let early = rig.script(&[&send_store(&id(6), &load()), "hello", "idle"]);
    let spec = rig.spec(&early);
    assert_eq!(rig.sup.start(spec), Err(CallError::NotStarted));
    assert_eq!(seen.lock().unwrap().as_slice(), &[true]);
    assert_eq!(rig.store.count(&id(6)), 0);
}

#[test]
fn a_link_failure_is_answered_internal_and_the_request_is_not_sent_again() {
    let rig = Rig::new("relay-link");
    rig.store.fail_all.store(1, Ordering::SeqCst);
    let script = rig.script(&[
        "hello",
        &ask_store(&id(7), &effect(&id(0x61), 1, "held")),
        "log asked",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("asked", SHORT), "{}", script.log());
    assert_eq!(rig.store.count(&id(7)), 1);
    assert_eq!(
        script.replies(),
        vec![format!(
            r#"{{"t":"store_reply","reply":{{"v":1,"id":"{}","error":"internal"}}}}"#,
            id(7)
        )]
    );
    rig.sup.kill(&run).unwrap();
}

#[test]
fn an_acknowledged_finish_removes_the_mark_owes_no_end_run_and_stops_the_relay() {
    let rig = Rig::new("relay-finish");
    let script = rig.script(&[
        "hello",
        &ask_store(&id(8), &finish()),
        "log finished",
        &ask_store(&id(9), &load()),
        r#"send {"t":"done","outcome":"completed"}"#,
        "exit 0",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("finished", SHORT), "{}", script.log());
    assert!(rig.mark(&run).is_none());
    let end = rig.rec.wait_end(&run, SHORT).expect("the end is reported");
    assert_eq!(end.end_run, EndRun::NotOwed);
    assert_eq!(end.outcome, Some(Outcome::Completed));
    assert_eq!(end.finish.map(|f| f.steps), Some(9));
    assert_eq!(rig.store.count(r#""op":"end_run""#), 0);
    assert_eq!(rig.store.count(&id(9)), 0);
    let replies = script.replies();
    assert_eq!(replies.len(), 2, "{}", script.log());
    assert_eq!(
        replies[1],
        format!(
            r#"{{"t":"store_reply","reply":{{"v":1,"id":"{}","error":"state"}}}}"#,
            id(9)
        )
    );
}

#[test]
fn a_killed_runner_leaves_a_run_whose_effects_the_store_holds_as_unknown_or_rejected_after_end_run()
{
    for by_library in [false, true] {
        let rig = Rig::new("relay-killed");
        let script = rig.script(&[
            "hello",
            "on-eof hang",
            &ask_store(&id(10), &effect(&id(0x62), 1, "held")),
            &ask_store(&id(11), &effect(&id(0x63), 1, "cleared")),
            &ask_store(&id(12), &effect(&id(0x64), 1, "held")),
            &ask_store(&id(13), &effect(&id(0x64), 2, "approved")),
            &ask_store(&id(14), &effect(&id(0x65), 1, "held")),
            &ask_store(&id(15), &effect(&id(0x65), 2, "approved")),
            &ask_store(&id(16), &effect(&id(0x65), 3, "releasing")),
            "log holding",
            "idle",
        ]);
        let spec = rig.spec(&script);
        let run = spec.run.clone();
        rig.sup.start(spec).unwrap();
        assert!(script.wait_log("holding", SHORT), "{}", script.log());
        let runner = rig.mark(&run).unwrap().runner.unwrap();
        if by_library {
            rig.sup.kill(&run).unwrap();
        } else {
            common::kill_pid(runner.pid.get());
        }
        let end = rig
            .rec
            .wait_end(&run, Duration::from_secs(30))
            .expect("the end is reported");
        assert_eq!(end.end_run, EndRun::Acknowledged);
        assert_eq!(end.finish, None);
        assert_eq!(rig.store.count(r#""op":"end_run""#), 1);
        let last = |n: u8| {
            rig.store
                .states(&run, &EffectId::parse(&id(n)).unwrap())
                .last()
                .copied()
        };
        assert_eq!(last(0x62), Some(EffectState::Rejected));
        assert_eq!(last(0x63), Some(EffectState::Rejected));
        assert_eq!(last(0x64), Some(EffectState::Rejected));
        assert_eq!(last(0x65), Some(EffectState::Unknown));
        assert!(rig.mark(&run).is_none());
        assert!(wait_until(SHORT, || common::line_gone(&runner)));
    }
}

#[test]
fn an_owner_reply_for_another_request_id_or_of_another_call_form_is_answered_internal() {
    let rig = Rig::new("relay-wrong-reply");
    rig.store.wrong_id_runner.store(1, Ordering::SeqCst);
    rig.store.wrong_form_runner.store(1, Ordering::SeqCst);
    let script = rig.script(&[
        "hello",
        &ask_store(&id(0x30), &effect(&id(0x68), 1, "held")),
        &ask_store(&id(0x31), &load()),
        &ask_store(&id(0x32), &load()),
        "log asked",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("asked", SHORT), "{}", script.log());
    let replies = script.replies();
    assert_eq!(replies.len(), 3, "{}", script.log());
    for (n, reply) in [0x30u8, 0x31].iter().zip(&replies) {
        assert_eq!(
            reply,
            &format!(
                r#"{{"t":"store_reply","reply":{{"v":1,"id":"{}","error":"internal"}}}}"#,
                id(*n)
            )
        );
    }
    assert!(replies[2].contains(r#""ok":{"config":"#), "{}", replies[2]);
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_finish_that_the_owner_did_not_store_still_owes_end_run_and_keeps_the_relay_open() {
    let rig = Rig::new("relay-finish-refused");
    rig.store.fail_all.store(1, Ordering::SeqCst);
    let script = rig.script(&[
        "hello",
        &ask_store(&id(0x33), &finish()),
        &ask_store(&id(0x34), &load()),
        "log finished",
        r#"send {"t":"done","outcome":"completed"}"#,
        "exit 0",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("finished", SHORT), "{}", script.log());
    let replies = script.replies();
    assert_eq!(
        replies[0],
        format!(
            r#"{{"t":"store_reply","reply":{{"v":1,"id":"{}","error":"internal"}}}}"#,
            id(0x33)
        )
    );
    assert!(replies[1].contains(r#""ok":{"config":"#), "{}", replies[1]);
    let end = rig.rec.wait_end(&run, SHORT).expect("the end is reported");
    assert_eq!(end.finish, None);
    assert_eq!(end.end_run, EndRun::Acknowledged);
    assert_eq!(rig.store.count(r#""op":"end_run""#), 1);
    assert!(rig.mark(&run).is_none());
}

#[test]
fn no_row_written_on_the_runner_output_after_the_library_closed_its_input_reaches_the_owner() {
    let rig = Rig::new("relay-after-close");
    let row = format!(
        r#"{{"t":"store","request":{{"v":1,"id":"{}","request":"runner","body":{}}}}}"#,
        id(0x40),
        effect(&id(0x69), 1, "held")
    );
    let script = rig.script(&[
        "hello",
        &format!("writer-child 3000 5 {row}"),
        "log spawned",
        r#"ask {"t":"secret","ref":"fixture_token"}"#,
        "exit 1",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("spawned", SHORT), "{}", script.log());
    assert!(rig.rec.wait_end(&run, SHORT).is_some());
    assert!(
        !script.log().contains("writer wrote"),
        "the row was written before the library closed the input: {}",
        script.log()
    );
    if cfg!(windows) {
        let writer = script.logged("child");
        assert!(wait_until(SHORT, || writer
            .iter()
            .all(|(p, c)| common::gone(*p, *c))));
    } else {
        assert!(script.wait_log("writer wrote=", SHORT), "{}", script.log());
        std::thread::sleep(Duration::from_millis(500));
    }
    assert_eq!(
        rig.store.count(&id(0x69)),
        0,
        "a row written on the runner's output after the close reached the owner"
    );
}
