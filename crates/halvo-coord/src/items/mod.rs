//! The items of I3 and the confirmation of I4: the closed list of what may wait for the
//! person, where each kind comes from and the budgets (`raise_step`), the item as the page gets
//! it (`compose_*`), the presenter table and the machine of a confirmation, and the decision of
//! the setup flow (`setup_step`), as pure functions, and `drive`, which carries out their
//! decisions through the ports.

pub mod chars;
mod compose;
mod confirm;
pub mod drive;
mod key;
mod presenter;
mod raise;
mod setup;

#[cfg(test)]
pub(crate) use compose::setup_on;
pub use compose::{
    admits, bound_field, class_fields, compose_bind, compose_check, compose_confirm,
    compose_link_account, compose_plain, compose_setup, compose_sign_in, field_from, kept_source,
    native_card, native_lines, param_hint, site_name, value_bound, Marking, NativeCard, Plain,
    SetupParam, CARD_COLUMNS, CARD_LINES,
};
pub use confirm::{confirm_step, ConfirmAnswer, ConfirmMove, ConfirmState, Confirmation};
pub use key::Key;
pub use presenter::{presenter, request_presenter};
pub use raise::{
    choices, handback_kind, handback_result, params, raise_confirm, raise_step, Raise, Source,
    CONFIRM_CLASSES,
};
#[cfg(test)]
pub(crate) use setup::typed_line;
pub use setup::{setup_step, Setup, SetupBody, SetupInput, SetupMove, SetupState};
