//! The halvo-capture command. `--version` works in every build; the gate's
//! modes `--timed-run`, `--verify-store` and `--verify-deletion` exist only in
//! builds with debug assertions, and a release build prints usage and exits 64
//! for each of them.

use std::io::Write;
use std::process::ExitCode;

use halvo_capture::daemon::{self, Mode};
use halvo_control::exit;
use halvo_control::paths::SystemAccount;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    if let Err(error) = daemon::signal::prepare() {
        return fail(format_args!("the signal flag was not installed: {error}"));
    }
    match daemon::parse(std::env::args_os().skip(1), &SystemAccount) {
        Ok(Mode::Version) => print_checked(&format!("{VERSION}\n")),
        #[cfg(debug_assertions)]
        Ok(Mode::TimedRun(run)) => match daemon::timed_run(&run) {
            Ok(outcome) => {
                let printed = print_checked(&outcome.report.render());
                if let Some(loss) = &outcome.loss {
                    let _ = writeln!(std::io::stderr(), "{loss}");
                }
                if printed != ExitCode::SUCCESS {
                    return printed;
                }
                status(outcome.status)
            }
            Err(error) => fail(format_args!("{error}")),
        },
        #[cfg(debug_assertions)]
        Ok(mode @ (Mode::VerifyStore { .. } | Mode::VerifyDeletion { .. })) => run_verify(
            mode,
            halvo_capture::store::measure_store,
            halvo_capture::store::measure_deletion,
        ),
        Err(usage) => {
            let _ = writeln!(std::io::stderr(), "{usage}");
            status(exit::USAGE)
        }
    }
}

/// Writes `text` to standard output and flushes it; a write that fails, as
/// into a pipe whose reader is gone, is exit 1 with no panic.
fn print_checked(text: &str) -> ExitCode {
    let mut out = std::io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(format_args!("the report was not written: {error}")),
    }
}

#[cfg(debug_assertions)]
type MeasureStore = fn(&std::path::Path, &str) -> halvo_capture::store::StoreResult<String>;

#[cfg(debug_assertions)]
type MeasureDeletion = fn(&std::path::Path) -> halvo_capture::store::StoreResult<String>;

#[cfg(debug_assertions)]
fn run_verify(mode: Mode, store: MeasureStore, deletion: MeasureDeletion) -> ExitCode {
    match mode {
        Mode::VerifyStore { store_dir, marker } => print_or_fail(store(&store_dir, &marker)),
        Mode::VerifyDeletion { store_dir } => print_or_fail(deletion(&store_dir)),
        _ => status(exit::USAGE),
    }
}

#[cfg(debug_assertions)]
fn print_or_fail<E: std::fmt::Display>(result: Result<String, E>) -> ExitCode {
    match result {
        Ok(text) => print_checked(&text),
        Err(error) => fail(format_args!("{error}")),
    }
}

fn fail(error: std::fmt::Arguments<'_>) -> ExitCode {
    let _ = writeln!(std::io::stderr(), "halvo-capture: {error}");
    status(exit::ERROR)
}

fn status(code: i32) -> ExitCode {
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    use super::*;

    thread_local! {
        static CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn recorded_store(dir: &Path, marker: &str) -> halvo_capture::store::StoreResult<String> {
        CALLS.with(|calls| {
            calls
                .borrow_mut()
                .push(format!("store {} {marker}", dir.display()))
        });
        Ok(String::new())
    }

    fn recorded_deletion(dir: &Path) -> halvo_capture::store::StoreResult<String> {
        CALLS.with(|calls| {
            calls
                .borrow_mut()
                .push(format!("deletion {}", dir.display()))
        });
        Ok(String::new())
    }

    fn calls() -> Vec<String> {
        CALLS.with(|calls| calls.borrow_mut().drain(..).collect())
    }

    #[test]
    fn verify_store_measures_the_store_dir_of_its_flag_with_its_marker() {
        let mode = Mode::VerifyStore {
            store_dir: PathBuf::from("/scratch/flag-dir"),
            marker: "M4rker.from-the-flag".to_owned(),
        };
        assert_eq!(
            run_verify(mode, recorded_store, recorded_deletion),
            ExitCode::SUCCESS
        );
        assert_eq!(calls(), ["store /scratch/flag-dir M4rker.from-the-flag"]);
    }

    #[test]
    fn verify_deletion_measures_the_store_dir_of_its_flag() {
        let mode = Mode::VerifyDeletion {
            store_dir: PathBuf::from("/scratch/flag-dir"),
        };
        assert_eq!(
            run_verify(mode, recorded_store, recorded_deletion),
            ExitCode::SUCCESS
        );
        assert_eq!(calls(), ["deletion /scratch/flag-dir"]);
    }
}
