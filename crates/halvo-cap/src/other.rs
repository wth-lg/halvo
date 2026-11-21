use std::io;
use std::sync::Arc;

use crate::{CapError, ChildSpec, EndsSpec, Limit, Mode, Outcome};

pub(crate) const MODE: Mode = Mode::Poll;
pub(crate) const PLATFORM: &str = "other";

/// An end that a child would inherit; no child is started on this system.
pub(crate) type End = std::fs::File;

pub(crate) fn name_of(index: usize, _end: &End) -> u64 {
    3 + index as u64
}

/// The call that ends a started child; none is started on this system.
pub(crate) struct Ending;

impl Ending {
    pub(crate) fn end(&self) {}

    pub(crate) fn reached(&self) -> bool {
        false
    }
}

/// A started child; none is started on this system.
pub(crate) struct Started;

impl Started {
    pub(crate) fn pid(&self) -> Option<u32> {
        None
    }

    pub(crate) fn ending(&self) -> Arc<Ending> {
        Arc::new(Ending)
    }

    pub(crate) fn wait(self) -> Result<Outcome, CapError> {
        Err(unsupported())
    }
}

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

pub(crate) fn start(_spec: EndsSpec, _limit: Limit) -> Result<Started, CapError> {
    Err(unsupported())
}
