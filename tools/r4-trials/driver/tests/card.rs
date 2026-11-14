use r4_driver_trial::card::{self, Answer, Form};
use tauri_plugin_dialog::MessageDialogResult;

#[test]
fn the_card_holds_the_title_the_five_lines_and_the_two_controls() {
    assert_eq!(card::TITLE, "Trial title");
    assert_eq!(card::REJECT, "Don't pay");
    assert_eq!(card::ALLOW, "Pay");
    let lines = card::lines();
    assert_eq!(lines.len(), 5);
    assert_eq!(lines[0], "Label 1: one");
    assert_eq!(lines[1], "Label 2: two");
    assert_eq!(lines[2], format!("Label 3: {}", "x".repeat(200)));
    assert_eq!(lines[3], "Label 4: <b>bold</b> &amp;");
    assert_eq!(lines[4], "Label 5: five");
    assert_eq!(card::body(), lines.join("\n"));
    assert_eq!(card::body().matches('\n').count(), 4);
}

#[test]
fn only_the_allow_label_of_the_plugin_allows() {
    let custom = |s: &str| MessageDialogResult::Custom(s.to_string());
    assert_eq!(card::from_plugin(&custom("Pay")), Answer::Allow);
    assert_eq!(card::from_plugin(&custom("Don't pay")), Answer::Reject);
    assert_eq!(card::from_plugin(&custom("pay")), Answer::Reject);
    assert_eq!(card::from_plugin(&custom("Pay ")), Answer::Reject);
    for other in [
        MessageDialogResult::Ok,
        MessageDialogResult::Yes,
        MessageDialogResult::No,
        MessageDialogResult::Cancel,
    ] {
        assert_eq!(card::from_plugin(&other), Answer::Reject, "{other:?}");
    }
}

#[test]
fn the_plugin_gets_the_rejecting_label_in_the_first_slot() {
    let (first, second) = card::plugin_slots();
    assert_eq!(first, "Don't pay");
    assert_eq!(second, "Pay");
}

#[test]
fn only_the_allow_button_of_the_system_form_allows() {
    assert_eq!(card::from_alert(card::ALERT_SECOND), Answer::Allow);
    assert_eq!(card::from_alert(card::ALERT_FIRST), Answer::Reject);
    assert_eq!(card::from_alert(card::ALERT_SECOND + 1), Answer::Reject);
    assert_eq!(card::from_alert(0), Answer::Reject);
    assert_eq!(card::from_alert(-1000), Answer::Reject);
    assert_eq!(card::ALERT_FIRST, 1000);
    assert_eq!(card::ALERT_SECOND, 1001);

    assert_eq!(card::from_task_dialog(true, card::ALLOW_ID), Answer::Allow);
    assert_eq!(
        card::from_task_dialog(true, card::REJECT_ID),
        Answer::Reject
    );
    assert_eq!(card::from_task_dialog(true, 2), Answer::Reject);
    assert_eq!(card::from_task_dialog(true, 0), Answer::Reject);
    assert_eq!(
        card::from_task_dialog(false, card::ALLOW_ID),
        Answer::Reject
    );
    assert_ne!(card::ALLOW_ID, card::REJECT_ID);
    for id in [card::ALLOW_ID, card::REJECT_ID] {
        assert!(id > 7, "{id} is one of the common button ids");
    }
}

#[test]
fn the_dialog_lines_name_the_form_the_count_and_the_answer() {
    assert_eq!(
        card::shown_line(Form::Plugin, 1),
        "dialog show form=plugin n=1"
    );
    assert_eq!(
        card::shown_line(Form::System, 3),
        "dialog show form=system n=3"
    );
    assert_eq!(
        card::answer_line(Form::Plugin, 2, Answer::Allow),
        "dialog answer form=plugin n=2 result=allow"
    );
    assert_eq!(
        card::answer_line(Form::System, 1, Answer::Reject),
        "dialog answer form=system n=1 result=reject"
    );
    assert_eq!(Form::Plugin.name(), "plugin");
    assert_eq!(Form::System.name(), "system");
}
