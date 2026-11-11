use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use r5_routine::files::Refusal;

enum Ask {
    Block,
    Sample(PathBuf),
}

fn ask(args: &[OsString]) -> Result<Ask, Refusal> {
    match args {
        [] => Ok(Ask::Block),
        [flag, file] if flag == "--sample" && !file.is_empty() => {
            Ok(Ask::Sample(PathBuf::from(file)))
        }
        _ => Err(Refusal::Usage(
            "r5-routine takes no argument or --sample FILE".to_owned(),
        )),
    }
}

fn answer(args: &[OsString]) -> Result<(), Refusal> {
    let ask = ask(args)?;
    let trial = r5_routine::run(Path::new("."))?;
    match ask {
        Ask::Block => {
            let mut out = std::io::stdout().lock();
            out.write_all(trial.block().as_bytes())
                .and_then(|()| out.flush())
                .map_err(|_| Refusal::Output)
        }
        Ask::Sample(file) => std::fs::write(file, trial.sample())
            .map_err(|_| Refusal::Usage("the sample file cannot be written".to_owned())),
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match answer(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(refusal) => {
            eprintln!("{refusal}");
            ExitCode::from(2)
        }
    }
}
