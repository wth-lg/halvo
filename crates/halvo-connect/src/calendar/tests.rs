use serde_json::{json, Value};

use super::{
    civil, date, days_of, listed, start_day, window, Event, EventToDelete, Events, RunDay,
};
use crate::error::{AnswerFault, CallError};

const NOV_12_2026: i64 = 20_769;
const FORM: Result<Vec<String>, CallError> = Err(CallError::Answer(AnswerFault::Form));

fn event(id: &str, start: &str, cancelled: bool, response: &str) -> Value {
    json!({
        "id": id,
        "subject": format!("s-{id}"),
        "start": {"dateTime": start, "timeZone": "UTC"},
        "end": {"dateTime": start, "timeZone": "UTC"},
        "isCancelled": cancelled,
        "responseStatus": {"response": response},
    })
}

fn ids(answer: &Value) -> Result<Vec<String>, CallError> {
    listed(answer, RunDay(NOV_12_2026)).map(|kept| kept.into_iter().map(|e| e.id).collect())
}

#[test]
fn days_and_dates_agree() {
    assert_eq!(days_of(1970, 1, 1), 0);
    assert_eq!(days_of(1969, 12, 31), -1);
    assert_eq!(days_of(2000, 3, 1), 11_017);
    assert_eq!(days_of(2024, 2, 29), 19_782);
    assert_eq!(days_of(2026, 11, 12), NOV_12_2026);
    assert_eq!(civil(-1), (1969, 12, 31));
    assert_eq!(civil(19_782), (2024, 2, 29));
    for days in -800_000..800_000 {
        let (year, month, day) = civil(days);
        assert!(
            (1..=12).contains(&month) && (1..=31).contains(&day),
            "{days}"
        );
        assert_eq!(days_of(year, month, day), days, "{days}");
    }
    assert_eq!(date(0), "1970-01-01");
    assert_eq!(date(NOV_12_2026 - 30), "2026-10-13");
    assert_eq!(date(NOV_12_2026 + 1), "2026-11-13");
}

#[test]
fn the_run_day_is_the_utc_day_of_its_instant() {
    assert_eq!(RunDay::from_unix_seconds(0), RunDay(0));
    assert_eq!(RunDay::from_unix_seconds(86_399), RunDay(0));
    assert_eq!(RunDay::from_unix_seconds(86_400), RunDay(1));
    assert_eq!(
        RunDay::from_unix_seconds(1_794_441_600),
        RunDay(NOV_12_2026)
    );
    assert_eq!(
        RunDay::from_unix_seconds(1_794_441_599),
        RunDay(NOV_12_2026 - 1)
    );
}

#[test]
fn the_window_is_31_whole_days_that_end_with_the_run_day() {
    assert_eq!(
        window(RunDay(NOV_12_2026)),
        "/v1.0/me/calendarView?startDateTime=2026-10-13T00:00:00Z&endDateTime=2026-11-13T00:00:00Z"
    );
}

#[test]
fn a_start_has_the_form_of_graph_in_utc() {
    let rows = [
        ("2026-11-08T09:00:00.0000000", Some(NOV_12_2026 - 4)),
        ("2026-11-08T09:00:00.1", Some(NOV_12_2026 - 4)),
        ("2026-11-08T09:00:00", Some(NOV_12_2026 - 4)),
        ("2024-02-29T23:59:59", Some(19_782)),
        ("2026-11-12T00:00:00", Some(NOV_12_2026)),
        ("2026-11-12T24:00:00", None),
        ("2026-11-12T23:60:00", None),
        ("2026-11-12T23:59:60", None),
        ("2026-11-12T99:99:99.0000000", None),
        ("2026-11-08T09:00:00.", None),
        ("2026-11-08T09:00:00.12345678", None),
        ("2026-11-08T09:00:00Z", None),
        ("2026-11-08T09:00:00+00:00", None),
        ("2026-11-08 09:00:00", None),
        ("2026-11-08T09:00", None),
        ("2026/11/08T09:00:00", None),
        ("2026-11-08T09-00-00", None),
        ("2026-02-30T00:00:00", None),
        ("2025-02-29T00:00:00", None),
        ("2026-13-01T00:00:00", None),
        ("2026-00-01T00:00:00", None),
        ("2026-11-00T00:00:00", None),
        ("2026-11-0aT00:00:00", None),
        ("2026-11-08T09:00:0é", None),
        ("", None),
    ];
    for (text, day) in rows {
        assert_eq!(start_day(text), day, "{text}");
    }
}

#[test]
fn the_rule_keeps_cancelled_or_declined_events_that_start_in_the_window() {
    let answer = json!({"value": [
        event("e1", "2026-10-12T23:59:59.9999999", true, "organizer"),
        event("e2", "2026-10-13T00:00:00.0000000", true, "organizer"),
        event("e3", "2026-11-12T23:59:59.9999999", false, "declined"),
        event("e4", "2026-11-13T00:00:00.0000000", true, "organizer"),
        event("e5", "2026-11-01T10:00:00.0000000", false, "accepted"),
        event("e6", "2026-11-01T10:00:00.0000000", false, "tentativelyAccepted"),
        event("e7", "2026-11-01T10:00:00.0000000", false, "Declined"),
        event("e8", "2026-11-02T10:00:00.0000000", true, "accepted"),
    ]});
    assert_eq!(
        ids(&answer),
        Ok(vec!["e2".to_owned(), "e3".to_owned(), "e8".to_owned()])
    );
    let kept = listed(&answer, RunDay(NOV_12_2026)).expect("a list");
    assert_eq!(
        (kept[0].subject(), kept[0].start()),
        ("s-e2", "2026-10-13T00:00:00.0000000")
    );
    assert_eq!(ids(&json!({"value": []})), Ok(Vec::new()));
}

#[test]
fn a_paged_list_a_repeated_id_and_a_lost_id_are_refused() {
    let one = event("e2", "2026-11-01T10:00:00", true, "organizer");
    assert_eq!(
        ids(&json!({"value": [one.clone()], "@odata.nextLink": "x"})),
        Err(CallError::Answer(AnswerFault::Paged))
    );
    let outside = event("e1", "2020-01-01T10:00:00", false, "accepted");
    assert_eq!(
        ids(&json!({"value": [outside.clone(), one, outside]})),
        Err(CallError::Answer(AnswerFault::Repeated))
    );
    for id in ["", ".", ".."] {
        assert_eq!(
            ids(&json!({"value": [event(id, "2020-01-01T10:00:00", false, "accepted")]})),
            Err(CallError::Answer(AnswerFault::Id)),
            "{id}"
        );
    }
}

#[test]
fn each_key_of_the_face_is_needed() {
    let whole = event("e1", "2026-11-01T10:00:00", true, "organizer");
    for pointer in [
        "/id",
        "/subject",
        "/start",
        "/start/dateTime",
        "/start/timeZone",
        "/isCancelled",
        "/responseStatus",
        "/responseStatus/response",
    ] {
        let mut item = whole.clone();
        let (parent, key) = pointer.rsplit_once('/').expect("a pointer");
        let target = if parent.is_empty() {
            &mut item
        } else {
            item.pointer_mut(parent).expect("the parent")
        };
        target
            .as_object_mut()
            .expect("an object")
            .remove(key)
            .expect("the key");
        assert_eq!(ids(&json!({"value": [item]})), FORM, "{pointer}");
    }
    for (pointer, value) in [
        ("/start/timeZone", json!("Pacific Standard Time")),
        ("/start/timeZone", json!("utc")),
        ("/start/dateTime", json!("2026-11-01T10:00:00Z")),
        ("/isCancelled", json!("true")),
        ("/subject", json!(null)),
    ] {
        let mut item = whole.clone();
        *item.pointer_mut(pointer).expect("the key") = value;
        assert_eq!(ids(&json!({"value": [item]})), FORM, "{pointer}");
    }
}

#[test]
fn debug_prints_no_value() {
    let event = Event {
        id: "e1".to_owned(),
        subject: "s1".to_owned(),
        start: "2026-11-01T10:00:00".to_owned(),
    };
    assert_eq!(format!("{event:?}"), "Event { .. }");
    assert_eq!(
        format!("{:?}", EventToDelete(event.clone())),
        "EventToDelete { .. }"
    );
    assert_eq!(format!("{:?}", Events(vec![event])), "Events { len: 1 }");
    assert_eq!(format!("{:?}", RunDay(3)), "RunDay(3)");
}
