use std::collections::BTreeSet;
use std::path::PathBuf;

use halvo_schema::executable::{ActionClass, Digest, EffectId, EffectState};
use serde_json::Value;

use crate::{effect_step, Decided, Decision, Effect, EffectMove};

const MOVE_KEYS: [&str; 10] = [
    "class",
    "move",
    "decision",
    "entry",
    "digest",
    "bound_class",
    "used",
    "answer",
    "state",
    "bound",
];

const MOVES: [&str; 8] = [
    "propose", "rebind", "approve", "reject", "end", "begin", "commit", "lose",
];

const STATES: [EffectState; 8] = [
    EffectState::Refused,
    EffectState::Cleared,
    EffectState::Held,
    EffectState::Rejected,
    EffectState::Approved,
    EffectState::Releasing,
    EffectState::Committed,
    EffectState::Unknown,
];

struct Values {
    name: &'static str,
    digests: [Digest; 2],
    ids: [EffectId; 2],
}

fn hex_digest(s: String) -> Digest {
    match Digest::parse(&s) {
        Ok(d) => d,
        Err(r) => panic!("{r}"),
    }
}

fn mappings() -> Vec<Values> {
    let zeros = "0".repeat(64);
    let mut last = [0u8; 16];
    last[15] = 1;
    let mut first = [0u8; 16];
    first[0] = 1;
    vec![
        Values {
            name: "every digit differs",
            digests: [hex_digest("1".repeat(64)), hex_digest("2".repeat(64))],
            ids: [
                EffectId::from_random([0x11; 16]),
                EffectId::from_random([0x22; 16]),
            ],
        },
        Values {
            name: "the first digit differs",
            digests: [
                hex_digest(zeros.clone()),
                hex_digest(format!("1{}", &zeros[1..])),
            ],
            ids: [EffectId::from_random([0; 16]), EffectId::from_random(first)],
        },
        Values {
            name: "the last digit differs",
            digests: [
                hex_digest(zeros.clone()),
                hex_digest(format!("{}1", &zeros[1..])),
            ],
            ids: [EffectId::from_random([0; 16]), EffectId::from_random(last)],
        },
    ]
}

impl Values {
    fn digest(&self, name: &str) -> Digest {
        match name {
            "d1" => self.digests[0].clone(),
            "d2" => self.digests[1].clone(),
            other => panic!("digest {other}"),
        }
    }

    fn id(&self, name: &str) -> EffectId {
        match name {
            "e1" => self.ids[0].clone(),
            "e2" => self.ids[1].clone(),
            other => panic!("effect {other}"),
        }
    }
}

fn file() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../formal/r3-confirm/traces.jsonl");
    match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => panic!("cannot read {}: {e}", path.display()),
    }
}

fn json(line: &str, what: &str) -> Value {
    match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => panic!("{what}: {e}"),
    }
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    match v.get(key).and_then(Value::as_str) {
        Some(s) => s,
        None => panic!("no string {key} in {v}"),
    }
}

fn opt<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    match v.get(key) {
        Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s),
        other => panic!("{key} is {other:?} in {v}"),
    }
}

fn need<'a>(v: &'a Value, key: &str) -> &'a str {
    match opt(v, key) {
        Some(s) => s,
        None => panic!("no {key} in {v}"),
    }
}

fn class(name: &str) -> ActionClass {
    match ActionClass::parse(name) {
        Ok(c) => c,
        Err(r) => panic!("{r}"),
    }
}

fn state(name: &str) -> EffectState {
    match EffectState::parse(name) {
        Ok(s) => s,
        Err(r) => panic!("{r}"),
    }
}

fn decision(name: &str) -> Decision {
    match Decision::ALL.iter().find(|d| d.as_str() == name) {
        Some(d) => *d,
        None => panic!("decision {name}"),
    }
}

fn step_move(m: &Value, of_effect: ActionClass, values: &Values) -> EffectMove {
    match text(m, "move") {
        "propose" => EffectMove::Propose(Decided::free(of_effect, decision(need(m, "decision")))),
        "rebind" => EffectMove::Rebind {
            digest: values.digest(need(m, "digest")),
            class: class(need(m, "bound_class")),
        },
        "approve" => EffectMove::Approve {
            entry: values.id(need(m, "entry")),
            digest: values.digest(need(m, "digest")),
            used: match need(m, "used") {
                "yes" => true,
                "no" => false,
                other => panic!("used {other}"),
            },
        },
        "reject" => EffectMove::Reject,
        "end" => EffectMove::End,
        "begin" => EffectMove::Begin {
            digest: values.digest(need(m, "digest")),
        },
        "commit" => EffectMove::Commit,
        "lose" => EffectMove::Lose,
        other => panic!("move {other}"),
    }
}

fn keys_of(m: &str) -> [bool; 5] {
    [
        m == "propose",
        m == "approve",
        matches!(m, "rebind" | "approve" | "begin"),
        m == "rebind",
        m == "approve",
    ]
}

fn quoted(v: &Value) -> String {
    match v {
        Value::Null => "null".to_owned(),
        Value::String(s) => format!("\"{s}\""),
        Value::Number(n) => n.to_string(),
        other => panic!("a value of the traces is {other}"),
    }
}

fn traces() -> (u64, Vec<Value>) {
    let whole = file();
    let mut lines = whole.lines();
    let head_line = match lines.next() {
        Some(l) => l,
        None => panic!("the traces file is empty"),
    };
    let head = json(head_line, "the first line");
    let (Some(seed), Some(count)) = (
        head.get("seed").and_then(Value::as_u64),
        head.get("count").and_then(Value::as_u64),
    ) else {
        panic!("the first line has no seed and count: {head_line}");
    };
    assert_eq!(
        head_line,
        format!("{{\"seed\":{seed},\"count\":{count}}}"),
        "the first line"
    );
    let traces: Vec<Value> = lines
        .enumerate()
        .map(|(i, l)| json(l, &format!("trace line {}", i + 2)))
        .collect();
    (count, traces)
}

#[test]
fn the_committed_traces_have_the_form_that_the_model_writes() {
    let whole = file();
    let (count, traces) = traces();
    assert_eq!(
        traces.len() as u64,
        count,
        "the traces of the first line's count"
    );
    assert!(count >= 200, "{count} traces, below 200");
    for (line, trace) in whole.lines().skip(1).zip(&traces) {
        let n = trace.get("n").and_then(Value::as_u64);
        let Some(moves) = trace.get("moves").and_then(Value::as_array) else {
            panic!("no moves in {line}");
        };
        assert!(
            (1..=12).contains(&moves.len()),
            "trace {n:?} has {} moves",
            moves.len()
        );
        let first = &moves[0];
        let mut rebuilt = format!(
            "{{\"n\":{},\"effect\":{},\"bound\":{},\"moves\":[",
            quoted(trace.get("n").unwrap_or(&Value::Null)),
            quoted(trace.get("effect").unwrap_or(&Value::Null)),
            quoted(trace.get("bound").unwrap_or(&Value::Null)),
        );
        for (i, m) in moves.iter().enumerate() {
            let kind = text(m, "move");
            assert!(MOVES.contains(&kind), "move {kind} in trace {n:?}");
            assert_eq!(
                m.get("class"),
                first.get("class"),
                "the class of trace {n:?} changes"
            );
            for (key, held) in ["decision", "entry", "digest", "bound_class", "used"]
                .iter()
                .zip(keys_of(kind))
            {
                assert_eq!(
                    opt(m, key).is_some(),
                    held,
                    "{key} of a {kind} move in trace {n:?}"
                );
            }
            if i > 0 {
                rebuilt.push(',');
            }
            rebuilt.push('{');
            for (j, key) in MOVE_KEYS.iter().enumerate() {
                if j > 0 {
                    rebuilt.push(',');
                }
                rebuilt.push_str(&format!(
                    "\"{key}\":{}",
                    quoted(m.get(*key).unwrap_or(&Value::Bool(false)))
                ));
            }
            rebuilt.push('}');
        }
        rebuilt.push_str("]}");
        assert_eq!(line, rebuilt, "trace {n:?} differs from its keys in order");
    }
    let numbers: Vec<u64> = traces
        .iter()
        .filter_map(|t| t.get("n").and_then(Value::as_u64))
        .collect();
    assert_eq!(
        numbers,
        (1..=count).collect::<Vec<u64>>(),
        "the trace numbers"
    );
}

#[test]
fn effect_step_accepts_and_refuses_as_the_model_at_every_move() {
    let (_, traces) = traces();
    for values in mappings() {
        let mut moves = 0u64;
        let mut differ = Vec::new();
        let mut accepted = BTreeSet::new();
        let mut refused = BTreeSet::new();
        let mut reached = BTreeSet::new();
        for trace in &traces {
            let n = trace.get("n").and_then(Value::as_u64).unwrap_or(0);
            let id = values.id(text(trace, "effect"));
            let mut now: Option<EffectState> = None;
            let mut bound = values.digest(text(trace, "bound"));
            let mut rebinds = 0u32;
            let Some(steps) = trace.get("moves").and_then(Value::as_array) else {
                panic!("trace {n} has no moves");
            };
            for (i, m) in steps.iter().enumerate() {
                moves += 1;
                let kind = text(m, "move");
                let of_effect = class(text(m, "class"));
                let effect = now.map(|s| Effect {
                    id: id.clone(),
                    state: s,
                    class: of_effect,
                    digest: bound.clone(),
                    rebinds,
                });
                let got = effect_step(effect.as_ref(), step_move(m, of_effect, &values));
                let model_ok = text(m, "answer") == "ok";
                let model_state = opt(m, "state").map(state);
                let next = match got {
                    Ok(s) => Some(s),
                    Err(_) => now,
                };
                if got.is_ok() != model_ok || next != model_state {
                    differ.push(format!(
                        "trace {n} move {} {kind}: the model {} to {model_state:?}, effect_step {} to {next:?}",
                        i + 1,
                        if model_ok { "accepts" } else { "refuses" },
                        if got.is_ok() { "accepts" } else { "refuses" },
                    ));
                }
                if let Ok(s) = got {
                    accepted.insert(kind);
                    reached.insert(s);
                    if kind == "rebind" {
                        bound = values.digest(need(m, "digest"));
                        rebinds += 1;
                    }
                } else {
                    refused.insert(kind);
                }
                if bound != values.digest(text(m, "bound")) {
                    differ.push(format!(
                        "trace {n} move {} {kind}: the digest after the move differs",
                        i + 1
                    ));
                }
                now = next;
            }
        }
        assert!(
            differ.is_empty(),
            "with {}, effect_step differs from the model at {} of {moves} moves, first: {:?}",
            values.name,
            differ.len(),
            &differ[..differ.len().min(10)]
        );
        for kind in MOVES {
            assert!(accepted.contains(kind), "no {kind} accepted in the traces");
            assert!(refused.contains(kind), "no {kind} refused in the traces");
        }
        for s in STATES {
            assert!(reached.contains(&s), "no move reaches {s:?} in the traces");
        }
    }
}
