#![cfg(target_os = "macos")]
#![deny(clippy::undocumented_unsafe_blocks)]

mod common;

use std::time::Duration;

use common::{gone, line_gone, wait_until, Rig};
use halvo_agent_client::marks::Group;

const SHORT: Duration = Duration::from_secs(10);

extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
}

const SIGCHLD: i32 = 20;
const SIG_IGN: usize = 1;
const SIG_DFL: usize = 0;

/// Alone in its test binary, because it makes the system reap every child of the process.
#[test]
fn a_runner_that_the_system_reaped_gets_no_signal_to_its_group() {
    // SAFETY: signal sets SIGCHLD to SIG_IGN for this whole process, which is alone in its binary, so the system
    // reaps every child of it; no handler is installed, so no code runs on the signal.
    unsafe {
        signal(SIGCHLD, SIG_IGN);
    }
    let rig = Rig::new("sigchld-ignored");
    *rig.store.hold_end_run.lock().unwrap() = true;
    let script = rig.script(&[
        "hello",
        r#"ask {"t":"secret","ref":"fixture_token"}"#,
        "child",
        "exit 1",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    let started = rig.sup.start(spec);
    let runner = rig.mark(&run).and_then(|m| m.runner);
    *rig.store.hold_end_run.lock().unwrap() = false;
    let child_seen = wait_until(SHORT, || !script.logged("child").is_empty());
    let child = script.logged("child").first().copied();
    let runner_gone = runner.is_some_and(|r| wait_until(SHORT, || line_gone(&r)));
    let ended = rig.rec.wait_end(&run, Duration::from_secs(60)).is_some();
    let child_alive_after_end = child.is_some_and(|(p, c)| !gone(p, c));
    // SAFETY: signal sets SIGCHLD back to SIG_DFL for this whole process, which is alone in its binary; no handler
    // is installed, so no code runs on the signal.
    unsafe {
        signal(SIGCHLD, SIG_DFL);
    }
    if let Some((p, c)) = child {
        if !gone(p, c) {
            common::kill_pid(p);
        }
    }
    assert!(started.is_ok(), "{started:?}");
    assert!(runner.is_some_and(|r| r.group == Group::Leader(r.pid)));
    assert!(child_seen && runner_gone, "{}", script.log());
    assert!(ended);
    assert!(
        child_alive_after_end,
        "the group of a runner whose number the system freed was signalled"
    );
}
