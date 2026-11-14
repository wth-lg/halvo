use std::path::Path;

use halvo_policy::check_playbook;
use halvo_schema::executable::names::Source;
use halvo_schema::executable::step::Target;
use halvo_schema::executable::Playbook;
use halvo_schema::SeedCategory;

fn file(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("playbooks")
            .join(name),
    )
    .expect("read the playbook file")
}

fn ops(playbook: &Playbook) -> Vec<String> {
    playbook
        .steps()
        .iter()
        .map(|s| match &s.target {
            Some(Target::Api(t)) => t.op.as_str().to_string(),
            _ => s.verb.as_str().to_string(),
        })
        .collect()
}

#[test]
fn the_three_built_in_playbooks_parse_with_the_steps_of_a11_4_and_their_classes() {
    for (name, category, want) in [
        (
            "fill.jsonl",
            SeedCategory::SpreadsheetFill,
            vec!["read", "write"],
        ),
        (
            "reply.jsonl",
            SeedCategory::Email,
            vec!["mail_list", "mail_draft_reply", "mail_send"],
        ),
        (
            "cleanup.jsonl",
            SeedCategory::CalendarCleanup,
            vec!["calendar_list", "calendar_delete"],
        ),
    ] {
        let playbook = Playbook::parse_file(&file(name)).expect(name);
        assert_eq!(ops(&playbook), want, "{name}");
        assert_eq!(check_playbook(category, playbook.steps()), Ok(()), "{name}");
        assert_eq!(
            playbook.to_file().expect("write").as_bytes(),
            file(name),
            "{name} is in the one form of the schema's writer"
        );
    }
}

#[test]
fn fill_reads_the_slot_source_as_rows_and_writes_rows() {
    let playbook = Playbook::parse_file(&file("fill.jsonl")).expect("fill");
    let [read, write] = playbook.steps() else {
        panic!("two steps")
    };
    let Some(Target::File(t)) = &read.target else {
        panic!("a file target")
    };
    assert_eq!(t.file.as_str(), "source");
    assert_eq!(
        read.args[0].value.as_ref().map(|v| v.as_str()),
        Some("rows")
    );
    let Some(Target::File(t)) = &write.target else {
        panic!("a file target")
    };
    assert_eq!(
        (
            t.file.as_str(),
            t.sheet.as_ref().map(|s| s.as_str()),
            t.range.as_ref().map(|s| s.as_str())
        ),
        ("workbook", Some("sheet"), Some("range"))
    );
    assert_eq!(write.args[0].from, Source::Extract);
    assert_eq!(write.args[0].key.as_ref().map(|k| k.as_str()), Some("rows"));
}

#[test]
fn the_reply_fallback_is_one_line_of_text() {
    let bytes = file("reply-fallback.txt");
    let text = std::str::from_utf8(&bytes).expect("UTF-8");
    assert!(!text.starts_with('\u{feff}') && !text.contains('\r'));
    assert_eq!(text.matches('\n').count(), 1);
    assert!(text.ends_with('\n') && text.len() > 1);
}
