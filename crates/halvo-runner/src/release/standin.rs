//! The suite's stand-in request maker: plain TCP against the fixture server, test code alone (the coordinator's
//! ruling CP-142 (3)), and the two refusal tests of A5.4 that run the port's suite with it.

use std::net::TcpStream;
use std::time::Duration;

use halvo_schema::executable::names::{FieldName, Source};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::OwnValue;
use halvo_schema::executable::{ActionClass, Bound, BoundField, Cause, WebOrigin};
use halvo_schema::Category;

use super::suite::{
    a_delete_answered_204_is_committed, a_delete_answered_401_is_not_committed,
    a_payment_answered_400_is_not_committed, a_rebind_then_a_reject_holds_no_approval,
    a_second_allow_for_an_instrument_not_offered_leaves_no_payment,
    an_approved_effect_that_a_later_load_refuses_holds_no_approval, answer_of, changed_on_page,
    commit_without_reply, http, origins_that_keep_the_pay_origin_let_the_request_leave,
    pay_origin_gone_at_the_load_before_the_begin, payee_changed_at_the_load_before_the_begin,
    request_bytes, send_bytes, slot, slot_changed_at_the_load_before_the_begin,
    stopped_between_begin_and_commit, Case, Fixture, Maker,
};
use super::{Left, MakerFacts, Paused, Replied};

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

/// A request that left on its own connection, and the one success status of its kind.
struct Sent(TcpStream, u16);

/// The one success status of the payment's `POST /charge`.
const PAY_MADE: u16 = 200;

/// The one success status of the delete's `DELETE`.
const DELETE_MADE: u16 = 204;

/// What the stand-in reads of the status of an answer, or of no answer, to a request whose one success status is
/// `success`: made for that status alone, so that no other success, no redirect and no refusal reads as made
/// (CP-159 G4, CP-166 E5).
fn replied(success: u16, status: Option<u16>) -> Replied {
    match status {
        Some(status) if status == success => Replied::Made,
        Some(_) => Replied::Answered,
        None => Replied::NoReply,
    }
}

impl Left for Sent {
    fn reply(self: Box<Self>) -> Replied {
        replied(self.1, answer_of(self.0, REPLY_WAIT).map(|a| a.status))
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

    fn bound(&mut self, _slots: &[Slot]) -> Result<Bound, Cause> {
        let id = self.ids.first().ok_or(Cause::StepFailed)?;
        Bound::new(
            ActionClass::Delete,
            vec![field(FieldName::Items, Source::Extract, None, id)],
        )
        .map_err(|_| Cause::StepFailed)
    }

    fn facts(&self) -> MakerFacts {
        MakerFacts {
            ids: self.ids.clone(),
            ..MakerFacts::default()
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
        Box::new(Sent(send_bytes(self.port, &bytes), DELETE_MADE))
    }

    fn renew(self: Box<Self>, _slots: &[Slot]) -> Result<Box<dyn Paused>, Cause> {
        Ok(self)
    }
}

/// The value of the slot `name` among `slots`.
fn slot_value(slots: &[Slot], name: &str) -> Result<String, Cause> {
    slots
        .iter()
        .find(|s| s.name.as_str() == name)
        .map(|s| s.value.joined())
        .ok_or(Cause::Changed)
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

    fn bound(&mut self, slots: &[Slot]) -> Result<Bound, Cause> {
        let payee = slot_value(slots, "payee")?;
        let instrument = slot_value(slots, "instrument")?;
        let path = format!("/frame?kind=bill&token={}", self.token);
        let frame = http(self.port, "GET", "pay.agent.test", &path, &[], b"");
        let amount =
            between(&frame.body, "data-testid=\"amount\">", "<").ok_or(Cause::StepFailed)?;
        Bound::new(
            ActionClass::Payment,
            vec![
                field(FieldName::Payee, Source::Slot, None, &payee),
                field(FieldName::Amount, Source::Extract, Some(&self.pay), amount),
                field(FieldName::Instrument, Source::Slot, None, &instrument),
                field(FieldName::Origin, Source::Const, None, self.pay.as_str()),
            ],
        )
        .map_err(|_| Cause::StepFailed)
    }

    fn facts(&self) -> MakerFacts {
        MakerFacts {
            request_origin: Some(self.pay.clone()),
            ..MakerFacts::default()
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
        Box::new(Sent(send_bytes(self.port, &bytes), PAY_MADE))
    }

    fn renew(self: Box<Self>, slots: &[Slot]) -> Result<Box<dyn Paused>, Cause> {
        if slot_value(slots, "payee")? != self.payee {
            return Err(Cause::Changed);
        }
        Ok(Box::new(Pay {
            instrument: slot_value(slots, "instrument")?,
            ..*self
        }))
    }
}

/// The instrument of the payment of `case`: the seed's first, or one that the service does not offer.
fn instrument_of(fixture: &Fixture, case: Case) -> String {
    if case == Case::NotOffered {
        return "Card ending 0000".to_string();
    }
    text(&fixture.seed()["utility"]["instruments"][0])
}

/// The stand-in maker.
struct Standin;

fn text(value: &serde_json::Value) -> String {
    value.as_str().expect("a text").to_string()
}

impl Maker for Standin {
    fn category(&self, case: Case) -> Category {
        match case {
            Case::NoReply | Case::AnotherToken => Category::CalendarCleanup,
            Case::AmountChanges | Case::NotOffered => Category::UtilityBillPayment,
        }
    }

    fn bindings(&self, fixture: &Fixture, case: Case) -> (Vec<WebOrigin>, Vec<Slot>) {
        let seed = fixture.seed();
        match case {
            Case::NoReply | Case::AnotherToken => (vec![fixture.origin("calendar")], Vec::new()),
            Case::AmountChanges | Case::NotOffered => (
                vec![fixture.origin("utility"), fixture.origin("pay")],
                vec![
                    slot("payee", &text(&seed["utility"]["payee"])),
                    slot("instrument", &instrument_of(fixture, case)),
                ],
            ),
        }
    }

    fn paused(&mut self, fixture: &Fixture, case: Case) -> Box<dyn Paused> {
        let seed = fixture.seed();
        let port = fixture.port();
        match case {
            Case::NoReply | Case::AnotherToken => {
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
                let token = if case == Case::AnotherToken {
                    "another-token".to_string()
                } else {
                    token
                };
                Box::new(Delete { port, token, ids })
            }
            Case::AmountChanges | Case::NotOffered => {
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
                    instrument: instrument_of(fixture, case),
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

#[test]
fn a_slot_changed_at_the_load_before_the_begin_is_asked_again_and_leaves() {
    slot_changed_at_the_load_before_the_begin(&mut Standin);
}

#[test]
fn a_payee_changed_at_the_load_before_the_begin_ends_the_effect_with_no_second_card() {
    payee_changed_at_the_load_before_the_begin(&mut Standin);
}

#[test]
fn the_pay_origin_gone_at_the_load_before_the_begin_ends_the_run_refused_origin() {
    pay_origin_gone_at_the_load_before_the_begin(&mut Standin);
}

#[test]
fn origins_that_keep_the_pay_origin_let_the_request_leave_once() {
    origins_that_keep_the_pay_origin_let_the_request_leave(&mut Standin);
}

#[test]
fn a_payment_answered_400_gets_no_committed_row() {
    a_payment_answered_400_is_not_committed(&mut Standin);
}

#[test]
fn a_delete_answered_401_gets_no_committed_row() {
    a_delete_answered_401_is_not_committed(&mut Standin);
}

#[test]
fn a_second_allow_for_an_instrument_not_offered_makes_no_payment() {
    a_second_allow_for_an_instrument_not_offered_leaves_no_payment(&mut Standin);
}

#[test]
fn a_delete_answered_204_is_made() {
    a_delete_answered_204_is_committed(&mut Standin);
}

#[test]
fn a_rebind_then_a_reject_stores_no_approval_in_the_rejected_row() {
    a_rebind_then_a_reject_holds_no_approval(&mut Standin);
}

#[test]
fn an_approved_effect_that_a_later_load_refuses_stores_no_approval() {
    an_approved_effect_that_a_later_load_refuses_holds_no_approval(&mut Standin);
}

/// CP-166 E5: the stand-in reads as made 200 for its payment and 204 for its delete, each for its own request
/// alone; 201, 202, every status from 300 to 399 and every other status are no made request, and no answer is none.
#[test]
fn the_stand_in_reads_its_own_success_status_alone_as_made() {
    for (success, other) in [(PAY_MADE, DELETE_MADE), (DELETE_MADE, PAY_MADE)] {
        for status in 100..=599u16 {
            let expected = if status == success {
                Replied::Made
            } else {
                Replied::Answered
            };
            assert_eq!(
                replied(success, Some(status)),
                expected,
                "status {status} for {success}"
            );
        }
        assert_eq!(
            replied(success, Some(other)),
            Replied::Answered,
            "the other request's success"
        );
        assert_eq!(replied(success, None), Replied::NoReply);
    }
}
