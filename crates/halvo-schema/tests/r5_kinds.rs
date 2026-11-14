use halvo_schema::kinds::{
    check_entries, check_record, check_registry, registry, FieldType, KindEntry, Origin,
    RetentionClass,
};
use halvo_schema::learning::names::EDIT_STATES;
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

const OTHER_EDITS: [&str; 3] = [
    r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d","rev":3,"base":1,"n":4,"state":"active","cause":null,"on":null,"runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0","8e7d6c5b4a39281706f5e4d3c2b1a09f","7d6c5b4a39281706f5e4d3c2b1a09f8e"],"first":20767,"last":20771,"clean":2,"target":{"desc":"","locators":[{"by":"role","value":"button","name":"Pay"},{"by":"label","value":"Pay","name":null},{"by":"text","value":"Pay","name":null},{"by":"test_id","value":"pay","name":null}]}}"#,
    r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d","rev":3,"base":2,"n":7,"state":"reverted","cause":"class_mismatch","on":"4b3a29180f7e6d5c4b3a29180f7e6d5c","runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0","8e7d6c5b4a39281706f5e4d3c2b1a09f"],"first":20767,"last":20770,"clean":1,"target":{"desc":"link Pay now","locators":[{"by":"label","value":"Pay now","name":null},{"by":"css","value":"a.pay","name":null}]}}"#,
    r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d","rev":2,"base":1,"n":4,"state":"withdrawn","cause":null,"on":null,"runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0"],"first":20767,"last":20767,"clean":0,"target":{"desc":"x","locators":[{"by":"css","value":"button.pay","name":null}]}}"#,
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
fn an_edit_row_of_each_state_passes_the_check_of_the_registered_entry_and_is_written_back_to_its_bytes(
) {
    let edit = entry("learn_edit");
    let mut states = Vec::new();
    for line in [edit.example].into_iter().chain(OTHER_EDITS) {
        let row = LearnEdit::parse(line.as_bytes()).unwrap();
        assert_eq!(row.to_json().unwrap(), line);
        assert_eq!(check_record(edit, line), Ok(()), "{line}");
        let read: serde_json::Value = serde_json::from_str(line).unwrap();
        states.push(read["state"].as_str().unwrap().to_owned());
    }
    assert_eq!(states, EDIT_STATES);
}

#[test]
fn the_generic_check_and_the_typed_parser_both_refuse_a_row_outside_the_entry() {
    let edit = entry("learn_edit").example;
    let id = r#""0f1e2d3c4b5a69788796a5b4c3d2e1f0""#;
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
        ("learn_edit", edit.replacen(r#""clean":0,"#, "", 1)),
        (
            "learn_edit",
            edit.replacen(r#""clean":0,"#, r#""clean":0,"day":20767,"#, 1),
        ),
        (
            "learn_edit",
            edit.replacen(r#""clean":0"#, r#""clean":false"#, 1),
        ),
        ("learn_edit", edit.replacen(r#""rev":1"#, r#""rev":1.0"#, 1)),
        (
            "learn_edit",
            edit.replacen(r#""clean":0,"#, r#""clean":0,"clean":0,"#, 1),
        ),
        (
            "learn_edit",
            edit.replacen(&format!(r#""workflow":{id}"#), r#""workflow":null"#, 1),
        ),
        (
            "learn_edit",
            edit.replacen(
                &format!(r#""workflow":{id}"#),
                &format!(r#""workflow":[{id}]"#),
                1,
            ),
        ),
        (
            "learn_edit",
            edit.replacen(
                &format!(r#""workflow":{id}"#),
                &format!(r#""workflow":{{"id":{id}}}"#),
                1,
            ),
        ),
        ("learn_edit", edit.replacen(r#""desc":"#, r#""Desc":"#, 1)),
        (
            "learn_edit",
            edit.replacen(r#""by":"role","#, r#""by":"role","0":"x","#, 1),
        ),
        (
            "learn_edit",
            edit.replacen(r#""name":"Submit payment""#, r#""name":true"#, 1),
        ),
        (
            "learn_edit",
            edit.replacen(
                r#""desc":"button \"Submit payment\"""#,
                &format!(r#""desc":"{}""#, "a".repeat(1025)),
                1,
            ),
        ),
    ];
    let taken = |kind: &str, line: &str| match kind {
        "learn_run" => LearnRun::parse(line.as_bytes()).is_ok(),
        "learn_routine" => LearnRoutine::parse(line.as_bytes()).is_ok(),
        "learn_edit" => LearnEdit::parse(line.as_bytes()).is_ok(),
        other => panic!("{other} is no kind of the learning run"),
    };
    for (kind, line) in cases {
        assert!(taken(kind, entry(kind).example), "{kind}");
        assert!(check_record(entry(kind), &line).is_err(), "{line}");
        assert!(!taken(kind, &line), "{line}");
    }
}

const A_NAME: &str = r#""x""#;
const AN_ID: &str = r#""0f1e2d3c4b5a69788796a5b4c3d2e1f0""#;
const A_HASH: &str = r#""abababababababababababababababababababababababababababababababab""#;
const A_TEXT: &str = r#""com.example.app""#;
const A_SITE: &str = r#"{"key":"northwind-power.test","source":"address_field"}"#;
const ONE: &str = "1";
const TWO: &str = "2";
const MINUS_ONE: &str = "-1";
const ABOVE_U32: &str = "4294967296";

const NOT_VERSION: &[&str] = &[
    A_NAME, AN_ID, A_HASH, A_TEXT, A_SITE, TWO, MINUS_ONE, ABOVE_U32,
];
const NOT_ID: &[&str] = &[
    A_NAME, A_HASH, A_TEXT, A_SITE, ONE, TWO, MINUS_ONE, ABOVE_U32,
];
const NOT_NAME: &[&str] = &[A_HASH, A_TEXT, A_SITE, ONE, TWO, MINUS_ONE, ABOVE_U32];
const NOT_COUNT: &[&str] = &[A_NAME, AN_ID, A_HASH, A_TEXT, A_SITE, MINUS_ONE, ABOVE_U32];
const NOT_TEXT: &[&str] = &[ONE, TWO, MINUS_ONE, ABOVE_U32];

const EDIT_KEYS: [(&str, &str, &str, &[&str]); 15] = [
    ("v", r#""v":1"#, r#""v":@"#, NOT_VERSION),
    (
        "workflow",
        r#""workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0""#,
        r#""workflow":@"#,
        NOT_ID,
    ),
    (
        "agent",
        r#""agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809""#,
        r#""agent":@"#,
        NOT_ID,
    ),
    (
        "edit",
        r#""edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d""#,
        r#""edit":@"#,
        NOT_ID,
    ),
    ("rev", r#""rev":3"#, r#""rev":@"#, NOT_COUNT),
    ("base", r#""base":2"#, r#""base":@"#, NOT_COUNT),
    ("n", r#""n":7"#, r#""n":@"#, NOT_COUNT),
    ("state", r#""state":"reverted""#, r#""state":@"#, NOT_NAME),
    (
        "cause",
        r#""cause":"class_mismatch""#,
        r#""cause":@"#,
        NOT_NAME,
    ),
    (
        "on",
        r#""on":"4b3a29180f7e6d5c4b3a29180f7e6d5c""#,
        r#""on":@"#,
        NOT_ID,
    ),
    (
        "runs",
        r#""runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0","8e7d6c5b4a39281706f5e4d3c2b1a09f"]"#,
        r#""runs":[@]"#,
        NOT_ID,
    ),
    ("first", r#""first":20767"#, r#""first":@"#, NOT_COUNT),
    ("last", r#""last":20770"#, r#""last":@"#, NOT_COUNT),
    ("clean", r#""clean":1"#, r#""clean":@"#, NOT_COUNT),
    ("target", r#""value":"a.pay""#, r#""value":@"#, NOT_TEXT),
];

const WITNESS_HOMES: [(&str, &str, &str, &str); 9] = [
    (
        A_NAME,
        "learn_edit",
        r#""cause":"class_mismatch""#,
        r#""cause":@"#,
    ),
    (
        AN_ID,
        "learn_edit",
        r#""on":"4b3a29180f7e6d5c4b3a29180f7e6d5c""#,
        r#""on":@"#,
    ),
    (
        A_HASH,
        "agent_effect",
        r#""digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa""#,
        r#""digest":@"#,
    ),
    (A_TEXT, "learn_edit", r#""value":"a.pay""#, r#""value":@"#),
    (A_SITE, "learn_routine", r#""site":null"#, r#""site":@"#),
    (ONE, "learn_edit", r#""rev":3"#, r#""rev":@"#),
    (TWO, "learn_edit", r#""n":7"#, r#""n":@"#),
    (
        MINUS_ONE,
        "agent_effect",
        r#""at":1794339000000000001"#,
        r#""at":@"#,
    ),
    (
        ABOVE_U32,
        "agent_effect",
        r#""at":1794339000000000001"#,
        r#""at":@"#,
    ),
];

fn registered(name: &str) -> &'static KindEntry {
    registry()
        .find(|k| k.name == name)
        .unwrap_or_else(|| panic!("no kind {name}"))
}

#[test]
fn a_value_of_another_field_type_under_each_key_of_an_edit_row_is_refused_by_the_entry_and_the_parser(
) {
    let witnesses = [
        A_NAME, AN_ID, A_HASH, A_TEXT, A_SITE, ONE, TWO, MINUS_ONE, ABOVE_U32,
    ];
    assert_eq!(WITNESS_HOMES.map(|(value, ..)| value), witnesses);
    for (value, kind, from, to) in WITNESS_HOMES {
        let row = match kind {
            "learn_edit" => OTHER_EDITS[1],
            "learn_routine" => OTHER_ROUTINES[0],
            _ => registered(kind).example,
        };
        assert_eq!(row.matches(from).count(), 1, "{from}");
        let line = row.replacen(from, &to.replacen('@', value, 1), 1);
        assert_eq!(
            check_record(registered(kind), &line),
            Ok(()),
            "{kind}: {value}"
        );
    }
    let edit = entry("learn_edit");
    let row = OTHER_EDITS[1];
    assert_eq!(check_record(edit, row), Ok(()));
    let read: serde_json::Map<String, serde_json::Value> = serde_json::from_str(row).unwrap();
    let mut keys: Vec<&str> = read.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut table: Vec<&str> = EDIT_KEYS.iter().map(|(key, ..)| *key).collect();
    table.sort_unstable();
    assert_eq!(table, keys);
    let mut cases = 0;
    for (key, from, to, values) in EDIT_KEYS {
        assert_eq!(row.matches(from).count(), 1, "{from}");
        for value in values {
            let line = row.replacen(from, &to.replacen('@', value, 1), 1);
            let error = check_record(edit, &line).expect_err(&format!("{key}: {value}"));
            assert!(
                error.starts_with(&format!("learn_edit: {key}: ")),
                "{key}: {value}: {error}"
            );
            assert!(LearnEdit::parse(line.as_bytes()).is_err(), "{key}: {value}");
            cases += 1;
        }
    }
    assert_eq!(cases, 108);
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

#[test]
fn the_edit_kind_is_kept_of_origin_own_and_holds_the_fields_of_its_row_in_a_closed_table() {
    let edit = entry("learn_edit");
    assert_eq!(edit.class, RetentionClass::Kept);
    assert_eq!(edit.origin, Origin::Own);
    assert_eq!(edit.family, "learn");
    assert_eq!(edit.workflow_key, Some("workflow"));
    assert_eq!(
        edit.fields,
        [
            ("v", Version),
            ("workflow", Id),
            ("agent", Id),
            ("edit", Id),
            ("rev", Count),
            ("base", Count),
            ("n", Count),
            ("state", Name),
            ("cause", Name),
            ("on", Id),
            ("runs", Id),
            ("first", Day),
            ("last", Day),
            ("clean", Count),
            ("target", FieldType::OwnText),
        ]
    );
}
