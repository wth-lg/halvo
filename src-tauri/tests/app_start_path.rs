#![cfg(target_os = "macos")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use halvo_control::paths::DataDir;

const NAME: &str = "the_built_app_starts_its_daemon_and_the_daemon_ends_with_it";
const SECOND: &str = "a_second_launch_exits_zero_and_leaves_one_halvo_and_one_daemon";
const EMPTY_WAIT: Duration = Duration::from_secs(5);
const APP_SECOND_LAUNCH_EXIT_MAX: Duration = Duration::from_secs(2);
const WITHIN: Duration = Duration::from_secs(60);
const STOP_WAIT: Duration = Duration::from_millis(halvo_control::STOP_WAIT_MS);
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

fn built() -> (PathBuf, PathBuf) {
    let app = PathBuf::from(env!("CARGO_BIN_EXE_halvo"));
    let daemon = app.with_file_name("halvo-capture");
    assert!(
        daemon.is_file(),
        "{} is not built: the test step builds it with the test targets of halvo-capture",
        daemon.display()
    );
    (app, daemon)
}

fn scratch_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}", std::process::id()))
}

fn private_dir(path: &Path) -> PathBuf {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)
        .unwrap_or_else(|e| panic!("{} cannot be made: {e}", path.display()));
    path.to_path_buf()
}

fn entries(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
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

fn processes() -> Vec<(u32, u32, String)> {
    let out = Command::new("/bin/ps")
        .args(["-axww", "-o", "pid=,ppid=,command="])
        .output()
        .expect("ps runs");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let pid = words.next()?.parse().ok()?;
            let ppid = words.next()?.parse().ok()?;
            Some((pid, ppid, words.collect::<Vec<_>>().join(" ")))
        })
        .collect()
}

fn daemons_under(daemon: &Path, parent: u32) -> Vec<(u32, u32, String)> {
    let program = daemon.display().to_string();
    processes()
        .into_iter()
        .filter(|(_, ppid, command)| *ppid == parent && command.starts_with(&program))
        .collect()
}

fn daemons_of(daemon: &Path, dir: &Path) -> Vec<(u32, u32, String)> {
    let program = daemon.display().to_string();
    let dir = dir.display().to_string();
    processes()
        .into_iter()
        .filter(|(_, _, command)| command.starts_with(&program) && command.contains(&dir))
        .collect()
}

fn provisioned(daemon: &Path, path: &Path) -> PathBuf {
    let dir = private_dir(path);
    let provisioner = Own(Command::new(daemon)
        .arg("--service-run")
        .arg("--data-dir")
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the daemon starts"));
    within(WITHIN, "the daemon that provisions answers status", || {
        ctl(daemon, &dir, "status").status.success().then_some(())
    });
    let stop = ctl(daemon, &dir, "stop");
    assert!(stop.status.success(), "ctl stop: {stop:?}");
    let mut provisioner = provisioner;
    within(
        WITHIN,
        "the daemon that provisions ends after its stop",
        || provisioner.0.try_wait().ok().flatten().map(|_| ()),
    );
    assert_eq!(
        DataDir::from_flag(&dir).unwrap().is_provisioned(),
        Ok(true),
        "the daemon provisioned the directory"
    );
    dir
}

fn one_child(daemon: &Path, app: &Own) -> (u32, u32, String) {
    let app_pid = app.0.id();
    within(WITHIN, "one halvo-capture child of the app", || {
        let children = daemons_under(daemon, app_pid);
        (children.len() == 1).then(|| children[0].clone())
    })
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
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn status_within(
    bound: Duration,
    what: &str,
    daemon: &Path,
    dir: &Path,
    wanted: &[&str],
) -> String {
    let until = Instant::now() + bound;
    let mut newest = None;
    loop {
        let out = ctl(daemon, dir, "status");
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout).into_owned();
            if wanted
                .iter()
                .all(|want| text.lines().any(|line| line == *want))
            {
                return text;
            }
            newest = Some(text);
        }
        if Instant::now() >= until {
            match &newest {
                Some(text) => panic!(
                    "{what}: not within {} s; the last status that ctl status printed:\n{text}",
                    bound.as_secs()
                ),
                None => panic!(
                    "{what}: not within {} s, and no ctl status answered with exit 0",
                    bound.as_secs()
                ),
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn the_built_app_starts_its_daemon_and_the_daemon_ends_with_it() {
    if !on_a_hosted_runner(NAME, "the built app and its capture daemon") {
        return;
    }
    let _one_at_a_time = ONE_BUILT_APP_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let began = Instant::now();
    let (app, daemon) = built();
    let root = scratch_root("app-start-path");

    let empty = private_dir(&root.join("empty"));
    let mut on_empty = start_app(&app, &empty);
    std::thread::sleep(EMPTY_WAIT);
    assert!(
        matches!(on_empty.0.try_wait(), Ok(None)),
        "the app still runs on the empty directory"
    );
    assert_eq!(
        daemons_under(&daemon, on_empty.0.id()),
        Vec::new(),
        "no child on an empty directory"
    );
    assert_eq!(
        entries(&empty),
        ["run"],
        "the empty directory holds run alone"
    );
    let mut run = entries(&empty.join("run"));
    run.sort();
    assert!(
        run == ["app.lock"] || run == ["app.lock", "failed-starts"],
        "run holds the app's lock and, until the tray stands, the count of failed starts, and nothing else: {run:?}"
    );
    drop(on_empty);

    let dir = provisioned(&daemon, &root.join("provisioned"));
    let on_provisioned = start_app(&app, &dir);
    let child = one_child(&daemon, &on_provisioned);
    assert_eq!(
        child.2,
        format!(
            "{} --service-run --lifeline --data-dir {}",
            daemon.display(),
            dir.display()
        )
    );
    let status = status_within(
        WITHIN,
        "the child's status with lease=fresh and store=open",
        &daemon,
        &dir,
        &["lease=fresh", "store=open"],
    );
    for line in [format!("pid={}", child.0), "parent=watched".to_owned()] {
        assert!(
            status.lines().any(|held| held == line),
            "the status holds {line}: {status}"
        );
    }

    drop(on_provisioned);
    within(
        STOP_WAIT,
        "no halvo-capture process with the directory after the kill of the app",
        || daemons_of(&daemon, &dir).is_empty().then_some(()),
    );
    let after = ctl(&daemon, &dir, "status");
    assert_eq!(
        after.status.code(),
        Some(69),
        "ctl status after the end: {after:?}"
    );
    say(NAME, &format!(
        "ran on a hosted runner in {} s: no child on the empty directory, one child with store=open, parent=watched and lease=fresh, and none after the kill of the app",
        began.elapsed().as_secs()
    ));
}

#[test]
fn a_second_launch_exits_zero_and_leaves_one_halvo_and_one_daemon() {
    if !on_a_hosted_runner(SECOND, "the built app three times and its capture daemon") {
        return;
    }
    let _one_at_a_time = ONE_BUILT_APP_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let began = Instant::now();
    let (app, daemon) = built();
    let dir = provisioned(
        &daemon,
        &scratch_root("app-second-launch").join("provisioned"),
    );
    let mut first = start_app(&app, &dir);
    let child = one_child(&daemon, &first);

    let mut second = start_app(&app, &dir);
    let ended = within(
        APP_SECOND_LAUNCH_EXIT_MAX,
        "the second launch on the first's directory ends",
        || second.0.try_wait().ok().flatten(),
    );
    assert_eq!(ended.code(), Some(0), "the second launch exits 0");
    assert!(
        matches!(first.0.try_wait(), Ok(None)),
        "the first Halvo still runs"
    );
    assert_eq!(
        daemons_under(&daemon, first.0.id()),
        [child],
        "the first Halvo keeps its one child"
    );

    drop(first);
    let mut third = start_app(&app, &dir);
    std::thread::sleep(APP_SECOND_LAUNCH_EXIT_MAX * 2);
    assert!(
        matches!(third.0.try_wait(), Ok(None)),
        "a third launch after the kill of the first takes the lock and stays"
    );
    drop(third);
    within(
        WITHIN,
        "no halvo-capture process with the directory after the kill of the third",
        || daemons_of(&daemon, &dir).is_empty().then_some(()),
    );
    say(SECOND, &format!(
        "ran on a hosted runner in {} s: the second launch exited 0 within {} s, the first kept its one child, and a third launch stayed after the kill of the first",
        began.elapsed().as_secs(),
        APP_SECOND_LAUNCH_EXIT_MAX.as_secs()
    ));
}
