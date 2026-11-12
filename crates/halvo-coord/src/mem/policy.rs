use crate::guard;
use crate::ports::PortError;
use crate::types::{Autonomy, Class, ConfigView};
use std::cell::RefCell;
use std::marker::PhantomData;

/// The memory `Policy` of the tests (I5.10, ruling XA-1a): it answers from the rows that the
/// test gives it, one per autonomy and class, so that the crate keeps no dependency on
/// halvo-policy. A pair with no row gives `confirm`, and a port made to fail answers that
/// error, so that it is never kinder than the real one. It records each question.
pub struct Policy<C> {
    rows: Vec<(Autonomy, Class, bool)>,
    asked: RefCell<Vec<(Autonomy, Class)>>,
    error: RefCell<Option<PortError>>,
    config: PhantomData<C>,
}

impl<C> Policy<C> {
    /// A memory `Policy` with these rows: the autonomy, the class and whether the table gives
    /// `run`.
    pub fn new(rows: &[(Autonomy, Class, bool)]) -> Policy<C> {
        Policy {
            rows: rows.to_vec(),
            asked: RefCell::new(Vec::new()),
            error: RefCell::new(None),
            config: PhantomData,
        }
    }

    /// Makes every question answer `error`.
    pub fn fail(&self, error: PortError) {
        *self.error.borrow_mut() = Some(error);
    }

    /// The questions asked, in order, as the autonomy of the configuration and the class.
    pub fn asked(&self) -> Vec<(Autonomy, Class)> {
        self.asked.borrow().clone()
    }
}

impl<C: ConfigView> guard::Policy for Policy<C> {
    type Config = C;

    fn gives_run(&self, config: &C, class: Class) -> Result<bool, PortError> {
        let autonomy = config.autonomy();
        self.asked.borrow_mut().push((autonomy, class));
        if let Some(e) = self.error.borrow().clone() {
            return Err(e);
        }
        Ok(self
            .rows
            .iter()
            .any(|(a, c, run)| *a == autonomy && *c == class && *run))
    }
}
