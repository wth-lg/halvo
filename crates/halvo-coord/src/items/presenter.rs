use crate::types::{Class, ConfirmAsk, Presenter};

/// The presenter of a class, the closed table of I4.1: `purchase`, `payment` and `sign` in the
/// native dialog, `send`, `delete` and `submit` as a card in the page, and every class that the
/// table does not name in the native dialog.
pub const fn presenter(class: Class) -> Presenter {
    match class {
        Class::Send | Class::Delete | Class::Submit => Presenter::Page,
        Class::Purchase | Class::Payment | Class::Sign => Presenter::Native,
        Class::Read | Class::Prepare | Class::LocalEdit | Class::Credential => Presenter::Native,
    }
}

/// The presenter of a request: `native` when any of its effects has the presenter `native`,
/// else `page` (I4.1).
pub fn request_presenter(ask: &ConfirmAsk) -> Presenter {
    if ask
        .effects()
        .iter()
        .any(|e| presenter(e.class) == Presenter::Native)
    {
        Presenter::Native
    } else {
        Presenter::Page
    }
}
