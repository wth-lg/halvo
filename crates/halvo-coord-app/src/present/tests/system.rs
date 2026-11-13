use super::pay;
#[cfg(windows)]
use super::REJECT;
#[cfg(windows)]
use crate::present::outcome::{from_task_dialog, TASK_ALLOW, TASK_REJECT};

#[test]
fn the_dialog_adds_the_rejecting_control_first() {
    assert_eq!(
        crate::present::system::controls(&pay()),
        ["Don't pay", "Pay"]
    );
}

#[cfg(windows)]
#[test]
fn the_task_dialog_rejects_by_default_cancels_and_closes_to_reject_and_sets_no_timer() {
    use crate::present::system::windows::{Texts, FLAGS};
    use windows::Win32::UI::Controls::{TDF_ALLOW_DIALOG_CANCELLATION, TDF_CALLBACK_TIMER};
    let card = pay();
    let texts = Texts::new(&card);
    let buttons = texts.buttons();
    let on_screen: &dyn Fn() = &|| {};
    let config = texts.config(&buttons, &on_screen);
    let (default, flags) = ({ config.nDefaultButton }, { config.dwFlags });
    assert_eq!(default, TASK_REJECT);
    assert_eq!(flags, FLAGS);
    assert_ne!(flags.0 & TDF_ALLOW_DIALOG_CANCELLATION.0, 0);
    assert_eq!(flags.0 & TDF_CALLBACK_TIMER.0, 0);
    assert_eq!({ config.dwCommonButtons }.0, 0);
    assert_eq!({ config.cButtons }, 2);
    let ids: Vec<i32> = buttons.iter().map(|b| b.nButtonID).collect();
    assert_eq!(ids, [TASK_REJECT, TASK_ALLOW]);
    let text = |i: usize| unsafe { { buttons[i].pszButtonText }.to_string() }.unwrap();
    assert_eq!((text(0), text(1)), ("Don't pay".into(), "Pay".into()));
    assert_eq!(
        from_task_dialog(Some(2)),
        REJECT,
        "Escape and close give IDCANCEL"
    );
}
