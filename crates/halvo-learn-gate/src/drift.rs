use crate::cli::{Refusal, Report};

pub fn run(_args: &[String]) -> Result<Report, Refusal> {
    Err(Refusal::usage("drift is not built yet"))
}
