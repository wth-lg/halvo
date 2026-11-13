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
