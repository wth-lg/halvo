//! The items of I3 and the confirmation of I4: the presenter table and the machine of a
//! confirmation, as pure functions.

mod confirm;
mod presenter;

pub use confirm::{confirm_step, ConfirmAnswer, ConfirmMove, ConfirmState, Confirmation};
pub use presenter::{presenter, request_presenter};
