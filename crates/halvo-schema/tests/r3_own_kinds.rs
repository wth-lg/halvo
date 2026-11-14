use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use halvo_schema::executable::{
    self, AgentBindings, AgentCheckpoint, AgentEffect, AgentLog, AgentPlaybook, AgentSession,
};
use halvo_schema::kinds::{
    check_entries, check_record, check_registry, registry, KindEntry, Origin, RetentionClass,
};

const OWN: [&str; 6] = [
    "agent_playbook",
    "agent_bindings",
    "agent_effect",
    "agent_log",
    "agent_session",
    "agent_checkpoint",
];

const CLASSES: [(&str, RetentionClass); 6] = [
    ("agent_playbook", RetentionClass::Kept),
    ("agent_bindings", RetentionClass::Kept),
    ("agent_effect", RetentionClass::Bounded { days: 93 }),
    ("agent_log", RetentionClass::Bounded { days: 93 }),
    ("agent_session", RetentionClass::Bounded { days: 35 }),
    ("agent_checkpoint", RetentionClass::Bounded { days: 93 }),
];

const LEFT_OUT: [&str; 6] = ["tier", "amount", "period", "week", "month", "role"];

fn entry(name: &str) -> &'static KindEntry {
    executable::KINDS
        .iter()
        .find(|k| k.name == name)
        .unwrap_or_else(|| panic!("no kind {name}"))
}

fn strict(name: &str, text: &str) -> Result<String, String> {
    let b = text.as_bytes();
    let read = match name {
        "agent_playbook" => AgentPlaybook::parse(b).and_then(|v| v.to_json()),
        "agent_bindings" => AgentBindings::parse(b).and_then(|v| v.to_json()),
        "agent_effect" => AgentEffect::parse(b).and_then(|v| v.to_json()),
        "agent_log" => AgentLog::parse(b).and_then(|v| v.to_json()),
        "agent_session" => AgentSession::parse(b).and_then(|v| v.to_json()),
        "agent_checkpoint" => AgentCheckpoint::parse(b).and_then(|v| v.to_json()),
        _ => panic!("the kind {name} has no strict reader"),
    };
    read.map_err(|e| e.to_string())
}

fn swap(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "{from}");
    text.replacen(from, to, 1)
}

#[test]
fn the_file_registers_the_six_kinds_of_origin_own_after_its_two_kinds_of_origin_capture() {
    let names: Vec<&str> = executable::KINDS.iter().map(|k| k.name).collect();
    assert_eq!(names[2..], OWN);
    for (name, class) in CLASSES {
        let kind = entry(name);
        assert_eq!(kind.class, class, "{name}");
        assert_eq!(kind.origin, Origin::Own, "{name}");
        assert_eq!(kind.family, "agent", "{name}");
        assert_eq!(kind.workflow_key, Some("workflow"), "{name}");
        assert_eq!(check_entries(&[kind]), Vec::<String>::new(), "{name}");
    }
    let registered: Vec<&str> = registry().map(|k| k.name).collect();
    assert_eq!(registered[6..12], OWN);
    assert_eq!(check_registry(), Vec::<String>::new());
}

#[test]
fn each_example_is_taken_by_the_store_and_byte_for_byte_by_the_strict_reader_where_its_kind_has_one(
) {
    for name in OWN {
        let kind = entry(name);
        assert_eq!(check_record(kind, kind.example), Ok(()), "{name}");
        assert_eq!(
            strict(name, kind.example).as_deref(),
            Ok(kind.example),
            "{name}"
        );
    }
}

#[test]
fn a_row_that_names_a_key_outside_its_shape_or_holds_a_value_of_another_type_is_refused() {
    for name in OWN {
        let kind = entry(name);
        let extra = format!(
            "{},\"note\":\"Paid\"}}",
            &kind.example[..kind.example.len() - 1]
        );
        assert_eq!(
            check_record(kind, &extra),
            Err(format!(
                "{name}: a key that the fields do not list: unknown_key"
            )),
            "{name}"
        );
        assert!(strict(name, &extra).is_err(), "{name}");
    }
    let cases = [
        (
            "agent_playbook",
            r#""number":1"#,
            r#""number":"1""#,
            "number: string_form",
        ),
        (
            "agent_bindings",
            r#""rev":3"#,
            r#""rev":"yes""#,
            "rev: string_form",
        ),
        (
            "agent_bindings",
            r#""payee","value":["Northwind Power"]"#,
            r#""payee","value":true"#,
            "slots: boolean",
        ),
        (
            "agent_effect",
            r#""seq":1,"#,
            r#""seq":1.0,"#,
            "seq: not_an_integer",
        ),
        (
            "agent_effect",
            r#""fields":2"#,
            r#""fields":"two""#,
            "fields: string_form",
        ),
        (
            "agent_log",
            r#""event":"model_call""#,
            r#""event":"Paid Northwind Power""#,
            "event: string_form",
        ),
        (
            "agent_log",
            r#""model":"minicpm5_2b""#,
            r#""model":"Qwen3 1.7B""#,
            "model: string_form",
        ),
        (
            "agent_session",
            r#""part":1"#,
            r#""part":-1"#,
            "part: integer_form",
        ),
        (
            "agent_checkpoint",
            r#""bytes":18432"#,
            r#""bytes":"18432""#,
            "bytes: string_form",
        ),
        (
            "agent_checkpoint",
            r#""state":"made""#,
            r#""state":"the copy is made""#,
            "state: string_form",
        ),
        (
            "agent_checkpoint",
            r#""sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb""#,
            r#""sha256":"bbbb""#,
            "sha256: string_form",
        ),
    ];
    for (name, from, to, rule) in cases {
        let kind = entry(name);
        let text = swap(kind.example, from, to);
        assert_eq!(
            check_record(kind, &text),
            Err(format!("{name}: {rule}")),
            "{to}"
        );
        assert!(strict(name, &text).is_err(), "{to}");
    }
    for name in OWN {
        assert!(cases.iter().any(|(n, ..)| *n == name), "{name}");
    }
}

#[test]
fn a_row_without_its_workflow_or_its_day_is_refused_so_none_escapes_the_delete_or_the_bound() {
    for name in OWN {
        let kind = entry(name);
        let workflow = r#""workflow":"30303030303030303030303030303030""#;
        for (to, rule) in [
            (r#""workflow":null"#, "not_one_value"),
            (r#""workflow":"3030""#, "string_form"),
            (
                r#""workflow":["30303030303030303030303030303030"]"#,
                "not_one_value",
            ),
        ] {
            assert_eq!(
                check_record(kind, &swap(kind.example, workflow, to)),
                Err(format!("{name}: workflow: {rule}")),
                "{to}"
            );
        }
        let without = swap(kind.example, &format!("{workflow},"), "");
        assert_eq!(
            check_record(kind, &without),
            Err(format!("{name}: workflow: missing_key"))
        );
        assert!(strict(name, &without).is_err(), "{name}");
        if let RetentionClass::Bounded { .. } = kind.class {
            let day = r#""day":20768"#;
            assert_eq!(
                check_record(kind, &swap(kind.example, day, r#""day":null"#)),
                Err(format!("{name}: day: not_one_value"))
            );
            let dropped = swap(kind.example, &format!("{day},"), "");
            assert_eq!(
                check_record(kind, &dropped),
                Err(format!("{name}: day: missing_key"))
            );
        }
    }
}

#[test]
fn a_piece_of_own_text_longer_than_1024_bytes_is_refused() {
    let long = "a".repeat(1025);
    for (name, from) in [
        ("agent_bindings", r#""Northwind Power""#),
        ("agent_effect", r#""B2:D6""#),
        ("agent_session", r#""eyJjb29raWVzIjpbXX0=""#),
        ("agent_checkpoint", r#""/data/books""#),
    ] {
        let kind = entry(name);
        let text = swap(kind.example, from, &format!("\"{long}\""));
        let refused = check_record(kind, &text);
        assert!(
            refused
                .as_ref()
                .is_err_and(|e| e.ends_with(": string_form")),
            "{name}: {refused:?}"
        );
        let fits = swap(kind.example, from, &format!("\"{}\"", &long[1..]));
        assert_eq!(check_record(kind, &fits), Ok(()), "{name}");
    }
}

#[test]
fn every_kind_name_family_and_field_key_of_kinds_stands_in_names_and_names_holds_no_other_kind() {
    let mut wanted = BTreeSet::new();
    for kind in executable::KINDS {
        wanted.insert(kind.name);
        wanted.insert(kind.family);
        wanted.extend(kind.fields.iter().map(|(key, _)| *key));
    }
    let lacking: Vec<&str> = wanted
        .into_iter()
        .filter(|name| !LEFT_OUT.contains(name) && !executable::NAMES.contains(name))
        .collect();
    assert_eq!(lacking, Vec::<&str>::new());
    for name in LEFT_OUT {
        assert!(!executable::NAMES.contains(&name), "{name}");
    }
    let kinds: BTreeSet<&str> = executable::KINDS.iter().map(|k| k.name).collect();
    let prefixes: BTreeSet<String> = executable::KINDS
        .iter()
        .map(|k| format!("{}_", k.family))
        .collect();
    let named: BTreeSet<&str> = executable::NAMES
        .iter()
        .copied()
        .filter(|name| prefixes.iter().any(|p| name.starts_with(p.as_str())))
        .collect();
    assert_eq!(named, kinds);
}

fn own_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/r3/own")
}

fn own_lines(name: &str) -> Vec<String> {
    let file = own_dir().join(format!("{name}.jsonl"));
    let text = std::fs::read_to_string(&file).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert!(text.ends_with('\n') && !text.contains('\r'), "{name}");
    text.lines().map(str::to_owned).collect()
}

#[test]
fn the_directory_of_own_rows_holds_one_file_for_each_kind_of_origin_own_and_nothing_else() {
    let mut listed: Vec<String> = std::fs::read_dir(own_dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    listed.sort();
    let mut expected: Vec<String> = OWN.iter().map(|k| format!("{k}.jsonl")).collect();
    expected.sort();
    assert_eq!(listed, expected);
}

#[test]
fn every_own_row_is_taken_by_the_store_and_byte_for_byte_by_the_strict_reader_where_its_kind_has_one(
) {
    for name in OWN {
        let kind = entry(name);
        let lines = own_lines(name);
        assert!(lines.len() >= 2, "{name}");
        assert_eq!(lines[0], kind.example, "{name}");
        let distinct: BTreeSet<&String> = lines.iter().collect();
        assert_eq!(distinct.len(), lines.len(), "{name}");
        for line in &lines {
            assert_eq!(check_record(kind, line), Ok(()), "{name}: {line}");
            assert_eq!(strict(name, line).as_ref(), Ok(line), "{name}");
        }
    }
}

#[test]
fn the_checkpoint_rows_hold_the_four_states_of_the_shape() {
    let pieces = |value: &serde_json::Value| -> String {
        let list = value.as_array().unwrap();
        assert!(!list.is_empty());
        list.iter()
            .map(|piece| {
                let piece = piece.as_str().unwrap();
                assert!(!piece.is_empty() && piece.len() <= 1024);
                piece
            })
            .collect()
    };
    let kind = entry("agent_checkpoint");
    let mut keys: Vec<&str> = Vec::new();
    for (key, _) in kind.fields {
        if !keys.contains(key) {
            keys.push(key);
        }
    }
    let mut states = BTreeSet::new();
    for line in own_lines("agent_checkpoint") {
        let at: Vec<Option<usize>> = keys
            .iter()
            .map(|key| line.find(&format!("\"{key}\":")))
            .collect();
        assert!(at.iter().all(Option::is_some), "{line}");
        assert!(
            at.windows(2).all(|w| w[0] < w[1]),
            "keys out of order: {line}"
        );
        let row: serde_json::Value = serde_json::from_str(&line).unwrap();
        let state = row["state"].as_str().unwrap();
        states.insert(state.to_owned());
        let digest = row["sha256"].as_str();
        assert_eq!(digest.is_some(), row["bytes"].is_u64(), "{line}");
        assert!(digest.is_none_or(|d| d.len() == 64), "{line}");
        match state {
            "planned" => assert_eq!((row["seq"].as_u64(), digest), (Some(1), None), "{line}"),
            "made" => assert!(row["seq"] == 2 && digest.is_some(), "{line}"),
            "gone" | "left" => assert!(row["seq"].as_u64() > Some(1), "{line}"),
            _ => panic!("{line}"),
        }
        match &row["left"] {
            serde_json::Value::Null => assert_ne!(state, "left", "{line}"),
            serde_json::Value::String(left) => {
                assert_eq!(state, "left", "{line}");
                let reasons = ["changed", "link", "not_file", "name", "unreadable"];
                assert!(reasons.contains(&left.as_str()), "{line}");
            }
            _ => panic!("left is neither null nor a name: {line}"),
        }
        let folder = pieces(&row["folder"]);
        let workbook = pieces(&row["workbook"]);
        assert!(workbook.starts_with(&format!("{folder}/")), "{line}");
        let file = workbook.rsplit('/').next().unwrap();
        let (name, ext) = match file.rsplit_once('.') {
            Some((name, ext)) => (name, format!(".{ext}")),
            None => (file, String::new()),
        };
        let run = row["run"].as_str().unwrap();
        let copy = pieces(&row["copy"]);
        assert_eq!(
            copy,
            format!("{name}.halvo-checkpoint-{run}{ext}"),
            "{line}"
        );
    }
    let all: BTreeSet<String> = ["planned", "made", "gone", "left"]
        .map(str::to_owned)
        .into();
    assert_eq!(states, all);
}

fn first_states(class: &str) -> &'static [&'static str] {
    match class {
        "local_edit" => &["cleared", "refused"],
        "payment" | "purchase" | "sign" => &["held", "refused"],
        "send" | "delete" | "submit" => &["cleared", "held", "refused"],
        _ => &[],
    }
}

const MOVES: [(&str, &str); 10] = [
    ("held", "held"),
    ("approved", "held"),
    ("held", "approved"),
    ("held", "rejected"),
    ("cleared", "rejected"),
    ("approved", "rejected"),
    ("cleared", "releasing"),
    ("approved", "releasing"),
    ("releasing", "committed"),
    ("releasing", "unknown"),
];

#[test]
fn the_rows_of_each_effect_start_as_a5_1_decides_for_the_class_and_move_as_a5_2_allows() {
    let mut effects: Vec<(String, Vec<serde_json::Value>)> = Vec::new();
    for line in own_lines("agent_effect") {
        let row: serde_json::Value = serde_json::from_str(&line).unwrap();
        let id = row["effect"].as_str().unwrap().to_owned();
        match effects.iter_mut().find(|(e, _)| *e == id) {
            Some((_, rows)) => rows.push(row),
            None => effects.push((id, vec![row])),
        }
    }
    assert!(!effects.is_empty());
    for (id, rows) in &effects {
        let class = rows[0]["class"].as_str().unwrap();
        let mut before: Option<&serde_json::Value> = None;
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(row["seq"].as_u64(), Some(i as u64 + 1), "{id}");
            assert_eq!(row["class"].as_str(), Some(class), "{id}");
            let state = row["state"].as_str().unwrap();
            match before.map(|b| b["state"].as_str().unwrap()) {
                None => assert!(
                    first_states(class).contains(&state),
                    "{id}: a {class} effect cannot start {state}"
                ),
                Some(from) => {
                    assert!(MOVES.contains(&(from, state)), "{id}: {from} to {state}");
                    let money = ["purchase", "payment", "sign"].contains(&class);
                    assert!(
                        !(money && state == "releasing" && from != "approved"),
                        "{id}"
                    );
                }
            }
            let approval = row["approval"].as_str();
            match state {
                "held" | "cleared" | "refused" => assert_eq!(approval, None, "{id}: {state}"),
                "approved" => assert!(approval.is_some(), "{id}: approved"),
                "releasing" | "committed" | "unknown" => assert_eq!(
                    approval,
                    before.and_then(|b| b["approval"].as_str()),
                    "{id}: {state}"
                ),
                _ => {}
            }
            before = Some(row);
        }
    }
}

const MODEL_TOKENS: [&str; 5] = [
    "qwen3_5_2b",
    "minicpm5_2b",
    "granite_4_2_3b",
    "ministral_3_3b",
    "qwen3_5_4b",
];

#[test]
fn the_model_of_each_log_row_is_null_or_a_token_of_the_table_of_a13_5() {
    let lines = own_lines("agent_log");
    assert_eq!(lines[0], entry("agent_log").example);
    let mut named = 0;
    for line in &lines {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        match row["model"].as_str() {
            Some(model) => {
                assert!(MODEL_TOKENS.contains(&model), "{model}");
                named += 1;
            }
            None => assert!(row["model"].is_null(), "{line}"),
        }
    }
    assert!(named > 0);
}

type Slots = Vec<(String, serde_json::Value)>;

fn body_between(
    origins: &[String],
    slots: &Slots,
    next_origins: &[String],
    next_slots: &Slots,
) -> Option<&'static str> {
    let one_added = next_origins.len() == origins.len() + 1
        && next_origins[..origins.len()] == *origins
        && !origins.contains(&next_origins[origins.len()]);
    if one_added && next_slots == slots {
        return Some("add_origin");
    }
    let kept =
        next_slots.len() >= slots.len() && slots.iter().zip(next_slots).all(|(a, b)| a.0 == b.0);
    let once =
        (0..next_slots.len()).all(|i| !next_slots[..i].iter().any(|s| s.0 == next_slots[i].0));
    if next_origins == origins && kept && once {
        return Some("set_bindings");
    }
    None
}

#[test]
fn each_revision_of_the_bindings_is_one_body_of_a7_2_applied_to_the_revision_before() {
    let mut revisions: Vec<(u64, Vec<String>, Slots)> = own_lines("agent_bindings")
        .iter()
        .map(|line| {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            let origins = row["origins"].as_array().unwrap().iter();
            let slots = row["slots"].as_array().unwrap().iter();
            (
                row["rev"].as_u64().unwrap(),
                origins.map(|o| o.as_str().unwrap().to_owned()).collect(),
                slots
                    .map(|s| (s["name"].as_str().unwrap().to_owned(), s["value"].clone()))
                    .collect(),
            )
        })
        .collect();
    revisions.sort_by_key(|r| r.0);
    let revs: Vec<u64> = revisions.iter().map(|r| r.0).collect();
    assert_eq!(revs, (1..=revisions.len() as u64).collect::<Vec<_>>());
    let none: (u64, Vec<String>, Slots) = (0, Vec::new(), Vec::new());
    let mut bodies = BTreeSet::new();
    for (i, after) in revisions.iter().enumerate() {
        let before = if i == 0 { &none } else { &revisions[i - 1] };
        let body = body_between(&before.1, &before.2, &after.1, &after.2);
        let body = body.unwrap_or_else(|| {
            panic!(
                "rev {} follows from rev {} by no body of A7.2",
                after.0, before.0
            )
        });
        bodies.insert(body);
    }
    assert_eq!(bodies, BTreeSet::from(["add_origin", "set_bindings"]));
}
