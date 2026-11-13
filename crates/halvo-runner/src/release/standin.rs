//! The suite's stand-in request maker: plain TCP against the fixture server, test code alone (the coordinator's
//! ruling CP-142 (3)), and the two refusal tests of A5.4 that run the port's suite with it.

use std::net::TcpStream;
use std::time::Duration;

use halvo_policy::Facts;
use halvo_schema::executable::names::{FieldName, Source};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::OwnValue;
use halvo_schema::executable::{ActionClass, Bound, BoundField, Cause, WebOrigin};
use halvo_schema::Category;

use super::suite::{
    answer_of, changed_on_page, commit_without_reply, http, request_bytes, send_bytes, slot,
    stopped_between_begin_and_commit, Case, Fixture, Maker,
};
use super::{Left, Paused, Replied};

/// How long the stand-in waits for the reply of a request that left.
const REPLY_WAIT: Duration = Duration::from_millis(2000);

fn field(name: FieldName, from: Source, origin: Option<&WebOrigin>, value: &str) -> BoundField {
    BoundField {
        name,
        from,
        origin: origin.cloned(),
        value: OwnValue::new(value),
    }
}

/// The text between `start` and the next `end` in `text`.
fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = text.find(start)? + start.len();
    let to = text[from..].find(end)? + from;
    Some(&text[from..to])
}

/// A request that left on its own connection.
struct Sent(TcpStream);

impl Left for Sent {
    fn reply(self: Box<Self>) -> Replied {
        match answer_of(self.0, REPLY_WAIT) {
            Some(_) => Replied::Reply,
            None => Replied::NoReply,
        }
    }
}

/// `DELETE` of one calendar event, the first that the run's own list call returned.
struct Delete {
    port: u16,
    token: String,
    ids: Vec<String>,
}

impl Paused for Delete {
    fn class(&self) -> ActionClass {
        ActionClass::Delete
    }

    fn bound(&mut self) -> Result<Bound, Cause> {
        let id = self.ids.first().ok_or(Cause::StepFailed)?;
        Bound::new(
            ActionClass::Delete,
            vec![field(FieldName::Items, Source::Extract, None, id)],
        )
        .map_err(|_| Cause::StepFailed)
    }

    fn facts(&self) -> Facts {
        Facts {
            ids: self.ids.clone(),
            ..Facts::default()
        }
    }

    fn release(self: Box<Self>) -> Box<dyn Left> {
        let auth = format!("Bearer {}", self.token);
        let path = format!("/v1.0/me/events/{}", self.ids[0]);
        let bytes = request_bytes(
            self.port,
            "DELETE",
            "calendar.agent.test",
            &path,
            &[("Authorization", &auth)],
            b"",
        );
        Box::new(Sent(send_bytes(self.port, &bytes)))
    }

    fn renew(self: Box<Self>) -> Result<Box<dyn Paused>, Cause> {
        Ok(self)
    }
}

/// The payment of the bill in the payment frame: `POST /charge` with the frame's one-time token.
struct Pay {
    port: u16,
    pay: WebOrigin,
    token: String,
    payee: String,
    instrument: String,
}

impl Paused for Pay {
    fn class(&self) -> ActionClass {
        ActionClass::Payment
    }

    fn bound(&mut self) -> Result<Bound, Cause> {
        let path = format!("/frame?kind=bill&token={}", self.token);
        let frame = http(self.port, "GET", "pay.agent.test", &path, &[], b"");
        let amount =
            between(&frame.body, "data-testid=\"amount\">", "<").ok_or(Cause::StepFailed)?;
        Bound::new(
            ActionClass::Payment,
            vec![
                field(FieldName::Payee, Source::Slot, None, &self.payee),
                field(FieldName::Amount, Source::Extract, Some(&self.pay), amount),
                field(FieldName::Instrument, Source::Slot, None, &self.instrument),
                field(FieldName::Origin, Source::Const, None, self.pay.as_str()),
            ],
        )
        .map_err(|_| Cause::StepFailed)
    }

    fn facts(&self) -> Facts {
        Facts {
            request_origin: Some(self.pay.clone()),
            ..Facts::default()
        }
    }

    fn release(self: Box<Self>) -> Box<dyn Left> {
        let form = format!(
            "token={}&kind=bill&instrument={}",
            self.token,
            self.instrument.replace(' ', "+")
        );
        let bytes = request_bytes(
            self.port,
            "POST",
            "pay.agent.test",
            "/charge",
            &[("Content-Type", "application/x-www-form-urlencoded")],
            form.as_bytes(),
        );
        Box::new(Sent(send_bytes(self.port, &bytes)))
    }

    fn renew(self: Box<Self>) -> Result<Box<dyn Paused>, Cause> {
        Ok(self)
    }
}

/// The stand-in maker.
struct Standin;

fn text(value: &serde_json::Value) -> String {
    value.as_str().expect("a text").to_string()
}

impl Maker for Standin {
    fn category(&self, case: Case) -> Category {
        match case {
            Case::NoReply => Category::CalendarCleanup,
            Case::AmountChanges => Category::UtilityBillPayment,
        }
    }

    fn bindings(&self, fixture: &Fixture, case: Case) -> (Vec<WebOrigin>, Vec<Slot>) {
        let seed = fixture.seed();
        match case {
            Case::NoReply => (vec![fixture.origin("calendar")], Vec::new()),
            Case::AmountChanges => (
                vec![fixture.origin("utility"), fixture.origin("pay")],
                vec![
                    slot("payee", &text(&seed["utility"]["payee"])),
                    slot("instrument", &text(&seed["utility"]["instruments"][0])),
                ],
            ),
        }
    }

    fn paused(&mut self, fixture: &Fixture, case: Case) -> Box<dyn Paused> {
        let seed = fixture.seed();
        let port = fixture.port();
        match case {
            Case::NoReply => {
                let token = text(&seed["credentials"]["token"]);
                let auth = format!("Bearer {token}");
                let listed = http(
                    port,
                    "GET",
                    "calendar.agent.test",
                    "/v1.0/me/calendarView?startDateTime=2000-01-01T00:00:00&endDateTime=2100-01-01T00:00:00",
                    &[("Authorization", &auth)],
                    b"",
                );
                let value: serde_json::Value =
                    serde_json::from_str(&listed.body).expect("the calendar view");
                let ids = value["value"]
                    .as_array()
                    .expect("events")
                    .iter()
                    .map(|e| text(&e["id"]))
                    .collect();
                Box::new(Delete { port, token, ids })
            }
            Case::AmountChanges => {
                let form = format!(
                    "email={}&password={}",
                    text(&seed["credentials"]["user"]).replace('@', "%40"),
                    text(&seed["credentials"]["password"])
                );
                let signed = http(
                    port,
                    "POST",
                    "utility.agent.test",
                    "/signin",
                    &[("Content-Type", "application/x-www-form-urlencoded")],
                    form.as_bytes(),
                );
                let session = signed
                    .headers
                    .iter()
                    .filter(|(n, _)| n == "set-cookie")
                    .find_map(|(_, v)| {
                        v.split(';')
                            .next()
                            .filter(|c| c.starts_with("halvo_fx_session="))
                    })
                    .expect("a session cookie")
                    .to_string();
                let page = http(
                    port,
                    "GET",
                    "utility.agent.test",
                    "/pay",
                    &[("Cookie", &session)],
                    b"",
                );
                let token = between(&page.body, "token=", "\"")
                    .expect("the frame's token")
                    .to_string();
                Box::new(Pay {
                    port,
                    pay: fixture.origin("pay"),
                    token,
                    payee: text(&seed["utility"]["payee"]),
                    instrument: text(&seed["utility"]["instruments"][0]),
                })
            }
        }
    }
}

#[test]
fn refusal_commit_without_reply() {
    commit_without_reply(&mut Standin);
}

#[test]
fn refusal_changed_on_page() {
    changed_on_page(&mut Standin);
}

#[test]
fn a_runner_stopped_between_begin_and_commit_leaves_its_effect_unknown_after_the_runs_end() {
    stopped_between_begin_and_commit(&mut Standin);
}
