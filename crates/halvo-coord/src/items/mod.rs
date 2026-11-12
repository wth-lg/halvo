//! The items of I3 and the confirmation of I4: the closed list of what may wait for the
//! person, where each kind comes from and the budgets (`raise_step`), the item as the page gets
//! it (`compose_*`), the presenter table and the machine of a confirmation, and the decision of
//! the setup flow (`setup_step`), as pure functions, and `drive`, which carries out their
//! decisions through the ports.

mod compose;
mod confirm;
pub mod drive;
mod key;
mod presenter;
mod raise;
mod setup;

pub use compose::{
    compose_bind, compose_check, compose_confirm, compose_link_account, compose_plain,
    compose_setup, field_from, kept_source, site_name, Plain, SetupParam,
};
pub use confirm::{confirm_step, ConfirmAnswer, ConfirmMove, ConfirmState, Confirmation};
pub use key::Key;
pub use presenter::{presenter, request_presenter};
pub use raise::{
    choices, handback_kind, handback_result, params, raise_confirm, raise_step, Raise, Source,
    CONFIRM_CLASSES,
};
pub use setup::{setup_step, Setup, SetupBody, SetupInput, SetupMove, SetupState};
