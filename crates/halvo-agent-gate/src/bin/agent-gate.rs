//! `agent-gate configs` makes one approved workflow per seed category on the gate host, sends `create_agent` for
//! each and prints one line per category and the count (A14.5). Exit 0 when every line is `stored=yes
//! state=approved agent=set result=valid`, 1 when a line is not, and 2 for a refusal, which prints one line
//! `REFUSED <cause>: <detail>` on standard error and nothing on standard output; the cause of a refusal is `usage`.
//!
//! `agent-gate privacy --data-dir DIR` plays the delete of a workflow on a new derived store under `DIR` and prints
//! one line `privacy check=<name> result=<ok|miss|not-run>` for `agent_deleted`, `agent_scrub` and
//! `agent_delete_all`, in that order (A14.4; `halvo_agent_gate::privacy`). Exit 0 when no line is `miss`, 1 when one
//! is, and 2 for a refusal: `usage`, or `store` when the play cannot run (the store does not open under `DIR`, or it
//! refuses a step of the play). The store stays under `DIR`.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use halvo_agent_gate::configs::configs;
use halvo_agent_gate::privacy::{self, Outcome};
use halvo_agent_gate::GateHost;

const USAGE: &str = "agent-gate configs | agent-gate privacy --data-dir DIR";

fn refuse(cause: &str, detail: &str) -> ExitCode {
    eprintln!("REFUSED {cause}: {detail}");
    ExitCode::from(2)
}

fn write_out(text: &str, pass: bool) -> ExitCode {
    let mut out = std::io::stdout().lock();
    if out.write_all(text.as_bytes()).is_err() || out.flush().is_err() {
        return ExitCode::from(1);
    }
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn run_configs() -> ExitCode {
    let mut host = GateHost::new();
    let report = configs(&mut host);
    write_out(&report.render(), report.pass())
}

fn run_privacy(data_dir: &Path) -> ExitCode {
    let played = privacy::process_key().and_then(|key| privacy::play(data_dir, &key));
    match played {
        Ok(delete) => {
            let outcomes = privacy::outcomes(&delete);
            let pass = outcomes.iter().all(|(_, o)| *o != Outcome::Miss);
            write_out(&privacy::render(&outcomes), pass)
        }
        Err(detail) => refuse("store", &detail),
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [command] if command == "configs" => run_configs(),
        [command, flag, dir] if command == "privacy" && flag == "--data-dir" => {
            run_privacy(Path::new(dir))
        }
        _ => refuse("usage", USAGE),
    }
}
