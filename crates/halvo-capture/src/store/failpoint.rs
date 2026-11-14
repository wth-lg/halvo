use std::cell::Cell;
#[cfg(test)]
use std::cell::RefCell;

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

#[cfg(test)]
type Act = Box<dyn FnMut()>;

#[cfg(test)]
thread_local! {
    static SEAMS: RefCell<Vec<(&'static str, Act)>> = const { RefCell::new(Vec::new()) };
}

/// A place where a test of this thread can act between two steps of the store.
#[cfg(test)]
pub(crate) fn seam(name: &'static str) {
    let taken = SEAMS.with(|seams| {
        let mut seams = seams.borrow_mut();
        let at = seams.iter().position(|(named, _)| *named == name)?;
        Some(seams.remove(at))
    });
    if let Some((named, mut act)) = taken {
        act();
        SEAMS.with(|seams| seams.borrow_mut().push((named, act)));
    }
}

#[cfg(test)]
pub(crate) struct Seam(&'static str);

#[cfg(test)]
pub(crate) fn at(name: &'static str, act: impl FnMut() + 'static) -> Seam {
    SEAMS.with(|seams| seams.borrow_mut().push((name, Box::new(act))));
    Seam(name)
}

#[cfg(test)]
impl Drop for Seam {
    fn drop(&mut self) {
        let name = self.0;
        SEAMS.with(|seams| seams.borrow_mut().retain(|(named, _)| *named != name));
    }
}
