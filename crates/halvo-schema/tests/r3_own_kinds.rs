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
fn each_example_is_taken_by_the_store_and_by_its_strict_reader_byte_for_byte() {
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
            r#""rev":2"#,
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
            r#""model":"qwen3_1_7b""#,
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
fn every_own_row_is_taken_by_the_store_and_by_its_strict_reader_byte_for_byte() {
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
    let mut states = BTreeSet::new();
    for line in own_lines("agent_checkpoint") {
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
        let left = row["left"].as_str();
        assert_eq!(left.is_some(), state == "left", "{line}");
        assert!(left.is_none_or(|l| {
            ["changed", "link", "not_file", "name", "unreadable"].contains(&l)
        }));
        let folder = pieces(&row["folder"]);
        let workbook = pieces(&row["workbook"]);
        assert!(workbook.starts_with(&format!("{folder}/")), "{line}");
        let run = row["run"].as_str().unwrap();
        let copy = pieces(&row["copy"]);
        assert_eq!(copy, format!("bills.halvo-checkpoint-{run}.xlsx"), "{line}");
    }
    let all: BTreeSet<String> = ["planned", "made", "gone", "left"]
        .map(str::to_owned)
        .into();
    assert_eq!(states, all);
}
