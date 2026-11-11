use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use r5_drift::Refusal;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let outcome =
        r5_drift::command(&args).and_then(|command| r5_drift::run(&command, Path::new(".")));
    let refusal = match outcome {
        Ok(text) => {
            let mut out = std::io::stdout().lock();
            match out.write_all(text.as_bytes()).and_then(|()| out.flush()) {
                Ok(()) => return ExitCode::SUCCESS,
                Err(_) => Refusal::Output,
            }
        }
        Err(refusal) => refusal,
    };
    let _ = writeln!(std::io::stderr(), "{refusal}");
    ExitCode::from(2)
}
