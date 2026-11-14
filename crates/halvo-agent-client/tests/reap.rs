mod common;

use std::fs;
use std::num::NonZeroU32;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use common::{gone, line_gone, wait_until, Rig};
use halvo_agent_client::marks::{first_line, marks_dir, proc_line, Group, ProcLine};
use halvo_agent_client::{process, Forced, ForcedPath, Reaped};
use halvo_schema::executable::text::{CreationTime, RunId};

const SHORT: Duration = Duration::from_secs(10);

struct Sleeper(Child, std::path::PathBuf);

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
        common::end_logged(&self.1);
    }
}

fn sleeper(role: &str, own_group: bool) -> (Sleeper, CreationTime) {
    let dir = common::scratch("sleeper");
    let mut cmd = Command::new(common::peer());
    cmd.arg(role)
        .arg(dir.join("sleeper.log"))
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
    (Sleeper(child, dir), created.unwrap())
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

#[cfg(target_os = "macos")]
#[test]
fn a_line_whose_process_is_no_longer_in_the_line_group_gets_no_signal_to_that_group() {
    let rig = Rig::new("reap-line-left-group");
    let (s, s_created) = sleeper("browser", true);
    let (b, b_created) = sleeper("browser", true);
    let (sp, bp) = (s.0.id(), b.0.id());
    let run = common::run_id();
    write_mark(
        &rig,
        &run,
        &[
            ("runner", line(sp, later(s_created, 1), sp)),
            ("browser", line(bp, b_created, sp)),
        ],
    );
    let answers = rig.sup.reap().unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].answer, Reaped::Gone);
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        process::is_running(sp, s_created),
        "the group of another process was signalled"
    );
    assert!(process::is_running(bp, b_created));
    drop((s, b));
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
fn the_reap_of_an_earlier_life_answers_what_it_ended_with_the_path_reap_and_nothing_for_a_mark_with_nothing_left(
) {
    let rig = Rig::new("reap-forced");
    let (r, r_created) = sleeper("browser", true);
    let (b, b_created) = sleeper("browser", true);
    let (rp, bp) = (r.0.id(), b.0.id());
    let with_browser = common::run_id();
    let runner = line(rp, r_created, rp);
    let browser = line(bp, b_created, bp);
    write_mark(
        &rig,
        &with_browser,
        &[("runner", runner), ("browser", browser)],
    );
    let (q, q_created) = sleeper("browser", true);
    let qp = q.0.id();
    let runner_alone = common::run_id();
    write_mark(&rig, &runner_alone, &[("runner", line(qp, q_created, qp))]);
    let (g, g_created) = sleeper("browser", true);
    let gp = g.0.id();
    let gone_already = common::run_id();
    write_mark(
        &rig,
        &gone_already,
        &[("runner", line(gp, later(g_created, 1), gp))],
    );
    let incomplete = common::run_id();
    write_mark(&rig, &incomplete, &[]);
    let answers = rig.sup.reap().unwrap();
    let forced = |run: &RunId| {
        answers
            .iter()
            .find(|a| &a.run == run)
            .map(|a| (a.answer, a.forced))
    };
    let reap = |browser| {
        Some(Forced {
            path: ForcedPath::Reap,
            browser,
        })
    };
    assert_eq!(forced(&with_browser), Some((Reaped::Gone, reap(true))));
    assert_eq!(forced(&runner_alone), Some((Reaped::Gone, reap(false))));
    assert_eq!(forced(&gone_already), Some((Reaped::Gone, None)));
    assert_eq!(forced(&incomplete), Some((Reaped::Gone, None)));
    assert!(wait_until(SHORT, || line_gone(&runner) && line_gone(&browser)));
    assert!(process::is_running(gp, g_created));
    drop((r, b, q, g));
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
    let script = rig.script(&[
        "hello",
        r#"ask {"t":"secret","ref":"fixture_token"}"#,
        "exit 1",
    ]);
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

#[test]
fn a_browser_and_a_child_of_a_runner_that_exits_by_itself_are_ended_with_the_run() {
    for placement in ["browser child", "browser in-runner-group"] {
        let rig = Rig::new("runner-exits");
        let script = rig.script(&["hello", "child", placement, "log ready", "exit 1"]);
        let spec = rig.spec(&script);
        let run = spec.run.clone();
        rig.sup.start(spec).unwrap();
        assert!(rig.rec.wait_end(&run, Duration::from_secs(30)).is_some());
        let mut left = script.logged("browser");
        left.extend(script.logged("child"));
        assert!(script.wait_log("ready", SHORT), "{}", script.log());
        left.extend(script.logged("browser-child"));
        assert!(left.len() >= 2, "{}", script.log());
        let all_gone = wait_until(SHORT, || {
            left.iter().all(|(pid, created)| gone(*pid, *created))
        });
        for (pid, created) in &left {
            if !gone(*pid, *created) {
                common::kill_pid(*pid);
            }
        }
        assert!(
            all_gone,
            "{placement}: a process of the run outlived its end"
        );
    }
}

#[test]
fn reap_answers_gone_for_a_run_of_this_life_whose_runner_is_gone_while_its_end_run_waits() {
    let rig = Rig::new("reap-held-end-run");
    *rig.store.hold_end_run.lock().unwrap() = true;
    let script = rig.script(&[
        "hello",
        r#"ask {"t":"secret","ref":"fixture_token"}"#,
        "exit 1",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    let runner = rig.mark(&run).unwrap().runner.unwrap();
    assert!(wait_until(SHORT, || line_gone(&runner)));
    assert!(wait_until(SHORT, || rig.store.count(r#""op":"end_run""#) >= 1));
    let answers = rig.sup.reap().unwrap();
    *rig.store.hold_end_run.lock().unwrap() = false;
    assert!(rig.rec.wait_end(&run, Duration::from_secs(30)).is_some());
    let mine: Vec<_> = answers.iter().filter(|a| a.run == run).collect();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].answer, Reaped::Gone);
    assert!(rig.mark(&run).is_none());
}

#[test]
fn a_browser_whose_frame_is_refused_is_ended_with_its_run() {
    let mut cases = vec!["browser created=wrong child", "browser group=other child"];
    if cfg!(target_os = "macos") {
        cases.push("browser group=null child");
        cases.push("browser group=runner-claim child");
    }
    for case in cases {
        let rig = Rig::new("browser-refused-own-group");
        let script = rig.script(&["hello", "on-eof hang", case, "idle"]);
        let spec = rig.spec(&script);
        let run = spec.run.clone();
        rig.sup.start(spec).unwrap();
        assert!(
            rig.rec.wait_end(&run, Duration::from_secs(30)).is_some(),
            "{case}"
        );
        assert!(
            !script.log().contains("browser_reply"),
            "{case}: {}",
            script.log()
        );
        let mut made = script.logged("browser-real");
        made.extend(script.logged("browser-child"));
        assert!(!made.is_empty(), "{case}: {}", script.log());
        assert!(
            wait_until(SHORT, || made.iter().all(|(p, c)| gone(*p, *c))),
            "{case}: a browser whose frame was refused outlived its run"
        );
    }
}

#[test]
fn a_second_browser_that_leads_its_own_group_is_ended_with_its_run() {
    let rig = Rig::new("browser-twice-own-group");
    let script = rig.script(&["hello", "on-eof hang", "browser", "browser child", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(rig.rec.wait_end(&run, Duration::from_secs(30)).is_some());
    let mut made = script.logged("browser-real");
    made.extend(script.logged("browser-child"));
    assert!(made.len() >= 2, "{}", script.log());
    assert!(wait_until(SHORT, || made.iter().all(|(p, c)| gone(*p, *c))));
}

#[test]
fn a_runner_that_exits_while_a_child_holds_its_output_is_seen_gone_and_its_child_ended() {
    let rig = Rig::new("exit-child-holds-output");
    let script = rig.script(&[
        "hello",
        "child-on-stdout",
        "log ready",
        r#"ask {"t":"secret","ref":"fixture_token"}"#,
        "exit 1",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("ready", SHORT), "{}", script.log());
    let child = script.logged("child");
    assert_eq!(child.len(), 1, "{}", script.log());
    assert!(
        rig.rec.wait_end(&run, SHORT).is_some(),
        "the run's end was not seen"
    );
    assert!(wait_until(SHORT, || gone(child[0].0, child[0].1)));
}

#[cfg(target_os = "macos")]
#[test]
fn a_browser_frame_whose_group_neither_the_browser_nor_the_runner_leads_is_refused_and_both_are_ended(
) {
    let rig = Rig::new("browser-third-group");
    let script = rig.script(&["hello", "on-eof hang", "browser in-group-of-child", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(rig.rec.wait_end(&run, Duration::from_secs(30)).is_some());
    assert!(!script.log().contains("browser_reply"), "{}", script.log());
    let mut made = script.logged("browser-real");
    made.extend(script.logged("child"));
    assert_eq!(made.len(), 2, "{}", script.log());
    assert!(wait_until(SHORT, || made.iter().all(|(p, c)| gone(*p, *c))));
}

fn profiles(rig: &Rig) -> std::path::PathBuf {
    rig.data().join("agents").join("profiles")
}

fn link_dir(target: &std::path::Path, link: &std::path::Path) {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(target, link);
    made.unwrap_or_else(|e| panic!("the test could not make its link: {e}"));
}

/// A profile directory of a run with a file, a folder and the temporary directory beside the profile in it.
fn profile_tree(dir: &std::path::Path) {
    fs::create_dir_all(dir.join("profile").join("Default")).unwrap();
    fs::create_dir_all(dir.join("tmp")).unwrap();
    fs::write(dir.join("profile").join("Default").join("Cookies"), b"x").unwrap();
    fs::write(dir.join("tmp").join("scratch"), b"x").unwrap();
}

#[test]
fn reap_removes_the_profile_directory_of_each_run_that_no_mark_of_a_live_run_names_and_nothing_else(
) {
    let rig = Rig::new("reap-profiles");
    let script = rig.script(&["hello", "idle"]);
    let spec = rig.spec(&script);
    let live = spec.run.clone();
    rig.sup.start(spec).unwrap();
    let outside = rig.dir.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep"), b"x").unwrap();
    let root = profiles(&rig);
    let left = common::run_id();
    let with_link = common::run_id();
    let link_entry = common::run_id();
    let file_entry = common::run_id();
    profile_tree(&root.join(live.as_str()));
    profile_tree(&root.join(left.as_str()));
    profile_tree(&root.join(with_link.as_str()));
    link_dir(
        &outside,
        &root.join(with_link.as_str()).join("tmp").join("link"),
    );
    link_dir(&outside, &root.join(link_entry.as_str()));
    fs::write(root.join(file_entry.as_str()), b"x").unwrap();
    profile_tree(&root.join("not-a-run-id"));
    profile_tree(&root.join(live.as_str().to_uppercase()));
    let answers = rig.sup.reap().unwrap();
    assert!(answers
        .iter()
        .any(|a| a.run == live && a.answer == Reaped::Alive));
    assert!(!root.join(left.as_str()).exists(), "the left profile stays");
    assert!(
        !root.join(with_link.as_str()).exists(),
        "the profile with a link in it stays"
    );
    assert!(
        outside.join("keep").exists(),
        "the removal went through a link"
    );
    assert!(
        root.join(live.as_str())
            .join("profile")
            .join("Default")
            .join("Cookies")
            .exists(),
        "the profile of the live run was removed"
    );
    assert!(
        fs::symlink_metadata(root.join(link_entry.as_str())).is_ok(),
        "an entry that is a link was removed"
    );
    assert!(
        root.join(file_entry.as_str()).is_file(),
        "a file entry was removed"
    );
    assert!(root
        .join("not-a-run-id")
        .join("tmp")
        .join("scratch")
        .exists());
    assert!(root
        .join(live.as_str().to_uppercase())
        .join("tmp")
        .join("scratch")
        .exists());
    rig.sup.kill(&live).unwrap();
}

#[test]
fn reap_removes_nothing_through_a_profiles_folder_or_an_agents_folder_that_is_a_link() {
    for linked in ["profiles", "agents"] {
        let rig = Rig::new("reap-profiles-link");
        let outside = rig.dir.join("outside");
        let run = common::run_id();
        let target = if linked == "profiles" {
            outside.clone()
        } else {
            outside.join("agents-target")
        };
        let held = if linked == "profiles" {
            outside.join(run.as_str())
        } else {
            target.join("profiles").join(run.as_str())
        };
        profile_tree(&held);
        let agents = rig.data().join("agents");
        if linked == "profiles" {
            fs::create_dir_all(&agents).unwrap();
            link_dir(&target, &agents.join("profiles"));
        } else {
            fs::create_dir_all(rig.data()).unwrap();
            link_dir(&target, &agents);
        }
        rig.sup.reap().unwrap();
        assert!(
            held.join("tmp").join("scratch").exists(),
            "{linked}: a profile was removed through a link"
        );
    }
}
