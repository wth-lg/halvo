#![deny(clippy::undocumented_unsafe_blocks)]

mod common;

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use common::{wait_until, Script};

const SHORT: Duration = Duration::from_secs(15);

struct Host {
    child: Child,
    dir: PathBuf,
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        common::end_logged(&self.dir);
    }
}

fn captured(mut stream: impl Read + Send + 'static) -> Arc<Mutex<Vec<u8>>> {
    let bytes = Arc::new(Mutex::new(Vec::new()));
    let b = Arc::clone(&bytes);
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
            b.lock().unwrap().extend_from_slice(&buf[..n]);
        }
    });
    bytes
}

fn holds(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

/// Starts the library in a process of its own on piped streams, runs `probes` in its runner, and returns what the
/// runner logged for each probe, the bytes of the host's two streams and of SCRIPT.mark, once the run has ended.
fn probe(name: &str, probes: &[&str], marker: &str) -> (Vec<String>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let dir = common::scratch(name);
    let mut lines = vec!["hello".to_owned()];
    lines.extend(
        probes
            .iter()
            .map(|p| format!("write-inherited {p} {marker}")),
    );
    lines.extend(["log probed".to_owned(), "exit 0".to_owned()]);
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let script = Script::new(&dir, &refs);
    let mut child = Command::new(common::peer())
        .arg("host")
        .arg(dir.join("data"))
        .arg(&script.path)
        .arg(dir.join("owner.log"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = captured(child.stdout.take().unwrap());
    let err = captured(child.stderr.take().unwrap());
    let mut host = Host {
        child,
        dir: dir.clone(),
    };
    let ended = || {
        String::from_utf8_lossy(&out.lock().unwrap())
            .lines()
            .any(|l| l.starts_with("end "))
    };
    assert!(
        wait_until(SHORT, || script.log().contains("probed") && ended()),
        "the runner probed and the host reported the run's end: {}; the host printed {:?}",
        script.log(),
        String::from_utf8_lossy(&out.lock().unwrap())
    );
    host.child.kill().unwrap();
    host.child.wait().unwrap();
    let results = script
        .log()
        .lines()
        .filter_map(|l| l.strip_prefix("inherited "))
        .map(|r| r.rsplit(' ').next().unwrap_or("").to_owned())
        .collect();
    let mark = fs::read(Path::new(&format!("{}.mark", script.path.display()))).unwrap_or_default();
    thread::sleep(Duration::from_millis(200));
    let out = out.lock().unwrap().clone();
    let err = err.lock().unwrap().clone();
    (results, out, err, mark)
}

#[cfg(target_os = "macos")]
#[test]
fn a_descriptor_that_the_library_holds_open_without_close_on_exec_is_not_open_in_the_runner() {
    let marker = format!("inherited-{}", common::run_id());
    let (results, out, err, mark) = probe("inherited-fd", &["{host-mark}"], &marker);
    assert_eq!(
        results,
        vec!["absent"],
        "the runner's write through the host's descriptor"
    );
    assert!(
        !holds(&mark, marker.as_bytes()),
        "the marker reached the file of the host's descriptor ({} bytes)",
        mark.len()
    );
    assert!(
        !holds(&out, marker.as_bytes()),
        "the marker reached the host's standard output"
    );
    assert!(
        !holds(&err, marker.as_bytes()),
        "the marker reached the host's standard error"
    );
}

#[cfg(windows)]
#[test]
fn a_handle_that_the_library_made_inheritable_and_its_own_standard_output_and_error_are_not_open_in_the_runner(
) {
    let marker = format!("inherited-{}", common::run_id());
    let (results, out, err, mark) = probe(
        "inherited-handle",
        &["{host-mark}", "{host-out}", "{host-err}"],
        &marker,
    );
    assert_eq!(
        results,
        vec!["absent", "absent", "absent"],
        "the runner's writes through the host's inheritable handle, its standard output and its standard error"
    );
    assert!(
        !holds(&mark, marker.as_bytes()),
        "the marker reached the file of the host's inheritable handle ({} bytes)",
        mark.len()
    );
    assert!(
        !holds(&out, marker.as_bytes()),
        "the marker reached the host's standard output"
    );
    assert!(
        !holds(&err, marker.as_bytes()),
        "the marker reached the host's standard error"
    );
}

fn logged_value(log: &str, key: &str) -> String {
    log.lines()
        .find_map(|l| l.strip_prefix(key))
        .unwrap_or("<none>")
        .to_owned()
}

#[cfg(windows)]
fn test_process_has_a_console() -> bool {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
    }
    let mut list = [0u32; 4];
    // SAFETY: `list` is live and writable for the count of entries passed, and the call writes at most that many.
    unsafe { GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32) > 0 }
}

#[cfg(windows)]
fn console_lines(case: &str, detached: bool) -> (String, String) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let own = test_process_has_a_console();
    let flags = if detached {
        DETACHED_PROCESS
    } else if own {
        0
    } else {
        CREATE_NEW_CONSOLE
    };
    eprintln!(
        "u49-console {case}: the test process has a console: {}; the host's creation flags: {flags:#x}",
        if own { "yes" } else { "no" }
    );
    let dir = common::scratch(&format!("console-{case}"));
    let script = Script::new(
        &dir,
        &["hello", "console {host-pid}", "log probed", "exit 0"],
    );
    let mut cmd = Command::new(common::peer());
    cmd.arg("host")
        .arg(dir.join("data"))
        .arg(&script.path)
        .arg(dir.join("owner.log"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(flags);
    let mut child = cmd.spawn().unwrap();
    let out = captured(child.stdout.take().unwrap());
    let _err = captured(child.stderr.take().unwrap());
    let host = Host {
        child,
        dir: dir.clone(),
    };
    assert!(
        wait_until(SHORT, || script.log().contains("probed")),
        "the runner probed its console: {}; the host printed {:?}",
        script.log(),
        String::from_utf8_lossy(&out.lock().unwrap())
    );
    drop(host);
    let runner = logged_value(&script.log(), "console ");
    let child = logged_value(&script.log(), "console-child ");
    eprintln!("u49-console {case}: the runner's console: {runner}");
    eprintln!("u49-console {case}: its child's console: {child}");
    (runner, child)
}

#[cfg(windows)]
fn assert_the_child_shares_the_runners_console(child: &str) {
    for word in [" runner=yes ", " host=no ", " window=none"] {
        assert!(
            child.contains(word),
            "the runner's console child reports {child}, not {word}"
        );
    }
}

#[cfg(windows)]
#[test]
fn a_windows_runner_has_a_console_of_its_own_that_the_library_is_not_attached_to_and_that_shows_no_window(
) {
    let (runner, child) = console_lines("shared", false);
    assert!(
        runner.contains(" host=no "),
        "the library's process is attached to the runner's console: {runner}"
    );
    assert!(
        !runner.contains("window=visible"),
        "the runner's console shows a window: {runner}"
    );
    assert_the_child_shares_the_runners_console(&child);
}

#[cfg(windows)]
#[test]
fn a_windows_runner_of_a_library_with_no_console_has_a_console_with_no_window_that_its_child_shares(
) {
    let (runner, child) = console_lines("detached", true);
    assert!(
        runner.contains(" host=no "),
        "the library's process is attached to the runner's console: {runner}"
    );
    assert!(
        runner.ends_with(" window=none"),
        "the runner's console has a window: {runner}"
    );
    assert_the_child_shares_the_runners_console(&child);
}

#[test]
fn two_runners_that_the_library_starts_at_the_same_moment_from_two_threads_hold_no_end_of_each_others_link(
) {
    use std::sync::Barrier;
    let rigs = [common::Rig::new("pairs-a"), common::Rig::new("pairs-b")];
    for pair in 0..30 {
        let barrier = Barrier::new(2);
        let runs: Vec<_> = thread::scope(|s| {
            let handles: Vec<_> = rigs
                .iter()
                .map(|rig| {
                    let barrier = &barrier;
                    s.spawn(move || {
                        let script = rig.script(&["hello", "pipes", "log probed", "idle"]);
                        let spec = rig.spec(&script);
                        let run = spec.run.clone();
                        barrier.wait();
                        let started = rig.sup.start(spec);
                        (run, script, started)
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for ((run, script, started), rig) in runs.iter().zip(&rigs) {
            assert_eq!(*started, Ok(()), "pair {pair}: the start");
            assert!(
                script.wait_log("probed", SHORT),
                "pair {pair}: the runner probed: {}",
                script.log()
            );
            assert_eq!(
                logged_value(&script.log(), "pipes beyond="),
                "0",
                "pair {pair}: a runner holds a pipe beyond its own standard input and output"
            );
            rig.sup.kill(run).unwrap();
        }
    }
}

struct Holder {
    #[cfg(windows)]
    child: Child,
}

impl Drop for Holder {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn hold_the_link(script: &Script, go: &Path) -> Holder {
    #[cfg(target_os = "macos")]
    {
        assert!(
            script.wait_log("holder made=yes", SHORT),
            "the holder was made: {}",
            script.log()
        );
        assert_eq!(script.logged("child").len(), 1, "the holder is logged");
        fs::write(go, b"").unwrap();
        Holder {}
    }
    #[cfg(windows)]
    {
        assert!(
            script.wait_log("own-handles ", SHORT),
            "the runner logged its handles: {}",
            script.log()
        );
        let line = logged_value(&script.log(), "own-handles ");
        let field = |key: &str| {
            line.split(' ')
                .find_map(|p| p.strip_prefix(key))
                .unwrap_or("0")
                .to_owned()
        };
        let child = Command::new(common::peer())
            .arg("hold")
            .arg(field("pid="))
            .arg(field("in="))
            .arg(field("out="))
            .arg(format!("{}.log", script.path.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let holder = Holder { child };
        assert!(
            script.wait_log("holder holds=2", SHORT),
            "the holder copied both ends of the runner's link: {}",
            script.log()
        );
        fs::write(go, b"").unwrap();
        holder
    }
}

fn held_run(
    name: &str,
    after: &[&str],
) -> (
    common::Rig,
    halvo_schema::executable::text::RunId,
    Script,
    Holder,
) {
    let rig = common::Rig::new(name);
    let go = rig.dir.join("go");
    let wait = format!("wait-for {}", go.display());
    let mut lines = vec!["hello", "holder", "on-eof hang", wait.as_str(), "log held"];
    lines.extend_from_slice(after);
    let script = rig.script(&lines);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    assert_eq!(rig.sup.start(spec), Ok(()), "the start");
    let holder = hold_the_link(&script, &go);
    assert!(script.wait_log("held", SHORT), "{}", script.log());
    (rig, run, script, holder)
}

fn runner_gone(script: &Script) -> bool {
    let runner = script.logged("runner");
    runner.len() == 1 && wait_until(SHORT, || common::gone(runner[0].0, runner[0].1))
}

impl Holder {
    fn lives(&mut self, script: &Script) -> bool {
        #[cfg(target_os = "macos")]
        {
            script
                .logged("child")
                .first()
                .is_some_and(|(pid, created)| !common::gone(*pid, *created))
        }
        #[cfg(windows)]
        {
            let _ = script;
            self.child.try_wait().is_ok_and(|s| s.is_none())
        }
    }
}

#[test]
fn a_kill_ends_a_runner_whose_link_ends_a_helper_also_holds_within_link_end_and_returns() {
    let (rig, run, script, mut holder) = held_run("held-kill", &["idle"]);
    let began = std::time::Instant::now();
    assert_eq!(rig.sup.kill(&run), Ok(()), "the kill");
    assert!(
        began.elapsed() < halvo_agent_client::LINK_END,
        "the kill returned after {:?}",
        began.elapsed()
    );
    assert!(runner_gone(&script), "the runner is gone: {}", script.log());
    assert!(
        rig.rec
            .wait_end(&run, halvo_agent_client::LINK_END)
            .is_some(),
        "the library reported the run's end within LINK_END"
    );
    assert!(
        holder.lives(&script),
        "the helper that holds the link still lives after the run ended"
    );
}

#[test]
fn a_run_that_the_library_ends_for_a_broken_frame_ends_its_runner_within_link_end_while_a_helper_holds_its_link(
) {
    let (rig, run, script, mut holder) = held_run("held-broken", &["garbage", "idle"]);
    assert!(
        rig.rec
            .wait_end(&run, halvo_agent_client::LINK_END)
            .is_some(),
        "the library reported the run's end within LINK_END: {}",
        script.log()
    );
    assert!(runner_gone(&script), "the runner is gone: {}", script.log());
    assert!(
        holder.lives(&script),
        "the helper that holds the link still lives after the run ended"
    );
}

#[test]
fn a_runner_that_exits_while_a_helper_holds_its_output_end_is_seen_as_ended() {
    let (rig, run, script, mut holder) = held_run("held-exit", &["exit 3"]);
    assert!(
        rig.rec
            .wait_end(&run, halvo_agent_client::LINK_END)
            .is_some(),
        "the library reported the run's end within LINK_END: {}",
        script.log()
    );
    assert!(runner_gone(&script), "the runner is gone: {}", script.log());
    assert!(
        holder.lives(&script),
        "the helper that holds the link still lives after the run ended"
    );
}

#[cfg(target_os = "macos")]
fn host_run(name: &str, lines: &[&str]) -> String {
    let dir = common::scratch(name);
    let script = Script::new(&dir, lines);
    let mut child = Command::new(common::peer())
        .arg("host")
        .arg(dir.join("data"))
        .arg(&script.path)
        .arg(dir.join("owner.log"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let out = captured(child.stdout.take().unwrap());
    let mut host = Host {
        child,
        dir: dir.clone(),
    };
    let ended = || {
        String::from_utf8_lossy(&out.lock().unwrap())
            .lines()
            .any(|l| l.starts_with("end "))
    };
    let done = wait_until(SHORT, || script.log().contains("\nprobed") && ended());
    let log = script.log();
    let printed = String::from_utf8_lossy(&out.lock().unwrap()).into_owned();
    host.child.kill().unwrap();
    host.child.wait().unwrap();
    assert!(
        done,
        "the runner probed and the host reported the run's end: {log}; the host printed {printed:?}"
    );
    log
}

#[cfg(target_os = "macos")]
#[test]
fn a_runner_holds_its_three_descriptors_and_no_other_while_the_library_holds_descriptors_of_each_kind_at_low_and_high_numbers(
) {
    let log = host_run(
        "held-kinds",
        &[
            "hello",
            "log {host-held}",
            "descriptors",
            "log probed",
            "exit 0",
        ],
    );
    let held = logged_value(&log, "held ");
    eprintln!("u49-held: {held}");
    for place in [
        " 3:file",
        " 101:pipe-read",
        " 102:directory",
        " 103:kqueue",
        " 104:socket",
        " 256:pipe-write",
    ] {
        assert!(
            held.contains(place),
            "the host holds {place} without close-on-exec: {held}"
        );
    }
    let high = held
        .rsplit(' ')
        .next()
        .and_then(|p| p.strip_suffix(":socket"))
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(0);
    assert!(
        high >= 257,
        "the host holds a socket end at a high number: {held}"
    );
    assert_eq!(
        logged_value(&log, "descriptors first="),
        "0,1,2",
        "the descriptors that stand open in the runner as its first act"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_library_that_holds_150_more_descriptors_still_starts_its_runner_which_holds_its_three() {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    extern "C" {
        fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    }
    const F_DUPFD_CLOEXEC: i32 = 67;
    let rig = common::Rig::new("many-fds");
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(rig.dir.join("held"))
        .unwrap();
    let mut held = Vec::new();
    for _ in 0..150 {
        // SAFETY: fcntl takes the number of `file`, which is open for the whole loop, and returns a new number.
        let fd = unsafe { fcntl(file.as_raw_fd(), F_DUPFD_CLOEXEC, 3) };
        assert!(fd > 2, "a duplicate descriptor: {fd}");
        // SAFETY: `fd` is a new descriptor of this process that nothing else owns.
        held.push(unsafe { OwnedFd::from_raw_fd(fd) });
    }
    let script = rig.script(&["hello", "descriptors", "log probed", "idle"]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    let started = rig.sup.start(spec);
    let probed = started.is_ok() && script.wait_log("probed", SHORT);
    let log = script.log();
    let _ = rig.sup.kill(&run);
    drop(held);
    assert_eq!(
        started,
        Ok(()),
        "the start while the library holds 150 more descriptors"
    );
    assert!(probed, "the runner probed: {log}");
    assert_eq!(
        logged_value(&log, "descriptors first="),
        "0,1,2",
        "the descriptors that stand open in the runner as its first act"
    );
}

#[test]
fn a_holder_that_the_test_starts_with_a_bound_of_one_second_exits_by_itself_with_the_status_0() {
    let dir = common::scratch("holder-bound");
    let log = dir.join("holder.log");
    fs::write(&log, b"").unwrap();
    let mut cmd = Command::new(common::peer());
    if cfg!(windows) {
        cmd.arg("hold")
            .arg("0")
            .arg("0")
            .arg("0")
            .arg(&log)
            .arg("1");
    } else {
        cmd.arg("holder").arg("1");
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let ended = wait_until(Duration::from_secs(10), || {
        child.try_wait().is_ok_and(|s| s.is_some())
    });
    if !ended {
        let _ = child.kill();
        let _ = child.wait();
    }
    assert!(ended, "the holder did not exit by itself within 10 s");
    assert_eq!(
        child.wait().unwrap().code(),
        Some(0),
        "the holder's exit status"
    );
}

const RUN_RECORD: &str = r#"{"v":1,"workflow":"{workflow}","agent":"{agent}","run":"{run}","day":20768,"mode":"replay","outcome":"completed","playbook":1,"steps":9,"prompts":2,"handbacks":1,"confirmations":1,"effects":1,"unknown":0,"model_calls":0,"tokens_in":0,"tokens_out":0,"wall_s":41,"model":null}"#;

const SHORT_LINK_END: Duration = Duration::from_secs(4);

fn ask_finish() -> String {
    let finish = r#"{"op":"finish","agent":"{agent}","run":"{run}","record":RECORD}"#
        .replace("RECORD", RUN_RECORD);
    common::ask_store(&common::id(0x51), &finish)
}

fn alive(script: &Script) -> bool {
    let runner = script.logged("runner");
    runner.len() == 1 && !common::gone(runner[0].0, runner[0].1)
}

fn short_rig(name: &str) -> common::Rig {
    common::Rig::configured(name, |c| c.link_end = SHORT_LINK_END)
}

fn start_held(
    rig: &common::Rig,
    after: &[&str],
) -> (halvo_schema::executable::text::RunId, Script, Holder) {
    let go = rig.dir.join("go");
    let wait = format!("wait-for {}", go.display());
    let mut lines = vec!["hello", "holder", "on-eof hang", wait.as_str(), "log held"];
    lines.extend_from_slice(after);
    let script = rig.script(&lines);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    assert_eq!(rig.sup.start(spec), Ok(()), "the start");
    let holder = hold_the_link(&script, &go);
    assert!(script.wait_log("held", SHORT), "{}", script.log());
    (run, script, holder)
}

#[test]
fn a_runner_that_outlives_its_stored_finish_after_closing_its_output_while_a_helper_holds_its_link_is_ended_within_twice_link_end(
) {
    let rig = short_rig("finish-closed-held");
    let finish = ask_finish();
    let (run, script, mut holder) = start_held(
        &rig,
        &[finish.as_str(), "log finished", "close-stdout", "idle"],
    );
    assert!(script.wait_log("stdout-closed", SHORT), "{}", script.log());
    let closed = std::time::Instant::now();
    std::thread::sleep(2 * SHORT_LINK_END - Duration::from_millis(1500));
    assert!(
        alive(&script),
        "the runner was ended before twice link_end less 1.5 s after it closed its output"
    );
    let bound = 2 * SHORT_LINK_END + Duration::from_secs(3);
    assert!(
        rig.rec
            .wait_end(&run, bound.saturating_sub(closed.elapsed()))
            .is_some(),
        "the library reported no end within twice link_end and 3 s after the runner closed its output: {}",
        script.log()
    );
    assert!(runner_gone(&script), "the runner is gone: {}", script.log());
    assert!(
        holder.lives(&script),
        "the helper that holds the link still lives after the run ended, so the bound was long enough"
    );
}

#[test]
fn a_runner_that_outlives_its_stored_finish_with_its_output_open_is_ended_within_twice_link_end() {
    let rig = short_rig("finish-open");
    let finish = ask_finish();
    let script = rig.script(&[
        "hello",
        "on-eof hang",
        finish.as_str(),
        "log finished",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    assert_eq!(rig.sup.start(spec), Ok(()), "the start");
    assert!(script.wait_log("finished", SHORT), "{}", script.log());
    let stored = std::time::Instant::now();
    std::thread::sleep(2 * SHORT_LINK_END - Duration::from_millis(1500));
    assert!(
        alive(&script),
        "the runner was ended before twice link_end less 1.5 s after its finish was stored"
    );
    let bound = 2 * SHORT_LINK_END + Duration::from_secs(3);
    assert!(
        rig.rec
            .wait_end(&run, bound.saturating_sub(stored.elapsed()))
            .is_some(),
        "the library reported no end within twice link_end and 3 s after the runner's finish was stored: {}",
        script.log()
    );
    assert!(runner_gone(&script), "the runner is gone: {}", script.log());
}

#[test]
fn a_run_with_no_stored_finish_is_not_ended_by_the_clock_and_ends_at_the_librarys_kill() {
    let rig = short_rig("no-finish-held");
    let (run, script, mut holder) = start_held(&rig, &["idle"]);
    let started = std::time::Instant::now();
    std::thread::sleep(2 * SHORT_LINK_END + Duration::from_secs(2));
    assert!(
        alive(&script),
        "a runner with no stored finish was ended twice link_end and 2 s after its start: {}",
        script.log()
    );
    assert!(
        rig.rec.wait_end(&run, Duration::ZERO).is_none(),
        "the end of a run with no stored finish was reported after {:?}",
        started.elapsed()
    );
    assert_eq!(rig.sup.kill(&run), Ok(()), "the kill");
    assert!(runner_gone(&script), "the runner is gone: {}", script.log());
    assert!(
        rig.rec
            .wait_end(&run, halvo_agent_client::LINK_END)
            .is_some(),
        "the library reported the run's end after the kill"
    );
    assert!(
        holder.lives(&script),
        "the helper that holds the link still lives after the run ended"
    );
}

#[test]
fn a_runner_that_stores_its_finish_sends_done_and_exits_ends_as_before_within_a_second_and_a_half()
{
    let rig = short_rig("finish-done-exit");
    let finish = ask_finish();
    let script = rig.script(&[
        "hello",
        finish.as_str(),
        "log finished",
        r#"send {"t":"done","outcome":"completed"}"#,
        "exit 0",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    assert_eq!(rig.sup.start(spec), Ok(()), "the start");
    assert!(script.wait_log("\nexit", SHORT), "{}", script.log());
    let end = rig.rec.wait_end(&run, Duration::from_millis(1500));
    let end = end.expect("the library reported the run's end within 1.5 s of the runner's exit");
    assert_eq!(
        end.outcome,
        Some(halvo_schema::executable::names::Outcome::Completed),
        "the outcome of the runner's done"
    );
    assert!(end.finish.is_some(), "the end carries the stored record");
    let log = script.log();
    let exit = log.find("\nexit").unwrap_or(usize::MAX);
    assert!(
        !log[..exit.min(log.len())].contains("\neof"),
        "the runner's input ended before it exited: {log}"
    );
}
