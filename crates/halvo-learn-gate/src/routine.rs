use crate::cli::{Refusal, Report};

/// The command of a later unit (contracts.md L10.4), which answers the refusal `usage` until its unit fills it.
pub fn run(_args: &[String]) -> Result<Report, Refusal> {
    Err(Refusal::usage("routine is not built yet"))
}
