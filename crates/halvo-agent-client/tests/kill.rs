mod common;

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{ask_store, effect, gone, id, line_gone, mark_of, wait_until, Rig, Script};
use halvo_agent_client::marks::ProcLine;
use halvo_agent_client::{Reaped, LINK_END};
use halvo_schema::executable::text::RunId;

const SHORT: Duration = Duration::from_secs(15);

struct Host {
    child: Child,
    run: RunId,
    data: PathBuf,
    owner_log: PathBuf,
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if mark_of(&self.data, &self.run).is_some() {
            let _ = reap_in(&self.data);
        }
        if let Some(dir) = self.data.parent() {
            common::end_logged(dir);
        }
    }
}

fn host(dir: &Path, script: &Script) -> Host {
    host_with(dir, script, Stdio::null())
}

fn host_with(dir: &Path, script: &Script, stderr: Stdio) -> Host {
    let data = dir.join("data");
    let owner_log = dir.join("owner.log");
    let mut child = Command::new(common::peer())
        .arg("host")
        .arg(&data)
        .arg(&script.path)
        .arg(&owner_log)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let run = line
        .trim()
        .strip_prefix("started ")
        .and_then(|r| RunId::parse(r).ok())
        .unwrap_or_else(|| panic!("the host started the run: {line:?}"));
    Host {
        child,
        run,
        data,
        owner_log,
    }
}

fn owner_lines(host: &Host) -> Vec<String> {
    fs::read_to_string(&host.owner_log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A fixture that counts the bytes that reach it.
fn fixture() -> (u16, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let bytes = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&bytes);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = Vec::new();
            let _ = stream.read_to_end(&mut buf);
            seen.fetch_add(buf.len().max(1), Ordering::SeqCst);
        }
    });
    (port, bytes)
}

fn holding_script(rig_dir: &Path, on_eof: &str) -> Script {
    Script::new(
        rig_dir,
        &[
            "hello",
            "browser child",
            &ask_store(&id(0x20), &effect(&id(0x66), 1, "held")),
            &ask_store(&id(0x21), &effect(&id(0x66), 2, "approved")),
            &ask_store(&id(0x22), &effect(&id(0x66), 3, "releasing")),
            &ask_store(&id(0x23), &effect(&id(0x67), 1, "held")),
            &ask_store(&id(0x24), &effect(&id(0x67), 2, "approved")),
            on_eof,
            "log holding",
            "idle",
        ],
    )
}

fn lines_of(host: &Host) -> (ProcLine, ProcLine) {
    let mark = mark_of(&host.data, &host.run).expect("the run has a mark");
    (
        mark.runner.expect("the mark is complete"),
        mark.browser.expect("the mark holds the browser"),
    )
}

#[test]
fn the_hard_kill_of_the_library_leaves_no_runner_no_browser_no_begin_and_no_byte_at_the_fixture() {
    let dir = common::scratch("hard-kill");
    let (_, bytes) = fixture();
    let on_eof = if cfg!(windows) {
        "on-eof hang".to_owned()
    } else {
        "on-eof end".to_owned()
    };
    let script = holding_script(&dir, &on_eof);
    let mut host = host(&dir, &script);
    assert!(script.wait_log("holding", SHORT), "{}", script.log());
    assert!(script.wait_log("browser-child", SHORT), "{}", script.log());
    let (runner, browser) = lines_of(&host);
    let child = script.logged("browser-child")[0];
    let before = owner_lines(&host);
    assert_eq!(before.len(), 5);
    let at = Instant::now();
    host.child.kill().unwrap();
    host.child.wait().unwrap();
    assert!(wait_until(LINK_END, || line_gone(&runner)
        && line_gone(&browser)
        && gone(child.0, child.1)));
    assert!(at.elapsed() < LINK_END);
    let after = owner_lines(&host);
    assert_eq!(after, before);
    assert!(!after
        .iter()
        .any(|l| l.contains(&id(0x67)) && l.contains(r#""state":"releasing""#)));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(bytes.load(Ordering::SeqCst), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn a_write_on_standard_error_after_the_end_of_file_of_the_hard_kill_reaches_no_standard_error_of_the_host(
) {
    let dir = common::scratch("hard-kill-stderr");
    let marker = format!("after-eof-{}", common::run_id());
    let script = holding_script(&dir, &format!("on-eof stderr {marker}"));
    let mut host = host_with(&dir, &script, Stdio::piped());
    let mut err = host.child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = err.read_to_end(&mut bytes);
        let _ = tx.send(bytes);
    });
    assert!(script.wait_log("holding", SHORT), "{}", script.log());
    assert!(script.wait_log("browser-child", SHORT), "{}", script.log());
    let (runner, browser) = lines_of(&host);
    let child = script.logged("browser-child")[0];
    let at = Instant::now();
    host.child.kill().unwrap();
    host.child.wait().unwrap();
    assert!(wait_until(LINK_END, || line_gone(&runner)
        && line_gone(&browser)
        && gone(child.0, child.1)));
    assert!(at.elapsed() < LINK_END);
    assert!(
        script.log().contains("stderr-after-eof wrote=yes"),
        "the stand-in wrote on standard error after the end of file: {}",
        script.log()
    );
    let bytes = rx
        .recv_timeout(SHORT)
        .expect("the host's standard error closed");
    assert!(
        !bytes.windows(marker.len()).any(|w| w == marker.as_bytes()),
        "the write after the end of file reached the host's standard error ({} bytes)",
        bytes.len()
    );
    assert!(
        bytes.is_empty(),
        "the host's standard error holds {} bytes",
        bytes.len()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn the_fixture_counts_a_request_that_a_runner_lets_go_when_its_link_closes() {
    let dir = common::scratch("hard-kill-control");
    let (port, bytes) = fixture();
    let script = holding_script(&dir, &format!("on-eof send {port}"));
    let mut host = host(&dir, &script);
    assert!(script.wait_log("holding", SHORT), "{}", script.log());
    let (runner, browser) = lines_of(&host);
    host.child.kill().unwrap();
    host.child.wait().unwrap();
    assert!(wait_until(SHORT, || bytes.load(Ordering::SeqCst) > 0));
    let answers = reap_in(&host.data);
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].answer, Reaped::Gone);
    assert!(wait_until(SHORT, || line_gone(&runner) && line_gone(&browser)));
}

fn reap_in(data: &Path) -> Vec<halvo_agent_client::ReapAnswer> {
    let rig = Rig::with("reap-next-life", common::peer(), SHORT);
    let next = halvo_agent_client::Supervisor::new(
        halvo_schema::request::CoordinatorRole::claim(),
        halvo_agent_client::Config::new(data.to_path_buf(), common::peer()),
        halvo_agent_client::Parts {
            link: rig.store.clone(),
            confirmer: rig.people.clone(),
            human: rig.people.clone(),
            secrets: rig.vault.clone(),
            marks: rig.rec.clone(),
        },
    );
    next.reap().unwrap()
}

#[test]
fn a_browser_whose_runner_was_killed_first_is_ended_by_the_next_life_reap_through_the_browser_line()
{
    let dir = common::scratch("next-life");
    let script = Script::new(
        &dir,
        &["hello", "on-eof hang", "browser child", "log ready", "idle"],
    );
    let mut host = host(&dir, &script);
    assert!(script.wait_log("ready", SHORT), "{}", script.log());
    assert!(script.wait_log("browser-child", SHORT), "{}", script.log());
    let (runner, browser) = lines_of(&host);
    let child = script.logged("browser-child")[0];
    host.child.kill().unwrap();
    host.child.wait().unwrap();
    if !line_gone(&runner) {
        common::kill_pid(runner.pid.get());
    }
    assert!(wait_until(SHORT, || line_gone(&runner)));
    if cfg!(target_os = "macos") {
        std::thread::sleep(Duration::from_millis(300));
        assert!(!line_gone(&browser), "the browser outlives its runner here");
    }
    let answers = reap_in(&host.data);
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].run, host.run);
    assert_eq!(answers[0].answer, Reaped::Gone);
    assert!(wait_until(SHORT, || line_gone(&browser) && gone(child.0, child.1)));
    assert!(mark_of(&host.data, &host.run).is_none());
}

#[test]
fn a_browser_in_the_runner_group_whose_runner_was_killed_first_is_ended_by_the_next_life_reap() {
    let dir = common::scratch("next-life-runner-group");
    let script = Script::new(
        &dir,
        &[
            "hello",
            "on-eof hang",
            "browser in-runner-group child",
            "log ready",
            "idle",
        ],
    );
    let mut host = host(&dir, &script);
    assert!(script.wait_log("ready", SHORT), "{}", script.log());
    let (runner, browser) = lines_of(&host);
    host.child.kill().unwrap();
    host.child.wait().unwrap();
    if !line_gone(&runner) {
        common::kill_pid(runner.pid.get());
    }
    assert!(wait_until(SHORT, || line_gone(&runner)));
    let child = script.logged("browser-child");
    if cfg!(target_os = "macos") {
        std::thread::sleep(Duration::from_millis(300));
        assert!(!line_gone(&browser), "the browser outlives its runner here");
    }
    let answers = reap_in(&host.data);
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].answer, Reaped::Gone);
    assert!(
        wait_until(SHORT, || line_gone(&browser)
            && child.iter().all(|(p, c)| gone(*p, *c))),
        "a browser in the runner's group outlived the next life's reap: {answers:?}"
    );
    assert!(mark_of(&host.data, &host.run).is_none());
}

#[test]
fn the_next_life_reap_of_a_living_runner_ends_what_it_started_outside_its_group() {
    let dir = common::scratch("next-life-living-runner");
    let script = Script::new(
        &dir,
        &[
            "hello",
            "on-eof hang",
            "child-own-group",
            "log ready",
            "idle",
        ],
    );
    let mut host = host(&dir, &script);
    assert!(script.wait_log("ready", SHORT), "{}", script.log());
    let child = script.logged("child");
    assert_eq!(child.len(), 1, "{}", script.log());
    host.child.kill().unwrap();
    host.child.wait().unwrap();
    let answers = reap_in(&host.data);
    assert_eq!(answers.len(), 1);
    assert_eq!(answers[0].answer, Reaped::Gone);
    assert!(
        wait_until(SHORT, || gone(child[0].0, child[0].1)),
        "a child that the runner made in a group of its own outlived the next life's reap"
    );
}
