use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Moment {
    Checked,
    Aside,
}

type Hook = Box<dyn Fn(Moment)>;

thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

pub(crate) fn reach(moment: Moment) {
    HOOK.with(|hook| {
        if let Some(hook) = hook.borrow().as_ref() {
            hook(moment);
        }
    });
}

struct Clear;

impl Drop for Clear {
    fn drop(&mut self) {
        HOOK.with(|hook| *hook.borrow_mut() = None);
    }
}

pub(crate) fn with<T>(hook: impl Fn(Moment) + 'static, body: impl FnOnce() -> T) -> T {
    HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    let _clear = Clear;
    body()
}
