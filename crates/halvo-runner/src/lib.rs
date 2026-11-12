#![forbid(unsafe_code)]
//! The runner of the agent-setup run (contracts.md A6): one process per run, started by its supervisor with three
//! pipes, which reads frames on standard input, writes frames on standard output and carries out one run of one
//! agent.

mod args;
mod clock;
mod effect;
mod file;
mod link;
mod playbook;
mod run;

use std::ffi::OsString;
use std::io::Write;

pub use link::LINK_END;

/// The one line that the panic hook writes: it names no message and no place, so that no text of a library or of
/// a value reaches standard error.
pub const PANIC_LINE: &str = "run panic=yes";

/// Runs the runner with its arguments (without the program name) and returns the exit status: 0 once the run's
/// `done` is written after a stored `finish`, 1 when the link closed or the `finish` was not stored, 2 for a
/// refusal with one line `REFUSED <cause>: <detail>` on standard error, the cause `usage` or `frame`.
pub fn main(args: Vec<OsString>) -> i32 {
    std::panic::set_hook(Box::new(|_| {
        let _ = writeln!(std::io::stderr(), "{PANIC_LINE}");
        std::process::exit(1);
    }));
    let args = match args::parse(&args) {
        Ok(args) => args,
        Err(detail) => {
            eprintln!("REFUSED usage: {detail}");
            return 2;
        }
    };
    let link = link::Link::start(
        Box::new(std::io::stdin()),
        Box::new(std::io::stdout()),
        true,
    );
    run::run(args, link)
}
