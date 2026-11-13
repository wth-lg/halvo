use halvo_coord::items::ConfirmAnswer;

/// The response of an `NSAlert` to its second button, the allowing control; the first button,
/// the rejecting control, gives 1000.
#[cfg(any(test, target_os = "macos"))]
pub const ALERT_ALLOW: isize = 1001;

/// The id of the task dialog's rejecting control, above the ids of the common buttons.
#[cfg(any(test, windows))]
pub const TASK_REJECT: i32 = 101;

/// The id of the task dialog's allowing control.
#[cfg(any(test, windows))]
pub const TASK_ALLOW: i32 = 100;

/// What a dialog's call ended with, before the app's mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome<T> {
    /// The call returned this response.
    Response(T),
    /// The call failed, or its result did not arrive.
    Failed,
    /// The dialog's own time limit ran out.
    TimedOut,
}

/// The app's one mapping (I4.9, ruling RD-42 D1): `allow` for the response `allow`, the press
/// on the allowing control, and `reject` for every other outcome, a failed call, a cancel, a
/// response that no control gives and a time limit among them.
pub fn decide<T: PartialEq>(outcome: Outcome<T>, allow: T) -> ConfirmAnswer {
    match outcome {
        Outcome::Response(response) if response == allow => ConfirmAnswer::Allow,
        _ => ConfirmAnswer::Reject,
    }
}

/// The answer of an `NSAlert`'s `runModal`, `None` when the alert did not run.
#[cfg(any(test, target_os = "macos"))]
pub fn from_alert(response: Option<isize>) -> ConfirmAnswer {
    decide(
        response.map_or(Outcome::Failed, Outcome::Response),
        ALERT_ALLOW,
    )
}

/// The answer of `TaskDialogIndirect`: the pressed button's id, `None` when the call failed.
#[cfg(any(test, windows))]
pub fn from_task_dialog(button: Option<i32>) -> ConfirmAnswer {
    decide(
        button.map_or(Outcome::Failed, Outcome::Response),
        TASK_ALLOW,
    )
}
