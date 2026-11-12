use halvo_schema::executable::{self, AgentConfig, AgentRun};
use halvo_schema::kinds::{
    check_entries, check_record, check_registry, registry, FieldType, KindEntry, Origin,
    RetentionClass,
};

fn entry(name: &str) -> &'static KindEntry {
    executable::KINDS
        .iter()
        .find(|k| k.name == name)
        .unwrap_or_else(|| panic!("no kind {name}"))
}

fn keys(entry: &KindEntry) -> Vec<&'static str> {
    let mut keys: Vec<&'static str> = Vec::new();
    for (key, _) in entry.fields {
        if !keys.contains(key) {
            keys.push(key);
        }
    }
    keys
}

#[test]
fn the_file_registers_the_two_kinds_of_origin_capture_of_the_family_agent() {
    let names: Vec<&str> = executable::KINDS.iter().map(|k| k.name).collect();
    assert_eq!(names, ["agent_config", "agent_run"]);
    let config = entry("agent_config");
    assert_eq!(config.class, RetentionClass::Kept);
    let run = entry("agent_run");
    assert_eq!(run.class, RetentionClass::Bounded { days: 93 });
    for kind in executable::KINDS {
        assert_eq!(kind.origin, Origin::Capture, "{}", kind.name);
        assert_eq!(kind.family, "agent", "{}", kind.name);
        assert_eq!(kind.workflow_key, Some("workflow"), "{}", kind.name);
    }
    let registered: Vec<&str> = registry().map(|k| k.name).collect();
    assert_eq!(&registered[4..6], ["agent_config", "agent_run"]);
    assert_eq!(check_registry(), Vec::<String>::new());
    let own: Vec<&KindEntry> = executable::KINDS.iter().collect();
    assert_eq!(check_entries(&own), Vec::<String>::new());
}

#[test]
fn the_fields_of_each_entry_are_the_keys_of_its_record_with_their_types() {
    use FieldType::*;
    assert_eq!(
        entry("agent_config").fields,
        [
            ("v", Version),
            ("workflow", Id),
            ("agent", Id),
            ("rev", Count),
            ("category", Name),
            ("tier", Name),
            ("reasoning", Name),
            ("autonomy", Name),
            ("allow", Name),
            ("caps", Name),
            ("limits", Count),
            ("budget", Count),
            ("budget", Name),
            ("permissions", Name),
            ("runtime", Name),
            ("playbook", Count),
        ]
    );
    let run: Vec<(&str, FieldType)> = entry("agent_run").fields.to_vec();
    let mut expected = vec![
        ("v", Version),
        ("workflow", Id),
        ("agent", Id),
        ("run", Id),
        ("day", Day),
        ("mode", Name),
        ("outcome", Name),
    ];
    for key in [
        "playbook",
        "steps",
        "prompts",
        "handbacks",
        "confirmations",
        "effects",
        "unknown",
        "model_calls",
        "tokens_in",
        "tokens_out",
        "wall_s",
    ] {
        expected.push((key, Count));
    }
    expected.push(("model", Name));
    assert_eq!(run, expected);
    for kind in executable::KINDS {
        for (_, field_type) in kind.fields {
            assert!(
                ![Instant, Ordinal, Token, Text, OwnText].contains(field_type),
                "{}",
                kind.name
            );
        }
    }
}

#[test]
fn each_example_is_one_record_that_the_store_and_the_strict_reader_take_alike() {
    let config = entry("agent_config");
    assert_eq!(check_record(config, config.example), Ok(()));
    let parsed = AgentConfig::parse(config.example.as_bytes()).unwrap();
    assert_eq!(parsed.to_json().unwrap(), config.example);
    assert_eq!(keys(config).len(), 15);

    let run = entry("agent_run");
    assert_eq!(check_record(run, run.example), Ok(()));
    assert_eq!(
        AgentRun::parse(run.example.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        run.example
    );
    assert_eq!(keys(run).len(), 19);
}

#[test]
fn the_store_refuses_a_row_without_its_rule_keys_and_the_strict_reader_refuses_what_the_walk_takes()
{
    let run = entry("agent_run");
    for (from, to) in [
        (r#""day":20768"#, r#""day":null"#),
        (r#""day":20768"#, r#""day":[20768]"#),
        (
            r#""workflow":"30000000000000000000000000000001""#,
            r#""workflow":null"#,
        ),
        (r#""v":1"#, r#""v":2"#),
    ] {
        let text = run.example.replacen(from, to, 1);
        assert!(check_record(run, &text).is_err(), "{to}");
        assert!(AgentRun::parse(text.as_bytes()).is_err(), "{to}");
    }
    let config = entry("agent_config");
    for (from, to) in [
        (r#""allow":[]"#, r#""allow":["payment"]"#),
        (
            r#""caps":{"untrusted":"yes","private":"yes","external":"yes"}"#,
            r#""caps":{"untrusted":"yes"}"#,
        ),
        (r#""tier":"on_device""#, r#""tier":"gpt5""#),
        (r#""playbook":null"#, r#""playbook":0"#),
    ] {
        assert_eq!(config.example.matches(from).count(), 1, "{from}");
        let text = config.example.replacen(from, to, 1);
        assert_eq!(check_record(config, &text), Ok(()), "{to}");
        assert!(AgentConfig::parse(text.as_bytes()).is_err(), "{to}");
    }
    let unknown = run.example.replacen(
        r#""outcome":"completed""#,
        r#""outcome":"completed","unknown":0"#,
        1,
    );
    assert!(check_record(run, &unknown).is_err());
    assert!(AgentRun::parse(unknown.as_bytes()).is_err());
}
