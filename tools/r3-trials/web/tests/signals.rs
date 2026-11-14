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
/// argument that names `scratch` as a whole path (read with the functions that the trial's library
/// had before CP-126, so that the test runs at both heads).
fn of_the_run(bundle: &Path, scratch: &Path, since: u64) -> Vec<r3_web::procs::Proc> {
    let s = scratch.to_string_lossy().into_owned();
    r3_web::procs::table()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.started_us >= since && p.pid != std::process::id())
        .filter(|p| p.exe.as_ref().is_some_and(|e| e.starts_with(bundle)))
        .filter(|p| {
            r3_web::checks::command_args(p.pid)
                .is_some_and(|a| a.iter().any(|x| r3_web::checks::names_profile(x, &s)))
        })
        .filter(r3_web::procs::running)
        .collect()
}

fn signal_case(signal: libc::c_int, name: &str) {
    use start::Start;
    let dir = std::env::temp_dir().join(format!("r3-web-signal-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let stand_in = standin::bundle(&dir);
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
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
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
