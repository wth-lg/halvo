mod common;

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{wait_until, Rig};
use halvo_agent_client::{CallError, Forced, ForcedPath, OriginAnswer, RunEnd};
use halvo_schema::executable::frame::HandbackResult;
use halvo_schema::executable::names::{Answer, HandbackOutcome, HandbackReason, Outcome};
use halvo_schema::executable::text::{EffectId, Name, RunId, V1};

const SHORT: Duration = Duration::from_secs(10);

const CONFIRM: &str = r#"ask {"t":"confirm","request":{"v":1,"run":"{run}","agent":"{agent}","cut":null,"effects":[{"effect":"60606060606060606060606060606060","class":"payment","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","bound":{"class":"payment","fields":[{"name":"payee","from":"slot","origin":null,"value":["Northwind Power"]},{"name":"amount","from":"extract","origin":"http://utility.test:18090","value":["84.20"]},{"name":"instrument","from":"slot","origin":null,"value":["card 4242"]},{"name":"origin","from":"const","origin":null,"value":["http://pay.test:18090"]}]},"new_origin":NEW}]}}"#;

fn confirm(new_origin: Option<&str>) -> String {
    CONFIRM.replace(
        "NEW",
        &new_origin.map_or_else(|| "null".to_owned(), |o| format!("\"{o}\"")),
    )
}

/// The effect id of 16 bytes `n`; `effect_id(0x60)` is the effect of `CONFIRM`.
fn effect_id(n: u8) -> EffectId {
    EffectId::from_random([n; 16])
}

/// An effect of the class `send`, which may stand beside another effect in one request.
const SEND: &str = r#"{"effect":"EFFECT","class":"send","digest":"DIGEST","bound":{"class":"send","fields":[{"name":"to","from":"extract","origin":null,"value":["landlord@mail.test"]},{"name":"subject","from":"extract","origin":null,"value":["Re: rent"]},{"name":"body","from":"model","origin":null,"value":["Thank you."]}]},"new_origin":null}"#;

/// A confirm of two effects of the class `send`, `first` and then `second`.
fn confirm_two(first: &EffectId, second: &EffectId) -> String {
    let send = |id: &EffectId| {
        SEND.replace("EFFECT", id.as_str())
            .replace("DIGEST", &id.as_str().repeat(2))
    };
    format!(
        r#"ask {{"t":"confirm","request":{{"v":1,"run":"{{run}}","agent":"{{agent}}","cut":null,"effects":[{},{}]}}}}"#,
        send(first),
        send(second)
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

fn sign_in() -> String {
    handback("sign_in", "http://pay.test:18090")
}

fn signed_in(rig: &Rig, run: &RunId, n: usize) {
    assert!(
        wait_until(SHORT, || rig.people.handbacks.lock().unwrap().len() >= n),
        "no handback number {n} was opened"
    );
    rig.sup
        .answer_handback(run, result(HandbackOutcome::Done, None))
        .unwrap();
}

fn binds(script: &common::Script) -> Vec<String> {
    script
        .replies()
        .into_iter()
        .filter(|r| r.contains(r#""t":"bind_reply""#))
        .collect()
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
    let script = rig.script(&[
        "hello",
        &sign_in(),
        &bind("http://pay.test:18090"),
        "log bound",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec.clone()).unwrap();
    signed_in(&rig, &run, 1);
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
    assert_eq!(binds(&script), vec![r#"{"t":"bind_reply","bound":"yes"}"#]);
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_bind_that_the_person_declines_is_answered_no_and_sends_no_add_origin() {
    let rig = Rig::new("bind-decline");
    *rig.people.allow.lock().unwrap() = OriginAnswer::Decline;
    let script = rig.script(&[
        "hello",
        &sign_in(),
        &bind("http://pay.test:18090"),
        "log bound",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    assert!(script.wait_log("bound", SHORT), "{}", script.log());
    assert_eq!(binds(&script), vec![r#"{"t":"bind_reply","bound":"no"}"#]);
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
        &sign_in(),
        &bind("http://pay.test:18090"),
        "log bound",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    assert!(script.wait_log("bound", SHORT), "{}", script.log());
    assert_eq!(binds(&script), vec![r#"{"t":"bind_reply","bound":"yes"}"#]);
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
        &sign_in(),
        &bind("http://pay.test:18090"),
        "log bound",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    assert!(wait_until(SHORT, || adds(&rig).len() >= 3));
    assert!(binds(&script).is_empty(), "{}", script.log());
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
        .push_back(vec![(effect_id(0x60), Answer::RejectOnce)]);
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
    rig.people.answers.lock().unwrap().push_back(vec![
        (effect_id(0x60), Answer::AllowOnce),
        (effect_id(0x60), Answer::AllowOnce),
    ]);
    let script = rig.script(&["hello", &confirm(None), "log answered", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("answered", SHORT), "{}", script.log());
    assert!(script.replies()[0].contains(r#""answer":"reject_once""#));
    assert!(!script.replies()[0].contains(r#""answer":"allow_once""#));
    rig.sup.kill(&run).unwrap();
}

/// U4.7, CP-226 item 1 reading (b), contracts.md A6.6: each answer of `Confirmer::answers` names its effect by id,
/// and the library releases a decision only when the ids match the asked effects in their order. The answers of two
/// effects in the asked order are applied each to its effect; the same answers in the other order, and a set of
/// answers that names an effect that the request does not ask for, reject both effects; the runner gets the decision
/// that `issued` saw.
#[test]
fn answers_in_the_asked_order_go_each_to_their_effect_and_another_order_or_a_wrong_id_rejects_every_effect(
) {
    let rig = Rig::new("confirm-by-id");
    let (first, second) = (effect_id(0x61), effect_id(0x62));
    let asked = vec![
        (first.clone(), Answer::AllowOnce),
        (second.clone(), Answer::RejectOnce),
    ];
    let other_order = asked.iter().rev().cloned().collect();
    rig.people.answers.lock().unwrap().extend([
        asked,
        other_order,
        vec![
            (first.clone(), Answer::AllowOnce),
            (effect_id(0x63), Answer::AllowOnce),
        ],
    ]);
    let ask = confirm_two(&first, &second);
    let script = rig.script(&[
        "hello",
        &ask,
        "log asked-order",
        &ask,
        "log other-order",
        &ask,
        "log wrong-id",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("wrong-id", SHORT), "{}", script.log());
    let issued = rig.people.issued.lock().unwrap().clone();
    let applied: Vec<Vec<(EffectId, Answer)>> = issued
        .iter()
        .map(|d| {
            d.approvals
                .iter()
                .map(|a| (a.effect.clone(), a.answer))
                .collect()
        })
        .collect();
    let rejected = vec![
        (first.clone(), Answer::RejectOnce),
        (second.clone(), Answer::RejectOnce),
    ];
    assert_eq!(
        applied,
        [
            vec![(first, Answer::AllowOnce), (second, Answer::RejectOnce)],
            rejected.clone(),
            rejected,
        ]
    );
    let sent: Vec<String> = issued
        .iter()
        .map(|d| {
            format!(
                r#"{{"t":"confirm_reply","decision":{}}}"#,
                d.to_json().unwrap()
            )
        })
        .collect();
    assert_eq!(script.replies(), sent);
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

#[test]
fn an_add_origin_reply_for_another_id_or_of_another_call_form_is_no_acknowledgement() {
    let rig = Rig::new("bind-wrong-reply");
    rig.store.wrong_id_coord.store(1, Ordering::SeqCst);
    rig.store.wrong_form_coord.store(1, Ordering::SeqCst);
    let script = rig.script(&[
        "hello",
        &sign_in(),
        &bind("http://pay.test:18090"),
        "log bound",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    assert!(script.wait_log("bound", SHORT), "{}", script.log());
    assert_eq!(adds(&rig).len(), 3);
    assert_eq!(binds(&script), vec![r#"{"t":"bind_reply","bound":"yes"}"#]);
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_confirm_request_of_another_run_ends_the_run_unasked() {
    let rig = Rig::new("confirm-other-run");
    let other = confirm(None).replace("{run}", &"f".repeat(32));
    let script = rig.script(&["hello", "on-eof hang", &other, "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(rig.rec.wait_end(&run, Duration::from_secs(30)).is_some());
    assert!(rig.people.issued.lock().unwrap().is_empty());
    assert!(script.replies().is_empty(), "{}", script.log());
}

#[test]
fn an_add_origin_without_acknowledgement_is_sent_again_while_the_runner_sends_frames() {
    let rig = Rig::new("add-origin-busy");
    rig.store
        .fail_add_origin
        .store(usize::MAX, Ordering::SeqCst);
    let mut lines: Vec<String> = vec![
        "hello".to_owned(),
        sign_in(),
        r#"send {"t":"bind","origin":"http://pay.test:18090"}"#.to_owned(),
    ];
    for _ in 0..75 {
        lines.push(r#"send {"t":"progress","step":1,"of":2}"#.to_owned());
        lines.push("sleep 40".to_owned());
    }
    lines.push("log burst-over".to_owned());
    lines.push("idle".to_owned());
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let script = rig.script(&refs);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    assert!(
        script.wait_log("burst-over", Duration::from_secs(30)),
        "{}",
        script.log()
    );
    let during = adds(&rig).len();
    rig.sup.kill(&run).unwrap();
    assert!(
        during >= 4,
        "add_origin was sent {during} times in 3 s of frames"
    );
}

#[test]
fn a_handback_request_of_another_run_or_agent_ends_the_run_unasked() {
    for (key, other) in [("{run}", "f".repeat(32)), ("{agent}", "e".repeat(32))] {
        let rig = Rig::new("handback-other-run");
        let frame = handback("account_link", "http://mail.test:18090")
            .replacen("ask ", "send ", 1)
            .replace(key, &other);
        let script = rig.script(&["hello", "on-eof hang", &frame, "idle"]);
        let spec = rig.spec(&script);
        let run = spec.run.clone();
        rig.sup.start(spec).unwrap();
        assert!(
            rig.rec.wait_end(&run, Duration::from_secs(30)).is_some(),
            "{key}"
        );
        assert!(rig.people.handbacks.lock().unwrap().is_empty(), "{key}");
        assert_eq!(
            rig.sup
                .answer_handback(&run, result(HandbackOutcome::Done, None)),
            Err(CallError::Ended)
        );
        assert!(adds(&rig).is_empty(), "{key}");
    }
}

#[test]
fn an_origin_whose_add_origin_was_acknowledged_is_bound_and_asks_no_one_again() {
    let rig = Rig::new("bind-twice");
    let script = rig.script(&[
        "hello",
        &sign_in(),
        &bind("http://pay.test:18090"),
        &sign_in(),
        &bind("http://pay.test:18090"),
        "log both-binds-answered",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    signed_in(&rig, &run, 2);
    assert!(
        script.wait_log("both-binds-answered", SHORT),
        "{}",
        script.log()
    );
    assert_eq!(rig.people.asked_origins.lock().unwrap().len(), 1);
    assert_eq!(adds(&rig).len(), 1);
    assert_eq!(
        binds(&script),
        vec![
            r#"{"t":"bind_reply","bound":"yes"}"#,
            r#"{"t":"bind_reply","bound":"yes"}"#
        ]
    );
    rig.sup.kill(&run).unwrap();
}

#[test]
fn an_account_link_handback_with_no_origin_ends_the_run_unasked() {
    let rig = Rig::new("handback-link-no-origin");
    let frame = handback("account_link", "X")
        .replacen("ask ", "send ", 1)
        .replace("\"X\"", "null");
    let script = rig.script(&["hello", "on-eof hang", &frame, "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(rig.rec.wait_end(&run, Duration::from_secs(30)).is_some());
    assert!(rig.people.handbacks.lock().unwrap().is_empty());
    assert!(adds(&rig).is_empty());
    assert!(script.replies().is_empty(), "{}", script.log());
}

#[test]
fn a_refused_answer_handback_leaves_the_wait_for_a_bind_reply_in_place() {
    let rig = Rig::new("refused-answer-bind-wait");
    rig.store
        .fail_add_origin
        .store(usize::MAX, Ordering::SeqCst);
    let script = rig.script(&[
        "hello",
        "on-eof hang",
        &sign_in(),
        &bind("http://pay.test:18090").replacen("ask ", "send ", 1),
        "log bind-sent",
        "sleep 1500",
        &confirm(None),
        "log confirm-answered",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    assert!(script.wait_log("bind-sent", SHORT), "{}", script.log());
    assert!(wait_until(SHORT, || !rig
        .people
        .asked_origins
        .lock()
        .unwrap()
        .is_empty()));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        rig.sup
            .answer_handback(&run, result(HandbackOutcome::Done, None)),
        Err(CallError::Ended)
    );
    assert!(rig.rec.wait_end(&run, Duration::from_secs(10)).is_some());
    assert!(
        !script.log().contains("confirm-answered"),
        "{}",
        script.log()
    );
    assert_eq!(rig.people.asked_confirms.load(Ordering::SeqCst), 0);
}

fn out_of_turn(
    name: &str,
    lines: &[&str],
    results: &[HandbackOutcome],
) -> (Rig, common::Script, RunEnd) {
    let rig = Rig::new(name);
    let script = rig.script(lines);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    for (n, outcome) in results.iter().enumerate() {
        assert!(
            wait_until(SHORT, || rig.people.handbacks.lock().unwrap().len() > n),
            "{}",
            script.log()
        );
        rig.sup
            .answer_handback(&run, result(*outcome, None))
            .unwrap();
    }
    let end = rig
        .rec
        .wait_end(&run, SHORT)
        .unwrap_or_else(|| panic!("the run did not end: {}", script.log()));
    (rig, script, end)
}

fn assert_lost_unasked(
    rig: &Rig,
    script: &common::Script,
    end: &RunEnd,
    asked: usize,
    added: usize,
) {
    assert_eq!(
        end.forced,
        Some(Forced {
            path: ForcedPath::LinkEnd,
            browser: false
        }),
        "the run did not end as a lost flow: {}",
        script.log()
    );
    assert_eq!(rig.people.asked_origins.lock().unwrap().len(), asked);
    assert_eq!(adds(rig).len(), added);
    assert_eq!(binds(script).len(), asked, "{}", script.log());
}

#[test]
fn a_bind_as_the_runners_first_request_ends_the_run_and_asks_nobody() {
    let send = bind("http://pay.test:18090").replacen("ask ", "send ", 1);
    let (rig, script, end) =
        out_of_turn("bind-first", &["hello", "on-eof hang", &send, "idle"], &[]);
    assert_lost_unasked(&rig, &script, &end, 0, 0);
}

#[test]
fn a_bind_after_a_handback_result_other_than_done_ends_the_run_and_asks_nobody() {
    for outcome in [HandbackOutcome::Declined, HandbackOutcome::Timeout] {
        let send = bind("http://pay.test:18090").replacen("ask ", "send ", 1);
        let (rig, script, end) = out_of_turn(
            "bind-after-declined",
            &["hello", "on-eof hang", &sign_in(), &send, "idle"],
            &[outcome],
        );
        assert_lost_unasked(&rig, &script, &end, 0, 0);
    }
}

#[test]
fn a_bind_after_a_done_handback_of_another_reason_ends_the_run_and_asks_nobody() {
    for reason in ["step_failed", "effect_unknown"] {
        let send = bind("http://pay.test:18090").replacen("ask ", "send ", 1);
        let other = handback(reason, "http://pay.test:18090");
        let (rig, script, end) = out_of_turn(
            "bind-after-other-reason",
            &["hello", "on-eof hang", &other, &send, "idle"],
            &[HandbackOutcome::Done],
        );
        assert_lost_unasked(&rig, &script, &end, 0, 0);
    }
}

#[test]
fn a_second_bind_after_the_same_done_ends_the_run_and_asks_nobody_again() {
    let first = bind("http://pay.test:18090");
    let second = bind("http://mail.test:18090").replacen("ask ", "send ", 1);
    let (rig, script, end) = out_of_turn(
        "bind-second",
        &["hello", "on-eof hang", &sign_in(), &first, &second, "idle"],
        &[HandbackOutcome::Done],
    );
    assert_lost_unasked(&rig, &script, &end, 1, 1);
}

#[test]
fn a_bind_after_a_done_handback_of_each_sign_in_reason_is_taken() {
    for reason in ["sign_in", "second_factor", "captcha"] {
        let rig = Rig::new("bind-each-reason");
        let script = rig.script(&[
            "hello",
            &handback(reason, "http://pay.test:18090"),
            &bind("http://pay.test:18090"),
            "log bound",
            "idle",
        ]);
        let spec = rig.spec(&script);
        let run = spec.run.clone();
        rig.sup.start(spec).unwrap();
        signed_in(&rig, &run, 1);
        assert!(
            script.wait_log("bound", SHORT),
            "{reason}: {}",
            script.log()
        );
        assert_eq!(
            binds(&script),
            vec![r#"{"t":"bind_reply","bound":"yes"}"#],
            "{reason}"
        );
        rig.sup.kill(&run).unwrap();
    }
}

fn trait_calls(rig: &Rig) -> (usize, usize, usize, usize) {
    (
        rig.people.asked_confirms.load(Ordering::SeqCst),
        rig.people.handbacks.lock().unwrap().len(),
        rig.people.asked_origins.lock().unwrap().len(),
        rig.vault.reads(),
    )
}

fn frame_out_of_turn(name: &str) -> String {
    match name {
        "confirm" => confirm(None).replacen("ask ", "send ", 1),
        "handback" => sign_in().replacen("ask ", "send ", 1),
        "bind" => bind("http://pay.test:18090").replacen("ask ", "send ", 1),
        "secret" => r#"send {"t":"secret","ref":"fixture_token"}"#.to_owned(),
        _ => "browser".to_owned(),
    }
}

fn after_finish(name: &str) {
    let finish = common::ask_finish();
    let frame = frame_out_of_turn(name);
    let (rig, script, end) = out_of_turn(
        "after-finish",
        &[
            "hello",
            "on-eof hang",
            &finish,
            "log finished",
            &frame,
            "idle",
        ],
        &[],
    );
    assert!(end.finish.is_some(), "{name}: the finish was stored");
    assert_eq!(
        end.forced,
        Some(Forced {
            path: ForcedPath::AfterFinish,
            browser: false
        }),
        "{name}: {}",
        script.log()
    );
    assert_eq!(trait_calls(&rig), (0, 0, 0, 0), "{name}");
    assert_eq!(
        script.replies().len(),
        1,
        "{name}: a reply other than the finish's: {}",
        script.log()
    );
}

#[test]
fn a_confirm_after_a_stored_finish_ends_the_run_after_finish_unanswered_and_unasked() {
    after_finish("confirm");
}

#[test]
fn a_handback_after_a_stored_finish_ends_the_run_after_finish_unanswered_and_unasked() {
    after_finish("handback");
}

#[test]
fn a_bind_after_a_stored_finish_ends_the_run_after_finish_unanswered_and_unasked() {
    after_finish("bind");
}

#[test]
fn a_secret_after_a_stored_finish_ends_the_run_after_finish_unanswered_and_unasked() {
    after_finish("secret");
}

#[test]
fn a_browser_frame_after_a_stored_finish_ends_the_run_after_finish_unanswered_and_unasked() {
    after_finish("browser");
}

fn load_store() -> String {
    common::ask_store(
        &common::id(0x52),
        r#"{"op":"load","agent":"{agent}","run":"{run}"}"#,
    )
    .replacen("ask ", "send ", 1)
}

fn after_bound_no(name: &str, frame: &str) {
    let rig = Rig::new("after-bound-no");
    *rig.people.allow.lock().unwrap() = OriginAnswer::Decline;
    let script = rig.script(&[
        "hello",
        "on-eof hang",
        &sign_in(),
        &bind("http://pay.test:18090"),
        "log declined",
        frame,
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    let end = rig
        .rec
        .wait_end(&run, SHORT)
        .unwrap_or_else(|| panic!("{name}: the run did not end: {}", script.log()));
    assert!(
        script.log().contains("declined"),
        "{name}: {}",
        script.log()
    );
    assert_eq!(
        binds(&script),
        vec![r#"{"t":"bind_reply","bound":"no"}"#],
        "{name}"
    );
    assert_eq!(
        end.forced,
        Some(Forced {
            path: ForcedPath::LinkEnd,
            browser: false
        }),
        "{name}: {}",
        script.log()
    );
    assert_eq!(trait_calls(&rig), (0, 1, 1, 0), "{name}");
    assert_eq!(script.replies().len(), 2, "{name}: {}", script.log());
    assert_eq!(rig.store.count(r#""op":"load""#), 0, "{name}");
    assert!(adds(&rig).is_empty(), "{name}");
}

#[test]
fn a_confirm_after_bound_no_ends_the_run_unasked() {
    after_bound_no("confirm", &frame_out_of_turn("confirm"));
}

#[test]
fn a_handback_after_bound_no_ends_the_run_unasked() {
    after_bound_no("handback", &frame_out_of_turn("handback"));
}

#[test]
fn a_bind_after_bound_no_ends_the_run_unasked() {
    after_bound_no("bind", &frame_out_of_turn("bind"));
}

#[test]
fn a_secret_after_bound_no_ends_the_run_unasked() {
    after_bound_no("secret", &frame_out_of_turn("secret"));
}

#[test]
fn a_browser_frame_after_bound_no_ends_the_run_unasked() {
    after_bound_no("browser", &frame_out_of_turn("browser"));
}

#[test]
fn a_store_of_another_body_than_finish_after_bound_no_ends_the_run_unrelayed() {
    after_bound_no("load", &load_store());
}

#[test]
fn the_declined_finish_after_bound_no_is_stored_and_its_done_taken() {
    let rig = Rig::new("bound-no-finish");
    *rig.people.allow.lock().unwrap() = OriginAnswer::Decline;
    let finish = common::ask_finish();
    let script = rig.script(&[
        "hello",
        &sign_in(),
        &bind("http://pay.test:18090"),
        r#"send {"t":"progress","step":2,"of":2}"#,
        &finish,
        r#"send {"t":"done","outcome":"declined"}"#,
        "exit 1",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    let end = rig.rec.wait_end(&run, SHORT).expect("the run ended");
    assert!(
        end.finish.is_some(),
        "the finish was stored: {}",
        script.log()
    );
    assert_eq!(end.outcome, Some(Outcome::Declined));
    assert_eq!(end.forced, None, "{}", script.log());
    assert_eq!(rig.store.count(r#""op":"finish""#), 1);
}

fn cancelled_bind_script(rig: &Rig) -> (common::Script, std::path::PathBuf) {
    let go = rig.dir.join("go");
    let finish = common::ask_finish();
    let script = rig.script(&[
        "hello",
        &sign_in(),
        &bind("http://pay.test:18090").replacen("ask ", "send ", 1),
        &format!("wait-for {}", go.display()),
        &finish,
        r#"send {"t":"done","outcome":"declined"}"#,
        "exit 1",
    ]);
    (script, go)
}

fn assert_cancelled_bind_ended_as_declined(rig: &Rig, script: &common::Script, end: &RunEnd) {
    assert!(script.log().contains("\ncancel"), "{}", script.log());
    assert!(
        binds(script).is_empty(),
        "a bind_reply was written: {}",
        script.log()
    );
    assert!(
        end.finish.is_some(),
        "the finish was not stored: {}",
        script.log()
    );
    assert_eq!(end.outcome, Some(Outcome::Declined));
    assert_eq!(end.forced, None, "the run was lost: {}", script.log());
    assert_eq!(rig.store.count(r#""op":"finish""#), 1);
}

#[test]
fn a_cancel_while_the_library_stands_in_allow_origin_drops_the_bind_and_takes_the_declined_finish()
{
    let rig = Rig::new("cancel-in-allow");
    *rig.people.hold_allow.lock().unwrap() = true;
    let (script, go) = cancelled_bind_script(&rig);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    assert!(wait_until(SHORT, || !rig
        .people
        .asked_origins
        .lock()
        .unwrap()
        .is_empty()));
    assert_eq!(rig.sup.cancel(&run), Ok(()));
    assert!(script.wait_log("\ncancel", SHORT), "{}", script.log());
    *rig.people.hold_allow.lock().unwrap() = false;
    std::fs::write(&go, b"").unwrap();
    let end = rig.rec.wait_end(&run, SHORT).expect("the run ended");
    assert_cancelled_bind_ended_as_declined(&rig, &script, &end);
    assert!(
        adds(&rig).is_empty(),
        "add_origin was sent: {:?}",
        adds(&rig)
    );
}

#[test]
fn a_cancel_while_the_add_origin_of_a_bind_waits_drops_it_and_takes_the_declined_finish() {
    let rig = Rig::new("cancel-in-bind-wait");
    rig.store
        .fail_add_origin
        .store(usize::MAX, Ordering::SeqCst);
    let (script, go) = cancelled_bind_script(&rig);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    assert!(wait_until(SHORT, || adds(&rig).len() >= 2));
    assert_eq!(rig.sup.cancel(&run), Ok(()));
    assert!(script.wait_log("\ncancel", SHORT), "{}", script.log());
    std::thread::sleep(Duration::from_millis(300));
    let at_cancel = adds(&rig).len();
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(
        adds(&rig).len(),
        at_cancel,
        "the add_origin of the bind was sent again after the cancel"
    );
    std::fs::write(&go, b"").unwrap();
    let end = rig.rec.wait_end(&run, SHORT).expect("the run ended");
    assert_cancelled_bind_ended_as_declined(&rig, &script, &end);
}

#[test]
fn two_add_origin_replies_that_cannot_be_read_and_then_an_acknowledgement_bind_the_origin() {
    let rig = Rig::new("add-refused-two");
    rig.store.garbage_add_origin.store(2, Ordering::SeqCst);
    let script = rig.script(&[
        "hello",
        &sign_in(),
        &bind("http://pay.test:18090"),
        "log bound",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    assert!(script.wait_log("bound", SHORT), "{}", script.log());
    assert_eq!(binds(&script), vec![r#"{"t":"bind_reply","bound":"yes"}"#]);
    assert_eq!(adds(&rig).len(), 3);
    assert!(rig.rec.wait_end(&run, Duration::ZERO).is_none());
    rig.sup.kill(&run).unwrap();
}

#[test]
fn three_add_origin_replies_that_cannot_be_read_end_a_run_that_waits_for_the_bind_or_the_account_link(
) {
    for reason in ["sign_in", "account_link"] {
        let rig = Rig::new("add-refused-three");
        rig.store
            .garbage_add_origin
            .store(usize::MAX, Ordering::SeqCst);
        let mut lines = vec!["hello".to_owned(), "on-eof hang".to_owned()];
        if reason == "sign_in" {
            lines.push(sign_in());
            lines.push(bind("http://pay.test:18090"));
        } else {
            lines.push(handback("account_link", "http://mail.test:18090"));
        }
        lines.push("idle".to_owned());
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let script = rig.script(&refs);
        let spec = rig.spec(&script);
        let run = spec.run.clone();
        rig.sup.start(spec).unwrap();
        signed_in(&rig, &run, 1);
        let end = rig
            .rec
            .wait_end(&run, SHORT)
            .unwrap_or_else(|| panic!("{reason}: the run did not end: {}", script.log()));
        assert_eq!(adds(&rig).len(), 3, "{reason}: three sends in all");
        assert_eq!(
            end.forced,
            Some(Forced {
                path: ForcedPath::LinkEnd,
                browser: false
            }),
            "{reason}"
        );
        assert!(binds(&script).is_empty(), "{reason}: {}", script.log());
        assert_eq!(rig.store.count(r#""op":"end_run""#), 1, "{reason}");
    }
}

#[test]
fn three_link_errors_on_an_add_origin_end_nothing_and_the_fourth_send_binds() {
    let rig = Rig::new("add-link-errors");
    rig.store.fail_add_origin.store(3, Ordering::SeqCst);
    let script = rig.script(&[
        "hello",
        &sign_in(),
        &bind("http://pay.test:18090"),
        "log bound",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    assert!(script.wait_log("bound", SHORT), "{}", script.log());
    assert_eq!(binds(&script), vec![r#"{"t":"bind_reply","bound":"yes"}"#]);
    assert_eq!(adds(&rig).len(), 4);
    assert!(rig.rec.wait_end(&run, Duration::ZERO).is_none());
    rig.sup.kill(&run).unwrap();
}

#[test]
fn three_add_origin_replies_that_cannot_be_read_drop_a_new_origin_and_the_run_goes_on() {
    let rig = Rig::new("add-refused-new-origin");
    rig.store
        .garbage_add_origin
        .store(usize::MAX, Ordering::SeqCst);
    let script = rig.script(&[
        "hello",
        &confirm(Some("http://pay.test:18090")),
        "log answered",
        "sleep 2500",
        r#"ask {"t":"secret","ref":"fixture_token"}"#,
        "log went-on",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("went-on", SHORT), "{}", script.log());
    assert_eq!(adds(&rig).len(), 3, "three sends in all");
    assert!(rig.rec.wait_end(&run, Duration::ZERO).is_none());
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_bind_after_a_sign_in_done_and_a_later_done_handback_of_another_reason_ends_the_run_and_asks_nobody(
) {
    for (reason, origin, added) in [
        ("step_failed", "http://pay.test:18090", 0),
        ("account_link", "http://mail.test:18090", 1),
    ] {
        let send = bind("http://pay.test:18090").replacen("ask ", "send ", 1);
        let other = handback(reason, origin);
        let (rig, script, end) = out_of_turn(
            "bind-after-later-handback",
            &["hello", "on-eof hang", &sign_in(), &other, &send, "idle"],
            &[HandbackOutcome::Done, HandbackOutcome::Done],
        );
        assert_lost_unasked(&rig, &script, &end, 0, added);
    }
}

fn bind_after_replies(name: &str, set: impl FnOnce(&Rig)) {
    let rig = Rig::new(name);
    set(&rig);
    let script = rig.script(&[
        "hello",
        &sign_in(),
        &bind("http://pay.test:18090"),
        "log bound",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    assert!(script.wait_log("bound", SHORT), "{name}: {}", script.log());
    assert_eq!(
        binds(&script),
        vec![r#"{"t":"bind_reply","bound":"yes"}"#],
        "{name}"
    );
    assert_eq!(
        adds(&rig).len(),
        4,
        "{name}: three sends that met no count and the acknowledged one"
    );
    assert!(rig.rec.wait_end(&run, Duration::ZERO).is_none(), "{name}");
    rig.sup.kill(&run).unwrap();
}

#[test]
fn three_add_origin_replies_of_another_id_and_then_an_acknowledgement_still_bind() {
    bind_after_replies("add-other-id-three", |rig| {
        rig.store.wrong_id_coord.store(3, Ordering::SeqCst)
    });
}

#[test]
fn three_add_origin_error_replies_or_replies_of_another_form_and_then_an_acknowledgement_still_bind(
) {
    bind_after_replies("add-error-three", |rig| {
        rig.store.error_add_origin.store(3, Ordering::SeqCst)
    });
    bind_after_replies("add-form-three", |rig| {
        rig.store.wrong_form_coord.store(3, Ordering::SeqCst)
    });
}

fn frame_after_cancelled_bind(name: &str, frame: &str, in_allow: bool) {
    let rig = Rig::new(name);
    if in_allow {
        *rig.people.hold_allow.lock().unwrap() = true;
    } else {
        rig.store
            .fail_add_origin
            .store(usize::MAX, Ordering::SeqCst);
    }
    let go = rig.dir.join("go");
    let wait = format!("wait-for {}", go.display());
    let script = rig.script(&[
        "hello",
        "on-eof hang",
        &sign_in(),
        &bind("http://pay.test:18090").replacen("ask ", "send ", 1),
        &wait,
        frame,
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    if in_allow {
        assert!(wait_until(SHORT, || !rig
            .people
            .asked_origins
            .lock()
            .unwrap()
            .is_empty()));
    } else {
        assert!(wait_until(SHORT, || !adds(&rig).is_empty()));
    }
    assert_eq!(rig.sup.cancel(&run), Ok(()));
    assert!(script.wait_log("\ncancel", SHORT), "{}", script.log());
    *rig.people.hold_allow.lock().unwrap() = false;
    std::fs::write(&go, b"").unwrap();
    let end = rig
        .rec
        .wait_end(&run, SHORT)
        .unwrap_or_else(|| panic!("{name}: the run did not end: {}", script.log()));
    assert_eq!(
        end.forced,
        Some(Forced {
            path: ForcedPath::LinkEnd,
            browser: false
        }),
        "{name}: {}",
        script.log()
    );
    assert_eq!(
        rig.people.asked_confirms.load(Ordering::SeqCst),
        0,
        "{name}"
    );
    assert_eq!(rig.vault.reads(), 0, "{name}");
}

#[test]
fn a_confirm_or_a_secret_after_a_cancel_written_while_allow_origin_runs_ends_the_run_unasked() {
    frame_after_cancelled_bind("cancel-allow-confirm", &frame_out_of_turn("confirm"), true);
    frame_after_cancelled_bind("cancel-allow-secret", &frame_out_of_turn("secret"), true);
}

#[test]
fn a_confirm_or_a_secret_after_a_cancel_written_while_the_add_origin_waits_ends_the_run_unasked() {
    frame_after_cancelled_bind("cancel-add-confirm", &frame_out_of_turn("confirm"), false);
    frame_after_cancelled_bind("cancel-add-secret", &frame_out_of_turn("secret"), false);
}

#[test]
fn a_store_of_finish_after_a_cancel_during_an_open_handback_stays_a_lost_flow() {
    let rig = Rig::new("cancel-in-handback");
    let go = rig.dir.join("go");
    let wait = format!("wait-for {}", go.display());
    let finish = common::ask_finish().replacen("ask ", "send ", 1);
    let script = rig.script(&[
        "hello",
        "on-eof hang",
        &sign_in().replacen("ask ", "send ", 1),
        &wait,
        &finish,
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(wait_until(SHORT, || rig
        .people
        .handbacks
        .lock()
        .unwrap()
        .len()
        == 1));
    assert_eq!(rig.sup.cancel(&run), Ok(()));
    assert!(script.wait_log("\ncancel", SHORT), "{}", script.log());
    std::fs::write(&go, b"").unwrap();
    let end = rig.rec.wait_end(&run, SHORT).expect("the run ended");
    assert_eq!(
        end.forced,
        Some(Forced {
            path: ForcedPath::LinkEnd,
            browser: false
        }),
        "{}",
        script.log()
    );
    assert!(
        end.finish.is_none(),
        "the finish was taken while a handback waited"
    );
    assert_eq!(rig.store.count(r#""op":"finish""#), 0);
}

#[test]
fn an_add_origin_acknowledged_after_the_library_wrote_cancel_gives_the_runner_no_bind_reply() {
    let rig = Rig::new("cancel-before-ack");
    *rig.store.hold_add_origin.lock().unwrap() = true;
    let go = rig.dir.join("go");
    let wait = format!("wait-for {}", go.display());
    let finish = common::ask_finish();
    let script = rig.script(&[
        "hello",
        &sign_in(),
        &bind("http://pay.test:18090").replacen("ask ", "send ", 1),
        &wait,
        &finish,
        r#"send {"t":"done","outcome":"declined"}"#,
        "exit 1",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    assert!(wait_until(SHORT, || !adds(&rig).is_empty()));
    assert_eq!(rig.sup.cancel(&run), Ok(()));
    assert!(script.wait_log("\ncancel", SHORT), "{}", script.log());
    *rig.store.hold_add_origin.lock().unwrap() = false;
    std::fs::write(&go, b"").unwrap();
    let end = rig.rec.wait_end(&run, SHORT).expect("the run ended");
    assert!(
        binds(&script).is_empty(),
        "the runner read a bind_reply after the cancel: {}",
        script.log()
    );
    assert!(end.finish.is_some(), "{}", script.log());
}

#[test]
fn a_confirm_after_a_done_account_link_and_its_reply_is_answered_and_the_run_ends_with_its_done() {
    let rig = Rig::new("account-link-then-confirm");
    let script = rig.script(&[
        "hello",
        &handback("account_link", "http://mail.test:18090"),
        &confirm(None),
        "log answered",
        r#"send {"t":"done","outcome":"completed"}"#,
        "exit 0",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    let end = rig.rec.wait_end(&run, SHORT).expect("the run ended");
    assert!(script.log().contains("answered"), "{}", script.log());
    assert_eq!(rig.people.issued.lock().unwrap().len(), 1);
    assert_eq!(end.outcome, Some(Outcome::Completed), "{}", script.log());
    assert_eq!(end.forced, None, "{}", script.log());
}
