mod common;

use common::values;
use halvo_schema::kinds::{
    check_entries, check_record, check_registry, registry, FieldType, KindEntry, Origin,
    RetentionClass, KINDS,
};
use halvo_schema::{coordinator, executable, learning, routing};
use halvo_schema::{Evidence, FullWorkflow, Summary, Support};

const HASH: &str = "abababababababababababababababababababababababababababababababab";
const W1: &str = "10000000000000000000000000000001";

fn entry(name: &str) -> &'static KindEntry {
    KINDS.iter().find(|k| k.name == name).expect(name)
}

fn names(entries: &[&KindEntry]) -> Vec<&'static str> {
    entries.iter().map(|k| k.name).collect()
}

#[test]
fn registry_is_sound() {
    assert_eq!(check_registry(), Vec::<String>::new());
}

#[test]
fn the_kinds_of_this_run_are_the_four_of_the_contract() {
    let rows: Vec<(&str, RetentionClass, Origin, &str, Option<&str>)> = KINDS
        .iter()
        .map(|k| (k.name, k.class, k.origin, k.family, k.workflow_key))
        .collect();
    assert_eq!(
        rows,
        [
            (
                "workflow",
                RetentionClass::Kept,
                Origin::Capture,
                "workflow",
                Some("id")
            ),
            (
                "summary",
                RetentionClass::Bounded { days: 93 },
                Origin::Capture,
                "workflow",
                None
            ),
            (
                "support",
                RetentionClass::Bounded { days: 93 },
                Origin::Capture,
                "workflow",
                Some("workflow")
            ),
            (
                "evidence",
                RetentionClass::Window,
                Origin::Capture,
                "workflow",
                None
            ),
        ]
    );
}

#[test]
fn the_fields_of_each_kind_are_its_top_level_keys_with_their_innermost_types() {
    use FieldType::*;
    assert_eq!(
        entry("workflow").fields,
        [
            ("v", Version),
            ("id", Id),
            ("state", Name),
            ("category", Name),
            ("site", Site),
            ("app", App),
            ("trigger", Name),
            ("recurrence", Count),
            ("recurrence", Day),
            ("confidence", Count),
            ("agent", Id),
        ]
    );
    assert_eq!(
        entry("summary").fields,
        [
            ("v", Version),
            ("id", Id),
            ("category", Name),
            ("site", Site),
            ("app", App),
            ("day", Day),
            ("part", Name),
            ("minutes", Count),
            ("keys", Count),
            ("pointer", Count),
        ]
    );
    assert_eq!(
        entry("support").fields,
        [
            ("v", Version),
            ("workflow", Id),
            ("summary", Id),
            ("day", Day)
        ]
    );
    assert_eq!(
        entry("evidence").fields,
        [
            ("v", Version),
            ("summary", Id),
            ("segments", Ordinal),
            ("expires", Instant),
            ("start", Instant),
            ("end", Instant),
            ("hint", Token),
            ("steps", App),
            ("steps", Name),
            ("steps", Text),
            ("steps", Domain),
        ]
    );
}

#[test]
fn each_example_is_a_record_of_its_kind_and_of_its_rust_type() {
    for kind in KINDS {
        assert_eq!(check_record(kind, kind.example), Ok(()), "{}", kind.name);
    }
    let workflow: FullWorkflow = serde_json::from_str(entry("workflow").example).unwrap();
    assert_eq!(
        serde_json::to_string(&workflow).unwrap(),
        entry("workflow").example
    );
    let summary: Summary = serde_json::from_str(entry("summary").example).unwrap();
    assert_eq!(
        serde_json::to_string(&summary).unwrap(),
        entry("summary").example
    );
    assert!(summary.keys.is_some() && summary.pointer.is_some());
    let support: Support = serde_json::from_str(entry("support").example).unwrap();
    assert_eq!(
        serde_json::to_string(&support).unwrap(),
        entry("support").example
    );
    let evidence: Evidence = serde_json::from_str(entry("evidence").example).unwrap();
    assert_eq!(
        serde_json::to_string(&evidence).unwrap(),
        entry("evidence").example
    );
    assert_eq!(evidence.steps.len(), 1);
    assert_eq!(workflow.agent, None);
    assert!(matches!(
        workflow.site,
        Some(halvo_schema::SiteValue::Title { .. })
    ));
}

#[test]
fn registry_chains_the_kinds_of_this_run_then_the_four_files_of_later_runs() {
    let chained: Vec<&KindEntry> = KINDS
        .iter()
        .chain(executable::KINDS)
        .chain(coordinator::KINDS)
        .chain(learning::KINDS)
        .chain(routing::KINDS)
        .collect();
    let registered: Vec<&KindEntry> = registry().collect();
    assert_eq!(registered, chained);
    assert_eq!(
        names(&registered[..4]),
        ["workflow", "summary", "support", "evidence"]
    );
}

#[test]
fn a_retention_class_has_its_name_and_a_bounded_class_its_days() {
    assert_eq!(RetentionClass::Window.name(), "window");
    assert_eq!(RetentionClass::Bounded { days: 93 }.name(), "bounded");
    assert_eq!(RetentionClass::Bounded { days: 1 }.name(), "bounded");
    assert_eq!(RetentionClass::Kept.name(), "kept");
}

#[test]
fn field_type_has_one_variant_per_row_of_the_table() {
    use FieldType::*;
    let all = [
        Version, Id, Name, App, Domain, Hash, Site, Count, Day, Instant, Ordinal, Token, Text,
        OwnText,
    ];
    for t in all {
        let row = match t {
            Version => "version",
            Id => "id",
            Name => "name",
            App => "app",
            Domain => "domain",
            Hash => "hash",
            Site => "site",
            Count => "count",
            Day => "day",
            Instant => "instant",
            Ordinal => "ordinal",
            Token => "token",
            Text => "text",
            OwnText => "own_text",
        };
        assert!(!row.is_empty());
    }
}

const LATER_BOUNDED: KindEntry = KindEntry {
    name: "probe_bounded",
    class: RetentionClass::Bounded { days: 30 },
    origin: Origin::Capture,
    family: "probe",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", FieldType::Version),
        ("workflow", FieldType::Id),
        ("day", FieldType::Day),
        ("count", FieldType::Count),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","day":20767,"count":2}"#,
};

const LATER_OWN: KindEntry = KindEntry {
    name: "probe_own",
    class: RetentionClass::Kept,
    origin: Origin::Own,
    family: "probe",
    workflow_key: Some("workflow"),
    fields: &[
        ("v", FieldType::Version),
        ("workflow", FieldType::Id),
        ("at", FieldType::Instant),
        ("note", FieldType::OwnText),
        ("site", FieldType::Site),
    ],
    example: r#"{"v":1,"workflow":"10000000000000000000000000000001","at":1794339000000000000,"note":"Pay on the first of the month","site":{"key":"shop.test","source":"sign_in"}}"#,
};

const LATER_WINDOW: KindEntry = KindEntry {
    name: "probe_window",
    class: RetentionClass::Window,
    origin: Origin::Capture,
    family: "probe",
    workflow_key: None,
    fields: &[
        ("v", FieldType::Version),
        ("segments", FieldType::Ordinal),
        ("expires", FieldType::Instant),
        ("hint", FieldType::Token),
        ("title", FieldType::Text),
    ],
    example: r#"{"v":1,"segments":[7],"expires":1795478400000000000,"hint":"shop","title":"Inbox"}"#,
};

fn one_report(entries: &[&KindEntry], fragment: &str) {
    let reports = check_entries(entries);
    assert_eq!(reports.len(), 1, "{reports:?}");
    assert!(
        reports[0].contains(fragment),
        "{:?} lacks {fragment:?}",
        reports[0]
    );
}

#[test]
fn sound_entries_of_later_runs_give_no_report() {
    let mut all: Vec<&KindEntry> = KINDS.iter().collect();
    all.extend([&LATER_BOUNDED, &LATER_OWN, &LATER_WINDOW]);
    assert_eq!(check_entries(&all), Vec::<String>::new());
    assert_eq!(check_entries(&[]), Vec::<String>::new());
}

#[test]
fn check_entries_reports_a_name_that_stands_twice() {
    one_report(
        &[&LATER_BOUNDED, &LATER_OWN, &LATER_BOUNDED],
        "stands 2 times",
    );
    let mut all: Vec<&KindEntry> = registry().collect();
    let copy = KindEntry {
        family: "probe",
        workflow_key: None,
        ..*entry("summary")
    };
    all.push(&copy);
    let reports = check_entries(&all);
    assert!(
        reports.iter().any(|r| r.contains("stands 2 times")),
        "{reports:?}"
    );
}

#[test]
fn check_entries_reports_a_name_or_a_family_outside_the_form() {
    for bad in [
        "",
        "Learn_routine",
        "learn-routine",
        "learn routine",
        "a23456789012345678901234567890123",
    ] {
        one_report(
            &[&KindEntry {
                name: bad,
                ..LATER_BOUNDED
            }],
            "the name is not 1 to 32 bytes",
        );
        one_report(
            &[&KindEntry {
                family: bad,
                ..LATER_BOUNDED
            }],
            "the family is not 1 to 32 bytes",
        );
    }
    let longest = "a2345678901234567890123456789012";
    assert_eq!(
        check_entries(&[&KindEntry {
            name: longest,
            family: longest,
            ..LATER_BOUNDED
        }]),
        Vec::<String>::new()
    );
}

#[test]
fn check_entries_reports_the_family_workflow_outside_the_four_kinds_of_this_run() {
    one_report(
        &[&KindEntry {
            family: "workflow",
            ..LATER_BOUNDED
        }],
        "the family workflow belongs to the four kinds of this run",
    );
    let changed = KindEntry {
        class: RetentionClass::Bounded { days: 92 },
        ..*entry("summary")
    };
    one_report(
        &[&changed],
        "the family workflow belongs to the four kinds of this run",
    );
    let four: Vec<&KindEntry> = KINDS.iter().collect();
    assert_eq!(check_entries(&four), Vec::<String>::new());
}

#[test]
fn check_entries_reports_a_bounded_kind_with_0_days_or_without_a_day() {
    one_report(
        &[&KindEntry {
            class: RetentionClass::Bounded { days: 0 },
            ..LATER_BOUNDED
        }],
        "a bounded kind with 0 days",
    );
    one_report(
        &[&KindEntry {
            fields: &[
                ("v", FieldType::Version),
                ("workflow", FieldType::Id),
                ("count", FieldType::Count),
            ],
            example: r#"{"v":1,"workflow":"10000000000000000000000000000001","count":2}"#,
            ..LATER_BOUNDED
        }],
        "a bounded kind without the key day of the type Day",
    );
    one_report(
        &[&KindEntry {
            fields: &[
                ("v", FieldType::Version),
                ("workflow", FieldType::Id),
                ("day", FieldType::Count),
                ("count", FieldType::Count),
            ],
            ..LATER_BOUNDED
        }],
        "a bounded kind without the key day of the type Day",
    );
}

#[test]
fn check_entries_reports_a_window_kind_without_expires_or_segments() {
    one_report(
        &[&KindEntry {
            fields: &[
                ("v", FieldType::Version),
                ("segments", FieldType::Ordinal),
                ("hint", FieldType::Token),
                ("title", FieldType::Text),
            ],
            example: r#"{"v":1,"segments":[7],"hint":"shop","title":"Inbox"}"#,
            ..LATER_WINDOW
        }],
        "a window kind without the key expires of the type Instant",
    );
    one_report(
        &[&KindEntry {
            fields: &[
                ("v", FieldType::Version),
                ("expires", FieldType::Instant),
                ("hint", FieldType::Token),
                ("title", FieldType::Text),
            ],
            example: r#"{"v":1,"expires":1795478400000000000,"hint":"shop","title":"Inbox"}"#,
            ..LATER_WINDOW
        }],
        "a window kind without the key segments of the type Ordinal",
    );
}

#[test]
fn check_entries_reports_a_workflow_key_that_is_no_key_of_the_type_id() {
    one_report(
        &[&KindEntry {
            workflow_key: Some("count"),
            ..LATER_BOUNDED
        }],
        "the workflow_key count is no key of the fields with the type Id",
    );
    one_report(
        &[&KindEntry {
            workflow_key: Some("agent"),
            ..LATER_BOUNDED
        }],
        "the workflow_key agent is no key of the fields with the type Id",
    );
}

#[test]
fn check_entries_reports_a_kind_of_origin_own_that_is_a_window_kind_or_names_no_workflow() {
    one_report(
        &[&KindEntry {
            origin: Origin::Own,
            workflow_key: Some("workflow"),
            fields: &[
                ("v", FieldType::Version),
                ("workflow", FieldType::Id),
                ("segments", FieldType::Ordinal),
                ("expires", FieldType::Instant),
            ],
            example: r#"{"v":1,"workflow":"10000000000000000000000000000001","segments":[7],"expires":1795478400000000000}"#,
            ..LATER_WINDOW
        }],
        "a kind of origin own with the class window",
    );
    one_report(
        &[&KindEntry {
            workflow_key: None,
            ..LATER_OWN
        }],
        "a kind of origin own without a workflow_key",
    );
}

#[test]
fn check_entries_reports_a_field_type_that_the_class_and_the_origin_do_not_allow() {
    let capture_kept = |t: FieldType, value: &'static str| KindEntry {
        class: RetentionClass::Kept,
        fields: Box::leak(Box::new([
            ("v", FieldType::Version),
            ("workflow", FieldType::Id),
            ("day", FieldType::Day),
            ("count", t),
        ])),
        example: Box::leak(
            format!(r#"{{"v":1,"workflow":"{W1}","day":20767,"count":{value}}}"#).into_boxed_str(),
        ),
        ..LATER_BOUNDED
    };
    for (t, value, name) in [
        (FieldType::Instant, "1794339000000000000", "Instant"),
        (FieldType::Ordinal, "7", "Ordinal"),
        (FieldType::Token, r#""shop""#, "Token"),
        (FieldType::Text, r#""Inbox""#, "Text"),
        (FieldType::OwnText, r#""Inbox""#, "OwnText"),
    ] {
        for class in [RetentionClass::Kept, RetentionClass::Bounded { days: 30 }] {
            let e = KindEntry {
                class,
                ..capture_kept(t, value)
            };
            one_report(
                &[&e],
                &format!(
                    "a bounded or kept kind of origin capture with the key count of the type {name}"
                ),
            );
        }
        let own = KindEntry {
            origin: Origin::Own,
            ..capture_kept(t, value)
        };
        let reports = check_entries(&[&own]);
        if matches!(t, FieldType::Instant | FieldType::OwnText) {
            assert_eq!(reports, Vec::<String>::new(), "{name}");
        } else {
            assert_eq!(reports.len(), 1, "{reports:?}");
            assert!(
                reports[0].contains(&format!(
                    "a kind of origin own with the key count of the type {name}"
                )),
                "{reports:?}"
            );
        }
    }
    one_report(
        &[&KindEntry {
            fields: &[
                ("v", FieldType::Version),
                ("segments", FieldType::Ordinal),
                ("expires", FieldType::Instant),
                ("hint", FieldType::Token),
                ("title", FieldType::OwnText),
            ],
            ..LATER_WINDOW
        }],
        "a window kind with the key title of the type OwnText",
    );
}

#[test]
fn check_entries_reports_an_example_that_check_record_refuses_or_whose_keys_are_out_of_order() {
    one_report(
        &[&KindEntry {
            example: r#"{"v":1,"workflow":"10000000000000000000000000000001","day":-1,"count":2}"#,
            ..LATER_BOUNDED
        }],
        "the example is refused: probe_bounded: day: integer_form",
    );
    one_report(
        &[&KindEntry {
            example: r#"{"v":1,"day":20767,"workflow":"10000000000000000000000000000001","count":2}"#,
            ..LATER_BOUNDED
        }],
        "the top-level keys of the example are not the keys of the fields in their order",
    );
    one_report(
        &[&KindEntry {
            example: "[1]",
            ..LATER_BOUNDED
        }],
        "the example is refused: probe_bounded: the record: not_an_object",
    );
}

const PROBE: KindEntry = KindEntry {
    name: "probe",
    class: RetentionClass::Window,
    origin: Origin::Capture,
    family: "probe",
    workflow_key: None,
    fields: &[
        ("version", FieldType::Version),
        ("id", FieldType::Id),
        ("name", FieldType::Name),
        ("app", FieldType::App),
        ("domain", FieldType::Domain),
        ("hash", FieldType::Hash),
        ("site", FieldType::Site),
        ("count", FieldType::Count),
        ("day", FieldType::Day),
        ("instant", FieldType::Instant),
        ("ordinal", FieldType::Ordinal),
        ("token", FieldType::Token),
        ("text", FieldType::Text),
        ("own_text", FieldType::OwnText),
        ("both", FieldType::Count),
        ("both", FieldType::Name),
    ],
    example: "{}",
};

const KEYS: [&str; 15] = [
    "version", "id", "name", "app", "domain", "hash", "site", "count", "day", "instant", "ordinal",
    "token", "text", "own_text", "both",
];

fn probe(key: &str, value: &str) -> String {
    let parts: Vec<String> = KEYS
        .iter()
        .map(|k| {
            if *k == key {
                format!("\"{k}\":{value}")
            } else {
                format!("\"{k}\":null")
            }
        })
        .collect();
    format!("{{{}}}", parts.join(","))
}

fn passes(key: &str, value: &str) {
    assert_eq!(
        check_record(&PROBE, &probe(key, value)),
        Ok(()),
        "{key}: {value}"
    );
}

fn refused(key: &str, value: &str, rule: &str) {
    let error = check_record(&PROBE, &probe(key, value)).expect_err(value);
    assert_eq!(error, format!("probe: {key}: {rule}"), "{key}: {value}");
}

#[test]
fn check_record_takes_null_for_every_key() {
    passes("version", "null");
}

#[test]
fn check_record_reads_each_field_type_in_its_json_form() {
    passes("version", "1");
    for v in ["2", "0", "-1"] {
        refused("version", v, "integer_form");
    }
    refused("version", "\"1\"", "string_form");

    passes("id", &format!("\"{W1}\""));
    for v in [
        "\"1000000000000000000000000000000\"",
        "\"100000000000000000000000000000001\"",
        "\"1000000000000000000000000000000A\"",
        "\"1000000000000000000000000000000g\"",
    ] {
        refused("id", v, "string_form");
    }
    refused("id", "1", "integer_form");

    for v in [
        "\"a\"",
        "\"proposed\"",
        "\"a2345678901234567890123456789012\"",
        "\"a_1\"",
    ] {
        passes("name", v);
    }
    for v in [
        "\"\"",
        "\"a23456789012345678901234567890123\"",
        "\"Proposed\"",
        "\"a-b\"",
        "\"a b\"",
    ] {
        refused("name", v, "string_form");
    }

    for v in ["\"com.example.browser\"", "\"browser.exe\"", "\"Pay bill\""] {
        passes("app", v);
    }
    for v in [
        "\"/usr/bin/vim\"",
        "\"\"",
        "\"..\"",
        "\"a\\\\b\"",
        "\"a\\tb\"",
    ] {
        refused("app", v, "string_form");
    }

    for v in [
        "\"shop.test\"",
        "\"github.io\"",
        "\"co.uk\"",
        "\"www.shop.test\"",
    ] {
        passes("domain", v);
    }
    for v in [
        "\"test\"",
        "\"Shop.test\"",
        "\"shop.test.\"",
        "\"192.0.2.1\"",
        "\"\"",
    ] {
        refused("domain", v, "string_form");
    }

    passes("hash", &format!("\"{HASH}\""));
    refused("hash", &format!("\"{}\"", &HASH[1..]), "string_form");
    refused(
        "hash",
        &format!("\"{}\"", HASH.to_uppercase()),
        "string_form",
    );

    passes("site", r#"{"key":"shop.test","source":"address_field"}"#);
    passes("site", r#"{"key":"shop.test","source":"sign_in"}"#);
    passes("site", &format!(r#"{{"key":"{HASH}","source":"title"}}"#));
    refused(
        "site",
        r#"{"key":"shop.test","source":"title"}"#,
        "string_form",
    );
    refused("site", &format!(r#""{HASH}""#), "string_form");
    refused("site", "1", "integer_form");

    for key in ["count", "day"] {
        passes(key, "0");
        passes(key, "4294967295");
        refused(key, "4294967296", "integer_form");
        refused(key, "-1", "integer_form");
        refused(key, "\"1\"", "string_form");
    }

    passes("instant", "0");
    passes("instant", "-9223372036854775808");
    passes("instant", "9223372036854775807");
    refused("instant", "9223372036854775808", "integer_form");
    refused("instant", "-9223372036854775809", "integer_form");
    refused(
        "instant",
        "100000000000000000000000000000000000000000",
        "integer_form",
    );

    passes("ordinal", "0");
    passes("ordinal", "9223372036854775807");
    refused("ordinal", "9223372036854775808", "integer_form");
    refused("ordinal", "-1", "integer_form");

    for v in ["\"shop\"", "\"a.b-c\"", &format!("\"{}\"", "a".repeat(64))] {
        passes("token", v);
    }
    for v in [
        "\"Shop\"",
        "\"\"",
        "\"a b\"",
        &format!("\"{}\"", "a".repeat(65)),
    ] {
        refused("token", v, "string_form");
    }

    for key in ["text", "own_text"] {
        passes(key, "\"\"");
        passes(key, &format!("\"{}\"", "a".repeat(1024)));
        passes(key, &format!("\"{}\"", "\u{e9}".repeat(512)));
        refused(key, &format!("\"{}\"", "a".repeat(1025)), "string_form");
        refused(
            key,
            &format!("\"{}a\"", "\u{e9}".repeat(512)),
            "string_form",
        );
    }

    passes("both", "7");
    passes("both", "\"seven\"");
    refused("both", "\"Seven\"", "string_form");
}

#[test]
fn check_record_never_passes_a_boolean_a_fraction_an_exponent_or_minus_zero() {
    for key in KEYS {
        refused(key, "true", "boolean");
        refused(key, "false", "boolean");
        for v in ["1.0", "1e0", "1E2", "0.5", "-0", "-0.0", "1e-1"] {
            refused(key, v, "not_an_integer");
        }
    }
}

#[test]
fn check_record_walks_objects_and_lists_by_the_types_of_their_key() {
    passes("count", "[]");
    passes("count", "[1,2,3]");
    passes("count", "[[1],[]]");
    passes("count", "{}");
    passes("count", r#"{"a":1,"b":[2,null]}"#);
    refused("count", "[1,\"x\"]", "string_form");
    refused("count", r#"{"a":true}"#, "boolean");
    passes(
        "site",
        &format!(r#"[{{"key":"{HASH}","source":"title"}},null]"#),
    );
}

#[test]
fn check_record_refuses_a_site_in_the_array_form_with_its_tag_first() {
    refused("site", r#"["address_field","shop.test"]"#, "string_form");
    refused("site", &format!(r#"["title","{HASH}"]"#), "string_form");
}

#[test]
fn check_record_refuses_each_example_in_the_array_form_and_every_other_value_that_is_no_object() {
    for kind in KINDS {
        let array = values(kind.example);
        assert!(array.starts_with("[1,"), "{array}");
        assert_eq!(
            check_record(kind, &array),
            Err(format!("{}: the record: not_an_object", kind.name))
        );
        for other in ["[]", "null", "true", "1", "\"x\"", "", "{", "{} {}"] {
            assert_eq!(
                check_record(kind, other),
                Err(format!("{}: the record: not_an_object", kind.name)),
                "{other}"
            );
        }
    }
}

#[test]
fn check_record_takes_the_keys_of_the_fields_each_once_in_any_order_and_no_other() {
    let support = entry("support");
    assert_eq!(
        check_record(
            support,
            r#"{"day":20767,"summary":"20000000000000000000000000000001","workflow":"10000000000000000000000000000001","v":1}"#
        ),
        Ok(())
    );
    assert_eq!(
        check_record(
            support,
            r#"{"v":1,"workflow":"10000000000000000000000000000001","summary":"20000000000000000000000000000001"}"#
        ),
        Err("support: day: missing_key".to_owned())
    );
    assert_eq!(
        check_record(
            support,
            r#"{"v":1,"workflow":"10000000000000000000000000000001","summary":"20000000000000000000000000000001","day":20767,"day":20767}"#
        ),
        Err("support: day: repeated_key".to_owned())
    );
    assert_eq!(
        check_record(
            support,
            r#"{"v":1,"workflow":"10000000000000000000000000000001","summary":"20000000000000000000000000000001","day":20767,"note":"Pay bill"}"#
        ),
        Err("support: a key that the fields do not list: unknown_key".to_owned())
    );
    let workflow = entry("workflow");
    assert_eq!(
        check_record(
            workflow,
            r#"{"v":1,"id":"10000000000000000000000000000003","state":"dismissed","category":"email","site":null,"app":"com.example.mail"}"#
        ),
        Err("workflow: trigger: missing_key".to_owned())
    );
}

#[test]
fn a_refusal_of_check_record_names_the_kind_the_key_and_the_rule_and_never_a_value() {
    let secret_like = "Pay the bill of Jane Example";
    for record in [
        format!(
            r#"{{"v":1,"workflow":"{secret_like}","summary":"20000000000000000000000000000001","day":20767}}"#
        ),
        format!(
            r#"{{"v":1,"workflow":"{W1}","summary":"20000000000000000000000000000001","day":20767,"{secret_like}":1}}"#
        ),
        format!(r#"["{secret_like}"]"#),
    ] {
        let error = check_record(entry("support"), &record).unwrap_err();
        assert!(error.starts_with("support: "), "{error}");
        assert!(!error.contains(secret_like), "{error}");
        assert!(!error.contains("Jane"), "{error}");
    }
}
