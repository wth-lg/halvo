use tauri_plugin_dialog::MessageDialogResult;

/// The dialog's title (I11.5).
pub const TITLE: &str = "Trial title";

/// The label of the rejecting control.
pub const REJECT: &str = "Don't pay";

/// The label of the allowing control.
pub const ALLOW: &str = "Pay";

/// The response of an `NSAlert` to its first and its second button.
pub const ALERT_FIRST: isize = 1000;
pub const ALERT_SECOND: isize = 1001;

/// The ids of the two buttons of the system form's task dialog, above the common button ids.
pub const REJECT_ID: i32 = 101;
pub const ALLOW_ID: i32 = 100;

/// A form of the native dialog, in the order of I11.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    Plugin,
    System,
}

/// What a dialog returned. For the form `system`, `allow` comes only from the control labelled `ALLOW`; for the form `plugin` it also comes from a failed or cancelled dialog and from a response that rfd does not list, which the plugin hands back as `ALLOW`'s label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Allow,
    Reject,
}

impl Form {
    pub fn name(self) -> &'static str {
        match self {
            Form::Plugin => "plugin",
            Form::System => "system",
        }
    }

    pub fn parse(word: &str) -> Option<Form> {
        [Form::Plugin, Form::System]
            .into_iter()
            .find(|form| form.name() == word)
    }
}

impl Answer {
    pub fn name(self) -> &'static str {
        match self {
            Answer::Allow => "allow",
            Answer::Reject => "reject",
        }
    }
}

/// The five lines `Label <i>: <value>` of I11.5.
pub fn lines() -> [String; 5] {
    let values = [
        "one".to_string(),
        "two".to_string(),
        "x".repeat(200),
        "<b>bold</b> &amp;".to_string(),
        "five".to_string(),
    ];
    let mut i = 0;
    values.map(|value| {
        i += 1;
        format!("Label {i}: {value}")
    })
}

/// The dialog's message: the five lines joined by line feeds.
pub fn body() -> String {
    lines().join("\n")
}

/// The two labels in the order that the plugin's `OkCancelCustom` takes them: the first slot
/// is the button that `rfd` adds first, which both systems make the default.
pub fn plugin_slots() -> (&'static str, &'static str) {
    (REJECT, ALLOW)
}

/// The plugin's result as an answer.
pub fn from_plugin(result: &MessageDialogResult) -> Answer {
    match result {
        MessageDialogResult::Custom(label) if label == ALLOW => Answer::Allow,
        _ => Answer::Reject,
    }
}

/// The response of the system form's `NSAlert`, whose second button is `ALLOW`.
pub fn from_alert(response: isize) -> Answer {
    if response == ALERT_SECOND {
        Answer::Allow
    } else {
        Answer::Reject
    }
}

/// The result of the system form's task dialog: `shown` is false when the call failed.
pub fn from_task_dialog(shown: bool, button: i32) -> Answer {
    if shown && button == ALLOW_ID {
        Answer::Allow
    } else {
        Answer::Reject
    }
}

/// The line printed before show `n`.
pub fn shown_line(form: Form, n: u32) -> String {
    format!("dialog show form={} n={n}", form.name())
}

/// The line printed when show `n` returned.
pub fn answer_line(form: Form, n: u32, answer: Answer) -> String {
    format!(
        "dialog answer form={} n={n} result={}",
        form.name(),
        answer.name()
    )
}
