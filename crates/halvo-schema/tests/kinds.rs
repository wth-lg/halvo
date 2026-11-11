mod common;

use common::{values, with};
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
    assert_eq!(
        check_entries(&[&KindEntry {
            fields: &[
                ("v", FieldType::Version),
                ("workflow", FieldType::Id),
                ("day", FieldType::Count),
                ("count", FieldType::Count),
            ],
            ..LATER_BOUNDED
        }]),
        [
            "kind \"probe_bounded\": a bounded kind without the key day of the type Day",
            "kind \"probe_bounded\": a bounded kind whose key day has a pair of the type Count",
        ]
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
    assert_eq!(
        check_entries(&[&KindEntry {
            workflow_key: Some("count"),
            ..LATER_BOUNDED
        }]),
        [
            "kind \"probe_bounded\": the workflow_key count is no key of the fields with the type Id",
            "kind \"probe_bounded\": the workflow_key count has a pair of the type Count",
            "kind \"probe_bounded\": the example is refused: probe_bounded: count: integer_form",
        ]
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
    let own_window = KindEntry {
        origin: Origin::Own,
        workflow_key: Some("workflow"),
        fields: &[
            ("v", FieldType::Version),
            ("workflow", FieldType::Id),
            ("segments", FieldType::Ordinal),
            ("expires", FieldType::Instant),
            ("note", FieldType::Text),
            ("hint", FieldType::Token),
            ("own", FieldType::OwnText),
        ],
        example: r#"{"v":1,"workflow":"10000000000000000000000000000001","segments":[7],"expires":1795478400000000000,"note":"Inbox","hint":"shop","own":"Pay"}"#,
        ..LATER_WINDOW
    };
    assert_eq!(
        check_entries(&[&own_window]),
        [
            "kind \"probe_window\": a kind of origin own with the class window",
            "kind \"probe_window\": a kind of origin own with the key segments of the type Ordinal",
            "kind \"probe_window\": a kind of origin own with the key note of the type Text",
            "kind \"probe_window\": a kind of origin own with the key hint of the type Token",
            "kind \"probe_window\": a window kind with the key own of the type OwnText",
        ]
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

const W2: &str = "10000000000000000000000000000002";

fn rule_key(kind: &KindEntry, key: &str, cases: &[(&str, &str)]) {
    assert_eq!(check_record(kind, kind.example), Ok(()), "{}", kind.name);
    for (value, rule) in cases {
        let record = with(kind.example, key, value);
        assert_eq!(
            check_record(kind, &record),
            Err(format!("{}: {key}: {rule}", kind.name)),
            "{record}"
        );
    }
}

#[test]
fn check_record_holds_the_day_of_a_bounded_kind_to_one_day() {
    let cases = [
        ("null", "not_one_value"),
        ("[20767]", "not_one_value"),
        ("[1,4294967295]", "not_one_value"),
        ("[]", "not_one_value"),
        (r#"{"x":null}"#, "not_one_value"),
        (r#"{"day":20767}"#, "not_one_value"),
        (r#""com.example.app""#, "string_form"),
    ];
    for kind in [entry("summary"), entry("support"), &LATER_BOUNDED] {
        rule_key(kind, "day", &cases);
    }
    let second_type = KindEntry {
        fields: &[
            ("v", FieldType::Version),
            ("workflow", FieldType::Id),
            ("day", FieldType::Day),
            ("day", FieldType::App),
            ("count", FieldType::Count),
        ],
        ..LATER_BOUNDED
    };
    rule_key(&second_type, "day", &cases);
}

#[test]
fn check_record_holds_expires_of_a_window_kind_to_one_instant() {
    let cases = [
        ("null", "not_one_value"),
        ("[1795478400000000000]", "not_one_value"),
        ("[]", "not_one_value"),
        (r#"{"at":1795478400000000000}"#, "not_one_value"),
        (r#""never""#, "string_form"),
    ];
    for kind in [entry("evidence"), &LATER_WINDOW] {
        rule_key(kind, "expires", &cases);
    }
    let second_type = KindEntry {
        fields: &[
            ("v", FieldType::Version),
            ("segments", FieldType::Ordinal),
            ("expires", FieldType::Instant),
            ("expires", FieldType::Name),
            ("hint", FieldType::Token),
            ("title", FieldType::Text),
        ],
        ..LATER_WINDOW
    };
    rule_key(&second_type, "expires", &cases);
}

#[test]
fn check_record_holds_segments_of_a_window_kind_to_a_list_of_one_or_more_ordinals() {
    let cases = [
        ("null", "not_a_list"),
        ("[]", "empty_list"),
        ("7", "not_a_list"),
        (r#"{"a":7}"#, "not_a_list"),
        (r#""7""#, "not_a_list"),
        ("[null]", "not_one_value"),
        ("[7,null]", "not_one_value"),
        ("[[7]]", "not_one_value"),
        (r#"[7,{"a":8}]"#, "not_one_value"),
        (r#"[7,"x"]"#, "string_form"),
        ("[-1]", "integer_form"),
    ];
    for kind in [entry("evidence"), &LATER_WINDOW] {
        rule_key(kind, "segments", &cases);
        assert_eq!(
            check_record(kind, &with(kind.example, "segments", "[0]")),
            Ok(())
        );
    }
    let second_type = KindEntry {
        fields: &[
            ("v", FieldType::Version),
            ("segments", FieldType::Ordinal),
            ("segments", FieldType::Name),
            ("expires", FieldType::Instant),
            ("hint", FieldType::Token),
            ("title", FieldType::Text),
        ],
        ..LATER_WINDOW
    };
    rule_key(&second_type, "segments", &[(r#"["never"]"#, "string_form")]);
}

#[test]
fn check_record_holds_the_workflow_key_to_one_id() {
    let one = format!(r#"["{W1}"]"#);
    let two = format!(r#"["{W1}","{W2}"]"#);
    let object = format!(r#"{{"id":"{W1}"}}"#);
    let cases = [
        ("null", "not_one_value"),
        (one.as_str(), "not_one_value"),
        (two.as_str(), "not_one_value"),
        ("[]", "not_one_value"),
        (object.as_str(), "not_one_value"),
        (r#""never_deleted""#, "string_form"),
    ];
    rule_key(entry("workflow"), "id", &cases);
    rule_key(entry("support"), "workflow", &cases);
    rule_key(&LATER_BOUNDED, "workflow", &cases);
    rule_key(&LATER_OWN, "workflow", &cases);
    let second_type = KindEntry {
        fields: &[
            ("v", FieldType::Version),
            ("workflow", FieldType::Id),
            ("workflow", FieldType::Name),
            ("at", FieldType::Instant),
            ("note", FieldType::OwnText),
            ("site", FieldType::Site),
        ],
        ..LATER_OWN
    };
    rule_key(&second_type, "workflow", &cases);
}

#[test]
fn check_record_holds_v_to_one_value_of_its_type() {
    let cases = [
        ("null", "not_one_value"),
        ("[1]", "not_one_value"),
        ("[]", "not_one_value"),
        (r#"{"v":1}"#, "not_one_value"),
        ("2", "integer_form"),
    ];
    for kind in KINDS
        .iter()
        .chain([&LATER_BOUNDED, &LATER_OWN, &LATER_WINDOW])
    {
        rule_key(kind, "v", &cases);
    }
}

fn in_nested_objects(key: &str) -> [(&'static KindEntry, &'static str, String); 4] {
    let k = serde_json::to_string(key).unwrap();
    [
        (
            entry("workflow"),
            "recurrence",
            format!(r#"{{"count":3,"period":31,"first":20705,{k}:20767}}"#),
        ),
        (
            entry("summary"),
            "keys",
            format!(r#"{{"printable":42,"editing":3,{k}:0}}"#),
        ),
        (
            entry("evidence"),
            "steps",
            format!(
                r#"[{{"app":{{"value":"com.example.browser","layer":"floor"}},{k}:{{"value":"Pay bill","layer":"floor"}}}}]"#
            ),
        ),
        (&LATER_BOUNDED, "count", format!(r#"[{{"n":1,{k}:2}}]"#)),
    ]
}

#[test]
fn check_record_refuses_a_key_of_a_nested_object_that_is_no_name() {
    let long = "a".repeat(2044);
    for key in [
        long.as_str(),
        "a23456789012345678901234567890123",
        "Printable",
        "printAble",
        "key-x",
        "2026-11-11T17:20:00Z",
        "-1794339000000000000",
        "",
        "a b",
        "\u{e9}",
        "a\u{0}",
    ] {
        for (kind, at, value) in in_nested_objects(key) {
            rule_key(kind, at, &[(value.as_str(), "key_form")]);
        }
    }
    for key in ["a2345678901234567890123456789012", "a", "wall_s", "0"] {
        for (kind, at, value) in in_nested_objects(key) {
            assert_eq!(
                check_record(kind, &with(kind.example, at, &value)),
                Ok(()),
                "{value}"
            );
        }
    }
}

#[test]
fn check_record_refuses_a_key_that_stands_twice_in_a_nested_object() {
    rule_key(
        entry("workflow"),
        "recurrence",
        &[(
            r#"{"count":3,"period":31,"first":20705,"last":20767,"last":20767}"#,
            "repeated_key",
        )],
    );
    rule_key(
        entry("summary"),
        "keys",
        &[(
            r#"{"printable":42,"editing":3,"printable":0}"#,
            "repeated_key",
        )],
    );
    rule_key(
        entry("evidence"),
        "steps",
        &[(
            r#"[{"app":{"value":"com.example.browser","layer":"floor","layer":"floor"}}]"#,
            "repeated_key",
        )],
    );
    rule_key(
        &LATER_BOUNDED,
        "count",
        &[(r#"[{"n":1,"n":2}]"#, "repeated_key")],
    );
    rule_key(
        entry("workflow"),
        "site",
        &[(
            &format!(r#"{{"key":"{HASH}","key":"{HASH}","source":"title"}}"#),
            "repeated_key",
        )],
    );
}

#[test]
fn check_record_reads_a_site_value_whole_and_holds_another_object_under_site_to_the_rule_of_keys() {
    let workflow = entry("workflow");
    for site in [
        format!(r#"{{"key":"{HASH}","source":"title"}}"#),
        r#"{"key":"shop.test","source":"address_field"}"#.to_owned(),
        r#"{"key":"shop.test","source":"sign_in"}"#.to_owned(),
    ] {
        assert_eq!(
            check_record(workflow, &with(workflow.example, "site", &site)),
            Ok(()),
            "{site}"
        );
    }
    rule_key(
        workflow,
        "site",
        &[
            (
                &format!(r#"{{"Key":"{HASH}","source":"title"}}"#),
                "key_form",
            ),
            (&format!(r#"{{"key-x":"{HASH}"}}"#), "key_form"),
        ],
    );
}

#[test]
fn check_entries_reports_a_rule_key_with_a_pair_of_another_type() {
    assert_eq!(
        check_entries(&[&KindEntry {
            fields: &[
                ("v", FieldType::Version),
                ("workflow", FieldType::Id),
                ("day", FieldType::Day),
                ("day", FieldType::App),
                ("count", FieldType::Count),
            ],
            ..LATER_BOUNDED
        }]),
        ["kind \"probe_bounded\": a bounded kind whose key day has a pair of the type App"]
    );
    assert_eq!(
        check_entries(&[&KindEntry {
            fields: &[
                ("v", FieldType::Version),
                ("segments", FieldType::Ordinal),
                ("expires", FieldType::Instant),
                ("expires", FieldType::Name),
                ("hint", FieldType::Token),
                ("title", FieldType::Text),
            ],
            ..LATER_WINDOW
        }]),
        ["kind \"probe_window\": a window kind whose key expires has a pair of the type Name"]
    );
    assert_eq!(
        check_entries(&[&KindEntry {
            fields: &[
                ("v", FieldType::Version),
                ("segments", FieldType::Ordinal),
                ("segments", FieldType::Name),
                ("expires", FieldType::Instant),
                ("hint", FieldType::Token),
                ("title", FieldType::Text),
            ],
            ..LATER_WINDOW
        }]),
        ["kind \"probe_window\": a window kind whose key segments has a pair of the type Name"]
    );
    assert_eq!(
        check_entries(&[&KindEntry {
            fields: &[
                ("v", FieldType::Version),
                ("workflow", FieldType::Id),
                ("workflow", FieldType::Name),
                ("at", FieldType::Instant),
                ("note", FieldType::OwnText),
                ("site", FieldType::Site),
            ],
            ..LATER_OWN
        }]),
        ["kind \"probe_own\": the workflow_key workflow has a pair of the type Name"]
    );
    let summary_keyed = KindEntry {
        workflow_key: Some("id"),
        family: "probe",
        ..*entry("summary")
    };
    assert_eq!(check_entries(&[&summary_keyed]), Vec::<String>::new());
}

#[test]
fn check_entries_reports_a_pair_that_stands_twice_and_the_pairs_of_one_key_apart() {
    assert_eq!(
        check_entries(&[&KindEntry {
            fields: &[
                ("v", FieldType::Version),
                ("workflow", FieldType::Id),
                ("day", FieldType::Day),
                ("count", FieldType::Count),
                ("count", FieldType::Count),
                ("count", FieldType::Count),
            ],
            ..LATER_BOUNDED
        }]),
        ["kind \"probe_bounded\": the pair of the key count and the type Count stands 3 times"]
    );
    assert_eq!(
        check_entries(&[&KindEntry {
            fields: &[
                ("v", FieldType::Version),
                ("workflow", FieldType::Id),
                ("count", FieldType::Count),
                ("day", FieldType::Day),
                ("count", FieldType::Name),
            ],
            example: r#"{"v":1,"workflow":"10000000000000000000000000000001","count":2,"day":20767}"#,
            ..LATER_BOUNDED
        }]),
        ["kind \"probe_bounded\": the pairs of the key count do not stand together"]
    );
    assert_eq!(
        check_entries(&[&KindEntry {
            fields: &[
                ("v", FieldType::Version),
                ("workflow", FieldType::Id),
                ("count", FieldType::Count),
                ("count", FieldType::Name),
                ("day", FieldType::Day),
            ],
            example: r#"{"v":1,"workflow":"10000000000000000000000000000001","count":2,"day":20767}"#,
            ..LATER_BOUNDED
        }]),
        Vec::<String>::new()
    );
}

#[test]
fn check_entries_reports_a_bounded_kind_of_origin_capture_above_93_days() {
    for days in [94, 365, 65535] {
        assert_eq!(
            check_entries(&[&KindEntry {
                class: RetentionClass::Bounded { days },
                ..LATER_BOUNDED
            }]),
            ["kind \"probe_bounded\": a bounded kind of origin capture with more than 93 days"],
            "{days}"
        );
    }
    for days in [1, 93] {
        assert_eq!(
            check_entries(&[&KindEntry {
                class: RetentionClass::Bounded { days },
                ..LATER_BOUNDED
            }]),
            Vec::<String>::new(),
            "{days}"
        );
    }
    let own = |days| KindEntry {
        class: RetentionClass::Bounded { days },
        fields: &[
            ("v", FieldType::Version),
            ("workflow", FieldType::Id),
            ("day", FieldType::Day),
            ("at", FieldType::Instant),
            ("note", FieldType::OwnText),
        ],
        example: r#"{"v":1,"workflow":"10000000000000000000000000000001","day":20767,"at":1794339000000000000,"note":"Pay on the first"}"#,
        ..LATER_OWN
    };
    for days in [1, 35, 93, 94, 65535] {
        assert_eq!(check_entries(&[&own(days)]), Vec::<String>::new(), "{days}");
    }
    assert_eq!(
        check_entries(&[&own(0)]),
        ["kind \"probe_own\": a bounded kind with 0 days"]
    );
}

#[test]
fn check_record_refuses_minus_zero_under_every_rule_key_and_under_the_other_keys() {
    let minus_zero = [("-0", "not_an_integer")];
    rule_key(entry("summary"), "day", &minus_zero);
    rule_key(entry("support"), "day", &minus_zero);
    rule_key(&LATER_BOUNDED, "day", &minus_zero);
    rule_key(entry("evidence"), "expires", &minus_zero);
    rule_key(&LATER_WINDOW, "expires", &minus_zero);
    for segments in ["[-0]", "[4410,-0]"] {
        rule_key(
            entry("evidence"),
            "segments",
            &[(segments, "not_an_integer")],
        );
    }
    rule_key(entry("support"), "workflow", &minus_zero);
    for kind in KINDS
        .iter()
        .chain([&LATER_BOUNDED, &LATER_OWN, &LATER_WINDOW])
    {
        rule_key(kind, "v", &minus_zero);
    }
    rule_key(entry("summary"), "minutes", &minus_zero);
    rule_key(entry("evidence"), "start", &minus_zero);
    rule_key(&LATER_OWN, "at", &minus_zero);
    for value in ["-0", "[-0]", r#"{"n":-0}"#] {
        rule_key(&LATER_BOUNDED, "count", &[(value, "not_an_integer")]);
    }
}

#[test]
fn check_record_takes_null_a_list_or_an_object_under_a_key_that_is_no_rule_key() {
    let takes = |kind: &KindEntry, key: &str, value: &str| {
        let record = with(kind.example, key, value);
        assert_eq!(check_record(kind, &record), Ok(()), "{record}");
    };
    for value in [
        "null",
        "[]",
        "[1,2]",
        "[[3],null]",
        r#"{"a":1,"b":[2,null]}"#,
    ] {
        takes(&LATER_BOUNDED, "count", value);
        takes(entry("summary"), "minutes", value);
    }
    takes(entry("workflow"), "recurrence", "null");
    takes(entry("workflow"), "recurrence", "[3,31,20705,20767]");
    takes(entry("workflow"), "agent", "null");
    takes(entry("summary"), "keys", "null");
    takes(entry("summary"), "site", "null");
    takes(entry("summary"), "site", "[]");
    takes(entry("evidence"), "start", "null");
    takes(entry("evidence"), "steps", "[]");
    takes(&LATER_OWN, "at", "[1,2]");
    takes(&LATER_OWN, "note", r#"["a","b"]"#);
    takes(&LATER_OWN, "note", r#"{"a":"x","b":{"c":"y"}}"#);
    let summary = entry("summary");
    let record =
        summary
            .example
            .replacen(r#""v":1,"#, r#""v":1,"Pay the bill of Jane Example":1,"#, 1);
    assert_eq!(
        check_record(summary, &record),
        Err("summary: a key that the fields do not list: unknown_key".to_owned())
    );
}

#[test]
fn check_record_reads_an_app_id_with_a_control_character_of_the_category_cc_as_no_app_id() {
    for c in ['\u{85}', '\u{9b}'] {
        let app = serde_json::to_string(&format!("com.example{c}app")).unwrap();
        refused("app", &app, "string_form");
    }
    passes("app", "\"com.example\u{a0}app\"");
}

#[test]
fn check_record_refuses_a_record_that_nests_deeper_than_128_levels_and_answers_at_any_depth() {
    let bounded =
        |count: &str| format!(r#"{{"v":1,"workflow":"{W1}","day":20767,"count":{count}}}"#);
    let lists = |levels: usize| format!("{}2{}", "[".repeat(levels), "]".repeat(levels));
    let objects = |levels: usize| format!("{}2{}", r#"{"a":"#.repeat(levels), "}".repeat(levels));
    assert_eq!(check_record(&LATER_BOUNDED, &bounded(&lists(127))), Ok(()));
    assert_eq!(
        check_record(&LATER_BOUNDED, &bounded(&objects(127))),
        Ok(())
    );
    for levels in [128, 5_000, 200_000] {
        for value in [lists(levels), objects(levels)] {
            assert_eq!(
                check_record(&LATER_BOUNDED, &bounded(&value)),
                Err("probe_bounded: the record: too_deep".to_owned()),
                "{levels}"
            );
        }
    }
    let note = format!(r#""a\"{}{}""#, "[".repeat(300), "{".repeat(300));
    let own = LATER_OWN
        .example
        .replacen(r#""Pay on the first of the month""#, &note, 1);
    assert_eq!(check_record(&LATER_OWN, &own), Ok(()), "{own}");
}
