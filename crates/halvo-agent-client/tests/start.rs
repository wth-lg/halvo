mod common;

use std::time::Duration;

use common::{gone, line_gone, wait_until, Rig};
use halvo_agent_client::marks::Group;
use halvo_agent_client::{process, CallError, EndRun};
use halvo_schema::executable::frame::HandbackResult;
use halvo_schema::executable::names::{HandbackOutcome, Outcome};
use halvo_schema::executable::text::V1;

const SHORT: Duration = Duration::from_secs(10);

#[test]
fn a_start_that_answers_ok_has_a_complete_mark_and_a_runner_of_its_own_group_or_job() {
    let rig = Rig::new("start-ok");
    let script = rig.script(&["hello", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    let mark = rig.mark(&run).expect("a started run has a mark");
    assert!(mark.is_complete());
    let runner = mark.runner.unwrap();
    assert!(process::is_running(runner.pid.get(), runner.created));
    if cfg!(windows) {
        assert_eq!(runner.group, Group::Job);
    } else {
        assert_eq!(runner.group, Group::Leader(runner.pid));
        assert_eq!(process::process_group(runner.pid.get()), Some(runner.pid));
    }
    assert!(script.wait_log("args 10", SHORT), "{}", script.log());
    assert_eq!(
        rig.rec.starts.lock().unwrap().as_slice(),
        std::slice::from_ref(&run)
    );
    rig.sup.kill(&run).unwrap();
    assert!(line_gone(&runner));
}

#[test]
fn a_runner_that_cannot_be_made_answers_not_started_and_leaves_no_mark() {
    let rig = Rig::with(
        "start-not-made",
        common::scratch("nothing").join("no-such-runner"),
        SHORT,
    );
    let script = rig.script(&["hello", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    assert_eq!(rig.sup.start(spec), Err(CallError::NotStarted));
    assert!(rig.mark(&run).is_none());
    let end = rig.rec.wait_end(&run, SHORT).expect("the end is reported");
    assert_eq!(end.end_run, EndRun::NoRunner);
    assert_eq!(rig.store.count("end_run"), 0);
    assert_eq!(rig.sup.kill(&run), Ok(()));
    assert_eq!(rig.sup.cancel(&run), Err(CallError::Ended));
}

#[test]
fn a_runner_that_ends_before_the_start_answers_gives_not_started_and_leaves_no_process() {
    let rig = Rig::new("start-ends");
    let script = rig.script(&["child", "sleep 200", "exit 3"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    assert_eq!(rig.sup.start(spec), Err(CallError::NotStarted));
    assert_runner_gone(&script);
    let children = script.logged("child");
    assert_eq!(children.len(), 1, "{}", script.log());
    assert!(children.iter().all(|(pid, created)| gone(*pid, *created)));
    let end = rig.rec.wait_end(&run, SHORT).expect("the end is reported");
    assert_eq!(end.end_run, EndRun::Acknowledged);
    assert_eq!(rig.store.count(r#""op":"end_run""#), 1);
    assert!(rig.mark(&run).is_none());
}

#[test]
fn a_runner_that_ends_with_no_stored_finish_gets_end_run_and_its_mark_goes_at_the_acknowledgement()
{
    let rig = Rig::new("end-run");
    let script = rig.script(&[
        "hello",
        r#"send {"t":"progress","step":1,"of":2}"#,
        r#"send {"t":"done","outcome":"failed"}"#,
        "exit 1",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec.clone()).unwrap();
    let end = rig.rec.wait_end(&run, SHORT).expect("the end is reported");
    assert_eq!(end.end_run, EndRun::Acknowledged);
    assert_eq!(end.outcome, Some(Outcome::Failed));
    assert_eq!(end.finish, None);
    let sent: Vec<String> = rig
        .store
        .bodies()
        .into_iter()
        .filter(|b| b.contains(r#""op":"end_run""#))
        .collect();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].contains(r#""request":"coordinator_write""#));
    assert!(sent[0].contains(&format!(
        r#""op":"end_run","workflow":"{}","agent":"{}","run":"{}""#,
        spec.workflow, spec.agent, spec.run
    )));
    assert!(rig.mark(&run).is_none());
    assert_eq!(rig.people.progress.lock().unwrap().as_slice(), &[(1, 2)]);
    assert_eq!(rig.rec.ends.lock().unwrap().len(), 1);
}

#[test]
fn an_end_run_that_is_not_acknowledged_leaves_the_mark() {
    let rig = Rig::new("end-run-lost");
    rig.store
        .fail_all
        .store(1, std::sync::atomic::Ordering::SeqCst);
    let script = rig.script(&["hello", "exit 1"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    let end = rig.rec.wait_end(&run, SHORT).expect("the end is reported");
    assert_eq!(end.end_run, EndRun::NotAcknowledged);
    assert_eq!(rig.store.count(r#""op":"end_run""#), 1);
    assert!(rig.mark(&run).is_some_and(|m| m.is_complete()));
}

#[test]
fn a_second_hello_ends_the_run_as_one_that_lost_its_way() {
    let rig = Rig::new("lost-way");
    let script = rig.script(&[
        "hello",
        "on-eof hang",
        r#"send {"t":"hello","run":"{run}","agent":"{agent}","v":1}"#,
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    let end = rig.rec.wait_end(&run, SHORT).expect("the run is ended");
    assert_eq!(end.end_run, EndRun::Acknowledged);
    assert_runner_gone(&script);
}

fn assert_runner_gone(script: &common::Script) {
    assert!(script.wait_log("runner pid=", SHORT), "{}", script.log());
    let runner = script.logged("runner");
    assert_eq!(runner.len(), 1, "{}", script.log());
    assert!(gone(runner[0].0, runner[0].1));
}

#[test]
fn a_runner_that_does_not_answer_within_the_wait_is_ended_before_not_started() {
    let rig = Rig::with("start-silent", common::peer(), Duration::from_millis(1500));
    let script = rig.script(&["child", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    assert_eq!(rig.sup.start(spec), Err(CallError::NotStarted));
    assert_runner_gone(&script);
    let children = script.logged("child");
    assert_eq!(children.len(), 1, "{}", script.log());
    assert!(children.iter().all(|(pid, created)| gone(*pid, *created)));
    assert!(rig.rec.wait_end(&run, SHORT).is_some());
}

#[test]
fn a_hello_of_another_run_is_no_start() {
    let rig = Rig::new("start-other-run");
    let script = rig.script(&["hello-other", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    assert_eq!(rig.sup.start(spec), Err(CallError::NotStarted));
    assert_runner_gone(&script);
    assert!(rig.rec.wait_end(&run, SHORT).is_some());
}

#[test]
fn a_second_start_of_a_run_id_is_refused_with_started_and_makes_no_runner() {
    let rig = Rig::new("start-twice");
    let script = rig.script(&["hello", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec.clone()).unwrap();
    let before =
        std::fs::read_to_string(rig.data().join("agents/marks").join(run.as_str())).unwrap();
    assert_eq!(rig.sup.start(spec), Err(CallError::Started));
    let after =
        std::fs::read_to_string(rig.data().join("agents/marks").join(run.as_str())).unwrap();
    assert_eq!(before, after);
    assert_eq!(script.log().matches("args ").count(), 1, "{}", script.log());
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_start_of_a_run_whose_mark_of_an_earlier_life_stands_is_refused_with_started() {
    let rig = Rig::new("start-old-mark");
    let script = rig.script(&["hello", "idle"]);
    let spec = rig.spec(&script);
    let dir = rig.data().join("agents/marks");
    std::fs::create_dir_all(&dir).unwrap();
    let first = format!("mark run={}\n", spec.run);
    std::fs::write(dir.join(spec.run.as_str()), &first).unwrap();
    assert_eq!(rig.sup.start(spec.clone()), Err(CallError::Started));
    assert_eq!(
        std::fs::read_to_string(dir.join(spec.run.as_str())).unwrap(),
        first
    );
    assert!(!script.wait_log("args", Duration::from_millis(300)));
}

#[test]
fn calls_on_a_run_id_never_started_in_this_life_are_unknown_run() {
    let rig = Rig::new("unknown-run");
    let run = common::run_id();
    assert_eq!(rig.sup.cancel(&run), Err(CallError::UnknownRun));
    assert_eq!(rig.sup.kill(&run), Err(CallError::UnknownRun));
    let result = HandbackResult {
        v: V1,
        outcome: HandbackOutcome::Done,
        secret: None,
    };
    assert_eq!(
        rig.sup.answer_handback(&run, result),
        Err(CallError::UnknownRun)
    );
}

#[test]
fn kill_ends_the_runner_and_answers_success_again_after_it_ended_while_cancel_is_ended() {
    let rig = Rig::new("kill-ended");
    let script = rig.script(&["hello", "on-eof hang", "child", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    let runner = rig.mark(&run).unwrap().runner.unwrap();
    assert!(script.wait_log("child pid=", SHORT));
    rig.sup.cancel(&run).unwrap();
    assert!(script.wait_log("cancel", SHORT), "{}", script.log());
    rig.sup.kill(&run).unwrap();
    assert!(line_gone(&runner));
    let children = script.logged("child");
    assert!(wait_until(SHORT, || children
        .iter()
        .all(|(pid, created)| gone(*pid, *created))));
    assert!(rig.rec.wait_end(&run, Duration::from_secs(30)).is_some());
    assert_eq!(rig.sup.kill(&run), Ok(()));
    assert_eq!(rig.sup.cancel(&run), Err(CallError::Ended));
}

#[cfg(windows)]
#[test]
fn no_process_of_the_job_breaks_away() {
    let rig = Rig::new("breakaway");
    let script = rig.script(&["hello", "breakaway", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("breakaway ", SHORT), "{}", script.log());
    assert!(
        script.log().contains("breakaway refused"),
        "{}",
        script.log()
    );
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_browser_frame_before_hello_is_answered_and_its_browser_is_ended_by_a_start_that_fails() {
    let rig = Rig::with(
        "browser-before-hello",
        common::peer(),
        Duration::from_millis(1500),
    );
    let script = rig.script(&["browser child", "idle"]);
    let spec = rig.spec(&script);
    assert_eq!(rig.sup.start(spec), Err(CallError::NotStarted));
    assert!(
        script.log().contains(r#"reply {"t":"browser_reply"}"#),
        "{}",
        script.log()
    );
    let browser = script.logged("browser");
    assert_eq!(browser.len(), 1, "{}", script.log());
    assert!(script.wait_log("browser-child", SHORT), "{}", script.log());
    let child = script.logged("browser-child");
    assert!(gone(browser[0].0, browser[0].1));
    assert!(wait_until(SHORT, || gone(child[0].0, child[0].1)));

    let ok = rig.script(&["browser", "hello", "idle"]);
    let spec = rig.spec(&ok);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    let mark = rig.mark(&run).unwrap();
    assert!(mark.browser.is_some());
    rig.sup.kill(&run).unwrap();
    assert!(line_gone(&mark.browser.unwrap()));
}

#[test]
fn a_kill_while_the_start_runs_answers_success_only_for_a_runner_that_is_then_gone() {
    for _ in 0..5 {
        let rig = Rig::new("kill-during-start");
        let script = rig.script(&["hello", "idle"]);
        let spec = rig.spec(&script);
        let run = spec.run.clone();
        let sup = rig.sup.clone();
        let starter = std::thread::spawn(move || sup.start(spec));
        loop {
            match rig.sup.kill(&run) {
                Ok(()) => break,
                Err(CallError::UnknownRun) => std::hint::spin_loop(),
                Err(e) => panic!("kill answered {e:?}"),
            }
        }
        let started = starter.join().unwrap();
        assert!(
            rig.rec.wait_end(&run, Duration::from_secs(30)).is_some(),
            "kill answered success while the start ran, the start answered {started:?}, and the run did not end"
        );
        let runner = script.logged("runner");
        assert!(
            runner.iter().all(|(pid, created)| gone(*pid, *created)),
            "kill answered success while the start ran, the start answered {started:?}, and the runner lives"
        );
    }
}

#[test]
fn a_refused_answer_handback_during_the_start_does_not_end_the_start() {
    let rig = Rig::new("handback-during-start");
    let script = rig.script(&["sleep 3000", "hello", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    let sup = rig.sup.clone();
    let starter = std::thread::spawn(move || sup.start(spec));
    assert!(script.wait_log("runner pid=", SHORT), "{}", script.log());
    std::thread::sleep(Duration::from_millis(700));
    let result = HandbackResult {
        v: V1,
        outcome: HandbackOutcome::Declined,
        secret: None,
    };
    let answered = rig.sup.answer_handback(&run, result);
    let started = starter.join().unwrap();
    assert!(answered.is_err(), "{answered:?}");
    assert_eq!(
        started,
        Ok(()),
        "the refused call {answered:?} ended the start"
    );
    let runner = rig.mark(&run).unwrap().runner.unwrap();
    assert!(!line_gone(&runner));
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_hello_of_another_agent_is_no_start() {
    let rig = Rig::new("start-other-agent");
    let script = rig.script(&["hello-other-agent", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    assert_eq!(rig.sup.start(spec), Err(CallError::NotStarted));
    assert_runner_gone(&script);
    assert!(rig.rec.wait_end(&run, SHORT).is_some());
}

#[test]
fn a_frame_that_cannot_be_read_ends_the_run_and_its_runner() {
    for bad in ["garbage", "oversize"] {
        let rig = Rig::new("broken-frame");
        let script = rig.script(&["hello", "on-eof hang", "child", bad, "log sent", "idle"]);
        let spec = rig.spec(&script);
        let run = spec.run.clone();
        rig.sup.start(spec).unwrap();
        assert!(script.wait_log("sent", SHORT), "{}", script.log());
        let end = rig.rec.wait_end(&run, SHORT);
        assert!(end.is_some(), "{bad}: the run did not end");
        assert_runner_gone(&script);
        let child = script.logged("child");
        assert!(
            wait_until(SHORT, || child.iter().all(|(p, c)| gone(*p, *c))),
            "{bad}"
        );
    }
}

#[test]
fn a_runner_that_closes_its_output_gets_link_end_to_end_by_itself_before_the_library_ends_it() {
    let link_end = Duration::from_secs(4);
    let rig = Rig::configured("link-end", |c| c.link_end = link_end);
    let script = rig.script(&["hello", "on-eof hang", "close-stdout", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("stdout-closed", SHORT), "{}", script.log());
    let closed = std::time::Instant::now();
    assert!(script.wait_log("eof", SHORT), "{}", script.log());
    let runner = script.logged("runner");
    std::thread::sleep(Duration::from_millis(1500));
    assert!(
        !gone(runner[0].0, runner[0].1),
        "the library ended a runner whose output closed before LINK_END passed"
    );
    assert!(rig.rec.wait_end(&run, SHORT + link_end).is_some());
    assert!(gone(runner[0].0, runner[0].1));
    assert!(closed.elapsed() >= link_end - Duration::from_millis(1600));
}
