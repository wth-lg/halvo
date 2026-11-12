use halvo_schema::kinds::{check_entries, check_record, FieldType, KindEntry, KINDS};
use halvo_schema::TypedRecord;

fn kind(name: &str) -> &'static KindEntry {
    KINDS
        .iter()
        .find(|entry| entry.name == name)
        .unwrap_or_else(|| panic!("the kind {name}"))
}

fn summary_with_keys(keys: &str) -> String {
    let example = kind("summary").example;
    let at = example.find("\"keys\":{").expect("keys in the example");
    let end = at + example[at..].find('}').expect("the end of keys") + 1;
    format!("{}\"keys\":{keys}{}", &example[..at], &example[end..])
}

#[test]
fn check_record_takes_a_record_with_more_than_128_lists_and_objects_side_by_side() {
    let evidence = kind("evidence");
    let example = evidence.example;
    let open = example.find("\"steps\":[").expect("steps") + "\"steps\":[".len();
    let step = &example[open..example.len() - 2];
    assert!(step.starts_with('{') && step.ends_with('}'), "{step}");
    let steps = vec![step; 22].join(",");
    let record = format!("{}{steps}]}}", &example[..open]);
    let levels = record.matches(['[', '{']).count();
    assert!(levels > 128, "{levels}");
    assert_eq!(check_record(evidence, &record), Ok(()));
}

#[test]
fn check_entries_reports_a_pair_that_stands_exactly_twice() {
    let support = kind("support");
    let with = |fields: Vec<(&'static str, FieldType)>| KindEntry {
        name: "later_twice",
        family: "later_test",
        fields: Box::leak(fields.into_boxed_slice()),
        ..*support
    };
    let mut twice = support.fields.to_vec();
    twice.push(("day", FieldType::Day));
    let reports = check_entries(&[&with(twice)]);
    assert!(
        reports.iter().any(|r| r
            == "kind \"later_twice\": the pair of the key day and the type Day stands 2 times"),
        "{reports:?}"
    );
    let once = check_entries(&[&with(support.fields.to_vec())]);
    assert!(!once.iter().any(|r| r.contains("stands")), "{once:?}");
}

#[test]
fn debug_of_a_typed_record_counts_the_bytes_of_its_text() {
    let record = TypedRecord {
        kind: "summary",
        json: "\u{e9}".repeat(4),
    };
    assert_eq!(
        format!("{record:?}"),
        "TypedRecord { kind: \"summary\", json_len: 8 }"
    );
}

#[test]
fn check_record_walks_every_value_of_a_nested_object() {
    let summary = kind("summary");
    assert_eq!(
        check_record(
            summary,
            &summary_with_keys("{\"printable\":42,\"editing\":3}")
        ),
        Ok(())
    );
    assert_eq!(
        check_record(
            summary,
            &summary_with_keys("{\"printable\":42,\"editing\":\"a text\"}")
        ),
        Err("summary: keys: string_form".to_owned())
    );
}

#[test]
fn check_record_reads_a_site_value_whole_only_under_a_key_of_the_type_site() {
    let summary = kind("summary");
    let site = "{\"key\":\"abababababababababababababababababababababababababababababababab\",\"source\":\"title\"}";
    assert!(summary.example.contains(&format!("\"site\":{site}")));
    assert_eq!(
        check_record(summary, &summary_with_keys(site)),
        Err("summary: keys: string_form".to_owned())
    );
}

#[test]
fn check_record_refuses_a_nested_key_of_exactly_32_digits() {
    let summary = kind("summary");
    let digits = "9".repeat(32);
    assert_eq!(
        check_record(
            summary,
            &summary_with_keys(&format!("{{\"printable\":42,\"{digits}\":1}}"))
        ),
        Err("summary: keys: key_form".to_owned())
    );
    let mixed = format!("{}a", "9".repeat(31));
    assert_eq!(
        check_record(
            summary,
            &summary_with_keys(&format!("{{\"printable\":42,\"{mixed}\":1}}"))
        ),
        Ok(())
    );
}
