//! `agent-gate configs` makes one approved workflow per seed category on the gate host, sends `create_agent` for
//! each and prints one line per category and the count (A14.5). Exit 0 when every line is `stored=yes
//! state=approved agent=set result=valid`, 1 when a line is not, and 2 for a refusal, which prints one line
//! `REFUSED <cause>: <detail>` on standard error and nothing on standard output; the cause of a refusal is `usage`.

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

use halvo_agent_gate::configs::configs;
use halvo_agent_gate::GateHost;

const USAGE: &str = "agent-gate configs";

fn refuse(cause: &str, detail: &str) -> ExitCode {
    eprintln!("REFUSED {cause}: {detail}");
    ExitCode::from(2)
}

fn run_configs() -> ExitCode {
    let mut host = GateHost::new();
    let report = configs(&mut host);
    let mut out = std::io::stdout().lock();
    if out.write_all(report.render().as_bytes()).is_err() || out.flush().is_err() {
        return ExitCode::from(1);
    }
    if report.pass() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [command] if command == "configs" => run_configs(),
        _ => refuse("usage", USAGE),
    }
}
