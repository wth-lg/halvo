use std::fmt;

use reqwest::Method;
use serde_json::Value;

use crate::answer::{self, Ids, FORM};
use crate::error::CallError;
use crate::graph::{segment, Graph};
use crate::token::Token;

const DAY_SECONDS: u64 = 86_400;
const WINDOW_DAYS: i64 = 30;

/// The calendar operations of A11.2, each of the class of its row: `calendar_list` is `read` and
/// `calendar_delete` is `delete`. Each takes the token for its call alone.
pub trait Calendar {
    /// GET of calendarView, once, for the window of the clean-up rule: from 00:00 UTC 30 days before `day` to
    /// 00:00 UTC of the day after `day`. Then the rule: the events that start on one of those 31 days in UTC and
    /// that are cancelled or that the user declined, in the order of the answer. Keeps `events`. Refuses an
    /// answer that goes on in another page, that holds an id twice, whose start is not a Graph time in UTC, or
    /// whose items lack a key of the face, with `Answer`.
    fn calendar_list(&self, token: &Token, day: RunDay) -> Result<Events, CallError>;

    /// DELETE of the one event that `event` holds: one effect. `Events::into_deletes` gives one handle per event
    /// and this call takes the handle, so an event is deleted at most once:
    ///
    /// ```compile_fail
    /// use halvo_connect::{Calendar, EventToDelete, Graph, Token};
    /// fn twice(graph: &Graph, token: &Token, event: EventToDelete) {
    ///     let _ = graph.calendar_delete(token, event);
    ///     let _ = graph.calendar_delete(token, event);
    /// }
    /// ```
    ///
    /// ```
    /// use halvo_connect::{Calendar, EventToDelete, Graph, Token};
    /// fn once(graph: &Graph, token: &Token, event: EventToDelete) {
    ///     let _ = graph.calendar_delete(token, event);
    /// }
    /// ```
    fn calendar_delete(&self, token: &Token, event: EventToDelete) -> Result<(), CallError>;
}

/// The run's day in UTC, counted in whole days from 1970-01-01.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RunDay(i64);

impl RunDay {
    /// The day in UTC that holds the instant `seconds` after 1970-01-01T00:00:00Z.
    pub fn from_unix_seconds(seconds: u64) -> RunDay {
        RunDay((seconds / DAY_SECONDS) as i64)
    }
}

/// An event that `calendar_list` kept, as the provider gave it. `Debug` prints none of its text.
#[derive(Clone)]
pub struct Event {
    id: String,
    subject: String,
    start: String,
}

impl Event {
    /// The provider's id of the event, the bound field `items` of `delete` (A9.4).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Its subject.
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// Its start in UTC, as the provider wrote it: `YYYY-MM-DDTHH:MM:SS` with up to seven digits of a fraction.
    pub fn start(&self) -> &str {
        &self.start
    }
}

/// The kept list `events` of A11.2. Only `calendar_list` makes one. It has no `Clone`, so that one list gives its
/// handles once:
///
/// ```compile_fail
/// fn copy(events: &halvo_connect::Events) -> halvo_connect::Events {
///     events.clone()
/// }
/// ```
pub struct Events(Vec<Event>);

impl Events {
    /// The events, in the order of the provider's answer.
    pub fn iter(&self) -> std::slice::Iter<'_, Event> {
        self.0.iter()
    }

    /// The count of events.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the rule selected no event.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// One handle per event, in order, each for one `calendar_delete`.
    pub fn into_deletes(self) -> impl Iterator<Item = EventToDelete> {
        self.0.into_iter().map(EventToDelete)
    }
}

/// The handle of one event of `events` for one `calendar_delete`. It has no `Clone` and no constructor outside
/// this crate:
///
/// ```compile_fail
/// fn copy(handle: &halvo_connect::EventToDelete) -> halvo_connect::EventToDelete {
///     handle.clone()
/// }
/// ```
///
/// ```compile_fail
/// fn make(item: &halvo_connect::Event) -> halvo_connect::EventToDelete {
///     halvo_connect::EventToDelete(item.clone())
/// }
/// ```
///
/// ```
/// fn look(handle: &halvo_connect::EventToDelete) -> &halvo_connect::Event {
///     handle.event()
/// }
/// ```
pub struct EventToDelete(Event);

impl EventToDelete {
    /// The event that the handle deletes.
    pub fn event(&self) -> &Event {
        &self.0
    }
}

impl Calendar for Graph {
    fn calendar_list(&self, token: &Token, day: RunDay) -> Result<Events, CallError> {
        let answer = self.json(token, Method::GET, &window(day), None, 200)?;
        listed(&answer, day).map(Events)
    }

    fn calendar_delete(&self, token: &Token, event: EventToDelete) -> Result<(), CallError> {
        self.effect(
            token,
            Method::DELETE,
            &format!("/v1.0/me/events/{}", segment(&event.0.id)),
            204,
        )
    }
}

fn days_of(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn date(days: i64) -> String {
    let (year, month, day) = civil(days);
    format!("{year:04}-{month:02}-{day:02}")
}

fn start_day(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let shape = (bytes.len() == 19 || (21..=27).contains(&bytes.len()))
        && bytes.iter().enumerate().all(|(i, &b)| match i {
            4 | 7 => b == b'-',
            10 => b == b'T',
            13 | 16 => b == b':',
            19 => b == b'.',
            _ => b.is_ascii_digit(),
        });
    if !shape {
        return None;
    }
    let number = |from: usize, to: usize| text[from..to].parse::<i64>().ok();
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    if number(11, 13)? > 23 || number(14, 16)? > 59 || number(17, 19)? > 59 {
        return None;
    }
    let days = days_of(year, month, day);
    (civil(days) == (year, month, day)).then_some(days)
}

fn window(day: RunDay) -> String {
    format!(
        "/v1.0/me/calendarView?startDateTime={}T00:00:00Z&endDateTime={}T00:00:00Z",
        date(day.0 - WINDOW_DAYS),
        date(day.0 + 1)
    )
}

fn listed(answer: &Value, day: RunDay) -> Result<Vec<Event>, CallError> {
    let mut ids = Ids::default();
    let mut kept = Vec::new();
    for item in answer::page(answer)? {
        let id = answer::id(item)?;
        ids.first(&id)?;
        let subject = answer::text(item, "subject")?;
        let start = item.get("start").ok_or(FORM)?;
        if start.get("timeZone").and_then(Value::as_str) != Some("UTC") {
            return Err(FORM);
        }
        let start = answer::text(start, "dateTime")?;
        let start_day = start_day(&start).ok_or(FORM)?;
        let cancelled = item
            .get("isCancelled")
            .and_then(Value::as_bool)
            .ok_or(FORM)?;
        let response = item
            .get("responseStatus")
            .and_then(|status| status.get("response"))
            .and_then(Value::as_str)
            .ok_or(FORM)?;
        if (day.0 - WINDOW_DAYS..=day.0).contains(&start_day)
            && (cancelled || response == "declined")
        {
            kept.push(Event { id, subject, start });
        }
    }
    Ok(kept)
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Event").finish_non_exhaustive()
    }
}

impl fmt::Debug for Events {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Events")
            .field("len", &self.0.len())
            .finish()
    }
}

impl fmt::Debug for EventToDelete {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventToDelete").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
