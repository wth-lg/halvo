use std::collections::BTreeSet;
use std::path::Path;

use halvo_schema::kinds::check_record;
use halvo_schema::learning::body::{LearnedRead, Refinement, Refinements};
use halvo_schema::learning::names::RefinementKind;
use halvo_schema::learning::record::{Day, LearnEdit, LearnRoutine, LearnRun};
use halvo_schema::learning::KINDS;
use halvo_schema::request::{
    decode, decode_reply, write_reply, CoordinatorRole, CoordinatorWrite, Decoded, ReplyResult,
};
use halvo_schema::{AgentId, RequestId, WorkflowId};
use serde_json::value::RawValue;

const EDIT_EXAMPLE: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d","rev":1,"base":1,"n":4,"state":"provisional","cause":null,"on":null,"runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0"],"first":20767,"last":20767,"clean":0,"target":{"desc":"button \"Submit payment\"","locators":[{"by":"role","value":"button","name":"Submit payment"}]}}"#;

const ROUTINE_EXAMPLE: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","rev":3,"day":20767,"category":"utility_bill_payment","site":{"key":"northwind-power.test","source":"address_field"},"app":"com.google.Chrome","period":30,"seen":3,"first":20705,"last":20765,"next":20795,"missed":0,"mark":"none","since":null}"#;

const RUN_EXAMPLE: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","rev":2,"day":20767,"mode":"replay","state":"verified","cause":null,"base":1,"assisted":1,"kept":1,"failed_step":null}"#;

fn golden(name: &str) -> String {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    let bytes = std::fs::read(&file).unwrap_or_else(|e| panic!("{name}: {e}"));
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.ends_with('\n'), "{name}: no LF at its end");
    assert!(!text.contains('\r'), "{name}: a CR");
    assert!(!text.starts_with('\u{feff}'), "{name}: a byte-order mark");
    text
}

fn keys_of(value: &serde_json::Value, out: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, inner) in map {
                out.insert(key.clone());
                keys_of(inner, out);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|item| keys_of(item, out)),
        _ => {}
    }
}

#[test]
fn r5_golden_records() {
    let text = golden("r5/records.jsonl");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines, [EDIT_EXAMPLE, ROUTINE_EXAMPLE, RUN_EXAMPLE]);

    let edit = LearnEdit::parse(lines[0].as_bytes()).unwrap();
    assert_eq!(edit.to_json().unwrap(), lines[0]);
    assert_eq!(
        serde_json::to_string(&serde_json::from_str::<LearnEdit>(lines[0]).unwrap()).unwrap(),
        lines[0]
    );
    let routine = LearnRoutine::parse(lines[1].as_bytes()).unwrap();
    assert_eq!(routine.to_json().unwrap(), lines[1]);
    assert_eq!(
        serde_json::to_string(&serde_json::from_str::<LearnRoutine>(lines[1]).unwrap()).unwrap(),
        lines[1]
    );
    let run = LearnRun::parse(lines[2].as_bytes()).unwrap();
    assert_eq!(run.to_json().unwrap(), lines[2]);
    assert_eq!(
        serde_json::to_string(&serde_json::from_str::<LearnRun>(lines[2]).unwrap()).unwrap(),
        lines[2]
    );

    let kinds = ["learn_edit", "learn_routine", "learn_run"];
    for entry in KINDS {
        let at = kinds
            .iter()
            .position(|k| *k == entry.name)
            .unwrap_or_else(|| panic!("{} has no golden line", entry.name));
        assert_eq!(entry.example, lines[at], "{}", entry.name);
        assert_eq!(check_record(entry, lines[at]), Ok(()), "{}", entry.name);
    }
    let held: Vec<&str> = KINDS.iter().map(|k| k.name).collect();
    assert!(held.contains(&"learn_run") && held.contains(&"learn_routine"));

    let names: BTreeSet<String> = golden("names.txt").lines().map(str::to_owned).collect();
    let mut keys = BTreeSet::new();
    for line in &lines {
        keys_of(&serde_json::from_str(line).unwrap(), &mut keys);
    }
    let missing: Vec<&String> = keys.difference(&names).collect();
    assert!(
        missing.is_empty(),
        "keys missing from names.txt: {missing:?}"
    );
}

#[test]
fn r5_golden_requests() {
    let text = golden("r5/requests.jsonl");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);

    let Ok(Decoded::Raw(request)) = decode(lines[0].as_bytes()) else {
        panic!("line 1 is no request that decode returns undecoded");
    };
    assert_eq!(request.name(), "coordinator_write");
    let body = LearnedRead::parse(request.body().get().as_bytes()).unwrap();
    let written = CoordinatorWrite::new(&CoordinatorRole::claim(), request.id().clone(), body)
        .to_bytes()
        .unwrap();
    assert_eq!(String::from_utf8(written).unwrap(), lines[0]);

    let reply = decode_reply(lines[1].as_bytes()).unwrap();
    assert_eq!(reply.id.as_ref(), Some(request.id()));
    let Ok(ReplyResult::Raw(result)) = reply.outcome else {
        panic!("line 2 is no reply with a raw result");
    };
    let refinements = Refinements::parse(result.get().as_bytes()).unwrap();
    assert_eq!(
        refinements.refinements,
        [Refinement {
            day: Day::new(20767).unwrap(),
            what: RefinementKind::TargetLearned,
        }]
    );
    let raw = RawValue::from_string(refinements.to_json().unwrap()).unwrap();
    let written = write_reply(reply.id.as_ref(), Ok(&ReplyResult::Raw(raw))).unwrap();
    assert_eq!(String::from_utf8(written).unwrap(), lines[1]);

    let id = RequestId::parse("2c3d4e5f60718293a4b5c6d7e8f90a1b").unwrap();
    let body = LearnedRead {
        workflow: WorkflowId::parse("0f1e2d3c4b5a69788796a5b4c3d2e1f0").unwrap(),
        agent: AgentId::parse("1a2b3c4d5e6f708192a3b4c5d6e7f809").unwrap(),
    };
    let from_values = CoordinatorWrite::new(&CoordinatorRole::claim(), id.clone(), body)
        .to_bytes()
        .unwrap();
    assert_eq!(String::from_utf8(from_values).unwrap(), lines[0]);
    let reply = Refinements {
        refinements: vec![Refinement {
            day: Day::new(20767).unwrap(),
            what: RefinementKind::TargetLearned,
        }],
    };
    let raw = RawValue::from_string(reply.to_json().unwrap()).unwrap();
    let from_values = write_reply(Some(&id), Ok(&ReplyResult::Raw(raw))).unwrap();
    assert_eq!(String::from_utf8(from_values).unwrap(), lines[1]);
}
