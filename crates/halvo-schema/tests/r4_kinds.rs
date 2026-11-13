use halvo_schema::kinds::{
    check_entries, check_record, check_registry, registry, FieldType, KindEntry, Origin,
    RetentionClass,
};
use halvo_schema::request::{decode_reply, write_reply, ErrorName};
use halvo_schema::RequestId;

use FieldType::{Count, Day, Hash, Id, Instant, Name, OwnText, Version};

const FIELDS: [(&str, FieldType); 18] = [
    ("v", Version),
    ("workflow", Id),
    ("agent", Id),
    ("seq", Count),
    ("day", Day),
    ("at", Instant),
    ("event", Name),
    ("run", Id),
    ("slot", Day),
    ("item", Id),
    ("kind", Name),
    ("effect", Id),
    ("approval", Id),
    ("digest", Hash),
    ("answer", Name),
    ("cause", Name),
    ("trigger", Name),
    ("origin", OwnText),
];

const ROW: &str = r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","seq":1,"day":20767,"at":1794339000000000000,"event":"run_started","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","slot":20767,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":null,"trigger":"schedule","origin":null}"#;

fn entry() -> &'static KindEntry {
    let found: Vec<&KindEntry> = registry().filter(|e| e.name == "coord_event").collect();
    assert_eq!(found.len(), 1, "coord_event stands once in the registry");
    found[0]
}

#[test]
fn r4_kind_coord_event_is_bounded_at_93_days_of_origin_own_in_the_family_coord() {
    let entry = entry();
    assert_eq!(entry.class, RetentionClass::Bounded { days: 93 });
    assert_eq!(entry.origin, Origin::Own);
    assert_eq!(entry.family, "coord");
    assert_eq!(entry.workflow_key, Some("workflow"));
    assert_eq!(halvo_schema::coordinator::KINDS, [*entry]);
    assert_eq!(
        halvo_schema_core::coordinator::KINDS,
        halvo_schema::coordinator::KINDS
    );
}

#[test]
fn r4_kind_coord_event_holds_the_keys_of_i7_1_and_origin_and_no_other() {
    assert_eq!(entry().fields, FIELDS);
    for (key, field_type) in entry().fields {
        assert!(
            !matches!(
                field_type,
                FieldType::Text | FieldType::Token | FieldType::Ordinal
            ),
            "{key}"
        );
        assert_eq!(*field_type == OwnText, *key == "origin", "{key}");
    }
}

#[test]
fn r4_kind_coord_event_passes_the_checks_of_the_registry() {
    assert_eq!(check_entries(&[entry()]), Vec::<String>::new());
    assert_eq!(check_registry(), Vec::<String>::new());
    assert_eq!(check_record(entry(), entry().example), Ok(()));
    assert_eq!(check_record(entry(), ROW), Ok(()));
}

#[test]
fn r4_kind_coord_event_refuses_a_row_without_its_workflow_its_day_or_with_a_text_key() {
    let refused = |row: String, error: &str| {
        assert_eq!(check_record(entry(), &row), Err(error.to_owned()), "{row}");
    };
    refused(
        ROW.replacen(
            r#""workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0""#,
            r#""workflow":null"#,
            1,
        ),
        "coord_event: workflow: not_one_value",
    );
    refused(
        ROW.replacen(r#""day":20767"#, r#""day":null"#, 1),
        "coord_event: day: not_one_value",
    );
    refused(
        ROW.replacen(
            r#""origin":null"#,
            r#""origin":null,"text":"Amount due""#,
            1,
        ),
        "coord_event: a key that the fields do not list: unknown_key",
    );
    refused(
        ROW.replacen(r#","origin":null"#, "", 1),
        "coord_event: origin: missing_key",
    );
    refused(
        ROW.replacen(r#""at":1794339000000000000"#, r#""at":"19:30""#, 1),
        "coord_event: at: string_form",
    );
}

#[test]
fn r4_error_names_are_binding_and_seq_and_no_other() {
    assert_eq!(halvo_schema::coordinator::ERROR_NAMES, ["binding", "seq"]);
    assert_eq!(
        halvo_schema_core::coordinator::ERROR_NAMES,
        halvo_schema::coordinator::ERROR_NAMES
    );
    let id = RequestId::parse("2c3d4e5f60718293a4b5c6d7e8f90a1b").unwrap();
    for name in ["binding", "seq"] {
        let error = ErrorName::read(name).unwrap();
        assert_eq!(error.as_str(), name);
        let bytes = write_reply(Some(&id), Err(error)).unwrap();
        let line = String::from_utf8(bytes.clone()).unwrap();
        assert_eq!(
            line,
            format!(r#"{{"v":1,"id":"2c3d4e5f60718293a4b5c6d7e8f90a1b","error":"{name}"}}"#)
        );
        assert_eq!(decode_reply(&bytes).unwrap().outcome.unwrap_err(), error);
    }
}
