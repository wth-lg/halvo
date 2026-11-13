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

const MODEL_LIMIT: u32 = 2;

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
    name: String,
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
    let forward = [
        (
            "every digit differs",
            [hex_digest("1".repeat(64)), hex_digest("2".repeat(64))],
            [
                EffectId::from_random([0x11; 16]),
                EffectId::from_random([0x22; 16]),
            ],
        ),
        (
            "the first digit differs",
            [
                hex_digest(zeros.clone()),
                hex_digest(format!("1{}", &zeros[1..])),
            ],
            [EffectId::from_random([0; 16]), EffectId::from_random(first)],
        ),
        (
            "the last digit differs",
            [
                hex_digest(zeros.clone()),
                hex_digest(format!("{}1", &zeros[1..])),
            ],
            [EffectId::from_random([0; 16]), EffectId::from_random(last)],
        ),
    ];
    let mut pairs: Vec<(String, [Digest; 2], [EffectId; 2])> = forward
        .into_iter()
        .map(|(name, d, e)| (name.to_owned(), d, e))
        .collect();
    for i in 0..64 {
        let j = i % 32;
        let mut bytes = [0u8; 16];
        bytes[j / 2] = if j % 2 == 0 { 0x10 } else { 0x01 };
        pairs.push((
            format!("digit {i} of the digests and digit {j} of the ids differ"),
            [
                hex_digest(zeros.clone()),
                hex_digest(format!("{}1{}", &zeros[..i], &zeros[i + 1..])),
            ],
            [EffectId::from_random([0; 16]), EffectId::from_random(bytes)],
        ));
    }
    let mut all = Vec::new();
    for (name, [d1, d2], [e1, e2]) in pairs {
        all.push(Values {
            name: format!("{name}, d1 and e1 below d2 and e2"),
            digests: [d1.clone(), d2.clone()],
            ids: [e1.clone(), e2.clone()],
        });
        all.push(Values {
            name: format!("{name}, d1 and e1 above d2 and e2"),
            digests: [d2, d1],
            ids: [e2, e1],
        });
    }
    all
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

struct Step {
    kind: &'static str,
    class: ActionClass,
    decision: Option<Decision>,
    entry: Option<usize>,
    digest: Option<usize>,
    bound_class: Option<ActionClass>,
    used: Option<bool>,
    ok: bool,
    state: Option<EffectState>,
    bound: usize,
}

struct Trace {
    n: u64,
    effect: usize,
    bound: usize,
    steps: Vec<Step>,
}

fn index(name: &str) -> usize {
    match name {
        "d1" | "e1" => 0,
        "d2" | "e2" => 1,
        other => panic!("name {other}"),
    }
}

fn used(name: &str) -> bool {
    match name {
        "yes" => true,
        "no" => false,
        other => panic!("used {other}"),
    }
}

fn parsed(traces: &[Value]) -> Vec<Trace> {
    traces
        .iter()
        .map(|t| {
            let n = t.get("n").and_then(Value::as_u64).unwrap_or(0);
            let Some(moves) = t.get("moves").and_then(Value::as_array) else {
                panic!("trace {n} has no moves");
            };
            let steps = moves
                .iter()
                .map(|m| {
                    let name = text(m, "move");
                    let Some(kind) = MOVES.iter().copied().find(|k| *k == name) else {
                        panic!("move {name}");
                    };
                    Step {
                        kind,
                        class: class(text(m, "class")),
                        decision: opt(m, "decision").map(decision),
                        entry: opt(m, "entry").map(index),
                        digest: opt(m, "digest").map(index),
                        bound_class: opt(m, "bound_class").map(class),
                        used: opt(m, "used").map(used),
                        ok: text(m, "answer") == "ok",
                        state: opt(m, "state").map(state),
                        bound: index(text(m, "bound")),
                    }
                })
                .collect();
            Trace {
                n,
                effect: index(text(t, "effect")),
                bound: index(text(t, "bound")),
                steps,
            }
        })
        .collect()
}

fn held<T: Copy>(value: Option<T>, key: &str, m: &Step) -> T {
    match value {
        Some(v) => v,
        None => panic!("no {key} in a {} move", m.kind),
    }
}

fn step_move(m: &Step, values: &Values, other: Option<ActionClass>) -> EffectMove {
    match m.kind {
        "propose" => EffectMove::Propose(Decided::free(m.class, held(m.decision, "decision", m))),
        "rebind" => EffectMove::Rebind {
            digest: values.digests[held(m.digest, "digest", m)].clone(),
            class: other.unwrap_or(held(m.bound_class, "bound_class", m)),
        },
        "approve" => EffectMove::Approve {
            entry: values.ids[held(m.entry, "entry", m)].clone(),
            digest: values.digests[held(m.digest, "digest", m)].clone(),
            used: held(m.used, "used", m),
        },
        "reject" => EffectMove::Reject,
        "end" => EffectMove::End,
        "begin" => EffectMove::Begin {
            digest: values.digests[held(m.digest, "digest", m)].clone(),
        },
        "commit" => EffectMove::Commit,
        "lose" => EffectMove::Lose,
        other => panic!("move {other}"),
    }
}

fn alike(m: &Step, rebinds: u32) -> Vec<(Option<ActionClass>, u32)> {
    let classes: Vec<Option<ActionClass>> = if m.kind == "rebind" && m.bound_class != Some(m.class)
    {
        ActionClass::ALL
            .iter()
            .copied()
            .filter(|c| *c != m.class)
            .map(Some)
            .collect()
    } else {
        vec![None]
    };
    let counts: Vec<u32> = if rebinds >= MODEL_LIMIT {
        vec![rebinds, rebinds + 1, u32::MAX]
    } else {
        (0..MODEL_LIMIT).collect()
    };
    let mut out = Vec::new();
    for c in &classes {
        for n in &counts {
            if c.is_some() || *n != rebinds {
                out.push((*c, *n));
            }
        }
    }
    out
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
    let (_, raw) = traces();
    let traces = parsed(&raw);
    let orders: BTreeSet<(bool, bool)> = mappings()
        .iter()
        .map(|v| {
            (
                v.digests[0].as_str() < v.digests[1].as_str(),
                v.ids[0].as_str() < v.ids[1].as_str(),
            )
        })
        .collect();
    assert_eq!(
        orders,
        BTreeSet::from([(false, false), (true, true)]),
        "the mappings give d1 and e1 below and above d2 and e2"
    );
    for values in mappings() {
        let mut moves = 0u64;
        let mut differ = Vec::new();
        let mut accepted = BTreeSet::new();
        let mut refused = BTreeSet::new();
        let mut reached = BTreeSet::new();
        let mut other_classes = 0u64;
        let mut higher_counts = 0u64;
        let mut lower_counts = 0u64;
        for trace in &traces {
            let n = trace.n;
            let id = &values.ids[trace.effect];
            let mut now: Option<EffectState> = None;
            let mut bound = trace.bound;
            let mut rebinds = 0u32;
            for (i, m) in trace.steps.iter().enumerate() {
                moves += 1;
                let kind = m.kind;
                let effect = now.map(|s| Effect {
                    id: id.clone(),
                    state: s,
                    class: m.class,
                    digest: values.digests[bound].clone(),
                    rebinds,
                });
                let got = effect_step(effect.as_ref(), step_move(m, &values, None));
                let model_ok = m.ok;
                let model_state = m.state;
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
                for (other, count) in alike(m, rebinds) {
                    let effect = now.map(|s| Effect {
                        id: id.clone(),
                        state: s,
                        class: m.class,
                        digest: values.digests[bound].clone(),
                        rebinds: count,
                    });
                    let alike_got = effect_step(effect.as_ref(), step_move(m, &values, other));
                    let alike_next = match alike_got {
                        Ok(s) => Some(s),
                        Err(_) => now,
                    };
                    if other.is_some() && count == rebinds {
                        other_classes += 1;
                    }
                    if other.is_none() && count != rebinds && rebinds >= MODEL_LIMIT {
                        higher_counts += 1;
                    }
                    if other.is_none() && count != rebinds && rebinds < MODEL_LIMIT {
                        lower_counts += 1;
                    }
                    if alike_got.is_ok() != model_ok || alike_next != model_state {
                        differ.push(format!(
                            "trace {n} move {} {kind} with the bound class {other:?} and {count} rebinds: the model {} to {model_state:?}, effect_step {} to {alike_next:?}",
                            i + 1,
                            if model_ok { "accepts" } else { "refuses" },
                            if alike_got.is_ok() { "accepts" } else { "refuses" },
                        ));
                    }
                }
                if let Ok(s) = got {
                    accepted.insert(kind);
                    reached.insert(s);
                    if kind == "rebind" {
                        bound = held(m.digest, "digest", m);
                        rebinds += 1;
                    }
                } else {
                    refused.insert(kind);
                }
                if bound != m.bound {
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
        assert!(other_classes > 0, "no rebind with another class");
        assert!(higher_counts > 0, "no move at the limit of rebinds");
        assert!(lower_counts > 0, "no move below the limit of rebinds");
    }
}
