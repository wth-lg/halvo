#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const PROGRAM: &str = env!("CARGO_BIN_EXE_profile-trial");
const RUN_BOUND: Duration = Duration::from_secs(120);
const PORT_WAIT: Duration = Duration::from_secs(60);

const CHECK: &str = "#!/bin/sh
shift 2
prev=
for a do
  shift
  if [ \"$prev\" = --repo ]; then set -- \"$@\" \"$R3_REAL_REPO\"; else set -- \"$@\" \"$a\"; fi
  prev=$a
done
echo 'egress mode=sandbox list=x command=1 denied=0 log=none' >&2
exec \"$@\"
";

struct Run(Option<Child>);

impl Drop for Run {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn write(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| panic!("{e}"));
}

fn real_repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("the repository")
}

fn own_root(name: &str) -> PathBuf {
    halvo_test_dir::make(Path::new(env!("CARGO_TARGET_TMPDIR")), name)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn run(mut cmd: Command) -> Output {
    let waited = Instant::now();
    loop {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut run = Run(Some(cmd.spawn().expect("the program starts")));
        let began = Instant::now();
        loop {
            let child = run.0.as_mut().expect("a child");
            if child.try_wait().expect("a wait").is_some() {
                break;
            }
            assert!(
                began.elapsed() < RUN_BOUND,
                "the program ran past {RUN_BOUND:?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        let out = run
            .0
            .take()
            .expect("a child")
            .wait_with_output()
            .expect("the output");
        let refused = String::from_utf8_lossy(&out.stderr).starts_with("REFUSED port");
        if refused && waited.elapsed() < PORT_WAIT {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        return out;
    }
}

#[test]
fn the_control_alone_starts_its_browser_with_the_sandbox_off() {
    let root = own_root("stand-in-control");
    let repo = root.join("repo");
    let scratch = root.join("scratch");
    let browsers = root.join("browser");
    for d in [
        repo.join("scripts/egress"),
        scratch.clone(),
        browsers.clone(),
    ] {
        std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{}: {e}", d.display()));
    }
    write(&repo.join("scripts/egress/check.sh"), CHECK);
    let browser = browsers.join("Google Chrome");
    write(
        &browser,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >\"$R3_BROWSER_ARGS\"\nexit 1\n",
    );
    let mut got = Vec::new();
    for off in [true, false] {
        let args_file = root.join(if off { "args-off" } else { "args-on" });
        let mut cmd = Command::new(PROGRAM);
        cmd.args(["--item", "3", "--browser"])
            .arg(&browser)
            .args(["--port", "18090", "--machine", "mac", "--repo"])
            .arg(&repo)
            .arg("--scratch")
            .arg(&scratch)
            .env("R3_REAL_REPO", real_repo())
            .env("R3_BROWSER_ARGS", &args_file);
        if off {
            cmd.arg("--sandbox-off");
        }
        let out = run(cmd);
        let args = std::fs::read_to_string(&args_file).unwrap_or_default();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        got.push((
            off,
            args.lines().filter(|l| *l == "--no-sandbox").count(),
            stdout,
        ));
    }
    let [(_, off_count, off_out), (_, on_count, on_out)] = &got[..] else {
        panic!("two runs");
    };
    assert_eq!(
        *off_count, 1,
        "the control's browser got no --no-sandbox:\n{off_out}"
    );
    assert_eq!(*on_count, 0, "a cell's browser got --no-sandbox:\n{on_out}");
    assert!(off_out.contains(" browser_started=no "), "{off_out}");
    assert!(off_out.contains("egress_control "), "{off_out}");
}

#[test]
fn the_task_shape_reads_a_start_that_never_answers_as_browser_start() {
    let root = own_root("stand-in-task");
    let scratch = root.join("scratch");
    let browsers = root.join("browser");
    for d in [scratch.clone(), browsers.clone()] {
        std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{}: {e}", d.display()));
    }
    let browser = browsers.join("Google Chrome");
    write(&browser, "#!/bin/sh\nexec 3<&- 4>&-\nexec sleep 20\n");
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("a port")
        .local_addr()
        .expect("its address")
        .port();
    let mut cmd = Command::new(PROGRAM);
    cmd.args(["--item", "task", "--browser"])
        .arg(&browser)
        .args(["--port", &port.to_string(), "--machine", "mac", "--repo"])
        .arg(real_repo())
        .arg("--scratch")
        .arg(&scratch);
    let out = run(cmd);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let task = stdout
        .lines()
        .find(|l| l.starts_with("task "))
        .unwrap_or_else(|| panic!("no task line:\n{stdout}"));
    assert!(task.contains(" result=miss "), "{task}");
    assert!(task.ends_with(" cause=browser_start"), "{task}");
}

#[test]
fn items_1_and_2_read_a_start_that_never_answers_as_browser_start() {
    let root = own_root("stand-in-session");
    let scratch = root.join("scratch");
    let browsers = root.join("browser");
    for d in [scratch.clone(), browsers.clone()] {
        std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{}: {e}", d.display()));
    }
    let browser = browsers.join("Google Chrome");
    write(&browser, "#!/bin/sh\nexec 3<&- 4>&-\nexec sleep 20\n");
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("a port")
        .local_addr()
        .expect("its address")
        .port();
    let mut cmd = Command::new(PROGRAM);
    cmd.args(["--item", "1", "--browser"])
        .arg(&browser)
        .args(["--port", &port.to_string(), "--machine", "mac", "--repo"])
        .arg(real_repo())
        .arg("--scratch")
        .arg(&scratch);
    let out = run(cmd);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        stdout.contains("notrun item=1 machine=mac browser=chrome cause=browser_start"),
        "{stdout}"
    );
}
