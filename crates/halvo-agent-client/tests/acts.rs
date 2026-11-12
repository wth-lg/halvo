mod common;

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{wait_until, Rig};
use halvo_agent_client::{CallError, OriginAnswer};
use halvo_schema::executable::frame::HandbackResult;
use halvo_schema::executable::names::{Answer, HandbackOutcome, HandbackReason};
use halvo_schema::executable::text::{Name, V1};

const SHORT: Duration = Duration::from_secs(10);

const CONFIRM: &str = r#"ask {"t":"confirm","request":{"v":1,"run":"{run}","agent":"{agent}","effects":[{"effect":"60606060606060606060606060606060","class":"payment","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","bound":{"class":"payment","fields":[{"name":"payee","from":"slot","origin":null,"value":["Northwind Power"]},{"name":"amount","from":"extract","origin":"http://utility.test:18090","value":["84.20"]},{"name":"instrument","from":"slot","origin":null,"value":["card 4242"]},{"name":"origin","from":"const","origin":null,"value":["http://pay.test:18090"]}]},"new_origin":NEW}]}}"#;

fn confirm(new_origin: Option<&str>) -> String {
    CONFIRM.replace(
        "NEW",
        &new_origin.map_or_else(|| "null".to_owned(), |o| format!("\"{o}\"")),
    )
}

fn handback(reason: &str, origin: &str) -> String {
    r#"ask {"t":"handback","request":{"v":1,"run":"{run}","agent":"{agent}","reason":"REASON","origin":ORIGIN,"endpoint":null}}"#
        .replace("REASON", reason)
        .replace("ORIGIN", &format!("\"{origin}\""))
}

fn bind(origin: &str) -> String {
    format!(r#"ask {{"t":"bind","origin":"{origin}"}}"#)
}

fn adds(rig: &Rig) -> Vec<String> {
    rig.store
        .bodies()
        .into_iter()
        .filter(|b| b.contains(r#""op":"add_origin""#))
        .collect()
}

fn result(outcome: HandbackOutcome, secret: Option<&str>) -> HandbackResult {
    HandbackResult {
        v: V1,
        outcome,
        secret: secret.map(|s| Name::parse(s).unwrap()),
    }
}

#[test]
fn a_bind_of_a_new_origin_asks_the_person_sends_add_origin_until_acknowledged_and_only_then_answers_yes(
) {
    let rig = Rig::new("bind-allow");
    rig.store.fail_add_origin.store(2, Ordering::SeqCst);
    let script = rig.script(&["hello", &bind("http://pay.test:18090"), "log bound", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec.clone()).unwrap();
    assert!(script.wait_log("bound", SHORT), "{}", script.log());
    assert_eq!(
        rig.people.asked_origins.lock().unwrap().as_slice(),
        &[common::origin("http://pay.test:18090")]
    );
    let sent = adds(&rig);
    assert_eq!(sent.len(), 3, "{sent:?}");
    assert!(sent.iter().all(|s| s == &sent[0]));
    assert!(sent[0].contains(&format!(
        r#""op":"add_origin","workflow":"{}","agent":"{}","run":"{}","origin":"http://pay.test:18090","way":"sign_in""#,
        spec.workflow, spec.agent, spec.run
    )));
    assert_eq!(
        script.replies(),
        vec![r#"{"t":"bind_reply","bound":"yes"}"#]
    );
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_bind_that_the_person_declines_is_answered_no_and_sends_no_add_origin() {
    let rig = Rig::new("bind-decline");
    *rig.people.allow.lock().unwrap() = OriginAnswer::Decline;
    let script = rig.script(&["hello", &bind("http://pay.test:18090"), "log bound", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("bound", SHORT), "{}", script.log());
    assert_eq!(script.replies(), vec![r#"{"t":"bind_reply","bound":"no"}"#]);
    assert!(adds(&rig).is_empty());
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_bind_of_an_origin_that_the_bindings_hold_is_answered_yes_at_once_with_no_question() {
    let rig = Rig::new("bind-held");
    rig.store
        .load_origins
        .lock()
        .unwrap()
        .push("http://pay.test:18090".to_owned());
    let script = rig.script(&[
        "hello",
        r#"ask {"t":"store","request":{"v":1,"id":"01010101010101010101010101010101","request":"runner","body":{"op":"load","agent":"{agent}","run":"{run}"}}}"#,
        &bind("http://pay.test:18090"),
        "log bound",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("bound", SHORT), "{}", script.log());
    assert_eq!(script.replies()[1], r#"{"t":"bind_reply","bound":"yes"}"#);
    assert!(rig.people.asked_origins.lock().unwrap().is_empty());
    assert!(adds(&rig).is_empty());
    rig.sup.kill(&run).unwrap();
}

#[test]
fn an_add_origin_that_is_never_acknowledged_answers_nothing_as_bound_and_stops_when_the_run_ends() {
    let rig = Rig::new("bind-never");
    rig.store
        .fail_add_origin
        .store(usize::MAX, Ordering::SeqCst);
    let script = rig.script(&[
        "hello",
        "on-eof hang",
        &bind("http://pay.test:18090"),
        "log bound",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(wait_until(SHORT, || adds(&rig).len() >= 3));
    assert!(script.replies().is_empty(), "{}", script.log());
    rig.sup.kill(&run).unwrap();
    assert!(rig.rec.wait_end(&run, Duration::from_secs(30)).is_some());
    let after = adds(&rig).len();
    std::thread::sleep(Duration::from_millis(1500));
    assert_eq!(adds(&rig).len(), after);
    let ids: HashSet<String> = adds(&rig)
        .iter()
        .map(|b| b[b.find(r#""id":""#).unwrap() + 6..][..32].to_owned())
        .collect();
    assert_eq!(ids.len(), 1);
}

#[test]
fn a_confirm_gets_minted_approval_ids_that_issued_saw_first_and_a_new_origin_gets_add_origin_after_the_release(
) {
    let rig = Rig::new("confirm-allow");
    let script = rig.script(&[
        "hello",
        &confirm(Some("http://pay.test:18090")),
        "log answered",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("answered", SHORT), "{}", script.log());
    let issued = rig.people.issued.lock().unwrap().clone();
    assert_eq!(issued.len(), 1);
    let approval = &issued[0].approvals[0];
    assert_eq!(approval.answer, Answer::AllowOnce);
    assert_eq!(approval.effect.as_str(), "60".repeat(16));
    assert_eq!(approval.digest.as_str(), "a".repeat(64));
    let reply = &script.replies()[0];
    assert_eq!(
        reply,
        &format!(
            r#"{{"t":"confirm_reply","decision":{}}}"#,
            issued[0].to_json().unwrap()
        )
    );
    assert!(wait_until(SHORT, || adds(&rig).len() == 1));
    assert!(adds(&rig)[0].contains(r#""origin":"http://pay.test:18090","way":"new_origin""#));
    rig.sup.kill(&run).unwrap();
}

#[test]
fn an_issued_that_fails_gives_reject_once_under_new_approval_ids_and_no_add_origin() {
    let rig = Rig::new("confirm-issue-fails");
    *rig.people.refuse_issue.lock().unwrap() = true;
    let script = rig.script(&[
        "hello",
        &confirm(Some("http://pay.test:18090")),
        "log answered",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("answered", SHORT), "{}", script.log());
    let issued = rig.people.issued.lock().unwrap().clone();
    let handed = issued[0].approvals[0].approval.as_str().to_owned();
    let reply = &script.replies()[0];
    assert!(reply.contains(r#""answer":"reject_once""#), "{reply}");
    assert!(!reply.contains(r#""answer":"allow_once""#), "{reply}");
    assert!(!reply.contains(&handed), "{reply}");
    std::thread::sleep(Duration::from_millis(700));
    assert!(adds(&rig).is_empty());
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_rejected_effect_with_a_new_origin_sends_no_add_origin() {
    let rig = Rig::new("confirm-reject");
    rig.people
        .answers
        .lock()
        .unwrap()
        .push_back(vec![Answer::RejectOnce]);
    let script = rig.script(&[
        "hello",
        &confirm(Some("http://pay.test:18090")),
        "log answered",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("answered", SHORT), "{}", script.log());
    assert!(script.replies()[0].contains(r#""answer":"reject_once""#));
    std::thread::sleep(Duration::from_millis(700));
    assert!(adds(&rig).is_empty());
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_confirmer_that_answers_for_another_count_of_effects_gets_every_effect_rejected() {
    let rig = Rig::new("confirm-count");
    rig.people
        .answers
        .lock()
        .unwrap()
        .push_back(vec![Answer::AllowOnce, Answer::AllowOnce]);
    let script = rig.script(&["hello", &confirm(None), "log answered", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("answered", SHORT), "{}", script.log());
    assert!(script.replies()[0].contains(r#""answer":"reject_once""#));
    assert!(!script.replies()[0].contains(r#""answer":"allow_once""#));
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_handback_opens_through_human_and_answer_handback_gives_the_result_to_the_runner() {
    let rig = Rig::new("handback-sign-in");
    let script = rig.script(&[
        "hello",
        &handback("sign_in", "http://utility.test:18090"),
        "log answered",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(wait_until(SHORT, || !rig
        .people
        .handbacks
        .lock()
        .unwrap()
        .is_empty()));
    assert_eq!(
        rig.people.handbacks.lock().unwrap()[0].reason,
        HandbackReason::SignIn
    );
    rig.sup
        .answer_handback(&run, result(HandbackOutcome::Done, None))
        .unwrap();
    assert!(script.wait_log("answered", SHORT), "{}", script.log());
    assert_eq!(
        script.replies(),
        vec![r#"{"t":"handback_reply","result":{"v":1,"outcome":"done","secret":null}}"#]
    );
    assert!(adds(&rig).is_empty());
    assert_eq!(
        rig.sup
            .answer_handback(&run, result(HandbackOutcome::Done, None)),
        Err(CallError::Ended)
    );
    assert_eq!(script.replies().len(), 1);
    rig.sup.kill(&run).unwrap();
    assert!(rig.rec.wait_end(&run, Duration::from_secs(30)).is_some());
    assert_eq!(
        rig.sup
            .answer_handback(&run, result(HandbackOutcome::Done, None)),
        Err(CallError::Ended)
    );
}

#[test]
fn an_account_link_done_sends_add_origin_with_the_request_origin_before_the_result_reaches_the_runner(
) {
    let rig = Rig::new("handback-link");
    rig.store.fail_add_origin.store(1, Ordering::SeqCst);
    let script = rig.script(&[
        "hello",
        &handback("account_link", "http://mail.test:18090"),
        "log answered",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(wait_until(SHORT, || !rig
        .people
        .handbacks
        .lock()
        .unwrap()
        .is_empty()));
    rig.sup
        .answer_handback(&run, result(HandbackOutcome::Done, Some("fixture_token")))
        .unwrap();
    assert!(script.wait_log("answered", SHORT), "{}", script.log());
    let sent = adds(&rig);
    assert_eq!(sent.len(), 2);
    assert!(sent[0].contains(r#""origin":"http://mail.test:18090","way":"account_link""#));
    assert_eq!(
        script.replies(),
        vec![
            r#"{"t":"handback_reply","result":{"v":1,"outcome":"done","secret":"fixture_token"}}"#
        ]
    );
    rig.sup.kill(&run).unwrap();
}

#[test]
fn an_account_link_that_the_person_declines_sends_no_add_origin() {
    let rig = Rig::new("handback-link-declined");
    let script = rig.script(&[
        "hello",
        &handback("account_link", "http://mail.test:18090"),
        "log answered",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(wait_until(SHORT, || !rig
        .people
        .handbacks
        .lock()
        .unwrap()
        .is_empty()));
    rig.sup
        .answer_handback(&run, result(HandbackOutcome::Declined, None))
        .unwrap();
    assert!(script.wait_log("answered", SHORT), "{}", script.log());
    assert!(adds(&rig).is_empty());
    rig.sup.kill(&run).unwrap();
}

#[test]
fn an_answer_with_no_open_handback_is_ended_and_reaches_no_runner() {
    let rig = Rig::new("handback-none");
    let script = rig.script(&["hello", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert_eq!(
        rig.sup
            .answer_handback(&run, result(HandbackOutcome::Done, None)),
        Err(CallError::Ended)
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(script.replies().is_empty());
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_request_frame_while_the_runner_waits_for_a_handback_ends_the_run() {
    let rig = Rig::new("handback-twice");
    let frame = handback("sign_in", "http://utility.test:18090").replacen("ask ", "send ", 1);
    let script = rig.script(&["hello", "on-eof hang", &frame, &frame, "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(rig.rec.wait_end(&run, Duration::from_secs(30)).is_some());
    assert_eq!(rig.people.handbacks.lock().unwrap().len(), 1);
}
