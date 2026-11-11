use std::io;

use crate::{CapError, ChildSpec, Limit, Mode, Outcome};

pub(crate) const MODE: Mode = Mode::Poll;
pub(crate) const PLATFORM: &str = "other";

fn unsupported() -> CapError {
    CapError {
        call: "platform",
        source: io::Error::new(io::ErrorKind::Unsupported, "neither macOS nor Windows"),
    }
}

pub(crate) fn check_supported(_limit: Limit) -> Result<Mode, CapError> {
    Err(unsupported())
}

pub(crate) fn run(_spec: ChildSpec, _limit: Limit) -> Result<Outcome, CapError> {
    Err(unsupported())
}
