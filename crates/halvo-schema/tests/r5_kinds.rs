use halvo_schema::kinds::{
    check_entries, check_record, check_registry, registry, FieldType, KindEntry, Origin,
    RetentionClass,
};
use halvo_schema::learning::record::{LearnEdit, LearnRoutine, LearnRun};
use halvo_schema::learning::{ERROR_NAMES, KINDS};
use halvo_schema::request::{decode_reply, write_reply, ErrorName};
use halvo_schema::RequestId;

use FieldType::{App, Count, Day, Id, Name, Site, Version};

const RUN_EXAMPLE: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","rev":2,"day":20767,"mode":"replay","state":"verified","cause":null,"base":1,"assisted":1,"kept":1,"failed_step":null}"#;

const ROUTINE_EXAMPLE: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","rev":3,"day":20767,"category":"utility_bill_payment","site":{"key":"northwind-power.test","source":"address_field"},"app":"com.google.Chrome","period":30,"seen":3,"first":20705,"last":20765,"next":20795,"missed":0,"mark":"none","since":null}"#;

const OTHER_RUNS: [&str; 2] = [
    r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","rev":1,"day":20767,"mode":"compile","state":"started","cause":null,"base":null,"assisted":0,"kept":0,"failed_step":null}"#,
    r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","rev":2,"day":20767,"mode":"replay","state":"unverified","cause":"effect_unknown","base":2,"assisted":3,"kept":0,"failed_step":5}"#,
];

const OTHER_ROUTINES: [&str; 2] = [
    r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","rev":1,"day":20700,"category":"shop_order","site":null,"app":"com.example.browser","period":null,"seen":0,"first":null,"last":null,"next":null,"missed":0,"mark":"none","since":null}"#,
    r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","rev":9,"day":20900,"category":"email","site":{"key":"abababababababababababababababababababababababababababababababab","source":"title"},"app":"com.example.mail","period":30,"seen":0,"first":20705,"last":20765,"next":20795,"missed":3,"mark":"stale","since":20890}"#,
];

fn entry(name: &str) -> &'static KindEntry {
    KINDS
        .iter()
        .find(|k| k.name == name)
        .unwrap_or_else(|| panic!("no kind {name}"))
}

fn workflow_of_a_record(entry: &KindEntry) -> String {
    let line = entry.example.as_bytes();
    match entry.name {
        "learn_run" => LearnRun::parse(line).unwrap().workflow.as_str().to_owned(),
        "learn_routine" => LearnRoutine::parse(line)
            .unwrap()
            .workflow
            .as_str()
            .to_owned(),
        "learn_edit" => LearnEdit::parse(line).unwrap().workflow.as_str().to_owned(),
        other => panic!("{other} is no kind of the learning run"),
    }
}

#[test]
fn kinds_have_workflow_key() {
    let names: Vec<&str> = KINDS.iter().map(|k| k.name).collect();
    assert!(names.contains(&"learn_run"), "{names:?}");
    assert!(names.contains(&"learn_routine"), "{names:?}");
    for entry in KINDS {
        assert_eq!(entry.workflow_key, Some("workflow"), "{}", entry.name);
        assert!(entry.fields.contains(&("workflow", Id)), "{}", entry.name);
        let row: serde_json::Value = serde_json::from_str(entry.example).unwrap();
        let under_the_key = row
            .get("workflow")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{}: no workflow id under the key", entry.name));
        assert_eq!(workflow_of_a_record(entry), under_the_key, "{}", entry.name);
        assert_eq!(check_record(entry, entry.example), Ok(()), "{}", entry.name);
    }
}

#[test]
fn the_two_kinds_with_no_text_are_bounded_at_93_days_of_origin_capture_in_the_family_learn() {
    let names: Vec<&str> = KINDS.iter().map(|k| k.name).collect();
    for name in &names {
        assert!(
            ["learn_edit", "learn_run", "learn_routine"].contains(name),
            "{name} is no kind of contracts.md L1.1"
        );
        assert_eq!(names.iter().filter(|n| *n == name).count(), 1, "{name}");
    }
    for entry in KINDS {
        assert_eq!(entry.family, "learn", "{}", entry.name);
        if entry.name == "learn_edit" {
            assert_eq!(entry.class, RetentionClass::Kept);
            assert_eq!(entry.origin, Origin::Own);
            continue;
        }
        assert_eq!(
            entry.class,
            RetentionClass::Bounded { days: 93 },
            "{}",
            entry.name
        );
        assert_eq!(entry.origin, Origin::Capture, "{}", entry.name);
    }
    assert_eq!(
        entry("learn_run").fields,
        [
            ("v", Version),
            ("workflow", Id),
            ("agent", Id),
            ("run", Id),
            ("rev", Count),
            ("day", Day),
            ("mode", Name),
            ("state", Name),
            ("cause", Name),
            ("base", Count),
            ("assisted", Count),
            ("kept", Count),
            ("failed_step", Count),
        ]
    );
    assert_eq!(
        entry("learn_routine").fields,
        [
            ("v", Version),
            ("workflow", Id),
            ("agent", Id),
            ("rev", Count),
            ("day", Day),
            ("category", Name),
            ("site", Site),
            ("app", App),
            ("period", Count),
            ("seen", Count),
            ("first", Day),
            ("last", Day),
            ("next", Day),
            ("missed", Count),
            ("mark", Name),
            ("since", Day),
        ]
    );
    assert_eq!(entry("learn_run").example, RUN_EXAMPLE);
    assert_eq!(entry("learn_routine").example, ROUTINE_EXAMPLE);
}

#[test]
fn the_entries_pass_the_rules_of_the_registry() {
    let entries: Vec<&KindEntry> = KINDS.iter().collect();
    assert_eq!(check_entries(&entries), Vec::<String>::new());
    assert_eq!(check_registry(), Vec::<String>::new());
    let registered: Vec<&str> = registry().map(|k| k.name).collect();
    for entry in KINDS {
        assert_eq!(
            registered.iter().filter(|n| **n == entry.name).count(),
            1,
            "{}",
            entry.name
        );
    }
}

#[test]
fn every_row_that_the_typed_parser_takes_passes_the_generic_check_of_its_entry() {
    for line in OTHER_RUNS.iter().chain([&RUN_EXAMPLE]) {
        let run = LearnRun::parse(line.as_bytes()).unwrap();
        assert_eq!(run.to_json().unwrap(), *line);
        assert_eq!(check_record(entry("learn_run"), line), Ok(()), "{line}");
    }
    for line in OTHER_ROUTINES.iter().chain([&ROUTINE_EXAMPLE]) {
        let routine = LearnRoutine::parse(line.as_bytes()).unwrap();
        assert_eq!(routine.to_json().unwrap(), *line);
        assert_eq!(check_record(entry("learn_routine"), line), Ok(()), "{line}");
    }
}

#[test]
fn the_generic_check_and_the_typed_parser_both_refuse_a_row_outside_the_entry() {
    let cases = [
        ("learn_run", RUN_EXAMPLE.replacen(r#""kept":1,"#, "", 1)),
        (
            "learn_run",
            RUN_EXAMPLE.replacen(r#""kept":1,"#, r#""kept":1,"edit":null,"#, 1),
        ),
        (
            "learn_run",
            RUN_EXAMPLE.replacen(r#""kept":1"#, r#""kept":true"#, 1),
        ),
        (
            "learn_run",
            RUN_EXAMPLE.replacen(r#""day":20767"#, r#""day":null"#, 1),
        ),
        (
            "learn_routine",
            ROUTINE_EXAMPLE.replacen(r#""seen":3"#, r#""seen":3.5"#, 1),
        ),
        (
            "learn_routine",
            ROUTINE_EXAMPLE.replacen(r#""missed":0,"#, r#""missed":0,"missed":0,"#, 1),
        ),
        (
            "learn_routine",
            ROUTINE_EXAMPLE.replacen(r#""app":"com.google.Chrome""#, r#""app":"/x""#, 1),
        ),
    ];
    for (kind, line) in cases {
        assert!(check_record(entry(kind), &line).is_err(), "{line}");
        let typed = match kind {
            "learn_run" => LearnRun::parse(line.as_bytes()).is_err(),
            _ => LearnRoutine::parse(line.as_bytes()).is_err(),
        };
        assert!(typed, "{line}");
    }
}

#[test]
fn the_error_names_of_the_learning_handlers_are_declared_and_read_back() {
    assert_eq!(ERROR_NAMES, ["binding"]);
    let id = RequestId::from_random([7; 16]);
    for name in ERROR_NAMES {
        let error = ErrorName::read(name).unwrap();
        assert_eq!(error.as_str(), *name);
        let bytes = write_reply(Some(&id), Err(error)).unwrap();
        assert_eq!(
            String::from_utf8(bytes.clone()).unwrap(),
            format!(r#"{{"v":1,"id":"{}","error":"{name}"}}"#, id.as_str())
        );
        let reply = decode_reply(&bytes).unwrap();
        assert_eq!(reply.outcome.map(|_| ()), Err(error));
    }
}
