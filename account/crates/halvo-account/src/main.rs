use halvo_account::environment::Environment;
use halvo_account::system::{Browser, SystemRandom};
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let outcome = halvo_account::run(&args, &Environment::of_process(), &Browser, &SystemRandom);
    let _ = writeln!(std::io::stdout().lock(), "{}", outcome.line());
    let _ = writeln!(
        std::io::stderr().lock(),
        "halvo-account: {}",
        outcome.note()
    );
    ExitCode::from(outcome.code())
}
