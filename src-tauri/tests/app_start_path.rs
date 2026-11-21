#![cfg(target_os = "macos")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use halvo_control::paths::DataDir;

const NAME: &str = "the_built_app_starts_its_daemon_and_the_daemon_ends_with_it";
const EMPTY_WAIT: Duration = Duration::from_secs(5);
const WITHIN: Duration = Duration::from_secs(60);
const STOP_WAIT: Duration = Duration::from_millis(halvo_control::STOP_WAIT_MS);

struct Own(Child);

impl Drop for Own {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn say(line: &str) {
    let _ = std::io::stderr().write_all(format!("{NAME}: {line}\n").as_bytes());
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

#[test]
fn the_built_app_starts_its_daemon_and_the_daemon_ends_with_it() {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
        say("not run: it starts the built app and its capture daemon, on a hosted runner alone");
        return;
    }
    let began = Instant::now();
    let app = PathBuf::from(env!("CARGO_BIN_EXE_halvo"));
    let daemon = app.with_file_name("halvo-capture");
    assert!(
        daemon.is_file(),
        "{} is not built: the test step builds it with the test targets of halvo-capture",
        daemon.display()
    );
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("app-start-path-{}", std::process::id()));

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
        Vec::<String>::new(),
        "the empty directory stays empty"
    );
    drop(on_empty);

    let dir = private_dir(&root.join("provisioned"));
    let provisioner = Own(Command::new(&daemon)
        .arg("--service-run")
        .arg("--data-dir")
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("the daemon starts"));
    within(WITHIN, "the daemon that provisions answers status", || {
        ctl(&daemon, &dir, "status").status.success().then_some(())
    });
    let stop = ctl(&daemon, &dir, "stop");
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

    let on_provisioned = start_app(&app, &dir);
    let app_pid = on_provisioned.0.id();
    let child = within(WITHIN, "one halvo-capture child of the app", || {
        let children = daemons_under(&daemon, app_pid);
        (children.len() == 1).then(|| children[0].clone())
    });
    assert_eq!(
        child.2,
        format!(
            "{} --service-run --lifeline --data-dir {}",
            daemon.display(),
            dir.display()
        )
    );
    let status = within(WITHIN, "the child's status with lease=fresh", || {
        let out = ctl(&daemon, &dir, "status");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        (out.status.success() && text.lines().any(|line| line == "lease=fresh")).then_some(text)
    });
    for line in [
        format!("pid={}", child.0),
        "parent=watched".to_owned(),
        "store=open".to_owned(),
    ] {
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
    say(&format!(
        "ran on a hosted runner in {} s: no child on the empty directory, one child with store=open, parent=watched and lease=fresh, and none after the kill of the app",
        began.elapsed().as_secs()
    ));
}
