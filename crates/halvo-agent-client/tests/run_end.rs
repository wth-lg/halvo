mod common;

use std::time::Duration;

use common::{ask_finish, Rig};
use halvo_agent_client::{Forced, ForcedPath, RunEnd, RunnerExit};

const SHORT: Duration = Duration::from_secs(10);

const SHORT_LINK_END: Duration = Duration::from_secs(4);

/// Starts `lines`, in which `wait-for GO` waits for the start's answer, kills the run after the log line
/// `kill_after` when one is named, and waits for the run's end.
fn ended(rig: &Rig, lines: &[&str], kill_after: Option<&str>) -> RunEnd {
    let go = rig.dir.join(format!("go-{}", common::run_id()));
    let wait = format!("wait-for {}", go.display());
    let lines: Vec<&str> = lines
        .iter()
        .map(|l| {
            if *l == "wait-for GO" {
                wait.as_str()
            } else {
                l
            }
        })
        .collect();
    let script = rig.script(&lines);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    std::fs::write(&go, b"").unwrap();
    if let Some(line) = kill_after {
        assert!(script.wait_log(line, SHORT), "{}", script.log());
        assert_eq!(rig.sup.kill(&run), Ok(()));
    }
    rig.rec
        .wait_end(&run, 2 * SHORT_LINK_END + SHORT)
        .unwrap_or_else(|| panic!("no end was reported: {}", script.log()))
}

fn forced(path: ForcedPath, browser: bool) -> Option<Forced> {
    Some(Forced { path, browser })
}

#[test]
fn a_runner_that_exits_with_1_by_itself_and_one_that_the_library_kills_end_with_two_different_values(
) {
    let rig = Rig::new("end-exit-1");
    let own = ended(&rig, &["hello", "wait-for GO", "exit 1"], None);
    assert_eq!(own.exit, Some(RunnerExit::Status(1)), "its own exit");
    assert_eq!(own.forced, None, "nothing was ended by force");
    let killed = ended(&rig, &["hello", "log ready", "idle"], Some("ready"));
    let by_force = if cfg!(windows) {
        RunnerExit::Status(1)
    } else {
        RunnerExit::Signal(9)
    };
    assert_eq!(killed.exit, Some(by_force), "the exit of a killed runner");
    assert_eq!(killed.forced, forced(ForcedPath::Kill, false));
    assert_ne!(
        (own.exit, own.forced),
        (killed.exit, killed.forced),
        "the two ends read the same"
    );
}

#[test]
fn a_kill_of_a_run_with_a_browser_ends_with_the_path_kill_and_the_browser() {
    let rig = Rig::new("end-kill-browser");
    let end = ended(
        &rig,
        &["hello", "on-eof hang", "browser", "log browsed", "idle"],
        Some("browsed"),
    );
    assert_eq!(end.forced, forced(ForcedPath::Kill, true));
}

#[test]
fn a_runner_that_outlives_its_stored_finish_ends_with_after_finish_and_with_the_browser() {
    let rig = Rig::configured("end-after-finish", |c| c.link_end = SHORT_LINK_END);
    let finish = ask_finish();
    let end = ended(
        &rig,
        &[
            "hello",
            "on-eof hang",
            "browser",
            finish.as_str(),
            "log finished",
            "idle",
        ],
        None,
    );
    assert!(end.finish.is_some(), "the finish was stored");
    assert_eq!(end.forced, forced(ForcedPath::AfterFinish, true));
    let by_force = if cfg!(windows) {
        RunnerExit::Status(1)
    } else {
        RunnerExit::Signal(9)
    };
    assert_eq!(end.exit, Some(by_force));
}

#[test]
fn a_broken_frame_ends_the_run_by_force_with_the_path_link_end() {
    let rig = Rig::new("end-broken");
    let end = ended(&rig, &["hello", "on-eof hang", "garbage", "idle"], None);
    assert_eq!(end.forced, forced(ForcedPath::LinkEnd, false));
}
