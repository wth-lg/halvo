use halvo_schema::executable::{
    self, AgentBindings, AgentEffect, AgentLog, AgentPlaybook, AgentSession,
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

const WITHOUT_STRICT_READER: [&str; 1] = ["agent_checkpoint"];

const KIND_NAMES_THAT_NAMES_LACKS: [&str; 6] = OWN;

fn entry(name: &str) -> &'static KindEntry {
    executable::KINDS
        .iter()
        .find(|k| k.name == name)
        .unwrap_or_else(|| panic!("no kind {name}"))
}

fn strict(name: &str, text: &str) -> Option<Result<String, String>> {
    let b = text.as_bytes();
    let read = match name {
        "agent_playbook" => AgentPlaybook::parse(b).and_then(|v| v.to_json()),
        "agent_bindings" => AgentBindings::parse(b).and_then(|v| v.to_json()),
        "agent_effect" => AgentEffect::parse(b).and_then(|v| v.to_json()),
        "agent_log" => AgentLog::parse(b).and_then(|v| v.to_json()),
        "agent_session" => AgentSession::parse(b).and_then(|v| v.to_json()),
        _ => return None,
    };
    Some(read.map_err(|e| e.to_string()))
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
        match strict(name, kind.example) {
            Some(read) => {
                assert!(!WITHOUT_STRICT_READER.contains(&name), "{name}");
                assert_eq!(read.as_deref(), Ok(kind.example), "{name}");
            }
            None => assert!(WITHOUT_STRICT_READER.contains(&name), "{name}"),
        }
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
        if let Some(read) = strict(name, &extra) {
            assert!(read.is_err(), "{name}");
        }
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
        if let Some(read) = strict(name, &text) {
            assert!(read.is_err(), "{to}");
        }
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
fn the_names_list_of_the_file_lacks_exactly_the_six_kind_names_of_origin_own() {
    let lacking: Vec<&str> = executable::KINDS
        .iter()
        .map(|k| k.name)
        .filter(|name| !executable::NAMES.contains(name))
        .collect();
    assert_eq!(lacking, KIND_NAMES_THAT_NAMES_LACKS);
}
