use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use r5_stat::Refusal;

fn main() -> ExitCode {
    let outcome = if std::env::args_os().len() > 1 {
        Err(Refusal::Usage("r5-stat takes no argument".to_owned()))
    } else {
        r5_stat::run(Path::new("."))
    };
    match outcome {
        Ok(block) => {
            let mut out = std::io::stdout().lock();
            if out
                .write_all(block.text.as_bytes())
                .and_then(|()| out.flush())
                .is_err()
            {
                return ExitCode::from(2);
            }
            if block.verdict.ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(refusal) => {
            eprintln!("{refusal}");
            ExitCode::from(2)
        }
    }
}
