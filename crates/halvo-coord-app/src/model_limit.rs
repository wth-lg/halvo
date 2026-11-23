//! The person's limit for hosted models in the app (r7 8.1L, its second pull request; the coordinator's log lines
//! 17895 and 18047): the page's two commands of the part of the section AI model that shows the count and its limit,
//! `model_limit_state` and `set_model_limit`. Each asks the agent store's `model_room` on an owner stream and answers
//! the page finished texts: a state word and each amount as `halvo_route::amount::written` writes it, so that the page
//! does no arithmetic. This file is the one sender of the body `new_model_count`, which lifts what is counted: a press
//! sends it only where the app's own last answer to the part was the state `unread` made from a `model_room` that
//! answered the fault of the store's read (`internal`), and the press's own `model_room` answers that fault again. The
//! page names no body and cannot choose one.
#![cfg(any(target_os = "macos", windows))]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use halvo_agent_client::{LinkError, OwnerLink};
use halvo_coord::ports::PortError;
use halvo_route::admit::{cap_state, CapState, Room};
use halvo_route::amount::{written, Amount};
use halvo_route::cap::{Currency, Period};
use halvo_route::release;
use serde::Serialize;
use serde_json::{json, Value};

use crate::product::{hex, link_over, random16, result_of, StreamCall};

/// The period that the part names: the count turns by calendar months whatever the build's cap line says.
pub const PERIOD: Period = Period::Month;

/// The bounds of a limit that a person sets, in minor units: 0.01 to 1,000,000.00, the bounds of a cap line.
const MINOR_MIN: u32 = 1;
const MINOR_MAX: u32 = 100_000_000;

/// The error name with which the owner answers a request whose read met a fault of the store.
const FAULT: &str = "internal";

/// What the part says of the count: by `cap_state` with no refusal kept, `ok`, `near` and `reached`; `none`, no count
/// yet; `no_limit`, no cap at all; `unread`, the count does not read; `not_known`, the app cannot ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitState {
    Ok,
    Near,
    Reached,
    None,
    NoLimit,
    Unread,
    NotKnown,
}

/// What a set that went through answers: `set`, or `set_new` where the count is new.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetAnswer {
    Set,
    SetNew,
}

/// Why a set did not go through: `no_amount`, a text that is no amount and went nowhere, or `failed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitCause {
    NoAmount,
    Failed,
}

/// A reply of a command of the limit to the window: finished texts and words, no figure for the page to work on but
/// the end of the period, which the page writes as a day on the person's own clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum LimitReply {
    /// `model_limit_state` in a build with a host.
    State {
        host: String,
        state: LimitState,
        cap: Option<String>,
        spent: Option<String>,
        held: Option<String>,
        period: &'static str,
        period_end_ms: Option<u64>,
    },
    /// `model_limit_state` in a build with no host: `{"host":null}`, with nothing sent.
    NoHost { host: () },
    /// A set that went through: the new limit as written for a person.
    Set {
        answer: SetAnswer,
        amount: String,
        period: &'static str,
    },
    /// A set that did not go through.
    Error { error: LimitCause },
}

/// The app's own memory of its last answer to the part: whether that answer was the state `unread` made from a
/// `model_room` that answered the fault of the store's read, and the number of that answer, one more at each answer
/// that it keeps. Only such a state answer keeps `unread`; a `model_room` that answers a room, every other error of
/// it, and every set that went through keep its end. It lives in the app, never in the page. It holds the part's one
/// lock too (the coordinator's log line 18279): a command of the part holds it from its ask of `model_room` to its
/// keep, so the app serves one command of the part at a time, and a command that comes meanwhile waits for it; no
/// command takes it twice. A press reads the memory once more BEFORE it waits for the lock, at its begin, so that a
/// state answer that a waiting press did not see before it began cannot open the start of a new count for it.
#[derive(Debug, Default)]
pub struct Memory {
    lock: Mutex<()>,
    kept: AtomicU64,
    #[cfg(test)]
    begins: (Mutex<usize>, std::sync::Condvar),
}

/// One answer of the memory: its number, and whether it is the state `unread` made from the fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Kept {
    number: u64,
    unread: bool,
}

impl Memory {
    pub const fn new() -> Memory {
        Memory {
            lock: Mutex::new(()),
            kept: AtomicU64::new(0),
            #[cfg(test)]
            begins: (Mutex::new(0), std::sync::Condvar::new()),
        }
    }

    /// The lock for one command, held until the guard drops. A poisoned lock is taken as it stands: it guards no value,
    /// and the memory's one word is written whole.
    fn hold(&self) -> MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The newest answer kept.
    fn read(&self) -> Kept {
        let word = self.kept.load(Ordering::SeqCst);
        Kept {
            number: word >> 1,
            unread: word & 1 == 1,
        }
    }

    /// What a press reads at its begin, before it waits for the lock.
    fn begin(&self) -> Kept {
        let begun = self.read();
        #[cfg(test)]
        {
            let (count, changed) = &self.begins;
            *count.lock().unwrap_or_else(PoisonError::into_inner) += 1;
            changed.notify_all();
        }
        begun
    }

    /// Keeps an answer under the lock that `_held` shows: the next number, and whether it is the state `unread`.
    fn keep(&self, _held: &MutexGuard<'_, ()>, unread: bool) {
        let next = ((self.read().number + 1) << 1) | u64::from(unread);
        self.kept.store(next, Ordering::SeqCst);
    }
}

/// What `model_room` gave: the room as the store answered it, the fault of the store's read, or nothing that reads.
enum Asked {
    Room(Found),
    Fault,
    Other,
}

/// The figures of a `model_room` reply, read as the store's reply holds them.
struct Found {
    room: String,
    cap_minor: Option<u64>,
    spent: Option<u64>,
    held: Option<u64>,
    period_end_ms: Option<u64>,
}

/// The two commands of the limit over ONE owner link for each call, with the app's memory of its last answer.
pub struct ModelLimit<'m> {
    link: Arc<dyn OwnerLink>,
    memory: &'m Memory,
}

impl<'m> ModelLimit<'m> {
    /// The limit's calls over `link`.
    pub fn new(link: Arc<dyn OwnerLink>, memory: &'m Memory) -> ModelLimit<'m> {
        ModelLimit { link, memory }
    }

    /// The limit's calls over the owner streams that `owner_stream` gives.
    pub fn over(owner_stream: StreamCall, memory: &'m Memory) -> ModelLimit<'m> {
        ModelLimit::new(link_over(owner_stream), memory)
    }

    /// The limit's calls where no service answers: each request fails as one that meets no service.
    pub fn no_service(memory: &'m Memory) -> ModelLimit<'m> {
        ModelLimit::new(Arc::new(NoService), memory)
    }

    /// The reply of the page's command `command` with `args` in a build whose host is `host`, none for a name that is
    /// not one of the two. `model_limit_state` sends nothing where there is no host; `set_model_limit` sets the limit
    /// of the text `amount` of `args`, the empty text where there is none.
    pub fn call(
        &self,
        host: Option<&str>,
        command: &str,
        args: Option<&Value>,
    ) -> Option<LimitReply> {
        Some(match command {
            "model_limit_state" => match host {
                Some(host) => self.state(host),
                None => LimitReply::NoHost { host: () },
            },
            "set_model_limit" => {
                let text = args.and_then(|a| a.get("amount")).and_then(Value::as_str);
                self.set(text.unwrap_or_default())
            }
            _ => return None,
        })
    }

    /// The state of the count by ONE body `model_room`, with its texts. The answer `internal` is the state `unread`
    /// and sets the app's memory; every other answer clears it.
    pub fn state(&self, host: &str) -> LimitReply {
        let currency = currency();
        let text = |amount: Option<u64>, unit: fn(u64) -> Amount| {
            amount.map(|amount| written(unit(amount), currency))
        };
        let asked = {
            let held = self.memory.hold();
            let asked = self.room();
            self.memory.keep(&held, matches!(asked, Asked::Fault));
            asked
        };
        let (state, cap, spent, held, end) = match asked {
            Asked::Fault => (LimitState::Unread, None, None, None, None),
            Asked::Other => (LimitState::NotKnown, None, None, None, None),
            Asked::Room(found) => match (found.room.as_str(), found.cap_minor) {
                ("reads", Some(cap_minor)) => {
                    let room = Room {
                        cap_minor,
                        spent: found.spent.unwrap_or_default(),
                        held: found.held.unwrap_or_default(),
                        period_end_ms: found.period_end_ms.unwrap_or_default(),
                    };
                    let state = match cap_state(Some(&room), false) {
                        CapState::Reached => LimitState::Reached,
                        CapState::Near => LimitState::Near,
                        CapState::Ok | CapState::NoCap => LimitState::Ok,
                    };
                    (
                        state,
                        Some(cap_minor),
                        found.spent,
                        found.held,
                        found.period_end_ms,
                    )
                }
                ("reads", None) => (
                    LimitState::NoLimit,
                    None,
                    found.spent,
                    found.held,
                    found.period_end_ms,
                ),
                ("none", Some(cap_minor)) => (LimitState::None, Some(cap_minor), None, None, None),
                ("none", None) => (LimitState::NoLimit, None, None, None, None),
                _ => (LimitState::Unread, None, None, None, None),
            },
        };
        LimitReply::State {
            host: host.to_owned(),
            state,
            cap: text(cap, Amount::Minor),
            spent: text(spent, Amount::Micro),
            held: text(held, Amount::Micro),
            period: PERIOD.as_str(),
            period_end_ms: end,
        }
    }

    /// Sets the person's limit to the amount of `text`. A text that is no amount (`minor_of`) is `no_amount` and is
    /// sent nowhere. Else ONE body `model_room` first: where it answers the fault of the store's read and the app's
    /// last answer to the part was the state made from that fault, ONE body `new_model_count`; where it answers the
    /// fault behind any other last answer, nothing more and `failed`; in every other case ONE body `set_model_cap`.
    /// The last answer counts only where the press read it at its begin, before it waited for the lock, and it still
    /// stands, unchanged, under the lock. A set that went through clears the app's memory and answers the new limit as
    /// written for a person.
    pub fn set(&self, text: &str) -> LimitReply {
        let Some(cap_minor) = minor_of(text) else {
            return LimitReply::Error {
                error: LimitCause::NoAmount,
            };
        };
        let begun = self.memory.begin();
        let held = self.memory.hold();
        let op = match self.room() {
            Asked::Fault if begun.unread && self.memory.read() == begun => "new_model_count",
            Asked::Fault => {
                return LimitReply::Error {
                    error: LimitCause::Failed,
                }
            }
            Asked::Room(_) | Asked::Other => {
                self.memory.keep(&held, false);
                "set_model_cap"
            }
        };
        let answer = self.ask(op, &format!(r#","cap_minor":{cap_minor}"#)).ok();
        let new_count = answer
            .as_ref()
            .filter(|result| result.get("cap_minor") == Some(&json!(cap_minor)))
            .and_then(|result| result.get("new_count"))
            .and_then(Value::as_str);
        let answer = match (op, new_count) {
            ("set_model_cap", Some("no")) => SetAnswer::Set,
            (_, Some("yes")) => SetAnswer::SetNew,
            _ => {
                return LimitReply::Error {
                    error: LimitCause::Failed,
                }
            }
        };
        self.memory.keep(&held, false);
        drop(held);
        LimitReply::Set {
            answer,
            amount: written(Amount::Minor(u64::from(cap_minor)), currency()),
            period: PERIOD.as_str(),
        }
    }

    /// ONE body `model_room`: the room, the fault of the store's read, or every other outcome.
    fn room(&self) -> Asked {
        match self.ask("model_room", "") {
            Ok(result) => found(&result).map_or(Asked::Other, Asked::Room),
            Err(PortError::Refused(name)) if name.as_str() == FAULT => Asked::Fault,
            Err(_) => Asked::Other,
        }
    }

    /// The result of ONE coordinator body `op` with `workflow` and `agent` null and the keys `more`.
    fn ask(&self, op: &str, more: &str) -> Result<Value, PortError> {
        let id = hex(&random16()?);
        let request = format!(
            r#"{{"v":1,"id":"{id}","request":"coordinator_write","body":{{"op":"{op}","workflow":null,"agent":null{more}}}}}"#
        );
        let reply = self
            .link
            .request(request.as_bytes())
            .map_err(|_| PortError::NoService)?;
        result_of(&reply, Some(&id))
    }
}

/// The currency of this build's cap line, and where that line does not parse the one currency of the closed list.
fn currency() -> Currency {
    release::cap().map_or(Currency::ALL[0], |cap| cap.currency)
}

/// The figures of a `model_room` result: `room` and the four figures, each a whole number or null; none for a result
/// of another shape.
fn found(result: &Value) -> Option<Found> {
    let figure = |key: &str| match result.get(key)? {
        Value::Null => Some(None),
        value => value.as_u64().map(Some),
    };
    let room = result.get("room")?.as_str()?;
    if !matches!(room, "none" | "reads" | "unread") {
        return None;
    }
    Some(Found {
        room: room.to_owned(),
        cap_minor: figure("cap_minor")?,
        spent: figure("spent")?,
        held: figure("held")?,
        period_end_ms: figure("period_end_ms")?,
    })
}

/// The minor units of `text`: digits, then at most two decimals behind a point, from 0.01 to 1,000,000.00; else
/// none.
fn minor_of(text: &str) -> Option<u32> {
    let (units, decimals) = text.split_once('.').unwrap_or((text, "00"));
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if !digits(units) || !digits(decimals) || decimals.len() > 2 || units.len() > 7 {
        return None;
    }
    let cents = if decimals.len() == 1 {
        decimals.parse::<u32>().ok()? * 10
    } else {
        decimals.parse::<u32>().ok()?
    };
    let minor = units
        .parse::<u32>()
        .ok()?
        .checked_mul(100)?
        .checked_add(cents)?;
    (MINOR_MIN..=MINOR_MAX).contains(&minor).then_some(minor)
}

/// The link where no service answers.
struct NoService;

impl OwnerLink for NoService {
    fn request(&self, _request: &[u8]) -> Result<Vec<u8>, LinkError> {
        Err(LinkError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Condvar, Mutex};
    use std::time::Duration;

    const HOST: &str = "api.host.invalid";

    /// What the stand-in owner answers one request: a result, a refusal by its error name, or no reply.
    #[derive(Clone)]
    enum Answer {
        Result(String),
        Refused(&'static str),
        NoReply,
    }

    fn result(text: &str) -> Answer {
        Answer::Result(text.to_owned())
    }

    /// Whether `n` presses have read the memory at their begin within `within`.
    fn begun_within(memory: &Memory, n: usize, within: Duration) -> bool {
        let (count, changed) = &memory.begins;
        let count = count.lock().unwrap();
        let (count, _) = changed
            .wait_timeout_while(count, within, |count| *count < n)
            .unwrap();
        *count >= n
    }

    /// A gate that holds a stand-in's answer until the test opens it.
    #[derive(Default)]
    struct Gate {
        open: Mutex<bool>,
        opened: Condvar,
    }

    impl Gate {
        fn open(&self) {
            *self.open.lock().unwrap() = true;
            self.opened.notify_all();
        }

        fn wait(&self) {
            let mut open = self.open.lock().unwrap();
            while !*open {
                open = self.opened.wait(open).unwrap();
            }
        }
    }

    /// A stand-in owner link that keeps each request as JSON and answers the requests in order with `script`; where a
    /// gate is set, the next answer waits until the gate opens.
    struct Stand {
        script: Mutex<VecDeque<Answer>>,
        kept: Mutex<Vec<Value>>,
        asked: Condvar,
        gate: Mutex<Option<Arc<Gate>>>,
    }

    impl Stand {
        fn new() -> Arc<Stand> {
            Arc::new(Stand {
                script: Mutex::new(VecDeque::new()),
                kept: Mutex::new(Vec::new()),
                asked: Condvar::new(),
                gate: Mutex::new(None),
            })
        }

        /// Holds the answer of the next request until `gate` opens.
        fn hold_next(&self, gate: Arc<Gate>) {
            *self.gate.lock().unwrap() = Some(gate);
        }

        /// Whether `n` requests have come within `within`.
        fn asked_within(&self, n: usize, within: Duration) -> bool {
            let kept = self.kept.lock().unwrap();
            let (kept, _) = self
                .asked
                .wait_timeout_while(kept, within, |kept| kept.len() < n)
                .unwrap();
            kept.len() >= n
        }

        /// The answers of the next requests, in their order.
        fn then(&self, answers: impl IntoIterator<Item = Answer>) {
            self.script.lock().unwrap().extend(answers);
        }

        fn ops(&self) -> Vec<String> {
            self.kept
                .lock()
                .unwrap()
                .iter()
                .map(|r| r["body"]["op"].as_str().unwrap_or_default().to_owned())
                .collect()
        }

        fn last(&self) -> Value {
            self.kept
                .lock()
                .unwrap()
                .last()
                .cloned()
                .unwrap_or(Value::Null)
        }
    }

    impl OwnerLink for Stand {
        fn request(&self, request: &[u8]) -> Result<Vec<u8>, LinkError> {
            let request: Value = serde_json::from_slice(request).expect("a request reads as JSON");
            let id = request["id"].to_string();
            self.kept.lock().unwrap().push(request);
            self.asked.notify_all();
            let gate = self.gate.lock().unwrap().take();
            if let Some(gate) = gate {
                gate.wait();
            }
            let answer = self.script.lock().unwrap().pop_front();
            match answer {
                Some(Answer::Result(result)) => {
                    Ok(format!(r#"{{"v":1,"id":{id},"ok":{result}}}"#).into_bytes())
                }
                Some(Answer::Refused(name)) => {
                    Ok(format!(r#"{{"v":1,"id":{id},"error":"{name}"}}"#).into_bytes())
                }
                Some(Answer::NoReply) | None => Err(LinkError),
            }
        }
    }

    fn state(limit: &ModelLimit<'_>) -> Value {
        serde_json::to_value(limit.call(Some(HOST), "model_limit_state", None)).unwrap()
    }

    fn press(limit: &ModelLimit<'_>, amount: &str) -> Value {
        let args = json!({ "amount": amount });
        serde_json::to_value(limit.call(Some(HOST), "set_model_limit", Some(&args))).unwrap()
    }

    const READS: &str = r#"{"room":"reads","cap_minor":2000,"spent":4384770,"held":10001,"period_end_ms":1797092000000}"#;
    const NONE: &str =
        r#"{"room":"none","cap_minor":2000,"spent":0,"held":0,"period_end_ms":null}"#;
    const UNREAD: &str =
        r#"{"room":"unread","cap_minor":null,"spent":null,"held":null,"period_end_ms":null}"#;

    fn cap_reply(cap_minor: u32, new_count: &str) -> Answer {
        Answer::Result(format!(
            r#"{{"cap_minor":{cap_minor},"new_count":"{new_count}"}}"#
        ))
    }

    /// r7 8.1L view, sentence (5): one press sets the limit, higher or lower, and the answer names the new amount. The
    /// count reads with a limit of 20.00 USD; a press of 25 sends `set_model_cap` of 2,500 minor units and answers
    /// "25.00 USD", a press of 5.5 sends 550 and answers "5.50 USD", and where the store answers that the count is new
    /// the answer is `set_new`.
    #[test]
    fn a_raise_of_the_cap_answers_the_new_amount() {
        let memory = Memory::new();
        for (text, cap_minor, new_count, answer, amount) in [
            ("25", 2_500, "no", "set", "25.00 USD"),
            ("5.5", 550, "no", "set", "5.50 USD"),
            ("20.05", 2_005, "yes", "set_new", "20.05 USD"),
        ] {
            let link = Stand::new();
            let limit = ModelLimit::new(link.clone(), &memory);
            link.then([result(READS), cap_reply(cap_minor, new_count)]);
            assert_eq!(
                press(&limit, text),
                json!({"answer": answer, "amount": amount, "period": "month"}),
                "{text}"
            );
            assert_eq!(link.ops(), ["model_room", "set_model_cap"], "{text}");
            assert_eq!(
                link.last()["body"],
                json!({"op": "set_model_cap", "workflow": null, "agent": null, "cap_minor": cap_minor}),
                "{text}"
            );
        }
    }

    /// r7 8.1L view, sentence (6): a text that is no amount sends nothing and answers `no_amount`; an amount is digits
    /// with at most two decimals behind a point, from 0.01 to 1000000.00.
    #[test]
    fn a_text_that_is_no_amount_sends_nothing() {
        let memory = Memory::new();
        for text in [
            "",
            " 20",
            "20 ",
            "20,50",
            "20.",
            ".5",
            "20.505",
            "0",
            "0.00",
            "1000000.01",
            "10000000",
            "-5",
            "+5",
            "1e3",
            "twenty",
            "2O",
            "\u{0662}0",
            "20.5.0",
        ] {
            let link = Stand::new();
            let limit = ModelLimit::new(link.clone(), &memory);
            assert_eq!(
                press(&limit, text),
                json!({"error": "no_amount"}),
                "{text:?}"
            );
            assert!(link.ops().is_empty(), "{text:?}");
        }
        for (text, minor) in [
            ("0.01", 1),
            ("20", 2_000),
            ("20.5", 2_050),
            ("20.50", 2_050),
            ("007", 700),
            ("1000000.00", 100_000_000),
        ] {
            assert_eq!(minor_of(text), Some(minor), "{text:?}");
        }
    }

    /// r7 8.1L view, sentence (8), THE GUARD: behind the fault of the store's read a press sends `new_model_count`; in
    /// every other state a press sends `set_model_cap`. Each row asks the state first, as the page does, then presses
    /// once: no count, a count that reads (ok, near, reached), no limit at all, a row that does not read, the error
    /// `store`, the error `unsupported` and no reply each send `model_room` twice and then `set_model_cap`; the fault
    /// `internal` at the state and at the press sends `model_room` twice and then `new_model_count`.
    #[test]
    fn behind_the_fault_of_the_read_a_press_sends_the_new_body_and_in_every_other_state_set_model_cap(
    ) {
        let near = r#"{"room":"reads","cap_minor":2000,"spent":16000000,"held":0,"period_end_ms":1797092000000}"#;
        let reached = r#"{"room":"reads","cap_minor":2000,"spent":20000000,"held":0,"period_end_ms":1797092000000}"#;
        let no_limit =
            r#"{"room":"reads","cap_minor":null,"spent":1,"held":0,"period_end_ms":1797092000000}"#;
        let state_of = |state: &str, cap: Value, spent: Value, held: Value, end: Value| json!({"host": HOST, "state": state, "cap": cap, "spent": spent, "held": held, "period": "month", "period_end_ms": end});
        let end = json!(1_797_092_000_000u64);
        let unknown = state_of(
            "not_known",
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
        );
        let unread = state_of("unread", Value::Null, Value::Null, Value::Null, Value::Null);
        let rows = [
            (
                result(NONE),
                state_of(
                    "none",
                    json!("20.00 USD"),
                    Value::Null,
                    Value::Null,
                    Value::Null,
                ),
                "set_model_cap",
                cap_reply(2_500, "yes"),
            ),
            (
                result(READS),
                state_of(
                    "ok",
                    json!("20.00 USD"),
                    json!("4.39 USD"),
                    json!("0.02 USD"),
                    end.clone(),
                ),
                "set_model_cap",
                cap_reply(2_500, "no"),
            ),
            (
                result(near),
                state_of(
                    "near",
                    json!("20.00 USD"),
                    json!("16.00 USD"),
                    json!("0.00 USD"),
                    end.clone(),
                ),
                "set_model_cap",
                cap_reply(2_500, "no"),
            ),
            (
                result(reached),
                state_of(
                    "reached",
                    json!("20.00 USD"),
                    json!("20.00 USD"),
                    json!("0.00 USD"),
                    end.clone(),
                ),
                "set_model_cap",
                cap_reply(2_500, "no"),
            ),
            (
                result(no_limit),
                state_of(
                    "no_limit",
                    Value::Null,
                    json!("0.01 USD"),
                    json!("0.00 USD"),
                    end.clone(),
                ),
                "set_model_cap",
                cap_reply(2_500, "no"),
            ),
            (
                result(UNREAD),
                unread.clone(),
                "set_model_cap",
                cap_reply(2_500, "yes"),
            ),
            (
                Answer::Refused("store"),
                unknown.clone(),
                "set_model_cap",
                cap_reply(2_500, "no"),
            ),
            (
                Answer::Refused("unsupported"),
                unknown.clone(),
                "set_model_cap",
                cap_reply(2_500, "no"),
            ),
            (
                Answer::NoReply,
                unknown.clone(),
                "set_model_cap",
                cap_reply(2_500, "no"),
            ),
            (
                Answer::Refused("internal"),
                unread,
                "new_model_count",
                cap_reply(2_500, "yes"),
            ),
        ];
        for (room, shown, op, reply) in rows {
            let memory = Memory::new();
            let link = Stand::new();
            let limit = ModelLimit::new(link.clone(), &memory);
            link.then([room.clone()]);
            assert_eq!(state(&limit), shown, "{op} behind {shown}");
            link.then([room, reply]);
            let pressed = press(&limit, "25");
            assert_eq!(link.ops(), ["model_room", "model_room", op], "{shown}");
            assert_eq!(
                link.last()["body"],
                json!({"op": op, "workflow": null, "agent": null, "cap_minor": 2_500}),
                "{shown}"
            );
            assert_eq!(pressed["amount"], "25.00 USD", "{shown}");
        }
    }

    /// r7 8.1L view, the guard's second side (the coordinator's log lines 18047 and 18224): one passing fault at a
    /// press behind a good count starts nothing, and a state read that answers the fault, then ONE press whose own read
    /// faults again, starts the count. (a) The state reads a good count; the press meets the fault: it sends `model_room` alone and answers
    /// `failed`, and the state then reads the count as before; a press then sends `set_model_cap`. (b) The state reads
    /// a good count; a press meets the fault and answers `failed`; the state meets the fault (`unread`); the next
    /// press meets it again and sends `new_model_count`, answered as a new count. Behind it the memory is clear: one
    /// more fault at a press answers `failed` and sends nothing more.
    #[test]
    fn one_fault_at_a_press_starts_nothing_and_two_faults_in_a_row_start_the_count() {
        let fault = || Answer::Refused("internal");
        let failed = json!({"error": "failed"});

        let memory = Memory::new();
        let link = Stand::new();
        let limit = ModelLimit::new(link.clone(), &memory);
        link.then([result(READS)]);
        assert_eq!(state(&limit)["state"], "ok");
        link.then([fault()]);
        assert_eq!(press(&limit, "25"), failed, "(a) one fault at a press");
        assert_eq!(
            link.ops(),
            ["model_room", "model_room"],
            "(a) the press sends model_room alone"
        );
        link.then([result(READS)]);
        assert_eq!(
            state(&limit)["spent"],
            "4.39 USD",
            "(a) the count reads as before"
        );
        link.then([result(READS), cap_reply(2_500, "no")]);
        assert_eq!(press(&limit, "25")["answer"], "set");
        assert_eq!(link.ops()[4], "set_model_cap");

        let memory = Memory::new();
        let link = Stand::new();
        let limit = ModelLimit::new(link.clone(), &memory);
        link.then([result(READS)]);
        assert_eq!(state(&limit)["state"], "ok");
        link.then([fault()]);
        assert_eq!(
            press(&limit, "25"),
            failed,
            "(b) the first fault at a press"
        );
        link.then([fault()]);
        assert_eq!(state(&limit)["state"], "unread", "(b) the state behind it");
        link.then([fault(), cap_reply(2_500, "yes")]);
        assert_eq!(
            press(&limit, "25"),
            json!({"answer": "set_new", "amount": "25.00 USD", "period": "month"}),
            "(b) the second fault at a press"
        );
        assert_eq!(
            link.ops(),
            [
                "model_room",
                "model_room",
                "model_room",
                "model_room",
                "new_model_count"
            ]
        );
        link.then([fault()]);
        assert_eq!(press(&limit, "25"), failed, "(b) behind the new count");
        assert_eq!(
            link.ops().len(),
            6,
            "(b) behind the new count the press sends model_room alone"
        );
    }

    /// r7 8.1L view, the round of verifier-54 (the coordinator's log line 18279): the app serves one command of the part
    /// at a time, so that two calls at once start no count on one passing fault. Each call goes on a link of its own, as
    /// the app gives each call a stream of its own, and both share the part's one memory. X1: behind a good count a press
    /// asks `model_room` and its answer is held; a state call starts beside it; the press's read is then answered the
    /// fault: the press answers `failed` and sends nothing more, and the state call, served behind it, answers `unread`.
    /// X2: the part was answered `unread` earlier; a state call asks `model_room` and its answer, a count that reads, is
    /// held; a press starts beside it and meets the fault behind it: the state call answers `ok`, and the press answers
    /// `failed` and sends nothing more. L3, the mirror of X1 (verifier-54's run): behind a good count a state call asks
    /// and its answer, the fault, is held; a press begins beside it and waits at the lock; the held answer opens and the
    /// press's own read meets the fault: the state call answers `unread`, and the press answers `failed` and sends
    /// nothing more, since the part was not answered `unread` before the press began. THREE: a press waits behind a
    /// state call whose answer reads, with a second state call that meets the fault waiting too: whatever order the lock
    /// serves them in, the press answers `failed` and sends nothing more. A call that comes while another holds the memory waits for it; the test gives
    /// each second call 100 ms to ask before it opens the held answer.
    #[test]
    fn two_calls_at_once_start_no_count_on_one_passing_fault() {
        let beside = Duration::from_millis(100);
        let long = Duration::from_secs(10);
        let failed = json!({"error": "failed"});
        let fault = || Answer::Refused("internal");

        let memory = Memory::new();
        let good = Stand::new();
        good.then([result(READS)]);
        assert_eq!(state(&ModelLimit::new(good, &memory))["state"], "ok");
        let (of_press, of_state, gate) = (Stand::new(), Stand::new(), Arc::new(Gate::default()));
        of_press.hold_next(Arc::clone(&gate));
        of_press.then([fault(), cap_reply(2_500, "yes")]);
        of_state.then([fault()]);
        std::thread::scope(|s| {
            let pressing = s.spawn(|| press(&ModelLimit::new(of_press.clone(), &memory), "25"));
            assert!(
                of_press.asked_within(1, long),
                "X1: the press asked model_room"
            );
            let reading = s.spawn(|| state(&ModelLimit::new(of_state.clone(), &memory)));
            of_state.asked_within(1, beside);
            gate.open();
            assert_eq!(pressing.join().unwrap(), failed, "X1: the press");
            assert_eq!(
                reading.join().unwrap()["state"],
                "unread",
                "X1: the state call"
            );
        });
        assert_eq!(of_press.ops(), ["model_room"], "X1: the press's requests");

        let memory = Memory::new();
        let first = Stand::new();
        first.then([fault()]);
        assert_eq!(state(&ModelLimit::new(first, &memory))["state"], "unread");
        let (of_press, of_state, gate) = (Stand::new(), Stand::new(), Arc::new(Gate::default()));
        of_state.hold_next(Arc::clone(&gate));
        of_state.then([result(READS)]);
        of_press.then([fault(), cap_reply(2_500, "yes")]);
        std::thread::scope(|s| {
            let reading = s.spawn(|| state(&ModelLimit::new(of_state.clone(), &memory)));
            assert!(
                of_state.asked_within(1, long),
                "X2: the state call asked model_room"
            );
            let pressing = s.spawn(|| press(&ModelLimit::new(of_press.clone(), &memory), "25"));
            of_press.asked_within(1, beside);
            gate.open();
            assert_eq!(reading.join().unwrap()["state"], "ok", "X2: the state call");
            assert_eq!(pressing.join().unwrap(), failed, "X2: the press");
        });
        assert_eq!(of_press.ops(), ["model_room"], "X2: the press's requests");

        let memory = Memory::new();
        let good = Stand::new();
        good.then([result(READS)]);
        assert_eq!(state(&ModelLimit::new(good, &memory))["state"], "ok");
        let (of_press, of_state, gate) = (Stand::new(), Stand::new(), Arc::new(Gate::default()));
        of_state.hold_next(Arc::clone(&gate));
        of_state.then([fault()]);
        of_press.then([fault(), cap_reply(2_500, "yes")]);
        std::thread::scope(|s| {
            let reading = s.spawn(|| state(&ModelLimit::new(of_state.clone(), &memory)));
            assert!(
                of_state.asked_within(1, long),
                "L3: the state call asked model_room"
            );
            let pressing = s.spawn(|| press(&ModelLimit::new(of_press.clone(), &memory), "25"));
            assert!(begun_within(&memory, 1, long), "L3: the press began");
            assert!(
                !of_press.asked_within(1, beside),
                "L3: the press waits at the lock"
            );
            gate.open();
            assert_eq!(
                reading.join().unwrap()["state"],
                "unread",
                "L3: the state call"
            );
            assert_eq!(pressing.join().unwrap(), failed, "L3: the press");
        });
        assert_eq!(of_press.ops(), ["model_room"], "L3: the press's requests");

        let memory = Memory::new();
        let good = Stand::new();
        good.then([result(READS)]);
        assert_eq!(state(&ModelLimit::new(good, &memory))["state"], "ok");
        let (of_press, first, second) = (Stand::new(), Stand::new(), Stand::new());
        let gate = Arc::new(Gate::default());
        first.hold_next(Arc::clone(&gate));
        first.then([result(READS)]);
        second.then([fault()]);
        of_press.then([fault(), cap_reply(2_500, "yes")]);
        std::thread::scope(|s| {
            let reading = s.spawn(|| state(&ModelLimit::new(first.clone(), &memory)));
            assert!(
                first.asked_within(1, long),
                "THREE: the first state call asked model_room"
            );
            let pressing = s.spawn(|| press(&ModelLimit::new(of_press.clone(), &memory), "25"));
            assert!(begun_within(&memory, 1, long), "THREE: the press began");
            let faulting = s.spawn(|| state(&ModelLimit::new(second.clone(), &memory)));
            second.asked_within(1, beside);
            gate.open();
            assert_eq!(
                reading.join().unwrap()["state"],
                "ok",
                "THREE: the first state call"
            );
            assert_eq!(
                faulting.join().unwrap()["state"],
                "unread",
                "THREE: the second state call"
            );
            assert_eq!(pressing.join().unwrap(), failed, "THREE: the press");
        });
        assert_eq!(
            of_press.ops(),
            ["model_room"],
            "THREE: the press's requests"
        );
    }

    /// r7 8.1L view: the part names the period month, because the count turns by calendar months, and the build's cap
    /// line names month too; this test fails when that line names another period.
    #[test]
    fn the_part_names_the_month_and_the_builds_cap_line_names_month() {
        assert_eq!(PERIOD.as_str(), "month");
        assert_eq!(release::cap().map(|cap| cap.period), Some(PERIOD));
    }

    /// r4 U3.17b sentence 8, here because the tree scan of the body that starts a new count allows this file: the read
    /// of the key's and the count's states for the agents' rows (`crate::product::hosting`) goes through `ModelLimit`
    /// with a memory of its own (`crate::product::RowsMemory`), so that it never moves the memory of the limit's part.
    /// Behind a part whose last answer was the state made from the fault of the store's read (set), a read for the
    /// rows that meets a room leaves it set: a press whose own read faults again still starts a new count. Behind a
    /// part whose last answer was a room (clear), a read for the rows that meets the fault leaves it clear: a press
    /// whose read faults answers failed and starts none.
    #[test]
    fn a_read_for_the_rows_leaves_the_memory_of_the_limits_part_as_it_was() {
        let fault = || Answer::Refused(FAULT);
        let room = || {
            result(
                r#"{"room":"reads","cap_minor":2000,"spent":0,"held":0,"period_end_ms":1800000000000}"#,
            )
        };
        let host = "hosted-model.test";
        for (part_last, rows_meet, starts) in [(fault(), room(), true), (room(), fault(), false)] {
            let part = Memory::new();
            let last = Stand::new();
            last.then([part_last]);
            ModelLimit::new(last, &part).state(host);
            let rows = Stand::new();
            rows.then([result(r#"{"stands":"yes"}"#), rows_meet]);
            let link: Arc<dyn OwnerLink> = rows.clone();
            let read = crate::product::hosting(
                &link,
                &crate::product::RowsMemory::default(),
                Some(host.to_owned()),
                &["test-model-small"],
                true,
            );
            assert!(read.is_some(), "{starts}");
            assert_eq!(rows.ops(), ["model_key_state", "model_room"]);
            let press = Stand::new();
            press.then([fault(), cap_reply(2_500, "yes")]);
            let answer = ModelLimit::new(press.clone(), &part).set("25");
            let ops = press.ops();
            assert_eq!(
                ops.iter().any(|op| op == "new_model_count"),
                starts,
                "{ops:?}"
            );
            assert_eq!(
                matches!(answer, LimitReply::Set { .. }),
                starts,
                "{answer:?}"
            );
        }
    }
}
