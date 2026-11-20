//! CP-126 E1 (5): INT and TERM reach the trial program (run.sh ends by exec), and after the
//! program's end no process of its run is left. The program runs one row with a stand-in browser
//! (`browser_late`) that starts a helper beside it while the row waits on a navigation; the test
//! sends the signal, waits for the program's end, then reads the table for a process whose program
//! lies in the stand-in's bundle and whose arguments name the run's scratch directory.
#![cfg(all(target_os = "macos", feature = "driver"))]

use r3_web::start;
use std::path::Path;
use std::time::{Duration, Instant};

#[allow(dead_code)]
mod standin {
    include!("../src/standin.rs");
}

/// The processes of the run: running, begun after `since`, with their program in `bundle` and an
/// argument whose value after its first `=` is a path at or below `scratch`, the test's own reading
/// and not the mark's, so that a process of the run that the mark would not take is found too.
fn of_the_run(bundle: &Path, scratch: &Path, since: u64) -> Vec<r3_web::procs::Proc> {
    let below = |a: &String| {
        a.split_once('=')
            .is_some_and(|(_, v)| Path::new(v).starts_with(scratch))
    };
    r3_web::procs::table()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.started_us >= since && p.pid != std::process::id())
        .filter(|p| p.exe.as_ref().is_some_and(|e| e.starts_with(bundle)))
        .filter(|p| r3_web::checks::command_args(p.pid).is_some_and(|a| a.iter().any(below)))
        .filter(r3_web::procs::running)
        .collect()
}

fn signal_case(signal: libc::c_int, name: &str) {
    use start::Start;
    let dir = std::env::temp_dir().join(format!("r3-web-signal-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stand_in = standin::bundle(&dir);
    r3_web::procs::fence::on();
    let _admitted = r3_web::procs::fence::admit_below(&dir);
    let fenced_dir = std::fs::canonicalize(&dir).unwrap();
    let stderr_path = dir.join("program-stderr.txt");
    // the process table names a program by its resolved path: a temporary directory under a link
    // (the hosted Mac's /var/folders) is compared resolved
    let macos = std::fs::canonicalize(stand_in.binary.parent().unwrap()).unwrap();
    let launcher = standin::launcher_as(&macos, "browser_late", "Google Chrome");
    let scratch = dir.join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let scratch = std::fs::canonicalize(&scratch).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let since = r3_web::procs::table()
        .unwrap()
        .into_iter()
        .find(|p| p.pid == std::process::id())
        .map(|p| p.started_us)
        .unwrap();
    let mut program = std::process::Command::new(env!("CARGO_BIN_EXE_web-trial"))
        .args(["--candidate", "cdp", "--browser"])
        .arg(&launcher)
        .args(["--port", &port.to_string(), "--machine", "mac", "--repo"])
        .arg(&repo)
        .arg("--scratch")
        .arg(&scratch)
        .args([
            "--runs",
            "1",
            "--round",
            "3",
            "--relay",
            "--step",
            "signin_form",
        ])
        .env(r3_web::procs::fence::VAR, &fenced_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&stderr_path).unwrap())
        .start()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(30);
    let mut seen = 0;
    while Instant::now() < until {
        seen = of_the_run(&macos, &scratch, since).len();
        if seen >= 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    // SAFETY: kill to this test's own child, which has not been waited for.
    unsafe { libc::kill(program.id() as libc::pid_t, signal) };
    let until = Instant::now() + Duration::from_secs(40);
    let mut ended = false;
    while Instant::now() < until {
        if matches!(program.try_wait(), Ok(Some(_))) {
            ended = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if !ended {
        let _ = program.kill();
    }
    let _ = program.wait();
    std::thread::sleep(Duration::from_millis(500));
    let left = of_the_run(&macos, &scratch, since);
    for p in &left {
        r3_web::procs::kill(p);
    }
    let refused = std::fs::read_to_string(&stderr_path)
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with("fence_refused "))
        .count();
    assert_eq!(
        refused, 0,
        "the program's fence refused an end of a process outside the test's directory"
    );
    assert!(
        seen >= 2,
        "the stand-in browser and its helper did not run ({seen})"
    );
    assert!(ended, "the program did not end on the signal");
    assert_eq!(
        left.len(),
        0,
        "processes of the run left after the program's end"
    );
}

#[test]
fn term_to_the_program_leaves_no_process_of_its_run() {
    signal_case(libc::SIGTERM, "term");
}

#[test]
fn int_to_the_program_leaves_no_process_of_its_run() {
    signal_case(libc::SIGINT, "int");
}

/// A child of the test, ended through its handle and reaped on every way out of the test.
struct Own(std::process::Child);

impl Drop for Own {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// CP-161 (3): the trial's program with `R3_WEB_FENCE_BELOW` naming a directory that holds no
/// program of the test refuses every end. Its guard entry is given the test's own `/bin/sleep` as
/// the browser and an executor that does not run; after its grace it asks the browser to end, and
/// the fence sends nothing, prints its line and stops the program. The test's bound is 20 s.
#[test]
fn the_program_s_fence_refuses_an_end_outside_its_directory() {
    use start::Start;
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let stderr_path = tmp.join("r3-program-fence-stderr.txt");
    let browser = Own(std::process::Command::new("/bin/sleep")
        .arg("20")
        .start()
        .unwrap());
    let row = r3_web::procs::table()
        .unwrap()
        .into_iter()
        .find(|p| p.pid == browser.0.id())
        .expect("the stand-in browser's row");
    let program = Own(std::process::Command::new(env!("CARGO_BIN_EXE_web-trial"))
        .arg(r3_web::relay::GUARD_FLAG)
        .args([row.pid.to_string(), row.started_us.to_string()])
        .args(["1", "1"])
        .env(r3_web::procs::fence::VAR, tmp.join("r3-fence-elsewhere"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::fs::File::create(&stderr_path).unwrap())
        .start()
        .unwrap());
    let mut program = program;
    let until = Instant::now() + Duration::from_secs(20);
    let mut status = None;
    while Instant::now() < until {
        if let Ok(Some(s)) = program.0.try_wait() {
            status = Some(s);
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let mut browser = browser;
    let alive = matches!(browser.0.try_wait(), Ok(None));
    let line = format!(
        "fence_refused pid={} started_us={} ",
        row.pid, row.started_us
    );
    let refused = std::fs::read_to_string(&stderr_path)
        .unwrap_or_default()
        .lines()
        .filter(|l| l.starts_with(&line))
        .count();
    assert!(alive, "the program's end reached the test's own process");
    let status = status.expect("the program did not stop within 20 s");
    assert!(!status.success(), "{status:?}");
    assert!(
        refused >= 1,
        "the program printed no fence_refused line for the browser"
    );
}

/// CP-215 G1: the fence of `r3_web::procs` is on in this test target from its start, so that no
/// test of the target reaches the real end of a process that it did not admit.
#[test]
fn the_fence_is_on_in_this_test_target() {
    assert!(
        r3_web::procs::fence::is_on(),
        "the fence is off in this test target"
    );
}
