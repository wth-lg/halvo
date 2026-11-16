mod common;

use std::path::PathBuf;

use common::{episode, item, step_line, HEADER, SEC};
use halvo_sample::labels::{parse_row, Row};
use halvo_sample::manifest::{parse_script_manifest, Counts, StepLine, TierManifest};
use halvo_sample::script::{Script, Sites, Step, TaskKind};
use halvo_sample::text::{site_masked, site_name, template, tokens};

const DAY9: i64 = 32_400 * SEC;

#[test]
fn site_names_tokens_masks_and_templates() {
    assert_eq!(site_name("northgrid-power.test"), "northgrid-power");
    assert_eq!(site_name("example.co.uk"), "example");
    assert_eq!(tokens("Pay Bill-2 é x"), vec!["pay", "bill", "2", "x"]);
    assert_eq!(tokens("  "), Vec::<String>::new());
    assert_eq!(
        site_masked("Your bill - Northgrid Power", Some("northgrid-power.test")),
        "your bill * *"
    );
    assert_eq!(site_masked("Notes - Writer", None), "notes writer");
    assert_eq!(
        template([
            "a".to_owned(),
            "a".to_owned(),
            "b".to_owned(),
            "a".to_owned()
        ]),
        vec!["a", "b", "a"]
    );
}

fn manifest() -> TierManifest {
    TierManifest {
        tier: "gen-dev".to_owned(),
        split: "dev".to_owned(),
        source: "generated".to_owned(),
        seed: 7,
        day1: 1_767_225_600_000_000_000,
        canaries: Vec::new(),
        streams: vec![("gen-dev-s1".to_owned(), Sites::Seen)],
        counts: Counts {
            streams: 1,
            non_seed: 2,
            planted: [1, 0, 0, 0, 0, 0, 0, 0, 0, 3],
            ..Counts::default()
        },
        files: vec![
            ("script-manifest.jsonl".to_owned(), "a".repeat(64)),
            ("streams/gen-dev-s1/labels.jsonl".to_owned(), "b".repeat(64)),
        ],
    }
}

#[test]
fn tier_manifest_lines_and_round_trip() {
    let text = manifest().write();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        &lines[..7],
        &[
            "version = 1",
            "tier = \"gen-dev\"",
            "split = \"dev\"",
            "source = \"generated\"",
            "seed = 7",
            "day1 = 1767225600000000000",
            "canaries = []",
        ]
    );
    assert_eq!(
        &lines[7..10],
        &["[[stream]]", "id = \"gen-dev-s1\"", "sites = \"seen\""]
    );
    assert!(text.contains("[counts.planted]\nutility_bill_payment = 1\n"));
    assert!(text.contains("daily_chart_check = 3\n[counts.one_off]\n"));
    assert!(text.ends_with(&format!(
        "[[file]]\npath = \"streams/gen-dev-s1/labels.jsonl\"\nsha256 = \"{}\"\n",
        "b".repeat(64)
    )));
    assert_eq!(TierManifest::parse(text.as_bytes()), Some(manifest()));
}

#[test]
fn tier_manifest_refusals() {
    let text = manifest().write();
    for bad in [
        text.replace("seed = 7", "seed  = 7"),
        text.replace("split = \"dev\"", "split = \"heldout\""),
        text.replace("tier = \"gen-dev\"", "tier = \"gen-other\""),
        text.replace("id = \"gen-dev-s1\"", "id = \"s1\""),
        text.replace("streams = 1", "streams = 2"),
        text.replace(&"a".repeat(64), &"A".repeat(64)),
        text.replace("version = 1\n", ""),
        format!("{text}# comment\n"),
        text.trim_end().to_owned(),
    ] {
        assert_eq!(TierManifest::parse(bad.as_bytes()), None, "{bad}");
    }
}

#[test]
fn label_rows_parse_only_their_compact_form() {
    let e = r#"{"row":"episode","episode":"e1","task":"t1","category":"email","kind":"planted","start":1,"end":2}"#;
    let r = r#"{"row":"range","episode":"e1","step":"p1","start":1,"end":2}"#;
    assert!(matches!(
        parse_row(e.as_bytes()),
        Some(Row::Episode {
            kind: TaskKind::Planted,
            ..
        })
    ));
    assert!(matches!(parse_row(r.as_bytes()), Some(Row::Range { .. })));
    for bad in [
        r#"{"row":"range","episode":"e1","step":"p1","start":1,"end":2,"x":1}"#,
        r#"{"row":"range", "episode":"e1","step":"p1","start":1,"end":2}"#,
        r#"{"row":"range","step":"p1","episode":"e1","start":1,"end":2}"#,
        r#"{"row":"range","episode":"E1","step":"p1","start":1,"end":2}"#,
        r#"{"row":"episode","episode":"e1","task":"t1","category":"travel","kind":"planted","start":1,"end":2}"#,
        r#"{"row":"episode","episode":"e1","task":"t1","category":"email","kind":"often","start":1,"end":2}"#,
        r#"{"row":"range","episode":"e1","step":"p1","start":1.0,"end":2}"#,
    ] {
        assert_eq!(parse_row(bad.as_bytes()), None, "{bad}");
    }
}

#[test]
fn script_manifest_masks_and_order() {
    let card = Step {
        step: "p2".to_owned(),
        app: "org.chromium.Chromium".to_owned(),
        site: Some("acorn-shop.test".to_owned()),
        titles: vec!["Pay 4242 4242 4242 4242 - Acorn".to_owned()],
        paths: vec!["/pay".to_owned()],
        password_input: false,
        card_title: true,
    };
    let line = StepLine::of(&card, true);
    assert!(line
        .write()
        .ends_with(",\"card_title\":true,\"seed_category\":true}"));
    assert_eq!(line.masked_titles, vec!["Pay [card] - Acorn".to_owned()]);
    let plain = StepLine::of(
        &Step {
            step: "p1".to_owned(),
            card_title: false,
            titles: vec!["Browse - Acorn".to_owned()],
            ..card.clone()
        },
        false,
    );
    assert!(plain
        .write()
        .ends_with(",\"card_title\":false,\"seed_category\":false}"));
    let text = format!("{}\n{}\n", plain.write(), line.write());
    assert_eq!(
        parse_script_manifest(text.as_bytes()).map(|v| v.len()),
        Some(2)
    );
    let reversed = format!("{}\n{}\n", line.write(), plain.write());
    assert_eq!(parse_script_manifest(reversed.as_bytes()), None);
    let wrong_mask = text.replace("Pay [card] - Acorn", "Pay - Acorn");
    assert_eq!(parse_script_manifest(wrong_mask.as_bytes()), None);
    assert!(line.produces(
        Some("org.chromium.Chromium"),
        "Pay [card] - Acorn",
        None,
        true
    ));
    assert!(!line.produces(
        Some("org.chromium.Chromium"),
        "Pay [card] - Acorn",
        None,
        false
    ));
    assert!(!line.produces(
        Some("org.chromium.Chromium"),
        "Pay 4242 4242 4242 4242 - Acorn",
        Some("other-shop.test"),
        false
    ));
}

/// A valid script of one stream: a planted routine on days 1 to 3 (sign-in, then a page), a one-off native
/// task on day 4, and a marked task in a file of its own on day 5.
fn base() -> (Vec<String>, Vec<String>) {
    let mut main = vec![
        HEADER.to_owned(),
        r#"{"line":"stream","stream":"s1","sites":"seen"}"#.to_owned(),
        step_line("pa", "alpha-one.test", "Sign in - Alpha", true),
        step_line("pb", "alpha-one.test", "Bills - Alpha", false),
        r#"{"line":"step","step":"pc","app":"org.libreoffice.script","site":null,"titles":["book.ods - Calc"],"paths":[],"password_input":false,"card_title":false}"#.to_owned(),
    ];
    for (k, day) in [1u32, 2, 3].iter().enumerate() {
        main.push(episode(
            "s1",
            &format!("e{k}"),
            "t1",
            "utility_bill_payment",
            "planted",
            *day,
            &[
                item("pa", DAY9, DAY9 + 10 * SEC, 0, 1, 0),
                item("pb", DAY9 + 10 * SEC, DAY9 + 40 * SEC, 3, 2, 1),
            ],
        ));
    }
    main.push(episode(
        "s1",
        "e9",
        "t2",
        "spreadsheet_fill",
        "one_off",
        4,
        &[item("pc", DAY9, DAY9 + 60 * SEC, 5, 1, 1)],
    ));
    let marked = vec![
        HEADER.to_owned(),
        r#"{"line":"stream","stream":"s1","sites":"seen"}"#.to_owned(),
        step_line("pm", "alpha-two.test", "Week - Two", false),
        format!(
            r#"{{"line":"mark","stream":"s1","day":5,"edge":"start","at":{}}}"#,
            DAY9 - SEC
        ),
        format!(
            r#"{{"line":"mark","stream":"s1","day":5,"edge":"end","at":{}}}"#,
            DAY9 + 500 * SEC
        ),
    ];
    let mut marked = marked;
    for k in 0..5 {
        let s = DAY9 + k * 60 * SEC;
        marked.push(episode(
            "s1",
            &format!("m{k}"),
            "t3",
            "calendar_cleanup",
            "marked",
            5,
            &[item("pm", s, s + 30 * SEC, 0, 1, 0)],
        ));
    }
    (main, marked)
}

/// An edit of the two files of the base script.
type Edit = Box<dyn FnOnce(&mut Vec<String>, &mut Vec<String>)>;

fn read(files: &[Vec<String>]) -> Result<Script, &'static str> {
    let paths: Vec<PathBuf> = (0..files.len())
        .map(|i| PathBuf::from(format!("f{i}.jsonl")))
        .collect();
    let texts: Vec<Vec<u8>> = files
        .iter()
        .map(|lines| {
            lines
                .iter()
                .map(|l| format!("{l}\n"))
                .collect::<String>()
                .into_bytes()
        })
        .collect();
    Script::from_bytes(&paths, &texts).map_err(|b| b.rule)
}

fn with(edit: impl FnOnce(&mut Vec<String>, &mut Vec<String>)) -> Result<Script, &'static str> {
    let (mut a, mut b) = base();
    edit(&mut a, &mut b);
    read(&[a, b])
}

#[test]
fn the_base_script_keeps_every_rule() {
    let s = with(|_, _| {}).expect("valid");
    assert_eq!(s.episodes.len(), 9);
    assert_eq!(s.steps.len(), 4);
}

#[test]
fn each_rule_of_the_script_is_refused() {
    let cases: Vec<(&str, Edit)> = vec![
        (
            "S1",
            Box::new(|_, b| b[0] = HEADER.replace("1767225600000000000", "1767312000000000000")),
        ),
        ("S1", Box::new(|a, _| a.swap(0, 1))),
        (
            "S1",
            Box::new(|_, b| b[2] = step_line("pa", "alpha-one.test", "Other", true)),
        ),
        (
            "S2",
            Box::new(|a, _| a[5] = a[5].replace("\"e0\"", "\"E0\"")),
        ),
        (
            "S2",
            Box::new(|a, _| a[6] = a[6].replace("\"e1\"", "\"e0\"")),
        ),
        (
            "S2",
            Box::new(|a, _| a[1] = a[1].replace("\"s1\"", &format!("\"{}\"", "s".repeat(37)))),
        ),
        (
            "S3",
            Box::new(|a, _| a[5] = a[5].replace("\"step\":\"pa\"", "\"step\":\"zz\"")),
        ),
        (
            "S3",
            Box::new(|a, _| a[5] = a[5].replace("\"title\":0", "\"title\":1")),
        ),
        (
            "S4",
            Box::new(|a, _| a[8] = a[8].replace("\"day\":4", "\"day\":94")),
        ),
        (
            "S4",
            Box::new(|a, _| {
                a.push(step_line("pn", "beta-news.test", "News - Beta", false));
                a.push(format!(
                    r#"{{"line":"noise","stream":"s1","day":1,"item":{}}}"#,
                    item("pn", DAY9 + 5 * SEC, DAY9 + 20 * SEC, 0, 0, 0)
                ));
            }),
        ),
        (
            "S5",
            Box::new(|a, _| {
                a.remove(7);
            }),
        ),
        (
            "S5",
            Box::new(|a, _| a[8] = a[8].replace("spreadsheet_fill", "other")),
        ),
        (
            "S5",
            Box::new(|_, b| {
                b.remove(4);
            }),
        ),
        (
            "S6",
            Box::new(|a, _| {
                a.push(format!(
                    r#"{{"line":"noise","stream":"s1","day":6,"item":{}}}"#,
                    item("pb", DAY9, DAY9 + 5 * SEC, 0, 0, 0)
                ))
            }),
        ),
        (
            "S7",
            Box::new(|a, _| a[5] = a[5].replacen("\"keys\":0", "\"keys\":1", 1)),
        ),
        (
            "S8",
            Box::new(|a, _| a[2] = step_line("pa", "alpha-one.test", &"x".repeat(1025), true)),
        ),
        (
            "S9",
            Box::new(|a, b| {
                a[0] = HEADER.replace("[]", "[\"alpha-one.test\"]");
                b[0] = a[0].clone();
            }),
        ),
        (
            "S10",
            Box::new(|a, _| a.push(step_line("px", "alpha-one.test", "Unused - Alpha", false))),
        ),
        (
            "S11",
            Box::new(|a, b| {
                let l = a.remove(8);
                b.push(l);
            }),
        ),
        (
            "S12",
            Box::new(|a, _| a[3] = a[3].replace("\"paths\":[\"/p\"]", "\"paths\":[]")),
        ),
        (
            "S13",
            Box::new(|a, _| a[5] = a[5].replacen("\"clicks\":1", "\"clicks\":10", 1)),
        ),
        (
            "S14",
            Box::new(|a, _| {
                a[5] = a[5].replacen(
                    &format!("\"start\":{DAY9}"),
                    &format!("\"start\":{}", DAY9 + 1),
                    1,
                )
            }),
        ),
        (
            "S14",
            Box::new(|a, b| {
                a[0] = HEADER.replace("1767225600000000000", "1767225600000000001");
                b[0] = a[0].clone();
            }),
        ),
        (
            "S15",
            Box::new(|a, _| a[1] = a[1].replace("{\"line\":\"stream\",", "{\"line\":\"stream\", ")),
        ),
        (
            "S15",
            Box::new(|a, _| {
                a[3] = a[3].replace("\"Bills - Alpha\"]", "\"Bills - Alpha\",\"Bills - Alpha\"]")
            }),
        ),
        (
            "S15",
            Box::new(|a, _| a[3] = a[3].replace("\"card_title\":false", "\"card_title\":true")),
        ),
    ];
    for (rule, edit) in cases {
        assert_eq!(with(edit).err(), Some(rule));
    }
}

#[test]
fn the_twice_rule_reads_sites_and_native_apps() {
    let twice = |a: &mut Vec<String>, category: &str, step: &str| {
        a.push(episode(
            "s1",
            "w1",
            "t8",
            category,
            "twice",
            10,
            &[item(step, DAY9, DAY9 + 30 * SEC, 0, 0, 0)],
        ));
        a.push(episode(
            "s1",
            "w2",
            "t8",
            category,
            "twice",
            11,
            &[item(step, DAY9, DAY9 + 30 * SEC, 0, 0, 0)],
        ));
    };
    assert!(with(|a, _| twice(a, "utility_bill_payment", "pb")).is_err());
    assert!(with(|a, _| twice(a, "email", "pb")).is_ok());
    assert_eq!(
        with(|a, _| twice(a, "spreadsheet_fill", "pc")).err(),
        Some("S5")
    );
}

#[test]
fn a_range_of_another_task_between_the_marks_is_refused() {
    let other = |a: &mut Vec<String>| {
        a.push(episode(
            "s1",
            "x1",
            "t9",
            "email",
            "one_off",
            5,
            &[item("pb", DAY9 + 100 * SEC, DAY9 + 110 * SEC, 0, 0, 0)],
        ))
    };
    assert_eq!(with(|a, _| other(a)).err(), Some("S5"));
    let after = |a: &mut Vec<String>| {
        a.push(episode(
            "s1",
            "x1",
            "t9",
            "email",
            "one_off",
            5,
            &[item("pb", DAY9 + 600 * SEC, DAY9 + 610 * SEC, 0, 0, 0)],
        ))
    };
    assert!(with(|a, _| after(a)).is_ok());
}
