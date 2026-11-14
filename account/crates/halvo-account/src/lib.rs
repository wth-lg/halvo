//! The account helper of Halvo: the one part of the app that talks to the backend and the
//! one reader and writer of the account's credential. `halvo-account [--print-url] COMMAND`
//! makes at most one request and exits; `outcome::Outcome` states what it prints and its
//! exit codes.

#![deny(clippy::undocumented_unsafe_blocks)]

pub mod answer;
pub mod environment;
pub mod flow;
pub mod grammar;
pub mod origin;
pub mod outcome;
pub mod sign_in;
pub mod system;
pub mod transport;

use environment::Environment;
use flow::{Opener, Random, Reach};
use outcome::{EnvironmentFault, Outcome};
use std::ffi::OsString;

/// One run of the helper: the arguments are read with the grammar of this build, then the
/// backend and the store are taken from `environment`, then the client is built and the
/// command runs. Arguments outside the grammar, and an environment that a debug build cannot
/// use, end the run before the store is read and before anything is sent.
pub fn run(
    args: &[OsString],
    environment: &Environment,
    opener: &dyn Opener,
    random: &dyn Random,
) -> Outcome {
    let invocation = match grammar::parse(args, grammar::THIS_BUILD) {
        Ok(invocation) => invocation,
        Err(usage) => return Outcome::Usage(usage),
    };
    let backend = match environment::backend(environment) {
        Ok(backend) => backend,
        Err(fault) => return Outcome::Environment(fault),
    };
    let store = match environment::store(environment) {
        Ok(store) => store,
        Err(fault) => return Outcome::Environment(fault),
    };
    let Some(client) = transport::Client::new(backend) else {
        return Outcome::Environment(EnvironmentFault::NoClient);
    };
    flow::run(
        &invocation,
        &Reach {
            keep: &store,
            transport: &client,
            opener,
            random,
            pages: &origin::PAGES,
        },
    )
}
