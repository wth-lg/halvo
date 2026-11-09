//! The halvo-capture command. The R1 gate drives `--timed-run`, `--verify-store`
//! and `--verify-deletion`; later phases fill the module functions these dispatch
//! to. `--version` works now.

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
        Some("--timed-run") => {
            let seconds = args
                .get(1)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(600);
            match daemon::timed_run(seconds) {
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
                "usage: halvo-capture --version | --timed-run [seconds] | --verify-store | --verify-deletion"
            );
            ExitCode::from(64)
        }
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
