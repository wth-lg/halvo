use std::collections::BTreeSet;
use std::path::PathBuf;

use halvo_schema::executable::{ActionClass, Digest, EffectId, EffectState};
use serde_json::Value;

use crate::{effect_step, Decided, Decision, Effect, EffectMove, StepCause};

fn traces() -> String {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../formal/r3-confirm/traces.jsonl");
    match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => panic!("cannot read {}: {e}", path.display()),
    }
}

fn digest(name: &str) -> Digest {
    let c = match name {
        "d1" => '1',
        "d2" => '2',
        other => panic!("digest {other}"),
    };
    match Digest::parse(&c.to_string().repeat(64)) {
        Ok(d) => d,
        Err(r) => panic!("{r}"),
    }
}

fn effect_id(name: &str) -> EffectId {
    let last = match name {
        "e1" => 1,
        "e2" => 2,
        other => panic!("effect {other}"),
    };
    let mut bytes = [0; 16];
    bytes[15] = last;
    EffectId::from_random(bytes)
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

fn cause_name(cause: StepCause) -> &'static str {
    cause.cause().as_str()
}

fn step_move(m: &Value, class_of_effect: ActionClass) -> EffectMove {
    let need = |key| match opt(m, key) {
        Some(s) => s,
        None => panic!("no {key} in {m}"),
    };
    match text(m, "move") {
        "propose" => {
            EffectMove::Propose(Decided::free(class_of_effect, decision(need("decision"))))
        }
        "rebind" => EffectMove::Rebind {
            digest: digest(need("digest")),
            class: class(need("bound_class")),
        },
        "approve" => EffectMove::Approve {
            entry: effect_id(need("entry")),
            digest: digest(need("digest")),
            used: match need("used") {
                "yes" => true,
                "no" => false,
                other => panic!("used {other}"),
            },
        },
        "reject" => EffectMove::Reject,
        "end" => EffectMove::End,
        "begin" => EffectMove::Begin {
            digest: digest(need("digest")),
        },
        "commit" => EffectMove::Commit,
        "lose" => EffectMove::Lose,
        other => panic!("move {other}"),
    }
}

/// Follows every committed trace of the model with `effect_step`, as the model's README says: the answer of each
/// move, `ok` or the cause, and the next state are the model's.
#[test]
fn effect_step_answers_every_move_of_the_model_traces_with_the_model_cause() {
    let text_of_file = traces();
    let mut lines = text_of_file.lines();
    let head: Value = match lines.next().map(serde_json::from_str) {
        Some(Ok(v)) => v,
        other => panic!("no first line: {other:?}"),
    };
    let count = head.get("count").and_then(Value::as_u64);
    let mut traces = 0u64;
    let mut moves = 0u64;
    let mut differ = Vec::new();
    let mut met = BTreeSet::new();
    for line in lines {
        let trace: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => panic!("trace {traces}: {e}"),
        };
        traces += 1;
        let n = trace.get("n").and_then(Value::as_u64).unwrap_or(0);
        let id = effect_id(text(&trace, "effect"));
        let mut now: Option<EffectState> = None;
        let mut bound = digest(text(&trace, "bound"));
        let mut rebinds = 0u32;
        let Some(steps) = trace.get("moves").and_then(Value::as_array) else {
            panic!("trace {n} has no moves");
        };
        for (i, m) in steps.iter().enumerate() {
            moves += 1;
            let effect_class = class(text(m, "class"));
            let mv = step_move(m, effect_class);
            let effect = now.map(|s| Effect {
                id: id.clone(),
                state: s,
                class: effect_class,
                digest: bound.clone(),
                rebinds,
            });
            let got = effect_step(effect.as_ref(), mv);
            let answer = text(m, "answer");
            let want_state = opt(m, "state").map(state);
            let got_answer = match got {
                Ok(_) => "ok",
                Err(c) => cause_name(c),
            };
            let got_state = match got {
                Ok(s) => Some(s),
                Err(_) => now,
            };
            met.insert((text(m, "move").to_owned(), answer.to_owned()));
            if got_answer != answer || got_state != want_state {
                differ.push(format!(
                    "trace {n} move {}: {} -> model {answer} {want_state:?}, effect_step {got_answer} {got_state:?}",
                    i + 1,
                    text(m, "move")
                ));
            }
            if text(m, "move") == "rebind" && answer == "ok" {
                rebinds += 1;
            }
            now = want_state;
            bound = digest(text(m, "bound"));
        }
    }
    assert_eq!(Some(traces), count, "the traces of the first line's count");
    assert!(
        differ.is_empty(),
        "effect_step differs from the model at {} of {moves} moves, first: {:?}",
        differ.len(),
        &differ[..differ.len().min(10)]
    );
    for (mv, answer) in [
        ("propose", "effect"),
        ("rebind", "class"),
        ("rebind", "changed"),
        ("approve", "approval"),
        ("approve", "digest"),
        ("approve", "used"),
        ("begin", "unknown"),
        ("begin", "approval"),
        ("begin", "digest"),
        ("lose", "ok"),
        ("commit", "ok"),
    ] {
        assert!(
            met.contains(&(mv.to_owned(), answer.to_owned())),
            "no {mv} answered {answer} in the traces"
        );
    }
}
