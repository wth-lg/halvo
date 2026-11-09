//! The halvo-capture command.
//!
//! The always-on daemon runs under `--service-run`; the LaunchAgent and the
//! Windows logon task launch the binary with that argument. The R1 gate drives
//! `--timed-run`, `--verify-store` and `--verify-deletion`. `--version` prints
//! the crate version.

use std::fmt::Display;
use std::process::ExitCode;

use halvo_capture::{daemon, store};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("{VERSION}");
            ExitCode::SUCCESS
        }
        Some("--service-run") => {
            write_pid_file();
            match daemon::service_run(daemon::default_store_dir()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(e),
            }
        }
        Some("--timed-run") => {
            write_pid_file();
            let seconds = args
                .get(1)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(600);
            // The secure-input switch is read here, at the CLI boundary, and
            // passed down; the backend takes it as a parameter.
            let secure_input = std::env::var("HALVO_GATE_SECURE_INPUT").as_deref() == Ok("1");
            match daemon::timed_run(seconds, secure_input) {
                Ok(report) => {
                    print!("{}", report.render());
                    ExitCode::SUCCESS
                }
                Err(e) => fail(e),
            }
        }
        Some("--verify-store") => print_or_fail(store::verify_store()),
        Some("--verify-deletion") => print_or_fail(store::verify_deletion()),
        _ => {
            eprintln!(
                "usage: halvo-capture --version | --service-run | --timed-run [seconds] | \
                 --verify-store | --verify-deletion"
            );
            ExitCode::from(64)
        }
    }
}

/// Write this process's pid to `HALVO_PID_FILE` if set, so a supervisor (the
/// gate's cost and memory sub-checks) can sample the capture process itself
/// rather than a wrapping shell. A write failure is not fatal to the run.
fn write_pid_file() {
    if let Ok(path) = std::env::var("HALVO_PID_FILE") {
        let _ = std::fs::write(path, format!("{}\n", std::process::id()));
    }
}

fn print_or_fail<E: Display>(result: Result<String, E>) -> ExitCode {
    match result {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => fail(e),
    }
}

fn fail<E: Display>(error: E) -> ExitCode {
    eprintln!("halvo-capture: {error}");
    ExitCode::FAILURE
}
