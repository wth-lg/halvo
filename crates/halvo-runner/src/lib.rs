#![forbid(unsafe_code)]
//! The runner of the agent-setup run (contracts.md A6): one process per run, started by its supervisor with three
//! pipes, which reads frames on standard input, writes frames on standard output and carries out one run of one
//! agent.

mod args;
mod clock;
mod connect;
mod effect;
mod file;
mod find;
mod handback;
mod link;
mod playbook;
#[cfg_attr(
    not(any(test, feature = "loopback")),
    expect(
        dead_code,
        reason = "the confirm path has no request maker in a build without the feature loopback until the adapters and the browser are linked (CP-142)"
    )
)]
mod release;
mod run;
mod secret;
mod session;
mod web;

use std::ffi::OsString;
use std::io::Write;

pub use link::LINK_END;

/// The one line that the panic hook writes: it names no message and no place, so that no text of a library or of
/// a value reaches standard error.
pub const PANIC_LINE: &str = "run panic=yes";

/// Writes one line on standard error; a standard error that the supervisor closed changes nothing.
fn line(text: &str) {
    let _ = writeln!(std::io::stderr(), "{text}");
}

/// The version that `--version` prints: the package's, which scripts/r1/ci.d/61-version.sh compares with the
/// version of the workspace.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Runs the runner with its arguments (without the program name) and returns the exit status: 0 once the run's
/// `done` is written after a stored `finish`, 1 when the link closed or the `finish` was not stored, 2 for a
/// refusal with one line `REFUSED <cause>: <detail>` on standard error, the cause `usage` or `frame`. A call whose
/// first argument is `--relay-guard` is the guard of the relay of a run's browser, which the runner starts with its
/// own binary: `halvo_web::guard::guard_main` takes its other arguments and gives its status, 2 for an argument that
/// is not UTF-8. The flag `--version` as the only argument writes the version and a line feed on standard output
/// and gives 0, or 1 when standard output takes no line; it starts no link and reads no frame.
pub fn main(args: Vec<OsString>) -> i32 {
    if args
        .first()
        .is_some_and(|first| first == halvo_web::guard::GUARD_FLAG)
    {
        let words: Option<Vec<String>> = args[1..]
            .iter()
            .map(|word| word.to_str().map(str::to_string))
            .collect();
        return words.map_or(2, |words| halvo_web::guard::guard_main(&words));
    }
    if args::asks_version(&args) {
        let mut out = std::io::stdout().lock();
        return match writeln!(out, "{VERSION}").and_then(|()| out.flush()) {
            Ok(()) => 0,
            Err(_) => 1,
        };
    }
    std::panic::set_hook(Box::new(|_| {
        line(PANIC_LINE);
        std::process::exit(1);
    }));
    let args = match args::parse(&args) {
        Ok(args) => args,
        Err(detail) => {
            line(&format!("REFUSED usage: {detail}"));
            return 2;
        }
    };
    let link = link::Link::start(
        Box::new(std::io::stdin()),
        Box::new(std::io::stdout()),
        Box::new(link::start_grace),
    );
    run::run(args, link)
}
