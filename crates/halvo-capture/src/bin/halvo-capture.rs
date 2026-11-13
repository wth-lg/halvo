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

static STDOUT_OPEN_AT_START: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

fn main() -> ExitCode {
    STDOUT_OPEN_AT_START.get_or_init(daemon::signal::standard_output_is_open);
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
        Ok(Mode::VerifyStore { .. }) => print_or_fail(halvo_capture::store::verify_store()),
        #[cfg(debug_assertions)]
        Ok(Mode::VerifyDeletion { .. }) => print_or_fail(halvo_capture::store::verify_deletion()),
        Err(usage) => {
            let _ = writeln!(std::io::stderr(), "{usage}");
            status(exit::USAGE)
        }
    }
}

fn print_checked(text: &str) -> ExitCode {
    if STDOUT_OPEN_AT_START.get() != Some(&true) {
        return fail(format_args!("the standard output is closed"));
    }
    let mut out = std::io::stdout().lock();
    match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(format_args!("the report was not written: {error}")),
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
