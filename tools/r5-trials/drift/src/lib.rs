pub mod case;
pub mod forms;
mod json;
pub mod model;
pub mod targets;
pub mod trial;

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use model::Arm;

pub const DEV_DIR: &str = "fixtures/r5/cases/dev";

pub const USAGE: &str = "the trial takes no argument, --case FILE --arm none|plain, or --count DIR";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Usage(String),
    Output,
    Case(String),
    Input(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Usage(detail) => write!(f, "REFUSED usage: {detail}"),
            Refusal::Output => f.write_str("REFUSED output: standard output cannot be written"),
            Refusal::Case(detail) => write!(f, "REFUSED case: {detail}"),
            Refusal::Input(detail) => write!(f, "REFUSED input: {detail}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Trial,
    Case(PathBuf, Arm),
    Count(PathBuf),
}

pub fn command(args: &[OsString]) -> Result<Command, Refusal> {
    let word = |index: usize| args.get(index).and_then(|arg| arg.to_str());
    let path = |index: usize| {
        args.get(index)
            .filter(|arg| !arg.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| Refusal::Usage(USAGE.to_owned()))
    };
    match (args.len(), word(0), word(2)) {
        (0, _, _) => Ok(Command::Trial),
        (2, Some("--count"), _) => Ok(Command::Count(path(1)?)),
        (4, Some("--case"), Some("--arm")) => {
            let arm = match word(3) {
                Some("none") => Arm::None,
                Some("plain") => Arm::Plain,
                _ => {
                    return Err(Refusal::Usage(
                        "the arm of --case is none or plain".to_owned(),
                    ))
                }
            };
            Ok(Command::Case(path(1)?, arm))
        }
        _ => Err(Refusal::Usage(USAGE.to_owned())),
    }
}

pub fn run(command: &Command, root: &Path) -> Result<String, Refusal> {
    let targets = targets::read(root)?;
    match command {
        Command::Trial => trial::trial(&root.join(DEV_DIR), &targets),
        Command::Case(file, arm) => trial::one(file, *arm, &targets),
        Command::Count(dir) => trial::count(dir, &targets),
    }
}

#[cfg(test)]
mod tests;
