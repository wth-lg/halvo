use std::cell::Cell;

use super::{StoreError, StoreResult};

pub(crate) const FROM_THE_ENVIRONMENT: &str = "HALVO_STORE_FAILPOINTS";

thread_local! {
    static ARMED: Cell<Option<&'static str>> = const { Cell::new(None) };
}

pub(crate) fn arm(name: &'static str) {
    ARMED.with(|armed| armed.set(Some(name)));
}

pub(crate) fn hit(name: &'static str) -> StoreResult<()> {
    let armed_here = ARMED.with(|armed| armed.get()) == Some(name);
    if armed_here {
        ARMED.with(|armed| armed.set(None));
    }
    let named_by_the_environment = std::env::var(FROM_THE_ENVIRONMENT)
        .is_ok_and(|names| names.split(',').any(|named| named == name));
    if armed_here || named_by_the_environment {
        return Err(StoreError::Io(std::io::ErrorKind::Other));
    }
    Ok(())
}
