use super::{ALLOW, REJECT};
use crate::present::outcome::{from_alert, from_task_dialog, ALERT_ALLOW, TASK_ALLOW, TASK_REJECT};
use crate::present::{decide, Outcome};

#[test]
fn the_mapping_allows_only_the_response_of_the_allowing_control() {
    assert_eq!(decide(Outcome::Response(7), 7), ALLOW);
    assert_eq!(decide(Outcome::Response(6), 7), REJECT);
    assert_eq!(decide(Outcome::Failed, 7), REJECT);
    assert_eq!(decide(Outcome::TimedOut, 7), REJECT);
}

#[test]
fn alert_the_press_on_allow_allows_and_the_press_on_reject_rejects() {
    assert_eq!(from_alert(Some(ALERT_ALLOW)), ALLOW);
    assert_eq!(from_alert(Some(1000)), REJECT);
}

#[test]
fn alert_a_failed_call_rejects() {
    assert_eq!(from_alert(None), REJECT);
}

#[test]
fn alert_a_cancel_response_rejects() {
    for cancel in [0, 1, -1000, -1001, -1002] {
        assert_eq!(from_alert(Some(cancel)), REJECT, "{cancel}");
    }
}

#[test]
fn alert_an_unknown_response_rejects() {
    for unknown in [1002, 1003, 999, -1, 2, isize::MAX, isize::MIN] {
        assert_eq!(from_alert(Some(unknown)), REJECT, "{unknown}");
    }
}

#[test]
fn alert_a_timeout_rejects() {
    assert_eq!(decide(Outcome::TimedOut, ALERT_ALLOW), REJECT);
}

#[test]
fn task_dialog_the_press_on_allow_allows_and_the_press_on_reject_rejects() {
    assert_eq!(from_task_dialog(Some(TASK_ALLOW)), ALLOW);
    assert_eq!(from_task_dialog(Some(TASK_REJECT)), REJECT);
}

#[test]
fn task_dialog_a_failed_call_rejects() {
    assert_eq!(from_task_dialog(None), REJECT);
}

#[test]
fn task_dialog_a_cancel_response_rejects() {
    for cancel in [2, 8, 3] {
        assert_eq!(from_task_dialog(Some(cancel)), REJECT, "{cancel}");
    }
}

#[test]
fn task_dialog_an_unknown_response_rejects() {
    for unknown in [0, 1, 4, 5, 6, 7, 99, 102, -1, i32::MAX, i32::MIN] {
        assert_eq!(from_task_dialog(Some(unknown)), REJECT, "{unknown}");
    }
}

#[test]
fn task_dialog_a_timeout_rejects() {
    assert_eq!(decide(Outcome::TimedOut, TASK_ALLOW), REJECT);
}

#[test]
fn task_dialog_the_two_ids_differ_and_neither_is_an_id_of_the_systems_own_buttons() {
    assert_ne!(TASK_ALLOW, TASK_REJECT);
    for own in (1..=11).chain([32000]) {
        assert_ne!(
            TASK_ALLOW, own,
            "the allowing id is the system's own id {own}"
        );
        assert_ne!(
            TASK_REJECT, own,
            "the rejecting id is the system's own id {own}"
        );
    }
}
