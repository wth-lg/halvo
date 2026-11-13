#![forbid(unsafe_code)]
//! `learn-gate`, the gate host of the learning run (contracts.md L10): it opens a store, links the agent store's
//! handlers and, by the arm, `LearnHandlers`, and plays the scripted cases of L9 occurrence by occurrence through
//! its stub runner against its own page model, on a clock that the command sets. Commands: `drift`, `delete` and
//! `routine`; `privacy`, `soak`, `model` and the product link of `drift` answer the refusal `usage` until their
//! units fill their modules. Exit status by L0.3: 0 done, 1 a check missed, 2 a refusal with one line
//! `REFUSED <cause>: <detail>` on standard error and nothing on standard output.

mod case;
mod cli;
mod delete;
mod drift;
mod forms;
mod host;
mod link;
mod model;
mod page;
mod privacy;
mod routine;
mod soak;
mod store;
mod stub;
mod targets;

#[cfg(test)]
mod tests;

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cli::run(&args) {
        Ok(report) => {
            let mut text = String::new();
            for line in &report.lines {
                text.push_str(line);
                text.push('\n');
            }
            let mut out = std::io::stdout().lock();
            if out
                .write_all(text.as_bytes())
                .and_then(|()| out.flush())
                .is_err()
            {
                return ExitCode::from(1);
            }
            ExitCode::from(if report.ok { 0 } else { 1 })
        }
        Err(refusal) => {
            let mut err = std::io::stderr().lock();
            let _ = writeln!(err, "{}", refusal.line());
            ExitCode::from(2)
        }
    }
}
