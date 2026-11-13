use crate::cli::{Refusal, Report};

pub fn run(_args: &[String]) -> Result<Report, Refusal> {
    Err(Refusal::usage("delete is not built yet"))
}
