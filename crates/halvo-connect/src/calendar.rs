use std::fmt;

use halvo_schema::executable::names::YesNo;
use reqwest::Method;
use serde_json::Value;

use crate::answer;
use crate::error::CallError;
use crate::graph::{segment, Collection, Graph};
use crate::list::{self, PAGE_SIZE};
use crate::mail::yes_no;
use crate::token::Token;

const DAY_SECONDS: u64 = 86_400;
const WINDOW_DAYS: i64 = 30;
const BOUND: usize = 20;
const FIELDS: &str = "id,subject,start,isCancelled,responseStatus,isOrganizer";

/// The calendar operations of A11.2, each of the class of its row: `calendar_list` is `read` and
/// `calendar_delete` is `delete`. Each takes the token for its call alone.
pub trait Calendar {
    /// One chain of GET requests of calendarView for the window of the clean-up rule, from 00:00 UTC 30 days before
    /// `day` to 00:00 UTC of the day after `day`, with a page size of 25 and the fields that the rule and the face
    /// read, `isOrganizer` among them; it names no order. The chain follows each next link as it stands when the
    /// link is of the request's scheme, host and port and names calendarView without regard to ASCII case, and
    /// ends with its pages or after 10 pages. Then the rule, applied to every item: the events that start on one
    /// of those 31 days in UTC, that are cancelled or that the user declined, and that the user did not organize,
    /// the first 20 in the order of the answers; it reads no further once it holds them. Keeps `events` with the
    /// counts of what it left out. Refuses with `Answer` a page that is not the list's form, that holds more than
    /// 25 items, an id twice or an id that is empty, `.` or `..`, and a link of another place, to which no
    /// request is sent.
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

/// The kept list `events` of A11.2, with the counts of what the rule left out. Only `calendar_list` makes one. It
/// has no `Clone`, so that one list gives its handles once:
///
/// ```compile_fail
/// fn copy(events: &halvo_connect::Events) -> halvo_connect::Events {
///     events.clone()
/// }
/// ```
pub struct Events {
    kept: Vec<Event>,
    counts: EventCounts,
}

/// What `calendar_list` left out and where it stopped, on the pages that it read, in the order of A11.2 and A7.4.
/// Each event that it left out stands in one count, the first that fits in the order `no_key`, `organized`,
/// `incomplete`; an event that the rule would not select if another person had organized it stands in none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventCounts {
    /// `yes` when the call stopped at its bound of 20 events with an item of a page or a link to a next page not
    /// yet read.
    pub cut: YesNo,
    /// Items without an id or without a key that the rule reads: a `start` in UTC of the form of Graph,
    /// `isCancelled`, the response of `responseStatus` and `isOrganizer`.
    pub no_key: u32,
    /// Events that start in the window, that are cancelled or declined, and that the user organized: deleting one
    /// can send a cancellation to the people invited.
    pub organized: u32,
    /// Events that the rule selects and whose subject is neither text nor null.
    pub incomplete: u32,
    /// `yes` only when the chain read 10 pages without meeting the bound and its tenth page carried a link.
    pub more_pages: YesNo,
    /// Entries left out because an earlier page of the call gave their id; they stand outside the sum of the
    /// counts above, and no value of it says that the list was read whole.
    pub earlier_page: u32,
}

impl Events {
    /// The events, in the order of the provider's answers.
    pub fn iter(&self) -> std::slice::Iter<'_, Event> {
        self.kept.iter()
    }

    /// The count of events.
    pub fn len(&self) -> usize {
        self.kept.len()
    }

    /// Whether the rule selected no event.
    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }

    /// The counts of what the call left out and where it stopped.
    pub fn counts(&self) -> EventCounts {
        self.counts
    }

    /// One handle per event, in order, each for one `calendar_delete`.
    pub fn into_deletes(self) -> impl Iterator<Item = EventToDelete> {
        self.kept.into_iter().map(EventToDelete)
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
        let mut counts = EventCounts {
            cut: YesNo::No,
            no_key: 0,
            organized: 0,
            incomplete: 0,
            more_pages: YesNo::No,
            earlier_page: 0,
        };
        let firsts = [window(day)];
        let chains = list::read(
            self,
            token,
            Collection::CalendarView,
            &firsts,
            BOUND,
            |item| {
                let count = match verdict(item, day) {
                    Verdict::Kept(event) => return Some(event),
                    Verdict::Other => return None,
                    Verdict::NoKey => &mut counts.no_key,
                    Verdict::Organized => &mut counts.organized,
                    Verdict::Incomplete => &mut counts.incomplete,
                };
                *count = count.saturating_add(1);
                None
            },
        )?;
        counts.cut = yes_no(chains.cut);
        counts.more_pages = yes_no(chains.more_pages);
        counts.earlier_page = chains.earlier_page;
        Ok(Events {
            kept: chains.kept,
            counts,
        })
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

pub(crate) fn instant_text(seconds: i64) -> String {
    let second = seconds.rem_euclid(DAY_SECONDS as i64);
    format!(
        "{}T{:02}:{:02}:{:02}Z",
        date(seconds.div_euclid(DAY_SECONDS as i64)),
        second / 3_600,
        second / 60 % 60,
        second % 60
    )
}

fn start_day(text: &str) -> Option<i64> {
    seconds(text).map(|seconds| seconds.div_euclid(DAY_SECONDS as i64))
}

pub(crate) fn seconds(text: &str) -> Option<i64> {
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
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_of(year, month, day);
    (civil(days) == (year, month, day))
        .then_some(days * DAY_SECONDS as i64 + hour * 3_600 + minute * 60 + second)
}

fn window(day: RunDay) -> String {
    format!(
        "/v1.0/me/calendarView?startDateTime={}&endDateTime={}&$top={PAGE_SIZE}&$select={}",
        segment(&format!("{}T00:00:00Z", date(day.0 - WINDOW_DAYS))),
        segment(&format!("{}T00:00:00Z", date(day.0 + 1))),
        segment(FIELDS)
    )
}

enum Verdict {
    Kept(Event),
    Other,
    NoKey,
    Organized,
    Incomplete,
}

fn verdict(item: &Value, day: RunDay) -> Verdict {
    let start = item
        .get("start")
        .filter(|start| start.get("timeZone").and_then(Value::as_str) == Some("UTC"))
        .and_then(|start| start.get("dateTime"))
        .and_then(Value::as_str);
    let read = (
        item.get("id").and_then(Value::as_str),
        start.and_then(|text| start_day(text).map(|day| (text, day))),
        item.get("isCancelled").and_then(Value::as_bool),
        item.get("responseStatus")
            .and_then(|status| status.get("response"))
            .and_then(Value::as_str),
        item.get("isOrganizer").and_then(Value::as_bool),
    );
    let (Some(id), Some((start, start_day)), Some(cancelled), Some(response), Some(organizer)) =
        read
    else {
        return Verdict::NoKey;
    };
    if !(day.0 - WINDOW_DAYS..=day.0).contains(&start_day) || !(cancelled || response == "declined")
    {
        return Verdict::Other;
    }
    if organizer {
        return Verdict::Organized;
    }
    let Some(subject) = answer::subject(item) else {
        return Verdict::Incomplete;
    };
    Verdict::Kept(Event {
        id: id.to_owned(),
        subject,
        start: start.to_owned(),
    })
}

impl fmt::Debug for Event {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Event").finish_non_exhaustive()
    }
}

impl fmt::Debug for Events {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Events")
            .field("len", &self.kept.len())
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
