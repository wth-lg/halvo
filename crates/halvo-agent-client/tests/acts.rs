mod common;

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::{wait_until, Rig};
use halvo_agent_client::{CallError, Forced, ForcedPath, ListFailed, OriginAnswer, RunEnd};
use halvo_schema::executable::body::CheckpointsReply;
use halvo_schema::executable::frame::HandbackResult;
use halvo_schema::executable::names::{Answer, HandbackOutcome, HandbackReason, Outcome};
use halvo_schema::executable::text::{EffectId, Name, RunId, V1};
use halvo_schema::request::REPLY_MAX;

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
fn a_no_at_the_bind_question_drops_the_waiting_add_of_that_origin() {
    let rig = Rig::new("bind-decline-drops-add");
    rig.store
        .fail_add_origin
        .store(usize::MAX, Ordering::SeqCst);
    *rig.people.allow.lock().unwrap() = OriginAnswer::Decline;
    let other = confirm(Some("http://mail.test:18090")).replace(&"60".repeat(16), &"61".repeat(16));
    let script = rig.script(&[
        "hello",
        &confirm(Some("http://pay.test:18090")),
        &other,
        &sign_in(),
        &bind("http://pay.test:18090"),
        "log declined",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    assert!(script.wait_log("declined", SHORT), "{}", script.log());
    assert_eq!(binds(&script), vec![r#"{"t":"bind_reply","bound":"no"}"#]);
    let to = |origin: &str| {
        let way = format!(r#""origin":"{origin}","way":"new_origin""#);
        adds(&rig).iter().filter(|b| b.contains(&way)).count()
    };
    let (pay, mail) = (to("http://pay.test:18090"), to("http://mail.test:18090"));
    assert!(pay >= 1 && mail >= 1, "both adds were sent before the no");
    assert!(
        wait_until(SHORT, || to("http://mail.test:18090") >= mail + 2),
        "the add of another origin is still sent; two more sends span more than one ADD_AGAIN"
    );
    assert_eq!(
        to("http://pay.test:18090"),
        pay,
        "an add_origin of the declined origin was sent after the no"
    );
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_no_behind_a_cancel_written_while_the_bind_question_is_open_drops_the_waiting_add_of_that_origin(
) {
    let rig = Rig::new("cancel-then-no-drops-add");
    rig.store
        .fail_add_origin
        .store(usize::MAX, Ordering::SeqCst);
    *rig.people.allow.lock().unwrap() = OriginAnswer::Decline;
    *rig.people.hold_allow.lock().unwrap() = true;
    let go = rig.dir.join("go");
    let other = confirm(Some("http://mail.test:18090")).replace(&"60".repeat(16), &"61".repeat(16));
    let script = rig.script(&[
        "hello",
        &confirm(Some("http://pay.test:18090")),
        &other,
        &sign_in(),
        &bind("http://pay.test:18090").replacen("ask ", "send ", 1),
        &format!("wait-for {}", go.display()),
        &end_row(None),
        &common::ask_finish(),
        r#"send {"t":"done","outcome":"declined"}"#,
        "exit 1",
    ]);
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
    let to = |origin: &str| {
        let way = format!(r#""origin":"{origin}","way":"new_origin""#);
        adds(&rig).iter().filter(|b| b.contains(&way)).count()
    };
    let (pay, mail) = (to("http://pay.test:18090"), to("http://mail.test:18090"));
    assert!(
        pay >= 1 && mail >= 1,
        "both adds were sent before the question"
    );
    *rig.people.hold_allow.lock().unwrap() = false;
    assert!(
        wait_until(SHORT, || to("http://mail.test:18090") >= mail + 2),
        "the add of another origin is still sent; two more sends span more than one ADD_AGAIN"
    );
    assert_eq!(
        to("http://pay.test:18090"),
        pay,
        "an add_origin of the declined origin was sent after the no behind a cancel"
    );
    std::fs::write(&go, b"").unwrap();
    let end = rig.rec.wait_end(&run, SHORT).expect("the run ended");
    assert_cancelled_bind_ended_as_declined(&rig, &script, &end);
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

/// The store of a run's end row as `Runner::finish` writes it before the `finish` of an end with a cause: a `log`
/// body of one `stopped` line with the cause `declined` and no step, or the step `step`, under the request id of 16
/// bytes 0x53.
fn end_row(step: Option<u32>) -> String {
    let step = step.map_or_else(|| "null".to_owned(), |s| s.to_string());
    let line = r#"{"v":1,"workflow":"{workflow}","agent":"{agent}","run":"{run}","seq":1,"day":20768,"at":1794339000000000001,"event":"stopped","step":STEP,"cause":"declined","model":null,"ms":null,"tokens_in":null,"tokens_out":null,"page_amount":null}"#
        .replace("STEP", &step);
    common::ask_store(
        &common::id(0x53),
        &format!(r#"{{"op":"log","agent":"{{agent}}","run":"{{run}}","records":[{line}]}}"#),
    )
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
    assert_eq!(rig.store.count(r#""request":"runner""#), 0, "{name}");
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
fn a_stopped_line_that_names_a_step_after_bound_no_ends_the_run_unrelayed() {
    after_bound_no(
        "stopped-step",
        &end_row(Some(2)).replacen("ask ", "send ", 1),
    );
}

#[test]
fn after_bound_no_the_end_row_sent_again_after_a_store_fault_then_the_finish_and_done_end_the_run_declined(
) {
    let rig = Rig::new("bound-no-end");
    *rig.people.allow.lock().unwrap() = OriginAnswer::Decline;
    rig.store.fail_all.store(1, Ordering::SeqCst);
    let row = end_row(None);
    let finish = common::ask_finish();
    let script = rig.script(&[
        "hello",
        &sign_in(),
        &bind("http://pay.test:18090"),
        &row,
        &row,
        &finish,
        r#"send {"t":"done","outcome":"declined"}"#,
        "exit 1",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    let end = rig.rec.wait_end(&run, SHORT).expect("the run ended");
    assert_eq!(end.forced, None, "the run was lost: {}", script.log());
    assert_eq!(end.outcome, Some(Outcome::Declined));
    assert!(
        end.finish.is_some(),
        "the finish was not stored: {}",
        script.log()
    );
    let answer = |id: u8, outcome: &str| {
        format!(
            r#"{{"t":"store_reply","reply":{{"v":1,"id":"{}",{outcome}}}}}"#,
            common::id(id)
        )
    };
    let stores: Vec<String> = script
        .replies()
        .into_iter()
        .filter(|r| r.contains(r#""t":"store_reply""#))
        .collect();
    assert_eq!(
        stores,
        vec![
            answer(0x53, r#""error":"store""#),
            answer(0x53, r#""ok":{"stored":1}"#),
            answer(0x51, r#""ok":{"stored":1}"#),
        ],
        "{}",
        script.log()
    );
    let bodies = rig.store.bodies();
    assert!(
        bodies.len() >= 3
            && bodies[..2].iter().all(|b| b.contains(r#""op":"log""#))
            && bodies[2].contains(r#""op":"finish""#),
        "{bodies:?}"
    );
    assert_eq!(rig.store.count(r#""op":"log""#), 2);
    assert_eq!(rig.store.count(r#""op":"finish""#), 1);
    assert_eq!(binds(&script), vec![r#"{"t":"bind_reply","bound":"no"}"#]);
    assert!(adds(&rig).is_empty());
}

#[test]
fn a_session_size_row_stored_before_the_bind_does_not_count_against_the_end_after_bound_no() {
    let rig = Rig::new("size-row-before-bind");
    *rig.people.allow.lock().unwrap() = OriginAnswer::Decline;
    let size_row = end_row(None)
        .replace(r#""cause":"declined""#, r#""cause":"session_size""#)
        .replace(&common::id(0x53), &common::id(0x54));
    let row = end_row(None).replace(r#""seq":1"#, r#""seq":2"#);
    let finish = common::ask_finish();
    let script = rig.script(&[
        "hello",
        &size_row,
        &sign_in(),
        &bind("http://pay.test:18090"),
        &row,
        &finish,
        r#"send {"t":"done","outcome":"declined"}"#,
        "exit 1",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    signed_in(&rig, &run, 1);
    let end = rig.rec.wait_end(&run, SHORT).expect("the run ended");
    assert_eq!(binds(&script), vec![r#"{"t":"bind_reply","bound":"no"}"#]);
    assert_eq!(end.forced, None, "the run was lost: {}", script.log());
    assert_eq!(end.outcome, Some(Outcome::Declined));
    assert!(
        end.finish.is_some(),
        "the finish was not stored: {}",
        script.log()
    );
    assert_eq!(rig.store.count(r#""op":"log""#), 2);
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
        &end_row(None),
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
    assert_eq!(rig.store.count(r#""op":"log""#), 1);
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

/// A page of the list of checkpoints with one checkpoint, `made`, of the run `run`, and `next`.
fn page(run: &RunId, next: Option<&RunId>) -> String {
    let (run, sha) = (run.as_str(), "b".repeat(64));
    let next = next.map_or_else(|| "null".to_owned(), |r| format!("\"{}\"", r.as_str()));
    format!(
        r#"{{"checkpoints":[{{"workflow":"30303030303030303030303030303030","agent":"40404040404040404040404040404040","run":"{run}","folder":["/data/books"],"workbook":["/data/books/rent.xlsx"],"copy":["rent.halvo-checkpoint-{run}.xlsx"],"state":"made","sha256":"{sha}","bytes":18432,"at":1794339000000000001,"ended":"yes","effect":"committed","superseded":"no"}}],"next":{next}}}"#
    )
}

fn parsed(page: &str) -> CheckpointsReply {
    CheckpointsReply::parse(page.as_bytes()).unwrap()
}

#[test]
fn the_list_of_checkpoints_is_read_a_page_at_a_time_by_after_and_next() {
    let rig = Rig::new("list-of-checkpoints");
    let (first, second) = (common::run_id(), common::run_id());
    let (one, two) = (page(&first, Some(&first)), page(&second, None));
    *rig.store.pages.lock().unwrap() =
        vec![(None, one.clone()), (Some(first.clone()), two.clone())];
    let read = rig.sup.checkpoints(None);
    assert_eq!(read, Ok(parsed(&one)));
    let next = read.unwrap().next;
    assert_eq!(next.as_ref(), Some(&first));
    assert_eq!(rig.sup.checkpoints(next), Ok(parsed(&two)));
    let asked: Vec<String> = rig
        .store
        .bodies()
        .into_iter()
        .filter(|b| b.contains("list_checkpoints"))
        .collect();
    let body = |after: &str| {
        format!(r#"{{"op":"list_checkpoints","workflow":null,"agent":null,"after":{after}}}"#)
    };
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(asked[0].contains(&body("null")), "{}", asked[0]);
    let after = format!("\"{}\"", first.as_str());
    assert!(asked[1].contains(&body(&after)), "{}", asked[1]);
}

#[test]
fn a_list_reply_that_white_space_takes_past_reply_max_gives_no_page() {
    let rig = Rig::new("list-reply-past-max");
    let value = page(&common::run_id(), None);
    // The 53 bytes of the envelope beside a request id: `{"v":1,"id":"`, the id's 32 signs, `","ok":` and `}`. A
    // reply of `REPLY_MAX` bytes gives the page and one of a byte more does not, so both cases hold that count.
    let padded = |len: usize| format!("{}{value}", " ".repeat(len - 53 - value.len()));
    let answer = |text: String| *rig.store.pages.lock().unwrap() = vec![(None, text)];
    answer(padded(REPLY_MAX + 1));
    assert_eq!(rig.sup.checkpoints(None), Err(ListFailed));
    answer(padded(REPLY_MAX));
    assert_eq!(rig.sup.checkpoints(None), Ok(parsed(&value)));
    for (name, lever) in [
        ("wrong_id_coord", &rig.store.wrong_id_coord),
        ("wrong_form_coord", &rig.store.wrong_form_coord),
        ("fail_all", &rig.store.fail_all),
    ] {
        lever.store(1, Ordering::SeqCst);
        assert_eq!(rig.sup.checkpoints(None), Err(ListFailed), "{name}");
    }
}

// The removal of checkpoints (contracts.md A6.6, A11.3), on real folders under the rig's scratch directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The bytes of each workbook of these tests, as the person keeps it.
const BOOK: &[u8] = b"the workbook as the person keeps it";
/// The bytes of each checkpoint of a `made` row, as `prepare` read the workbook.
const COPY: &[u8] = b"the workbook as prepare read it";

/// The instant `days` days before now, in nanoseconds since 1970.
fn days_ago(days: i64) -> i64 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    i64::try_from(now.as_nanos()).unwrap() - days * 24 * 60 * 60 * 1_000_000_000
}

/// The folder `books` under the rig's scratch directory, in its canonical spelling, and the rig's data directory.
fn books(rig: &Rig) -> PathBuf {
    let folder = rig.dir.canonicalize().unwrap().join("books");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(rig.data()).unwrap();
    folder
}

/// The checkpoint of `run` beside `workbook`, under the name that halvo-checkpoint builds.
fn copy_of(workbook: &Path, run: &RunId) -> PathBuf {
    let run = halvo_checkpoint::RunId::parse(run.as_str()).unwrap();
    workbook.with_file_name(halvo_checkpoint::checkpoint_name(workbook, &run).unwrap())
}

/// The workbook `name` in `folder`, holding `BOOK`, with the checkpoint of `run` beside it holding `copy`.
fn with_copy(folder: &Path, name: &str, run: &RunId, copy: &[u8]) -> PathBuf {
    let workbook = folder.join(name);
    std::fs::write(&workbook, BOOK).unwrap();
    std::fs::write(copy_of(&workbook, run), copy).unwrap();
    workbook
}

fn absent(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_err()
}

/// Every entry under `dir` by its path below `dir`: a file with its bytes, a directory with none.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut found = BTreeMap::new();
    let mut open = vec![dir.to_path_buf()];
    while let Some(at) = open.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let path = entry.unwrap().path();
            let below = path.strip_prefix(dir).unwrap().to_path_buf();
            if std::fs::symlink_metadata(&path).unwrap().is_dir() {
                found.insert(below, None);
                open.push(path);
            } else {
                found.insert(below, Some(std::fs::read(&path).unwrap()));
            }
        }
    }
    found
}

/// A path as the list writes it, one piece of `own_text`.
fn own(path: &Path) -> String {
    let text = path.to_str().unwrap();
    format!(r#"["{}"]"#, text.replace('\\', r"\\").replace('"', r#"\""#))
}

/// One row of the list of checkpoints. `made` holds the copy's bytes of a `made` row and is none for a `planned` row.
struct Listed<'a> {
    run: &'a RunId,
    folder: &'a Path,
    workbook: &'a Path,
    made: Option<&'a [u8]>,
    at: i64,
    ended: bool,
    effect: Option<&'a str>,
    superseded: bool,
}

/// A `made` row of `copy`, made now, of a run whose end is stored, with no effect and not superseded.
fn row<'a>(run: &'a RunId, folder: &'a Path, workbook: &'a Path, copy: &'a [u8]) -> Listed<'a> {
    Listed {
        run,
        folder,
        workbook,
        made: Some(copy),
        at: days_ago(0),
        ended: true,
        effect: None,
        superseded: false,
    }
}

impl Listed<'_> {
    fn json(&self) -> String {
        let yes = |b: bool| if b { "yes" } else { "no" };
        let (state, sha256, bytes) = match self.made {
            Some(copy) => (
                "made",
                format!("\"{}\"", halvo_checkpoint::Sha256::of(copy).hex()),
                copy.len().to_string(),
            ),
            None => ("planned", "null".to_owned(), "null".to_owned()),
        };
        let effect = self
            .effect
            .map_or_else(|| "null".to_owned(), |e| format!("\"{e}\""));
        let copy = copy_of(self.workbook, self.run);
        format!(
            r#"{{"workflow":"30303030303030303030303030303030","agent":"40404040404040404040404040404040","run":"{}","folder":{},"workbook":{},"copy":{},"state":"{state}","sha256":{sha256},"bytes":{bytes},"at":{},"ended":"{}","effect":{effect},"superseded":"{}"}}"#,
            self.run.as_str(),
            own(self.folder),
            own(self.workbook),
            own(Path::new(copy.file_name().unwrap())),
            self.at,
            yes(self.ended),
            yes(self.superseded),
        )
    }
}

/// Gives the store one page that lists `rows`, in the order of their run ids.
fn list(rig: &Rig, rows: &[Listed<'_>]) {
    let mut rows: Vec<&Listed<'_>> = rows.iter().collect();
    rows.sort_by(|a, b| a.run.as_str().cmp(b.run.as_str()));
    let rows: Vec<String> = rows.iter().map(|r| r.json()).collect();
    let page = format!(r#"{{"checkpoints":[{}],"next":null}}"#, rows.join(","));
    *rig.store.pages.lock().unwrap() = vec![(None, page)];
}

/// The `mark_checkpoint` bodies that the store got, in order.
fn marks(rig: &Rig) -> Vec<String> {
    rig.store
        .bodies()
        .into_iter()
        .filter(|b| b.contains(r#""op":"mark_checkpoint""#))
        .collect()
}

/// The body of the mark of the checkpoint of `run` and `workbook` with `state` and the reason `left`.
fn marked(run: &RunId, workbook: &Path, state: &str, left: Option<&str>) -> String {
    let left = left.map_or_else(|| "null".to_owned(), |l| format!("\"{l}\""));
    format!(
        r#"{{"op":"mark_checkpoint","workflow":"30303030303030303030303030303030","agent":"40404040404040404040404040404040","run":"{}","workbook":{},"state":"{state}","left":{left}}}"#,
        run.as_str(),
        own(workbook)
    )
}

/// The store got exactly the marks `expected`, each once.
fn assert_marks(rig: &Rig, expected: &[String]) {
    let sent = marks(rig);
    assert_eq!(sent.len(), expected.len(), "{sent:#?}");
    for body in expected {
        let found = sent.iter().filter(|s| s.contains(body.as_str())).count();
        assert_eq!(found, 1, "{body} in {sent:#?}");
    }
}

#[test]
fn a_pass_removes_superseded_aged_and_unreplaced_copies() {
    let rig = Rig::new("pass-removes");
    let folder = books(&rig);
    let runs: Vec<RunId> = (0..4).map(|_| common::run_id()).collect();
    let names = [
        "superseded.xlsx",
        "aged.xlsx",
        "no-effect.xlsx",
        "not-made.xlsx",
    ];
    let wbs: Vec<PathBuf> = names
        .iter()
        .zip(&runs)
        .map(|(name, run)| with_copy(&folder, name, run, COPY))
        .collect();
    list(
        &rig,
        &[
            Listed {
                effect: Some("committed"),
                superseded: true,
                ..row(&runs[0], &folder, &wbs[0], COPY)
            },
            Listed {
                effect: Some("committed"),
                at: days_ago(31),
                ..row(&runs[1], &folder, &wbs[1], COPY)
            },
            row(&runs[2], &folder, &wbs[2], COPY),
            Listed {
                effect: Some("not_made"),
                ..row(&runs[3], &folder, &wbs[3], COPY)
            },
        ],
    );
    assert_eq!(rig.sup.remove_checkpoints(), Ok(()));
    let left: BTreeMap<PathBuf, Option<Vec<u8>>> = names
        .iter()
        .map(|name| (PathBuf::from(name), Some(BOOK.to_vec())))
        .collect();
    assert_eq!(
        tree(&folder),
        left,
        "each copy is removed, each workbook stays"
    );
    let gone: Vec<String> = wbs
        .iter()
        .zip(&runs)
        .map(|(wb, run)| marked(run, wb, "gone", None))
        .collect();
    assert_marks(&rig, &gone);
}

#[test]
fn a_pass_leaves_an_unknown_edits_copy_and_the_way_back() {
    let rig = Rig::new("pass-leaves");
    let folder = books(&rig);
    let runs: Vec<RunId> = (0..3).map(|_| common::run_id()).collect();
    let names = ["unknown.xlsx", "not-ended.xlsx", "young.xlsx"];
    let wbs: Vec<PathBuf> = names
        .iter()
        .zip(&runs)
        .map(|(name, run)| with_copy(&folder, name, run, COPY))
        .collect();
    list(
        &rig,
        &[
            Listed {
                effect: Some("unknown"),
                superseded: true,
                at: days_ago(31),
                ..row(&runs[0], &folder, &wbs[0], COPY)
            },
            Listed {
                ended: false,
                superseded: true,
                at: days_ago(31),
                ..row(&runs[1], &folder, &wbs[1], COPY)
            },
            Listed {
                effect: Some("committed"),
                ..row(&runs[2], &folder, &wbs[2], COPY)
            },
        ],
    );
    let before = tree(&folder);
    assert_eq!(rig.sup.remove_checkpoints(), Ok(()));
    assert_eq!(rig.store.count("list_checkpoints"), 1, "the list was read");
    assert_eq!(tree(&folder), before, "no file changed");
    assert_marks(&rig, &[]);
}

#[test]
fn a_pass_removes_the_named_copy_and_no_other_file() {
    let rig = Rig::new("pass-named");
    let folder = books(&rig);
    let (run, other, outside_run) = (common::run_id(), common::run_id(), common::run_id());
    let workbook = with_copy(&folder, "rent.xlsx", &run, COPY);
    // Another run's checkpoint of the same workbook, of the same bytes, which the list does not name; a file under the
    // copy's own name in a directory below the folder; and files of the person's own.
    std::fs::write(copy_of(&workbook, &other), COPY).unwrap();
    std::fs::create_dir(folder.join("sub")).unwrap();
    std::fs::write(copy_of(&folder.join("sub").join("rent.xlsx"), &run), COPY).unwrap();
    std::fs::write(folder.join("rent.xlsx.bak"), COPY).unwrap();
    std::fs::write(folder.join("notes.txt"), b"the person's notes").unwrap();
    // A row whose workbook lies outside its folder, with a checkpoint of the row's bytes beside that workbook.
    let away = rig.dir.canonicalize().unwrap().join("away");
    std::fs::create_dir(&away).unwrap();
    let outside = with_copy(&away, "rent.xlsx", &outside_run, COPY);
    list(
        &rig,
        &[
            row(&run, &folder, &workbook, COPY),
            row(&outside_run, &folder, &outside, COPY),
        ],
    );
    let (mut in_folder, in_away) = (tree(&folder), tree(&away));
    assert_eq!(rig.sup.remove_checkpoints(), Ok(()));
    let named = copy_of(&workbook, &run);
    let named = named.strip_prefix(&folder).unwrap();
    assert!(in_folder.remove(named).is_some());
    assert_eq!(tree(&folder), in_folder, "only the named copy is removed");
    assert_eq!(tree(&away), in_away, "nothing outside the folder changed");
    assert_marks(
        &rig,
        &[
            marked(&run, &workbook, "gone", None),
            marked(&outside_run, &outside, "left", Some("not_file")),
        ],
    );
}

#[test]
fn a_changed_copy_stays_and_is_marked_left() {
    let rig = Rig::new("pass-changed");
    let folder = books(&rig);
    let finish = common::ask_finish();
    let lines: [&str; 4] = [
        "hello",
        &finish,
        r#"send {"t":"done","outcome":"completed"}"#,
        "exit 0",
    ];
    // A run whose pass finds no list.
    let script = rig.script(&lines);
    let spec = rig.spec(&script);
    let first = spec.run.clone();
    rig.sup.start(spec).unwrap();
    let without = rig
        .rec
        .wait_end(&first, SHORT)
        .expect("the first run ended");
    assert!(wait_until(SHORT, || rig.store.count("list_checkpoints") == 1));
    // A `made` copy that the person changed to the workbook's own bytes, which a check against the workbook would
    // take; and a copy whose own name holds nothing, with a directory under the name that the removal moves a copy to.
    let (changed_run, aside_run) = (common::run_id(), common::run_id());
    let changed = with_copy(&folder, "changed.xlsx", &changed_run, BOOK);
    let aside = folder.join("aside.xlsx");
    std::fs::write(&aside, BOOK).unwrap();
    let copy = copy_of(&aside, &aside_run);
    let moved = copy.with_file_name(halvo_checkpoint::removing_name(copy.file_name().unwrap()));
    std::fs::create_dir(&moved).unwrap();
    std::fs::write(moved.join("inside.xlsx"), COPY).unwrap();
    list(
        &rig,
        &[
            row(&changed_run, &folder, &changed, COPY),
            row(&aside_run, &folder, &aside, COPY),
        ],
    );
    let before = tree(&folder);
    let script = rig.script(&lines);
    let spec = rig.spec(&script);
    let second = spec.run.clone();
    rig.sup.start(spec).unwrap();
    let with = rig
        .rec
        .wait_end(&second, SHORT)
        .expect("the second run ended");
    assert!(
        wait_until(SHORT, || marks(&rig).len() == 2),
        "{:#?}",
        marks(&rig)
    );
    assert_eq!(tree(&folder), before, "no file changed");
    assert_marks(
        &rig,
        &[
            marked(&changed_run, &changed, "left", Some("changed")),
            marked(&aside_run, &aside, "left", Some("not_file")),
        ],
    );
    let facts = |end: &RunEnd| {
        (
            end.outcome,
            end.end_run,
            end.exit,
            end.forced,
            end.finish.is_some(),
        )
    };
    assert_eq!(facts(&with), facts(&without));
    assert_eq!(with.outcome, Some(Outcome::Completed));
    assert!(with.finish.is_some());
}

#[test]
fn a_planned_copy_equal_to_its_workbook_goes() {
    let rig = Rig::new("pass-planned");
    let folder = books(&rig);
    let run = common::run_id();
    let workbook = with_copy(&folder, "rent.xlsx", &run, BOOK);
    list(
        &rig,
        &[Listed {
            made: None,
            ..row(&run, &folder, &workbook, BOOK)
        }],
    );
    assert_eq!(rig.sup.remove_checkpoints(), Ok(()));
    assert!(absent(&copy_of(&workbook, &run)));
    assert_eq!(std::fs::read(&workbook).unwrap(), BOOK);
    assert_marks(&rig, &[marked(&run, &workbook, "gone", None)]);
}

#[test]
fn an_absent_folder_writes_no_row_and_is_asked_again() {
    let rig = Rig::new("pass-absent-folder");
    let folder = books(&rig);
    let run = common::run_id();
    let workbook = with_copy(&folder, "rent.xlsx", &run, COPY);
    list(&rig, &[row(&run, &folder, &workbook, COPY)]);
    let away = folder.with_file_name("books-moved");
    std::fs::rename(&folder, &away).unwrap();
    let before = tree(&away);
    assert_eq!(rig.sup.remove_checkpoints(), Ok(()));
    assert!(absent(&folder), "the pass made no folder");
    assert_eq!(tree(&away), before, "no file changed");
    assert_marks(&rig, &[]);
    std::fs::rename(&away, &folder).unwrap();
    assert_eq!(rig.sup.remove_checkpoints(), Ok(()));
    assert!(absent(&copy_of(&workbook, &run)));
    assert_marks(&rig, &[marked(&run, &workbook, "gone", None)]);
}

#[test]
fn a_removal_whose_mark_failed_is_marked_gone_later() {
    let rig = Rig::new("pass-mark-failed");
    let folder = books(&rig);
    let run = common::run_id();
    let workbook = with_copy(&folder, "rent.xlsx", &run, COPY);
    list(&rig, &[row(&run, &folder, &workbook, COPY)]);
    rig.store.fail_mark.store(1, Ordering::SeqCst);
    assert_eq!(rig.sup.remove_checkpoints(), Ok(()));
    assert!(absent(&copy_of(&workbook, &run)));
    assert_eq!(
        rig.store.fail_mark.load(Ordering::SeqCst),
        0,
        "the mark met the cut link"
    );
    let gone = marked(&run, &workbook, "gone", None);
    assert_marks(&rig, std::slice::from_ref(&gone));
    assert_eq!(rig.sup.remove_checkpoints(), Ok(()));
    let sent = marks(&rig);
    assert_eq!(sent.len(), 2, "{sent:#?}");
    assert!(sent.iter().all(|s| s.contains(&gone)), "{sent:#?}");
    let left = BTreeMap::from([(PathBuf::from("rent.xlsx"), Some(BOOK.to_vec()))]);
    assert_eq!(tree(&folder), left);
}
