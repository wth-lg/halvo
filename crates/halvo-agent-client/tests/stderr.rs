mod common;

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use common::{wait_until, Script};

const SHORT: Duration = Duration::from_secs(15);

const FLOOD: usize = 2 * 1024 * 1024 + 1;

const CONFIRM: &str = r#"send {"t":"confirm","request":{"v":1,"run":"{run}","agent":"{agent}","effects":[{"effect":"60606060606060606060606060606060","class":"payment","digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","bound":{"class":"payment","fields":[{"name":"payee","from":"slot","origin":null,"value":["Northwind Power"]},{"name":"amount","from":"extract","origin":"http://utility.test:18090","value":["84.20"]},{"name":"instrument","from":"slot","origin":null,"value":["card 4242"]},{"name":"origin","from":"const","origin":null,"value":["http://pay.test:18090"]}]},"new_origin":null}]}}"#;

struct Captured {
    bytes: Arc<Mutex<Vec<u8>>>,
    closed: Arc<AtomicBool>,
}

impl Captured {
    fn read(mut stream: impl Read + Send + 'static) -> Captured {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(AtomicBool::new(false));
        let (b, c) = (Arc::clone(&bytes), Arc::clone(&closed));
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match stream.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => b.lock().unwrap().extend_from_slice(&buf[..n]),
                }
            }
            c.store(true, Ordering::SeqCst);
        });
        Captured { bytes, closed }
    }

    fn bytes(&self) -> Vec<u8> {
        self.bytes.lock().unwrap().clone()
    }

    fn lines(&self) -> Vec<String> {
        BufReader::new(&self.bytes()[..])
            .lines()
            .map_while(Result::ok)
            .collect()
    }

    fn has_line(&self, start: &str) -> bool {
        self.lines().iter().any(|l| l.starts_with(start))
    }

    fn wait_closed(&self) -> bool {
        wait_until(SHORT, || self.closed.load(Ordering::SeqCst))
    }
}

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

fn start(dir: &Path, script: &Script, hold: Option<&Path>) -> (Host, ChildStdout, ChildStderr) {
    let mut cmd = Command::new(common::peer());
    cmd.arg("host")
        .arg(dir.join("data"))
        .arg(&script.path)
        .arg(dir.join("owner.log"));
    if let Some(hold) = hold {
        cmd.arg(hold);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let host = Host {
        child,
        dir: dir.to_path_buf(),
    };
    (host, out, err)
}

fn holds(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn files_under(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else if let Ok(bytes) = fs::read(&path) {
            found.push((path, bytes));
        }
    }
    found
}

fn marker() -> String {
    format!("stderr-marker-{}", common::run_id())
}

fn kind_of(log: &str) -> String {
    log.lines()
        .find_map(|l| l.strip_prefix("stderr-kind "))
        .unwrap_or("none")
        .to_owned()
}

fn kind_by_hand(dir: &Path, stderr: Stdio) -> String {
    let script = Script::new(dir, &["stderr-kind", "exit 0"]);
    let mut child = Command::new(common::peer())
        .arg("--run")
        .arg("kind")
        .arg("--browser")
        .arg(&script.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .unwrap();
    let input = child.stdin.take();
    let mut host = Host {
        child,
        dir: dir.to_path_buf(),
    };
    assert!(
        wait_until(SHORT, || host.child.try_wait().is_ok_and(|s| s.is_some())),
        "the stand-in ended: {}",
        script.log()
    );
    drop(input);
    kind_of(&script.log())
}

#[test]
fn the_marker_that_the_runner_and_its_child_print_on_standard_error_reaches_no_stream_no_file_and_no_run_end_of_the_host(
) {
    let dir = common::scratch("stderr-marker");
    let marker = marker();
    let script = Script::new(
        &dir,
        &[
            "hello",
            "stderr-kind",
            &format!("stderr-marker {marker}"),
            "log marked",
            "exit 0",
        ],
    );
    let (mut host, out, err) = start(&dir, &script, None);
    let (out, err) = (Captured::read(out), Captured::read(err));
    assert!(
        wait_until(SHORT, || out.has_line("end ")),
        "the host reported the run's end: {:?}; {}",
        out.lines(),
        script.log()
    );
    let end = out
        .lines()
        .into_iter()
        .find(|l| l.starts_with("end "))
        .unwrap_or_default();
    for word in ["RunEnd {", "outcome: ", "finish: ", "end_run: "] {
        assert!(
            end.contains(word),
            "the host's end line holds the Debug text of a RunEnd: {end:?}"
        );
    }
    assert!(
        script
            .log()
            .contains("stderr-marker line=yes mixed=yes child=yes last=yes"),
        "the stand-in and its child wrote the marker: {}",
        script.log()
    );
    host.child.kill().unwrap();
    host.child.wait().unwrap();
    assert!(out.wait_closed(), "the host's standard output closed");
    assert!(err.wait_closed(), "the host's standard error closed");
    let needle = marker.as_bytes();
    assert!(
        !holds(&err.bytes(), needle),
        "the marker reached the host's standard error ({} bytes)",
        err.bytes().len()
    );
    assert!(
        err.bytes().is_empty(),
        "the host's standard error holds {} bytes",
        err.bytes().len()
    );
    assert!(
        !holds(&out.bytes(), needle),
        "the marker reached the host's standard output or the Debug text of its RunEnd"
    );
    let owner = fs::read(dir.join("owner.log")).unwrap_or_default();
    assert!(!owner.is_empty(), "the owner log holds the run's requests");
    assert!(!holds(&owner, needle), "the marker reached the owner log");
    for (path, bytes) in files_under(&dir.join("data")) {
        assert!(
            !holds(&bytes, needle),
            "the marker reached {}",
            path.display()
        );
    }
    let kind = kind_of(&script.log());
    assert_eq!(
        kind, "null",
        "the runner's standard error is {kind}, not the null device"
    );
}

#[test]
fn more_than_a_mebibyte_on_standard_error_while_the_library_waits_inside_answers_does_not_block_the_runner(
) {
    let dir = common::scratch("stderr-flood");
    let hold = dir.join("hold");
    fs::create_dir_all(&hold).unwrap();
    let asked = hold.join("asked");
    let script = Script::new(
        &dir,
        &[
            "hello",
            CONFIRM,
            &format!("stderr-flood {FLOOD} {}", asked.display()),
            "log flooded",
            "idle",
        ],
    );
    let (mut host, out, err) = start(&dir, &script, Some(&hold));
    let out = Captured::read(out);
    assert!(
        wait_until(SHORT, || out.has_line("answers-in")),
        "the library waits inside Confirmer::answers: {:?}; {}",
        out.lines(),
        script.log()
    );
    assert!(
        script.wait_log("flooded", SHORT),
        "the stand-in's step after {FLOOD} bytes on standard error was not logged: {}",
        script.log()
    );
    let log = script.log();
    assert!(log.contains("stderr-flood start after=yes"), "{log}");
    assert!(
        log.contains(&format!("stderr-flood wrote={FLOOD}")),
        "{log}"
    );
    assert!(
        !out.has_line("answers-out"),
        "the library was still inside Confirmer::answers when the stand-in logged its next step"
    );
    fs::write(hold.join("release"), b"").unwrap();
    assert!(wait_until(SHORT, || out.has_line("answers-out")));
    host.child.kill().unwrap();
    host.child.wait().unwrap();
    let err = Captured::read(err);
    assert!(err.wait_closed(), "the host's standard error closed");
    assert!(
        err.bytes().is_empty(),
        "the host's standard error holds {} bytes",
        err.bytes().len()
    );
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

#[test]
fn the_stand_in_started_by_hand_names_its_standard_error_and_writes_each_form_of_the_marker_the_flood_and_the_write_after_the_end_of_file(
) {
    let dir = common::scratch("stderr-control");
    let marker = marker();
    let after = format!("after-eof-{}", common::run_id());
    let asked = dir.join("asked");
    fs::write(&asked, b"").unwrap();
    let script = Script::new(
        &dir,
        &[
            &format!("on-eof stderr {after}"),
            "stderr-kind",
            &format!("stderr-flood {FLOOD} {}", asked.display()),
            &format!("stderr-marker {marker}"),
            "log ready",
            "idle",
        ],
    );
    let mut child = Command::new(common::peer())
        .arg("--run")
        .arg("control")
        .arg("--browser")
        .arg(&script.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = child.stdin.take().unwrap();
    let err = Captured::read(child.stderr.take().unwrap());
    let mut host = Host {
        child,
        dir: dir.clone(),
    };
    assert!(script.wait_log("ready", SHORT), "{}", script.log());
    assert_eq!(kind_of(&script.log()), "pipe", "a pipe reads as pipe");
    let m = marker.as_bytes();
    let mut mixed = vec![0xff, 0xfe, 0x80];
    mixed.extend_from_slice(m);
    mixed.extend_from_slice(&[0xc3, 0x28, b'\n']);
    let mut line = m.to_vec();
    line.push(b'\n');
    let written = FLOOD + 2 * line.len() + mixed.len() + m.len();
    wait_until(SHORT, || err.bytes().len() >= written);
    let printed = err.bytes();
    assert_eq!(count(&printed, b"x"), FLOOD, "the flood's bytes");
    assert_eq!(count(&printed, m), 4, "the marker's forms");
    assert_eq!(
        count(&printed, &line),
        2,
        "the stand-in's and its child's lines"
    );
    assert_eq!(
        count(&printed, &mixed),
        1,
        "the marker among bytes that are no UTF-8"
    );
    assert!(
        printed.ends_with(m),
        "the marker as last bytes with no line end"
    );
    drop(input);
    let ended = format!("{after}\n").into_bytes();
    assert!(
        wait_until(SHORT, || err.bytes().ends_with(&ended)),
        "the stand-in's write after the end of file"
    );
    assert!(err.wait_closed(), "the stand-in's standard error closed");
    assert!(host.child.wait().is_ok_and(|s| s.code() == Some(1)));
    let file = fs::File::create(dir.join("stderr-file")).unwrap();
    assert_eq!(
        kind_by_hand(&dir, Stdio::from(file)),
        "file",
        "a file reads as file"
    );
    if cfg!(target_os = "macos") {
        let zero = fs::OpenOptions::new()
            .write(true)
            .open("/dev/zero")
            .unwrap();
        assert_eq!(
            kind_by_hand(&dir, Stdio::from(zero)),
            "other",
            "a character device that is not the null device reads as other"
        );
    }
}
