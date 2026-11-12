mod common;

use std::fs;
use std::num::NonZeroU32;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use common::{gone, line_gone, wait_until, Rig};
use halvo_agent_client::marks::{first_line, marks_dir, proc_line, Group, ProcLine};
use halvo_agent_client::{process, Reaped};
use halvo_schema::executable::text::{CreationTime, RunId};

const SHORT: Duration = Duration::from_secs(10);

struct Sleeper(Child);

impl Drop for Sleeper {
    fn drop(&mut self) {
        let pid = self.0.id();
        if cfg!(unix) && process::process_group(pid).map(NonZeroU32::get) == Some(pid) {
            let _ = Command::new("/bin/kill")
                .args(["-KILL", &format!("-{pid}")])
                .status();
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn sleeper(role: &str, own_group: bool) -> (Sleeper, CreationTime) {
    let mut cmd = Command::new(common::peer());
    cmd.arg(role)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    if own_group {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    let _ = own_group;
    let child = cmd.spawn().unwrap();
    let pid = child.id();
    let mut created = None;
    assert!(wait_until(SHORT, || {
        created = process::creation_time(pid);
        created.is_some()
    }));
    (Sleeper(child), created.unwrap())
}

fn line(pid: u32, created: CreationTime, group: u32) -> ProcLine {
    ProcLine {
        pid: NonZeroU32::new(pid).unwrap(),
        created,
        group: if cfg!(windows) {
            Group::Job
        } else {
            Group::Leader(NonZeroU32::new(group).unwrap())
        },
    }
}

fn later(c: CreationTime, by: u64) -> CreationTime {
    CreationTime::from_micros(c.micros().checked_add(by).unwrap())
}

fn write_mark(rig: &Rig, run: &RunId, lines: &[(&str, ProcLine)]) {
    let dir = marks_dir(&rig.data());
    fs::create_dir_all(&dir).unwrap();
    let mut text = first_line(run);
    for (word, l) in lines {
        text.push_str(&proc_line(word, l));
    }
    fs::write(dir.join(run.as_str()), text).unwrap();
}

#[test]
fn reap_answers_gone_for_a_run_whose_mark_is_not_complete_and_removes_it() {
    let rig = Rig::new("reap-incomplete");
    let run = common::run_id();
    write_mark(&rig, &run, &[]);
    let answers = rig.sup.reap().unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].run, run);
    assert_eq!(answers[0].answer, Reaped::Gone);
    assert!(rig.mark(&run).is_none());
}

#[cfg(target_os = "macos")]
#[test]
fn reap_answers_alive_for_a_runner_that_lives_and_cannot_be_ended_and_keeps_its_mark() {
    let rig = Rig::new("reap-alive");
    let (p, created) = sleeper("child", false);
    let pid = p.0.id();
    assert_ne!(process::process_group(pid).map(NonZeroU32::get), Some(pid));
    let run = common::run_id();
    write_mark(&rig, &run, &[("runner", line(pid, created, pid))]);
    let answers = rig.sup.reap().unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].answer, Reaped::Alive);
    assert!(process::is_running(pid, created));
    assert!(rig.mark(&run).is_some_and(|m| m.is_complete()));
}

#[cfg(windows)]
#[test]
fn reap_answers_alive_for_a_runner_that_lives_and_cannot_be_ended_and_keeps_its_mark() {
    let rig = Rig::new("reap-alive");
    let system = 4;
    let created = process::creation_time(system).expect("the System process has a creation time");
    let run = common::run_id();
    write_mark(&rig, &run, &[("runner", line(system, created, system))]);
    let answers = rig.sup.reap().unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].answer, Reaped::Alive);
    assert!(process::is_running(system, created));
    assert!(rig.mark(&run).is_some_and(|m| m.is_complete()));
}

#[test]
fn a_group_whose_leader_number_another_process_has_taken_gets_no_signal() {
    let rig = Rig::new("reap-reused");
    let role = if cfg!(windows) {
        "browser"
    } else {
        "browser-with-child"
    };
    let (p, created) = sleeper(role, true);
    let pid = p.0.id();
    let run = common::run_id();
    write_mark(
        &rig,
        &run,
        &[
            ("runner", line(pid, later(created, 1), pid)),
            ("browser", line(pid, later(created, 2), pid)),
        ],
    );
    let answers = rig.sup.reap().unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].answer, Reaped::Gone);
    std::thread::sleep(Duration::from_millis(300));
    assert!(process::is_running(pid, created));
    if cfg!(target_os = "macos") {
        assert!(answers[0].left.iter().any(|l| l.get() == pid));
    }
    assert!(rig.mark(&run).is_none());
}

#[test]
fn reap_ends_a_runner_of_an_earlier_life_and_its_browser_through_their_lines() {
    let rig = Rig::new("reap-ends");
    let role = if cfg!(windows) {
        "browser"
    } else {
        "browser-with-child"
    };
    let (r, r_created) = sleeper(role, true);
    let (b, b_created) = sleeper(role, true);
    let (rp, bp) = (r.0.id(), b.0.id());
    let run = common::run_id();
    let runner = line(rp, r_created, rp);
    let browser = line(bp, b_created, bp);
    write_mark(&rig, &run, &[("runner", runner), ("browser", browser)]);
    let answers = rig.sup.reap().unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].answer, Reaped::Gone);
    assert!(answers[0].left.is_empty());
    assert!(wait_until(SHORT, || line_gone(&runner) && line_gone(&browser)));
    if cfg!(target_os = "macos") {
        assert!(wait_until(SHORT, || process::process_group(rp).is_none()
            && process::process_group(bp).is_none()));
    }
    assert!(rig.mark(&run).is_none());
    drop((r, b));
}

#[test]
fn reap_leaves_a_run_that_this_life_started_alive_and_running() {
    let rig = Rig::new("reap-own");
    let script = rig.script(&["hello", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    let runner = rig.mark(&run).unwrap().runner.unwrap();
    let answers = rig.sup.reap().unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].answer, Reaped::Alive);
    assert!(!line_gone(&runner));
    rig.sup.kill(&run).unwrap();
}

#[test]
fn a_browser_frame_puts_the_browser_line_in_the_mark_before_browser_reply_and_kill_ends_its_group()
{
    let rig = Rig::new("browser-line");
    let script = rig.script(&["hello", "on-eof hang", "browser child", "log after", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("after", SHORT), "{}", script.log());
    assert!(
        script.log().contains(r#"reply {"t":"browser_reply"}"#),
        "{}",
        script.log()
    );
    let mark = rig.mark(&run).unwrap();
    let browser = mark.browser.expect("the mark holds the browser line");
    let logged = script.logged("browser");
    assert_eq!(logged, vec![(browser.pid.get(), browser.created)]);
    if cfg!(windows) {
        assert_eq!(browser.group, Group::Job);
    } else {
        assert_eq!(browser.group, Group::Leader(browser.pid));
    }
    assert!(script.wait_log("browser-child", SHORT), "{}", script.log());
    let child = script.logged("browser-child");
    assert_eq!(child.len(), 1);
    rig.sup.kill(&run).unwrap();
    assert!(line_gone(&mark.runner.unwrap()));
    assert!(wait_until(SHORT, || line_gone(&browser)
        && gone(child[0].0, child[0].1)));
}

#[test]
fn a_browser_frame_that_names_no_process_of_the_run_ends_the_run_with_no_browser_line() {
    for case in [
        "browser in-runner-group created=wrong",
        "browser in-runner-group group=other",
        "browser in-runner-group group=null",
    ] {
        if cfg!(windows) && case.ends_with("group=null") {
            continue;
        }
        let rig = Rig::new("browser-foreign");
        let script = rig.script(&["hello", "on-eof hang", case, "idle"]);
        let spec = rig.spec(&script);
        let run = spec.run.clone();
        rig.sup.start(spec).unwrap();
        let runner = rig.mark(&run).unwrap().runner.unwrap();
        let end = rig.rec.wait_end(&run, Duration::from_secs(30)).expect(case);
        assert_eq!(end.outcome, None, "{case}");
        assert!(line_gone(&runner), "{case}");
        assert!(
            !script.log().contains("browser_reply"),
            "{case}: {}",
            script.log()
        );
        let logged = script.logged("browser");
        assert_eq!(logged.len(), 1, "{case}: {}", script.log());
        assert!(wait_until(SHORT, || logged
            .iter()
            .all(|(pid, created)| gone(*pid, *created))));
    }
}

#[test]
fn a_second_browser_frame_ends_the_run() {
    let rig = Rig::new("browser-twice");
    let script = rig.script(&[
        "hello",
        "on-eof hang",
        "browser",
        "browser in-runner-group",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    let mark = rig.rec.wait_end(&run, Duration::from_secs(30));
    assert!(mark.is_some());
    assert_eq!(
        script.log().matches("browser_reply").count(),
        1,
        "{}",
        script.log()
    );
    let logged = script.logged("browser");
    assert_eq!(logged.len(), 2);
    assert!(wait_until(SHORT, || gone(logged[0].0, logged[0].1)));
}

#[test]
fn a_browser_frame_that_names_a_process_outside_the_run_is_refused_and_the_process_is_left_alone() {
    let rig = Rig::new("browser-outside");
    let (p, created) = sleeper("browser", true);
    let pid = p.0.id();
    let script = rig.script(&[
        "hello",
        "on-eof hang",
        &format!("browser-foreign {pid}"),
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    let end = rig.rec.wait_end(&run, Duration::from_secs(30));
    assert!(end.is_some());
    assert!(!script.log().contains("browser_reply"), "{}", script.log());
    std::thread::sleep(Duration::from_millis(300));
    assert!(process::is_running(pid, created));
}

#[cfg(target_os = "macos")]
#[test]
fn a_browser_frame_that_claims_the_runner_group_for_a_browser_that_leads_its_own_is_refused() {
    let rig = Rig::new("browser-claim");
    let script = rig.script(&["hello", "on-eof hang", "browser group=runner-claim", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    let end = rig.rec.wait_end(&run, Duration::from_secs(30));
    assert!(end.is_some());
    assert!(!script.log().contains("browser_reply"), "{}", script.log());
    for (pid, created) in script.logged("browser") {
        if process::is_running(pid, created) {
            common::kill_pid(pid);
        }
    }
}

#[test]
fn reap_answers_gone_for_an_ended_run_of_this_life_whose_end_run_was_not_acknowledged() {
    let rig = Rig::new("reap-own-ended");
    rig.store
        .fail_all
        .store(1, std::sync::atomic::Ordering::SeqCst);
    let script = rig.script(&["hello", "exit 1"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(rig.rec.wait_end(&run, Duration::from_secs(30)).is_some());
    assert!(rig.mark(&run).is_some());
    let answers = rig.sup.reap().unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].answer, Reaped::Gone);
    assert!(rig.mark(&run).is_none());
}
