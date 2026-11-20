use serde_json::{json, Value};

use halvo_schema::executable::names::YesNo;

use super::{
    civil, date, days_of, instant_text, seconds, start_day, verdict, window, Event, EventCounts,
    EventToDelete, Events, RunDay, Verdict,
};

const NOV_12_2026: i64 = 20_769;

fn event(id: &str, start: &str, cancelled: bool, response: &str) -> Value {
    json!({
        "id": id,
        "subject": format!("s-{id}"),
        "start": {"dateTime": start, "timeZone": "UTC"},
        "end": {"dateTime": start, "timeZone": "UTC"},
        "isCancelled": cancelled,
        "responseStatus": {"response": response},
        "isOrganizer": false,
    })
}

fn kind(item: &Value) -> &'static str {
    match verdict(item, RunDay(NOV_12_2026)) {
        Verdict::Kept(_) => "kept",
        Verdict::Other => "other",
        Verdict::NoKey => "no_key",
        Verdict::Organized => "organized",
        Verdict::Incomplete => "incomplete",
    }
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
        "/v1.0/me/calendarView?startDateTime=2026-10-13T00%3A00%3A00Z&endDateTime=2026-11-13T00%3A00%3A00Z&$top=25&$select=id%2Csubject%2Cstart%2CisCancelled%2CresponseStatus%2CisOrganizer"
    );
}

#[test]
fn an_instant_is_written_to_its_whole_second_in_utc() {
    assert_eq!(instant_text(0), "1970-01-01T00:00:00Z");
    assert_eq!(instant_text(-1), "1969-12-31T23:59:59Z");
    assert_eq!(instant_text(1_794_441_600 + 3_723), "2026-11-12T01:02:03Z");
    assert_eq!(seconds("2026-11-12T01:02:03"), Some(1_794_441_600 + 3_723));
    assert_eq!(
        seconds("2026-11-12T01:02:03.9999999"),
        Some(1_794_441_600 + 3_723)
    );
    assert_eq!(seconds("1969-12-31T23:59:59"), Some(-1));
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
fn the_rule_keeps_cancelled_or_declined_events_that_start_in_the_window_and_that_another_person_organized(
) {
    let rows = [
        (
            event("e1", "2026-10-12T23:59:59.9999999", true, "organizer"),
            "other",
        ),
        (
            event("e2", "2026-10-13T00:00:00.0000000", true, "organizer"),
            "kept",
        ),
        (
            event("e3", "2026-11-12T23:59:59.9999999", false, "declined"),
            "kept",
        ),
        (
            event("e4", "2026-11-13T00:00:00.0000000", true, "organizer"),
            "other",
        ),
        (
            event("e5", "2026-11-01T10:00:00.0000000", false, "accepted"),
            "other",
        ),
        (
            event(
                "e6",
                "2026-11-01T10:00:00.0000000",
                false,
                "tentativelyAccepted",
            ),
            "other",
        ),
        (
            event("e7", "2026-11-01T10:00:00.0000000", false, "Declined"),
            "other",
        ),
        (
            event("e8", "2026-11-02T10:00:00.0000000", true, "accepted"),
            "kept",
        ),
    ];
    for (item, want) in rows {
        assert_eq!(kind(&item), want, "{item}");
    }
    let Verdict::Kept(kept) = verdict(&rows_first_kept(), RunDay(NOV_12_2026)) else {
        panic!("kept");
    };
    assert_eq!(
        (kept.id(), kept.subject(), kept.start()),
        ("e2", "s-e2", "2026-10-13T00:00:00.0000000")
    );
}

fn rows_first_kept() -> Value {
    event("e2", "2026-10-13T00:00:00.0000000", true, "organizer")
}

#[test]
fn an_event_that_the_person_organized_is_left_out_and_counted_only_where_the_rule_would_select_it()
{
    let organized = |mut item: Value| {
        item["isOrganizer"] = json!(true);
        item
    };
    let rows = [
        (
            organized(event("e1", "2026-11-01T10:00:00", true, "organizer")),
            "organized",
        ),
        (
            organized(event("e2", "2026-11-01T10:00:00", false, "declined")),
            "organized",
        ),
        (
            organized(event("e3", "2026-11-01T10:00:00", false, "organizer")),
            "other",
        ),
        (
            organized(event("e4", "2026-10-12T10:00:00", true, "organizer")),
            "other",
        ),
        (
            organized(event("e5", "2026-11-01T10:00:00", false, "accepted")),
            "other",
        ),
    ];
    for (item, want) in rows {
        assert_eq!(kind(&item), want, "{item}");
    }
}

#[test]
fn an_item_without_an_id_or_a_key_that_the_rule_reads_counts_in_no_key() {
    let whole = event("e1", "2026-11-01T10:00:00", true, "organizer");
    assert_eq!(kind(&whole), "kept");
    for pointer in [
        "/id",
        "/start",
        "/start/dateTime",
        "/start/timeZone",
        "/isCancelled",
        "/responseStatus",
        "/responseStatus/response",
        "/isOrganizer",
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
        assert_eq!(kind(&item), "no_key", "{pointer}");
    }
    for (pointer, value) in [
        ("/id", json!(7)),
        ("/start/timeZone", json!("Pacific Standard Time")),
        ("/start/timeZone", json!("utc")),
        ("/start/dateTime", json!("2026-11-01T10:00:00Z")),
        ("/isCancelled", json!("true")),
        ("/isOrganizer", json!("false")),
        ("/isOrganizer", json!(null)),
    ] {
        let mut item = whole.clone();
        *item.pointer_mut(pointer).expect("the key") = value;
        assert_eq!(kind(&item), "no_key", "{pointer}");
    }
    let mut outside = event("e9", "2020-01-01T10:00:00", false, "accepted");
    outside
        .as_object_mut()
        .expect("an object")
        .remove("isOrganizer");
    assert_eq!(kind(&outside), "no_key", "the key counts before the window");
}

#[test]
fn a_subject_that_is_null_or_missing_reads_as_empty_and_another_one_is_incomplete() {
    let mut item = event("e1", "2026-11-01T10:00:00", true, "organizer");
    item["subject"] = json!(null);
    let Verdict::Kept(kept) = verdict(&item, RunDay(NOV_12_2026)) else {
        panic!("kept");
    };
    assert_eq!(kept.subject(), "");
    item.as_object_mut().expect("an object").remove("subject");
    assert_eq!(kind(&item), "kept");
    item["subject"] = json!(3);
    assert_eq!(kind(&item), "incomplete");
    item["isOrganizer"] = json!(true);
    assert_eq!(
        kind(&item),
        "organized",
        "the organizer's count comes first"
    );
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
    let counts = EventCounts {
        cut: YesNo::No,
        no_key: 0,
        organized: 0,
        incomplete: 0,
        more_pages: YesNo::No,
        earlier_page: 0,
    };
    assert_eq!(
        format!(
            "{:?}",
            Events {
                kept: vec![event],
                counts
            }
        ),
        "Events { len: 1 }"
    );
    assert_eq!(format!("{:?}", RunDay(3)), "RunDay(3)");
}
