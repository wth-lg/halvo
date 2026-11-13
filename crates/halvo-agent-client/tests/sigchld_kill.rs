#![cfg(target_os = "macos")]

mod common;

use std::time::Duration;

use common::{gone, line_gone, wait_until, Rig};

const SHORT: Duration = Duration::from_secs(10);

extern "C" {
    fn signal(sig: i32, handler: usize) -> usize;
}

const SIGCHLD: i32 = 20;
const SIG_IGN: usize = 1;
const SIG_DFL: usize = 0;

const CONFIRM: &str = r#"send {"t":"confirm","request":{"v":1,"run":"{run}","agent":"{agent}","cut":null,"effects":[{"effect":"60606060606060606060606060606060","class":"payment","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","bound":{"class":"payment","fields":[{"name":"payee","from":"slot","origin":null,"value":["Northwind Power"]},{"name":"amount","from":"extract","origin":"http://utility.test:18090","value":["84.20"]},{"name":"instrument","from":"slot","origin":null,"value":["card 4242"]},{"name":"origin","from":"const","origin":null,"value":["http://pay.test:18090"]}]},"new_origin":null}]}}"#;

/// Alone in its test binary, because it makes the system reap every child of the process. While the run's thread
/// waits on the person, the runner exits and the system reaps it; a kill then must signal no group by the freed
/// number, so the child that the runner left in its group lives on and is reported, not ended.
#[test]
fn a_kill_after_the_system_reaped_the_runner_signals_no_group() {
    unsafe {
        signal(SIGCHLD, SIG_IGN);
    }
    let rig = Rig::new("sigchld-kill");
    *rig.people.hold_answers.lock().unwrap() = true;
    let script = rig.script(&["hello", "child-on-stdout", CONFIRM, "log asked", "exit 1"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    let started = rig.sup.start(spec);
    let runner = rig.mark(&run).and_then(|m| m.runner);
    let asked = wait_until(SHORT, || {
        rig.people
            .asked_confirms
            .load(std::sync::atomic::Ordering::SeqCst)
            > 0
    });
    let runner_gone = runner.is_some_and(|r| wait_until(SHORT, || line_gone(&r)));
    let killed = rig.sup.kill(&run);
    std::thread::sleep(Duration::from_millis(300));
    let child = script.logged("child").first().copied();
    let child_alive = child.is_some_and(|(p, c)| !gone(p, c));
    *rig.people.hold_answers.lock().unwrap() = false;
    let ended = rig.rec.wait_end(&run, Duration::from_secs(30)).is_some();
    unsafe {
        signal(SIGCHLD, SIG_DFL);
    }
    if let Some((p, c)) = child {
        if !gone(p, c) {
            common::kill_pid(p);
        }
    }
    assert!(started.is_ok(), "{started:?}");
    assert!(asked && runner_gone, "{}", script.log());
    assert_eq!(killed, Ok(()));
    assert!(ended);
    assert!(
        child_alive,
        "a kill signalled the group of a runner whose number the system freed"
    );
}
