use halvo_schema::coordinator::body::{CoordLog, History};
use halvo_schema::coordinator::names::{RunEnd, Trigger};
use halvo_schema::coordinator::record::{CoordEvent, What};

const WORKFLOW: &str = "0f1e2d3c4b5a69788796a5b4c3d2e1f0";
const AGENT: &str = "1a2b3c4d5e6f708192a3b4c5d6e7f809";

const KEYS: [&str; 18] = [
    "v", "workflow", "agent", "seq", "day", "at", "event", "run", "slot", "item", "kind", "effect",
    "approval", "digest", "answer", "cause", "trigger", "origin",
];

const RUN: &str = r#""9f8e7d6c5b4a39281706f5e4d3c2b1a0""#;
const ITEM: &str = r#""7e6d5c4b3a2918070f1e2d3c4b5a6978""#;
const EFFECT: &str = r#""5c4b3a29180f7e6d5c4b3a29180f7e6d""#;
const APPROVAL: &str = r#""6d5c4b3a29180f7e6d5c4b3a29180f7e""#;
const DIGEST: &str = r#""abababababababababababababababababababababababababababababababab""#;
const ORIGIN: &str = r#""https://northwind-power.test:443""#;

/// One value of each key's own type, for a key that a form does not set.
const OWN_TYPE: [(&str, &str); 11] = [
    ("run", RUN),
    ("slot", "20767"),
    ("item", ITEM),
    ("kind", r#""stuck""#),
    ("effect", EFFECT),
    ("approval", APPROVAL),
    ("digest", DIGEST),
    ("answer", r#""done""#),
    ("cause", r#""run_ended""#),
    ("trigger", r#""person""#),
    ("origin", ORIGIN),
];

struct Form {
    name: &'static str,
    event: &'static str,
    sets: &'static [(&'static str, &'static str)],
    may_be_null: &'static [&'static str],
}

/// The row forms of I7.1 with the keys that each sets, and the keys among them that may be null there (I3.1 for
/// the sources of item_raised, I5.3 and I5.4 for the slot of slot_skipped, IG-19 for the origin), written out
/// from the contract's words.
const FORMS: &[Form] = &[
    Form {
        name: "run_started by the scheduler",
        event: "run_started",
        sets: &[
            ("run", RUN),
            ("slot", "20767"),
            ("trigger", r#""schedule""#),
        ],
        may_be_null: &[],
    },
    Form {
        name: "run_started by the person",
        event: "run_started",
        sets: &[("run", RUN), ("slot", "20767"), ("trigger", r#""person""#)],
        may_be_null: &["slot"],
    },
    Form {
        name: "run_ended",
        event: "run_ended",
        sets: &[("run", RUN), ("cause", r#""completed""#)],
        may_be_null: &[],
    },
    Form {
        name: "item_raised by a run",
        event: "item_raised",
        sets: &[("item", ITEM), ("kind", r#""confirm""#), ("run", RUN)],
        may_be_null: &[],
    },
    Form {
        name: "item_raised by the scheduler",
        event: "item_raised",
        sets: &[("item", ITEM), ("kind", r#""ready""#)],
        may_be_null: &[],
    },
    Form {
        name: "item_raised check",
        event: "item_raised",
        sets: &[("item", ITEM), ("kind", r#""check""#), ("run", RUN)],
        may_be_null: &["run"],
    },
    Form {
        name: "item_answered confirm",
        event: "item_answered",
        sets: &[
            ("item", ITEM),
            ("kind", r#""confirm""#),
            ("effect", EFFECT),
            ("approval", APPROVAL),
            ("digest", DIGEST),
            ("answer", r#""allow_once""#),
        ],
        may_be_null: &[],
    },
    Form {
        name: "item_answered sign_in second step",
        event: "item_answered",
        sets: &[
            ("item", ITEM),
            ("kind", r#""sign_in""#),
            ("answer", r#""allow""#),
            ("origin", ORIGIN),
        ],
        may_be_null: &[],
    },
    Form {
        name: "item_answered link_account",
        event: "item_answered",
        sets: &[
            ("item", ITEM),
            ("kind", r#""link_account""#),
            ("answer", r#""done""#),
            ("origin", ORIGIN),
        ],
        may_be_null: &[],
    },
    Form {
        name: "item_answered sign_in first step",
        event: "item_answered",
        sets: &[
            ("item", ITEM),
            ("kind", r#""sign_in""#),
            ("answer", r#""done""#),
        ],
        may_be_null: &[],
    },
    Form {
        name: "item_answered stuck",
        event: "item_answered",
        sets: &[
            ("item", ITEM),
            ("kind", r#""stuck""#),
            ("answer", r#""decline""#),
        ],
        may_be_null: &[],
    },
    Form {
        name: "item_answered check",
        event: "item_answered",
        sets: &[
            ("item", ITEM),
            ("kind", r#""check""#),
            ("answer", r#""went_through""#),
        ],
        may_be_null: &[],
    },
    Form {
        name: "item_answered setup",
        event: "item_answered",
        sets: &[
            ("item", ITEM),
            ("kind", r#""setup""#),
            ("answer", r#""choose""#),
        ],
        may_be_null: &[],
    },
    Form {
        name: "item_answered ready",
        event: "item_answered",
        sets: &[
            ("item", ITEM),
            ("kind", r#""ready""#),
            ("answer", r#""start""#),
        ],
        may_be_null: &[],
    },
    Form {
        name: "item_answered show",
        event: "item_answered",
        sets: &[
            ("item", ITEM),
            ("kind", r#""show""#),
            ("answer", r#""done""#),
        ],
        may_be_null: &[],
    },
    Form {
        name: "item_lapsed confirm",
        event: "item_lapsed",
        sets: &[
            ("item", ITEM),
            ("kind", r#""confirm""#),
            ("cause", r#""digest""#),
        ],
        may_be_null: &[],
    },
    Form {
        name: "item_lapsed stuck",
        event: "item_lapsed",
        sets: &[
            ("item", ITEM),
            ("kind", r#""stuck""#),
            ("cause", r#""run_ended""#),
        ],
        may_be_null: &[],
    },
    Form {
        name: "item_released",
        event: "item_released",
        sets: &[
            ("item", ITEM),
            ("kind", r#""confirm""#),
            ("effect", EFFECT),
            ("approval", APPROVAL),
            ("digest", DIGEST),
            ("answer", r#""reject_once""#),
        ],
        may_be_null: &[],
    },
    Form {
        name: "hold_set",
        event: "hold_set",
        sets: &[("cause", r#""person""#)],
        may_be_null: &[],
    },
    Form {
        name: "hold_cleared check",
        event: "hold_cleared",
        sets: &[
            ("run", RUN),
            ("answer", r#""try_again""#),
            ("cause", r#""check""#),
        ],
        may_be_null: &[],
    },
    Form {
        name: "hold_cleared person",
        event: "hold_cleared",
        sets: &[("cause", r#""person""#)],
        may_be_null: &[],
    },
    Form {
        name: "hold_cleared failures",
        event: "hold_cleared",
        sets: &[("cause", r#""failures""#)],
        may_be_null: &[],
    },
    Form {
        name: "slot_skipped",
        event: "slot_skipped",
        sets: &[("slot", "20766")],
        may_be_null: &["slot"],
    },
];

fn head(v: &str, at: &str) -> String {
    format!(r#"{{"v":{v},"workflow":"{WORKFLOW}","agent":"{AGENT}","seq":7,"day":20767,"at":{at},"#)
}

fn row_with(form: &Form, change: Option<(&str, &str)>) -> String {
    let mut text = format!(
        r#"{}"event":"{}""#,
        head("1", "1794339000000000000"),
        form.event
    );
    for key in &KEYS[7..] {
        let mut value = form
            .sets
            .iter()
            .find(|(k, _)| k == key)
            .map_or("null", |(_, v)| *v);
        if let Some((k, v)) = change {
            if k == *key {
                value = v;
            }
        }
        text.push_str(&format!(r#","{key}":{value}"#));
    }
    text.push('}');
    text
}

#[test]
fn r4_rows_every_form_of_i7_1_is_read_and_written_again() {
    for form in FORMS {
        let text = row_with(form, None);
        let value =
            CoordEvent::parse(text.as_bytes()).unwrap_or_else(|r| panic!("{}: {r}", form.name));
        assert_eq!(value.to_json().unwrap(), text, "{}", form.name);
    }
}

#[test]
fn r4_rows_a_key_that_a_form_does_not_set_is_refused_at_that_key() {
    for form in FORMS {
        for (key, value) in OWN_TYPE {
            if form.sets.iter().any(|(k, _)| *k == key) {
                continue;
            }
            let text = row_with(form, Some((key, value)));
            let refusal = CoordEvent::parse(text.as_bytes()).err().unwrap_or_else(|| {
                panic!(
                    "I7.1: {} sets no {key}, and a row with it is taken",
                    form.name
                )
            });
            assert_eq!(
                refusal.key(),
                Some(key),
                "I7.1: {} sets no {key}; the refusal is {refusal}",
                form.name
            );
        }
    }
}

#[test]
fn r4_rows_a_key_that_a_form_sets_is_not_null_unless_the_contract_lets_it_be() {
    for form in FORMS {
        for (key, _) in form.sets {
            if form.may_be_null.contains(key) {
                let text = row_with(form, Some((key, "null")));
                assert!(
                    CoordEvent::parse(text.as_bytes()).is_ok(),
                    "{}: {key} may be null",
                    form.name
                );
                continue;
            }
            let text = row_with(form, Some((key, "null")));
            assert!(
                CoordEvent::parse(text.as_bytes()).is_err(),
                "I7.1: {} sets {key}, and a row with it null is taken",
                form.name
            );
        }
    }
}

#[test]
fn r4_rows_null_means_null() {
    for form in FORMS {
        for key in &KEYS[7..] {
            let set = form.sets.iter().any(|(k, _)| k == key);
            if set && !form.may_be_null.contains(key) {
                continue;
            }
            for other in ["[]", "{}", r#""""#, "false"] {
                let text = row_with(form, Some((key, other)));
                let refusal = CoordEvent::parse(text.as_bytes()).err().unwrap_or_else(|| {
                    panic!("{}: {key} {other} stands for null and is taken", form.name)
                });
                assert_eq!(refusal.key(), Some(*key), "{}: {key} {other}", form.name);
            }
        }
    }
}

fn run_started(seq: u32, trigger: &str, slot: &str) -> String {
    format!(
        r#"{{"v":1,"workflow":"{WORKFLOW}","agent":"{AGENT}","seq":{seq},"day":20767,"at":1794339000000000000,"event":"run_started","run":{RUN},"slot":{slot},"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":null,"trigger":"{trigger}","origin":null}}"#
    )
}

#[test]
fn r4_rows_a_start_of_the_scheduler_with_a_null_slot_is_refused() {
    let text = run_started(1, "schedule", "null");
    let refusal = CoordEvent::parse(text.as_bytes()).expect_err(
        "I5.3 and I7.1: a start by the scheduler is for a slot, and run_started schedule with a null slot is taken",
    );
    assert_eq!(
        (refusal.type_name(), refusal.key(), refusal.rule()),
        ("CoordEvent", Some("slot"), "no_slot")
    );
    let log = format!(
        r#"{{"op":"coord_log","workflow":"{WORKFLOW}","agent":"{AGENT}","records":[{text}]}}"#
    );
    assert_eq!(
        CoordLog::parse(log.as_bytes())
            .map(|_| ())
            .map_err(|r| r.rule()),
        Err("no_slot"),
        "a coord_log body with run_started schedule and a null slot"
    );
    let reply = format!(
        r#"{{"records":[{},{}],"last":2}}"#,
        run_started(1, "schedule", "20767"),
        run_started(2, "schedule", "null")
    );
    assert_eq!(
        History::parse(reply.as_bytes())
            .map(|_| ())
            .map_err(|r| r.rule()),
        Err("no_slot"),
        "a coord_history reply with run_started schedule and a null slot"
    );
    let mut value = CoordEvent::parse(run_started(1, "person", "null").as_bytes()).unwrap();
    if let What::RunStarted { trigger, .. } = &mut value.what {
        *trigger = Trigger::Schedule;
    }
    assert_eq!(value.to_json().unwrap_err().rule(), "no_slot");
    assert!(serde_json::to_string(&value).is_err());
}

#[test]
fn r4_rows_a_start_of_the_person_with_a_null_slot_is_taken() {
    for (trigger, slot) in [
        ("person", "null"),
        ("person", "20767"),
        ("schedule", "20767"),
    ] {
        let text = run_started(1, trigger, slot);
        let value = CoordEvent::parse(text.as_bytes())
            .unwrap_or_else(|r| panic!("I5.3: run_started {trigger} with slot {slot}: {r}"));
        assert_eq!(value.to_json().unwrap(), text, "{trigger} {slot}");
    }
    let log = format!(
        r#"{{"op":"coord_log","workflow":"{WORKFLOW}","agent":"{AGENT}","records":[{}]}}"#,
        run_started(1, "person", "null")
    );
    assert!(
        CoordLog::parse(log.as_bytes()).is_ok(),
        "a coord_log body with run_started person and a null slot"
    );
}

fn hold_set(v: &str, at: &str) -> String {
    format!(
        r#"{}"event":"hold_set","run":null,"slot":null,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":"person","trigger":null,"origin":null}}"#,
        head(v, at)
    )
}

#[test]
fn r4_rows_at_is_any_signed_64_bit_count_but_minus_zero() {
    for at in ["-1", "-9223372036854775808", "9223372036854775807", "0"] {
        let text = hold_set("1", at);
        let value = CoordEvent::parse(text.as_bytes())
            .unwrap_or_else(|r| panic!("I7.1, W-C0.6: at {at} is a signed i64 instant; {r}"));
        assert_eq!(value.to_json().unwrap(), text, "at {at}");
    }
    for at in ["-0", "9223372036854775808", "-9223372036854775809"] {
        let refusal = CoordEvent::parse(hold_set("1", at).as_bytes()).unwrap_err();
        assert_eq!(refusal.key(), Some("at"), "at {at}");
    }
}

#[test]
fn r4_rows_v_is_the_integer_1_and_nothing_else() {
    for v in [
        "10", "0", "2", "-1", "1.0", "1e0", "1e2", r#""1""#, "true", "null",
    ] {
        let refusal = CoordEvent::parse(hold_set(v, "1").as_bytes())
            .err()
            .unwrap_or_else(|| panic!("v {v} is taken"));
        assert_eq!(refusal.key(), Some("v"), "v {v}");
    }
}

const STORED_ENDS: [&str; 14] = [
    "completed",
    "nothing_to_do",
    "declined",
    "handed_back",
    "class_mismatch",
    "refused",
    "limit",
    "effect_unknown",
    "failed",
    "cancelled",
    "lapsed",
    "interrupted",
    "budget",
    "kind",
];

fn run_ended(seq: u32, cause: &str) -> String {
    format!(
        r#"{{"v":1,"workflow":"{WORKFLOW}","agent":"{AGENT}","seq":{seq},"day":20767,"at":1794339000000000000,"event":"run_ended","run":{RUN},"slot":null,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":"{cause}","trigger":null,"origin":null}}"#
    )
}

#[test]
fn r4_rows_run_ended_holds_the_fourteen_ends_of_a_stored_row() {
    for cause in STORED_ENDS {
        let text = run_ended(1, cause);
        let value =
            CoordEvent::parse(text.as_bytes()).unwrap_or_else(|r| panic!("run_ended {cause}: {r}"));
        assert_eq!(value.to_json().unwrap(), text);
    }
    let names: Vec<&str> = RunEnd::ALL.iter().map(|e| e.as_str()).collect();
    assert_eq!(
        names, STORED_ENDS,
        "I5.6 and I7.1: a stored run_ended row holds fourteen ends, deleted not among them"
    );
}

#[test]
fn r4_rows_run_ended_with_the_cause_deleted_is_never_read() {
    let deleted = run_ended(1, "deleted");
    let refusal = CoordEvent::parse(deleted.as_bytes()).expect_err(
        "I5.6 and I7.1: deleted is the cause of no stored row, and run_ended deleted is taken",
    );
    assert_eq!(
        (refusal.key(), refusal.rule()),
        (Some("cause"), "unknown_name")
    );

    let log = format!(
        r#"{{"op":"coord_log","workflow":"{WORKFLOW}","agent":"{AGENT}","records":[{deleted}]}}"#
    );
    assert_eq!(
        CoordLog::parse(log.as_bytes())
            .map(|_| ())
            .map_err(|r| r.rule()),
        Err("unknown_name"),
        "a coord_log body with run_ended deleted"
    );
    let reply = format!(
        r#"{{"records":[{},{}],"last":2}}"#,
        run_ended(1, "completed"),
        run_ended(2, "deleted")
    );
    assert_eq!(
        History::parse(reply.as_bytes())
            .map(|_| ())
            .map_err(|r| r.rule()),
        Err("unknown_name"),
        "a coord_history reply with run_ended deleted"
    );
}
