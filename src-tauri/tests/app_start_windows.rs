#![cfg(windows)]
//! The built app's start on Windows, the gate form of check 34 (scripts/r1/ci.d/34-app-windows-start.sh) on the
//! programs that the gate's test build made: three apps at once, A on a data directory that is not provisioned, B
//! and C on provisioned ones; no child and no store of A before the Start step, one halvo-capture.exe of B and of C,
//! each a child of its app with no window of its own, none of B behind a ctl stop while its app runs, and none of C
//! behind a kill of its app. Each test returns at its first line off a hosted runner.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const START: &str = "the_built_app_on_windows_starts_one_child_only_on_a_provisioned_directory_and_none_behind_a_stop_or_a_kill";
const STATE_FILE: &str = "daemon-state.json";
const BEFORE_START: Duration = Duration::from_secs(5);
const CHILD_START: Duration = Duration::from_secs(60);
const ANSWER: Duration = Duration::from_secs(30);
const OWN_ANSWER: Duration = Duration::from_secs(4);
const STOP_HOLD: Duration = Duration::from_secs(10);
const KILL_END: Duration = Duration::from_secs(12);
const POLL: Duration = Duration::from_millis(200);
static ONE_BUILT_APP_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Every halvo.exe and halvo-capture.exe of the machine, one line each: the name, the process id, the id of its
/// parent, the handle of its main window (0 for none) and its command line, as check 34 reads them.
const LIST: &str = r#"$ErrorActionPreference = "Stop"
$found = Get-CimInstance -ClassName Win32_Process -Filter "Name = 'halvo.exe' OR Name = 'halvo-capture.exe'"
foreach ($p in $found) {
  $w = 0
  $live = Get-Process -Id $p.ProcessId -ErrorAction SilentlyContinue
  if ($live) { $w = [int64]$live.MainWindowHandle }
  [Console]::Out.WriteLine(("{0} {1} {2} {3} {4}" -f $p.Name, $p.ProcessId, $p.ParentProcessId, $w, $p.CommandLine))
}
"#;

struct Own(Child);

impl Drop for Own {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn say(name: &str, line: &str) {
    eprintln!("{name}: {line}");
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

fn start_app(app: &Path, dir: &Path) -> Own {
    Own(Command::new(app)
        .arg("--background")
        .arg("--data-dir")
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|e| panic!("{} does not start: {e}", app.display())))
}

fn ctl(daemon: &Path, dir: &Path, op: &str) -> Output {
    Command::new(daemon)
        .arg("ctl")
        .arg("--data-dir")
        .arg(dir)
        .arg(op)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("{} ctl does not start: {e}", daemon.display()))
}

/// A process of the list: its name, id, parent, main window and command line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    name: String,
    pid: u32,
    parent: u32,
    window: i64,
    command: String,
}

/// The processes of `LIST`, read by PowerShell from the script that `script` names.
fn processes(script: &Path) -> Vec<Seen> {
    if !script.is_file() {
        std::fs::write(script, LIST).expect("the list's script is written");
    }
    let out = Command::new("powershell.exe")
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .stdin(Stdio::null())
        .output()
        .expect("powershell.exe runs");
    assert!(out.status.success(), "the list of processes: {out:?}");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut words = line.trim_end_matches('\r').splitn(5, ' ');
            Some(Seen {
                name: words.next()?.to_owned(),
                pid: words.next()?.parse().ok()?,
                parent: words.next()?.parse().ok()?,
                window: words.next()?.parse().ok()?,
                command: words.next().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

/// The processes named `name` whose command line names the directory `dir`.
fn of<'a>(seen: &'a [Seen], name: &str, dir: &Path) -> Vec<&'a Seen> {
    let dir = dir.display().to_string();
    seen.iter()
        .filter(|process| process.name == name && process.command.contains(&dir))
        .collect()
}

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

/// Of `seen`: the one app of `dir`, which is `app`, and the one halvo-capture.exe of `dir`, its child with no
/// window; answers that child's id.
fn one_child_of_the_app(seen: &[Seen], dir: &Path, app: &Own, case: &str) -> u32 {
    let apps = of(seen, "halvo.exe", dir);
    assert_eq!(
        apps.len(),
        1,
        "case {case}: one app of the directory: {apps:?}"
    );
    assert_eq!(
        apps[0].pid,
        app.0.id(),
        "case {case}: the app is the one that the test started"
    );
    let children = of(seen, "halvo-capture.exe", dir);
    assert_eq!(
        children.len(),
        1,
        "case {case}: one halvo-capture.exe of the directory: {children:?}"
    );
    let child = children[0];
    say(
        START,
        &format!(
            "case {case}: halvo-capture.exe {}, its parent {}, the app {}, its window {}",
            child.pid, child.parent, apps[0].pid, child.window
        ),
    );
    assert_eq!(
        child.parent,
        app.0.id(),
        "case {case}: the halvo-capture.exe is a child of the app"
    );
    assert_eq!(
        child.window, 0,
        "case {case}: the halvo-capture.exe has no window of its own"
    );
    child.pid
}

#[test]
fn the_built_app_on_windows_starts_one_child_only_on_a_provisioned_directory_and_none_behind_a_stop_or_a_kill(
) {
    if !on_a_hosted_runner(START, "the built app three times and its capture service") {
        return;
    }
    let _one_at_a_time = ONE_BUILT_APP_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let began = Instant::now();
    let (app, daemon) = built();
    let root = scratch_root("app-start-windows");
    let script = root.join("list.ps1");
    let [dir_a, dir_b, dir_c] = ["a", "b", "c"].map(|name| {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).expect("the data directory is made");
        dir
    });
    for dir in [&dir_b, &dir_c] {
        std::fs::write(dir.join(STATE_FILE), "{}\n")
            .expect("the state file of a provisioned directory");
    }
    let on_a = start_app(&app, &dir_a);
    let on_b = start_app(&app, &dir_b);
    let mut on_c = start_app(&app, &dir_c);

    within(
        CHILD_START,
        "a halvo-capture.exe of each provisioned directory",
        || {
            let seen = processes(&script);
            (!of(&seen, "halvo-capture.exe", &dir_b).is_empty()
                && !of(&seen, "halvo-capture.exe", &dir_c).is_empty())
            .then_some(())
        },
    );
    say(
        START,
        &format!(
            "case provisioned: a halvo-capture.exe of each provisioned directory after {} s",
            began.elapsed().as_secs()
        ),
    );
    within(
        ANSWER,
        "ctl status of each provisioned directory answers",
        || {
            (ctl(&daemon, &dir_b, "status").status.success()
                && ctl(&daemon, &dir_c, "status").status.success())
            .then_some(())
        },
    );
    let seen = processes(&script);
    let child_b = one_child_of_the_app(&seen, &dir_b, &on_b, "provisioned");
    let child_c = one_child_of_the_app(&seen, &dir_c, &on_c, "second provisioned");

    on_c.0
        .kill()
        .expect("the app of the second provisioned directory is ended");
    let _ = on_c.0.wait();
    let killed = Instant::now();
    say(
        START,
        &format!(
            "case kill: the app {} of the second provisioned directory is ended",
            on_c.0.id()
        ),
    );

    std::thread::sleep(OWN_ANSWER);
    let stop = ctl(&daemon, &dir_b, "stop");
    assert!(
        stop.status.success(),
        "case stop: ctl stop of the provisioned directory: {stop:?}"
    );
    let stopped = Instant::now();

    let wait = BEFORE_START.saturating_sub(began.elapsed());
    std::thread::sleep(wait);
    let seen = processes(&script);
    let apps_a = of(&seen, "halvo.exe", &dir_a);
    let children_a = of(&seen, "halvo-capture.exe", &dir_a);
    let stored: Vec<&str> = ["store", STATE_FILE]
        .into_iter()
        .filter(|name| dir_a.join(name).exists())
        .collect();
    say(START, &format!(
        "case not provisioned: after {} s {} app, {} halvo-capture.exe, in the directory: {stored:?}",
        began.elapsed().as_secs(),
        apps_a.len(),
        children_a.len()
    ));
    assert_eq!(apps_a.len(), 1, "case not provisioned: its app runs");
    assert_eq!(
        apps_a[0].pid,
        on_a.0.id(),
        "case not provisioned: the app is the one that the test started"
    );
    assert!(
        children_a.is_empty(),
        "case not provisioned: no halvo-capture.exe before the Start step: {children_a:?}"
    );
    assert!(
        stored.is_empty(),
        "case not provisioned: the directory holds {stored:?}"
    );

    let mut gone_c = None;
    let children_b = loop {
        let seen = processes(&script);
        let children_b: Vec<Seen> = of(&seen, "halvo-capture.exe", &dir_b)
            .into_iter()
            .cloned()
            .collect();
        for child in &children_b {
            assert_eq!(
                child.pid,
                child_b,
                "case stop: a halvo-capture.exe of the provisioned directory started again {} s after the stop",
                stopped.elapsed().as_secs()
            );
        }
        assert_eq!(
            of(&seen, "halvo.exe", &dir_b).len(),
            1,
            "case stop: the app of the provisioned directory runs {} s after the stop",
            stopped.elapsed().as_secs()
        );
        if gone_c.is_none() {
            if of(&seen, "halvo-capture.exe", &dir_c)
                .iter()
                .all(|child| child.pid != child_c)
            {
                gone_c = Some(killed.elapsed());
            } else {
                assert!(
                    killed.elapsed() < KILL_END,
                    "case kill: the halvo-capture.exe of the killed app still runs {} s after the kill",
                    killed.elapsed().as_secs()
                );
            }
        }
        if gone_c.is_some() && stopped.elapsed() >= STOP_HOLD {
            break children_b;
        }
        std::thread::sleep(POLL);
    };
    assert!(
        children_b.is_empty(),
        "case stop: the stopped halvo-capture.exe still runs {} s after the stop: {children_b:?}",
        stopped.elapsed().as_secs()
    );
    say(
        START,
        &format!(
        "case kill: no halvo-capture.exe of the second provisioned directory {} s after the kill",
        gone_c.map_or(0, |gone| gone.as_secs())
    ),
    );
    say(START, &format!(
        "case stop: no halvo-capture.exe of the provisioned directory {} s after the stop, and its app runs",
        stopped.elapsed().as_secs()
    ));
    drop((on_a, on_b));
    say(
        START,
        &format!(
            "ran on a hosted runner in {} s: the four cases of check 34",
            began.elapsed().as_secs()
        ),
    );
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

/// What stands in `dir` of the store, its key and its root. The store's writer lock on Windows, an empty file that
/// stays after a delete until the store lets it go, is not counted while it is empty.
fn what_stays_of_the_store(dir: &Path) -> Vec<PathBuf> {
    let lock = dir.join("store").join("writer.lock");
    let mut stays: Vec<PathBuf> = files_under(&dir.join("store"))
        .into_iter()
        .filter(|file| *file != lock || std::fs::metadata(file).map_or(true, |m| m.len() > 0))
        .collect();
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
