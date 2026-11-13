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
    unsafe { GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32) > 0 }
}

#[cfg(windows)]
#[test]
fn a_windows_runner_has_a_console_of_its_own_that_the_library_is_not_attached_to_and_that_shows_no_window(
) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    let own = test_process_has_a_console();
    eprintln!(
        "u49-console: the test process has a console: {}",
        if own {
            "yes"
        } else {
            "no; the host gets one of its own"
        }
    );
    let dir = common::scratch("console");
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
        .stderr(Stdio::piped());
    if !own {
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }
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
    let line = logged_value(&script.log(), "console ");
    eprintln!("u49-console: the runner's console: {line}");
    assert!(
        line.contains(" host=no "),
        "the library's process is attached to the runner's console: {line}"
    );
    assert!(
        !line.contains("window=visible"),
        "the runner's console shows a window: {line}"
    );
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
