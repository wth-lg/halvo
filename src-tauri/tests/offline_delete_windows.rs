#![cfg(windows)]
//! The app's offline delete on Windows, on the programs that the merge gate's test build made: the app's own entry
//! `halvo.exe --delete-all --data-dir DIR` on a scratch directory whose store a service run made and left, and the
//! same entry while a service run holds the directory. The test returns at its first line off a hosted runner.
//!
//! Every wait of this target is a wait on a state that the service or a program writes: the service's status answer
//! `store=open` (`a_service_with_its_store_open`), the end of the service run after its stop (`within` on its exit),
//! and the end of each run of halvo.exe, whose answer the test reads (`app_delete`). It waits on no count of seconds,
//! so it stands in the merge gate.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const CHILD_START: Duration = Duration::from_secs(60);
const STOP_HOLD: Duration = Duration::from_secs(10);
const POLL: Duration = Duration::from_millis(200);
static ONE_BUILT_APP_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Own(Child);

impl Drop for Own {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

/// A line of the test on standard error, written past libtest's capture, so that a passing run shows it too.
fn say(name: &str, line: &str) {
    let _ = std::io::stderr().write_all(format!("{name}: {line}\n").as_bytes());
}

fn on_a_hosted_runner(name: &str, what: &str) -> bool {
    let hosted = std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true");
    if !hosted {
        say(
            name,
            &format!("not run: it starts {what}, on a hosted runner alone"),
        );
    }
    hosted
}

/// The app and the capture service that the gate's test build made beside it.
fn built() -> (PathBuf, PathBuf) {
    let app = PathBuf::from(env!("CARGO_BIN_EXE_halvo"));
    let daemon = app.with_file_name("halvo-capture.exe");
    assert!(
        daemon.is_file(),
        "{} is not built: the test step builds it with the test targets of halvo-capture",
        daemon.display()
    );
    (app, daemon)
}

fn scratch_root(name: &str) -> PathBuf {
    let root =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()));
    std::fs::create_dir_all(&root)
        .unwrap_or_else(|e| panic!("{} cannot be made: {e}", root.display()));
    root
}

fn ctl(daemon: &Path, dir: &Path, op: &str) -> std::process::Output {
    Command::new(daemon)
        .arg("ctl")
        .arg("--data-dir")
        .arg(dir)
        .arg(op)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("{} ctl does not start: {e}", daemon.display()))
}

/// Reads `read` until it answers, at most `bound`: a wait on the state that `read` reads.
fn within<T>(bound: Duration, what: &str, mut read: impl FnMut() -> Option<T>) -> T {
    let until = Instant::now() + bound;
    loop {
        if let Some(value) = read() {
            return value;
        }
        assert!(
            Instant::now() < until,
            "{what}: not within {} s",
            bound.as_secs()
        );
        std::thread::sleep(POLL);
    }
}

const DELETE: &str = "the_apps_offline_delete_on_windows_leaves_no_store_and_deletes_nothing_while_a_service_holds_the_directory";

/// Runs the app's own entry of the offline delete, `halvo.exe --delete-all --data-dir DIR`, the scratch form of the
/// run that a press on Windows makes where no service runs.
fn app_delete(app: &Path, dir: &Path) -> (Option<i32>, serde_json::Value) {
    let out = Command::new(app)
        .args(["--delete-all", "--data-dir"])
        .arg(dir)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("{} does not start: {e}", app.display()));
    let answer = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {out:?}"));
    (out.status.code(), answer)
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.map(Result::unwrap) {
        if entry.file_type().unwrap().is_dir() {
            found.extend(files_under(&entry.path()));
        } else {
            found.push(entry.path());
        }
    }
    found
}

/// What stands in `dir` of the store, its key, its root and the intent: every file under store/, its writer lock among
/// them, and under roots/.
fn what_stays_of_the_store(dir: &Path) -> Vec<PathBuf> {
    let mut stays = files_under(&dir.join("store"));
    stays.extend(files_under(&dir.join("roots")));
    if dir.join("delete-all.intent").exists() {
        stays.push(dir.join("delete-all.intent"));
    }
    stays
}

/// A service run on `dir` until its store opens.
fn a_service_with_its_store_open(daemon: &Path, dir: &Path) -> Own {
    let service = Own(Command::new(daemon)
        .args(["--service-run", "--data-dir"])
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .expect("the service run starts"));
    within(CHILD_START, "the service's store=open", || {
        let out = ctl(daemon, dir, "status");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|line| line == "store=open")
            .then_some(())
    });
    service
}

/// Test 1 of app-4b: on Windows the app runs the offline delete as on a Mac. On a scratch directory whose store a
/// service run made and left, the app's own entry ends as deleted, names the directory and leaves no store; while a
/// service run holds the directory it deletes nothing and says that the directory is held.
#[test]
fn the_apps_offline_delete_on_windows_leaves_no_store_and_deletes_nothing_while_a_service_holds_the_directory(
) {
    if !on_a_hosted_runner(
        DELETE,
        "the built app's offline delete and its capture service",
    ) {
        return;
    }
    let _one_at_a_time = ONE_BUILT_APP_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let began = Instant::now();
    let (app, daemon) = built();
    let dir = scratch_root("app-offline-delete").join("data");
    std::fs::create_dir_all(&dir).expect("the data directory is made");
    let shown = halvo_control::paths::one_line(&dir.to_string_lossy());

    let mut left = a_service_with_its_store_open(&daemon, &dir);
    let stop = ctl(&daemon, &dir, "stop");
    assert!(stop.status.success(), "ctl stop: {stop:?}");
    within(STOP_HOLD, "the service run ends after its stop", || {
        left.0.try_wait().ok().flatten().map(|_| ())
    });
    assert!(
        !files_under(&dir.join("roots")).is_empty(),
        "a store stands before the delete"
    );

    let (code, answer) = app_delete(&app, &dir);
    assert_eq!(code, Some(0), "{answer}");
    assert_eq!(answer["outcome"], "deleted", "{answer}");
    assert_eq!(answer["nothing_left"], true, "{answer}");
    assert_eq!(answer["looked_in"], shown, "{answer}");
    assert_eq!(
        what_stays_of_the_store(&dir),
        Vec::<PathBuf>::new(),
        "{answer}"
    );

    let holding = a_service_with_its_store_open(&daemon, &dir);
    let roots = files_under(&dir.join("roots"));
    let (code, answer) = app_delete(&app, &dir);
    assert_eq!(code, Some(1), "{answer}");
    assert_eq!(answer["error"], "delete_held", "{answer}");
    assert_eq!(answer["looked_in"], shown, "{answer}");
    assert_eq!(answer["outcome_not_known"], false, "{answer}");
    assert_eq!(
        files_under(&dir.join("roots")),
        roots,
        "the held store keeps its root"
    );
    drop(holding);
    say(DELETE, &format!(
        "ran on a hosted runner in {} s: deleted, nothing left, looked_in the directory; held, delete_held, the root kept",
        began.elapsed().as_secs()
    ));
}
