mod common;

use std::fs;
use std::path::Path;

use common::{build_golden, copy_dir, golden, refused, run, s, scratch, write};
use halvo_sample::manifest::{Counts, TierManifest};
use halvo_sample::script::Sites;

fn copy_golden(name: &str) -> std::path::PathBuf {
    let dir = scratch(name).join("g");
    copy_dir(&golden(), &dir);
    dir
}

fn labels(dir: &Path) -> std::path::PathBuf {
    dir.join("streams/golden/labels.jsonl")
}

fn edit_labels(dir: &Path, from: &str, to: &str) {
    let p = labels(dir);
    let text = fs::read_to_string(&p).expect("labels");
    assert_eq!(text.matches(from).count(), 1, "{from}");
    fs::write(&p, text.replacen(from, to, 1)).expect("write");
}

#[test]
fn golden_passes_under_both_rules() {
    for rule in [None, Some("generated"), Some("recorded")] {
        let mut args = vec!["check-labels".to_owned()];
        if let Some(r) = rule {
            args.extend(["--rule".to_owned(), r.to_owned()]);
        }
        args.push(s(&golden()));
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let o = run(&a);
        assert_eq!(o.code, 0, "{}{}", o.stdout, o.stderr);
        let lines: Vec<&str> = o.stdout.lines().collect();
        assert_eq!(lines[0], format!("rule {}", rule.unwrap_or("generated")));
        assert_eq!(
            &lines[1..],
            &[
                "marked ranges=10 unmarked=0",
                "password ranges=9 key_events=0",
                "card masked=0 scripted=4",
                "checked streams=1 episodes=15 ranges=41 title_events=41 failures=0",
                "check-labels PASS",
            ]
        );
    }
}

#[test]
fn control_one_a_range_that_loses_its_title_event_fails_and_differs_from_the_build() {
    let dir = copy_golden("control-one");
    edit_labels(
        &dir,
        r#""step":"g-cart-basket","start":1767430830000000000,"end":1767430875000000000"#,
        r#""step":"g-cart-basket","start":1767430831000000000,"end":1767430875000000000"#,
    );
    let o = run(&["check-labels", &s(&dir)]);
    assert_eq!(o.code, 1);
    assert!(o.stdout.contains("FAIL range golden golden-e01 g-cart-basket 1767430831000000000 1767430875000000000 titles=0 own=0\n"), "{}", o.stdout);
    assert!(
        o.stdout
            .contains("FAIL outside golden 1767430830000000000 g-cart-basket\n"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.ends_with("check-labels FAIL\n"));
    let built = build_golden(dir.parent().expect("parent"), "built");
    assert_ne!(fs::read(labels(&built)).ok(), fs::read(labels(&dir)).ok());
}

#[test]
fn control_two_a_range_on_a_stretch_with_no_title_event_fails_the_recorded_rule() {
    let dir = copy_golden("control-two");
    edit_labels(
        &dir,
        r#""step":"g-budget-sheet","start":1770652800000000000,"end":1770652920000000000"#,
        r#""step":"g-budget-sheet","start":1770660000000000000,"end":1770660120000000000"#,
    );
    let o = run(&["check-labels", "--rule", "recorded", &s(&dir)]);
    assert_eq!(o.code, 1);
    assert!(o.stdout.starts_with("rule recorded\n"));
    assert!(o.stdout.contains("FAIL range golden golden-e10 g-budget-sheet 1770660000000000000 1770660120000000000 titles=0 own=0\n"), "{}", o.stdout);
}

#[test]
fn each_row_rule_names_its_token_and_the_later_row() {
    let dir = copy_golden("rows");
    let p = labels(&dir);
    let text = fs::read_to_string(&p).expect("labels");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    lines[0] = lines[0].replace("\"end\":1767431000000000000", "\"end\":1767431001000000000");
    lines[7] = lines[7].replace("\"step\":\"g-power-signin\"", "\"step\":\"g-unknown\"");
    lines.swap(10, 11);
    lines[13] = lines[13].replace(
        "\"start\":1768036530250000000,\"end\":1768036560250000000",
        "\"start\":1768036530250000000,\"end\":1768036570250000000",
    );
    fs::write(
        &p,
        lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
    )
    .expect("write");
    let o = run(&["check-labels", &s(&dir)]);
    assert_eq!(o.code, 1);
    let fails: Vec<&str> = o
        .stdout
        .lines()
        .filter(|l| l.starts_with("FAIL row"))
        .collect();
    assert_eq!(
        fails,
        vec![
            "FAIL row golden 6 span",
            "FAIL row golden 8 reference",
            "FAIL row golden 11 span",
            "FAIL row golden 12 order",
            "FAIL row golden 15 overlap",
        ],
        "{}",
        o.stdout
    );
}

#[test]
fn a_marked_range_outside_its_marks_and_keys_in_a_password_range_fail() {
    let dir = copy_golden("marks");
    edit_labels(
        &dir,
        r#""step":"g-agenda-clear","start":1771509912000000000,"end":1771509942000000000"#,
        r#""step":"g-agenda-clear","start":1771509912000000000,"end":1771509946000000000"#,
    );
    edit_labels(
        &dir,
        r#""step":"g-cart-signin","start":1767430800000000000,"end":1767430830000000000"#,
        r#""step":"g-cart-signin","start":1767430800000000000,"end":1767430835000000000"#,
    );
    let o = run(&["check-labels", "--rule", "recorded", &s(&dir)]);
    assert_eq!(o.code, 1);
    assert!(o.stdout.contains("FAIL unmarked golden golden-e15 g-agenda-clear 1771509912000000000 1771509946000000000\n"), "{}", o.stdout);
    assert!(o.stdout.contains("marked ranges=10 unmarked=1\n"));
    assert!(o.stdout.contains("FAIL keys golden golden-e01 g-cart-signin 1767430800000000000 1767430835000000000 key_events=1\n"), "{}", o.stdout);
    assert!(o.stdout.contains("password ranges=9 key_events=1\n"));
}

#[test]
fn a_manifest_source_sets_the_rule_and_the_flag_overrides_it() {
    let dir = copy_golden("source");
    fs::rename(
        dir.join("streams/golden"),
        dir.join("streams/rec-dev-golden"),
    )
    .expect("rename");
    let m = TierManifest {
        tier: "rec-dev".to_owned(),
        split: "dev".to_owned(),
        source: "recorded".to_owned(),
        seed: 0,
        day1: 1_767_225_600_000_000_000,
        canaries: Vec::new(),
        streams: vec![("rec-dev-golden".to_owned(), Sites::Seen)],
        counts: Counts {
            streams: 1,
            ..Counts::default()
        },
        files: Vec::new(),
    };
    write(&dir.join("manifest.toml"), &m.write());
    assert!(run(&["check-labels", &s(&dir)])
        .stdout
        .starts_with("rule recorded\n"));
    assert!(run(&["check-labels", "--rule", "generated", &s(&dir)])
        .stdout
        .starts_with("rule generated\n"));
    write(&dir.join("manifest.toml"), "version = 2\n");
    refused(&run(&["check-labels", &s(&dir)]), "tier-manifest");
}

#[test]
fn check_labels_refusals() {
    let g = s(&golden());
    refused(&run(&["check-labels"]), "usage");
    refused(&run(&["check-labels", "--rule", "both", &g]), "usage");
    refused(
        &run(&[
            "check-labels",
            "--rule",
            "recorded",
            "--rule",
            "recorded",
            &g,
        ]),
        "usage",
    );
    refused(&run(&["check-labels", &g, &g]), "usage");
    let dir = copy_golden("sm");
    fs::write(dir.join("script-manifest.jsonl"), "{}\n").expect("w");
    refused(&run(&["check-labels", &s(&dir)]), "script-manifest");
    let dir = copy_golden("labels");
    fs::write(labels(&dir), "{\"row\":\"other\"}\n").expect("w");
    refused(&run(&["check-labels", &s(&dir)]), "labels");
    let dir = copy_golden("nolabels");
    fs::rename(labels(&dir), dir.join("streams/golden/labels.txt")).expect("mv");
    refused(&run(&["check-labels", &s(&dir)]), "labels");
    let dir = copy_golden("batches");
    let day = dir.join("streams/golden/day-003.jsonl");
    let text = fs::read_to_string(&day).expect("day");
    fs::write(&day, text.replacen("\"v\":1", "\"v\":2", 1)).expect("w");
    refused(&run(&["check-labels", &s(&dir)]), "batches");
    let dir = scratch("empty");
    fs::write(dir.join("script-manifest.jsonl"), "").expect("w");
    refused(&run(&["check-labels", &s(&dir)]), "batches");
}

#[test]
fn a_range_with_two_title_events_fails_the_generated_rule_alone() {
    let dir = copy_golden("two-titles");
    edit_labels(
        &dir,
        r#""step":"g-rides-signin","start":1767787200000000000,"end":1767787215000000000"#,
        r#""step":"g-rides-signin","start":1767787200000000000,"end":1767787216000000000"#,
    );
    let o = run(&["check-labels", &s(&dir)]);
    assert!(
        o.stdout.contains(
            "FAIL range golden golden-e07 g-rides-signin 1767787200000000000 1767787216000000000 titles=2 own=1\n"
        ),
        "{}",
        o.stdout
    );
    let r = run(&["check-labels", "--rule", "recorded", &s(&dir)]);
    assert!(!r.stdout.contains("FAIL range"), "{}", r.stdout);
}

#[test]
fn a_task_id_in_two_streams_breaks_rule_five() {
    let dir = copy_golden("task-twice");
    copy_dir(&dir.join("streams/golden"), &dir.join("streams/other"));
    let p = dir.join("streams/other/labels.jsonl");
    let text = fs::read_to_string(&p).expect("labels");
    fs::write(
        &p,
        text.replace("\"episode\":\"golden-e", "\"episode\":\"other-e"),
    )
    .expect("write");
    let o = run(&["check-labels", "--rule", "recorded", &s(&dir)]);
    assert!(
        o.stdout.contains("FAIL row other 1 reference\n"),
        "{}",
        o.stdout
    );
    assert!(!o.stdout.contains("FAIL row golden"), "{}", o.stdout);
}

#[test]
fn every_range_that_overlaps_an_earlier_one_is_named_once() {
    let dir = copy_golden("overlap-three");
    let p = labels(&dir);
    let text = fs::read_to_string(&p).expect("labels");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    let second = lines[2].clone();
    let end_of_fourth = lines[4]
        .split("\"end\":")
        .nth(1)
        .and_then(|e| e.strip_suffix('}'))
        .expect("end")
        .to_owned();
    let start_of_second = second
        .split("\"start\":")
        .nth(1)
        .and_then(|r| r.split(',').next())
        .expect("start")
        .to_owned();
    let old_end = second
        .split("\"end\":")
        .nth(1)
        .and_then(|e| e.strip_suffix('}'))
        .expect("end")
        .to_owned();
    lines[2] = second.replace(
        &format!("\"start\":{start_of_second},\"end\":{old_end}"),
        &format!("\"start\":{start_of_second},\"end\":{end_of_fourth}"),
    );
    assert_ne!(lines[2], second);
    fs::write(
        &p,
        lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
    )
    .expect("write");
    for (rule, failures) in [("generated", 3), ("recorded", 2)] {
        let o = run(&["check-labels", "--rule", rule, &s(&dir)]);
        assert_eq!(o.code, 1);
        let rows: Vec<&str> = o
            .stdout
            .lines()
            .filter(|l| l.starts_with("FAIL row"))
            .collect();
        assert_eq!(
            rows,
            vec!["FAIL row golden 4 overlap", "FAIL row golden 5 overlap"],
            "{}",
            o.stdout
        );
        assert!(
            o.stdout.contains(&format!(" failures={failures}\n")),
            "{}",
            o.stdout
        );
    }
}

/// A body of one stream `zz` with no day file, the golden script manifest and the given label lines.
fn body_with_rows(name: &str, rows: &[String]) -> std::path::PathBuf {
    let dir = scratch(name).join("b");
    fs::create_dir_all(dir.join("streams/zz")).expect("stream directory");
    fs::copy(
        golden().join("script-manifest.jsonl"),
        dir.join("script-manifest.jsonl"),
    )
    .expect("script manifest");
    write(
        &dir.join("streams/zz/labels.jsonl"),
        &rows.iter().map(|r| format!("{r}\n")).collect::<String>(),
    );
    dir
}

const T0: i64 = 1_767_225_600_000_000_000;

fn episode_row(episode: &str, task: &str, start: i64, end: i64) -> String {
    format!(
        r#"{{"row":"episode","episode":"{episode}","task":"{task}","category":"utility_bill_payment","kind":"planted","start":{},"end":{}}}"#,
        T0 + start * common::SEC,
        T0 + end * common::SEC
    )
}

fn range_row(episode: &str, start: i64, end: i64) -> String {
    format!(
        r#"{{"row":"range","episode":"{episode}","step":"g-power-bill","start":{},"end":{}}}"#,
        T0 + start * common::SEC,
        T0 + end * common::SEC
    )
}

#[test]
fn the_later_row_of_every_overlapping_pair_is_named_when_the_farthest_range_lies_between() {
    let dir = body_with_rows(
        "overlap-farthest",
        &[
            episode_row("s01-e0001", "s01-t001", 1000, 3000),
            range_row("s01-e0001", 1000, 2500),
            range_row("s01-e0001", 2000, 3000),
            episode_row("s01-e0002", "s01-t001", 1500, 10000),
            range_row("s01-e0002", 1500, 10000),
        ],
    );
    for rule in ["generated", "recorded"] {
        let o = run(&["check-labels", "--rule", rule, &s(&dir)]);
        assert_eq!(o.code, 1, "{}", o.stdout);
        let rows: Vec<&str> = o
            .stdout
            .lines()
            .filter(|l| l.starts_with("FAIL row"))
            .collect();
        assert_eq!(
            rows,
            vec!["FAIL row zz 3 overlap", "FAIL row zz 5 overlap"],
            "{rule}: {}",
            o.stdout
        );
        assert!(o.stdout.contains(" failures=5\n"), "{rule}: {}", o.stdout);
    }
}

#[test]
fn overlap_of_a_last_range_that_starts_first_and_reaches_farthest_names_each_later_row() {
    let dir = body_with_rows(
        "overlap-last-first",
        &[
            episode_row("s01-e0001", "s01-t001", 2000, 2500),
            range_row("s01-e0001", 2000, 2500),
            episode_row("s01-e0002", "s01-t001", 3000, 3500),
            range_row("s01-e0002", 3000, 3500),
            range_row("s01-e0002", 3400, 3500),
            range_row("s01-e0002", 1000, 5000),
        ],
    );
    let o = run(&["check-labels", "--rule", "recorded", &s(&dir)]);
    assert_eq!(o.code, 1, "{}", o.stdout);
    let rows: Vec<&str> = o
        .stdout
        .lines()
        .filter(|l| l.starts_with("FAIL row"))
        .collect();
    assert_eq!(
        rows,
        vec!["FAIL row zz 5 overlap", "FAIL row zz 6 order"],
        "{}",
        o.stdout
    );
}

#[test]
fn under_the_recorded_rule_a_range_with_only_another_steps_title_fails() {
    let dir = copy_golden("recorded-other-step");
    edit_labels(
        &dir,
        r#""step":"g-budget-sheet","start":1770652800000000000,"end":1770652920000000000"#,
        r#""step":"g-cart-basket","start":1770652800000000000,"end":1770652920000000000"#,
    );
    let o = run(&["check-labels", "--rule", "recorded", &s(&dir)]);
    assert_eq!(o.code, 1);
    let fails: Vec<&str> = o
        .stdout
        .lines()
        .filter(|l| l.starts_with("FAIL "))
        .collect();
    assert_eq!(
        fails,
        vec!["FAIL range golden golden-e10 g-cart-basket 1770652800000000000 1770652920000000000 titles=1 own=0"],
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains(" failures=1\n"), "{}", o.stdout);
}

#[test]
fn a_title_event_of_a_step_of_each_negative_kind_outside_every_range_fails() {
    let rows = [
        (
            "one_off",
            r#""step":"g-budget-sheet","start":1770652800000000000,"#,
            r#""step":"g-budget-sheet","start":1770652801000000000,"#,
            "FAIL outside golden 1770652800000000000 g-budget-sheet",
        ),
        (
            "twice",
            r#""step":"g-power-bill","start":1767607220000000000,"#,
            r#""step":"g-power-bill","start":1767607221000000000,"#,
            "FAIL outside golden 1767607220000000000 g-power-bill",
        ),
        (
            "near_miss",
            r#""step":"g-rides-fares","start":1767787215000000000,"#,
            r#""step":"g-rides-fares","start":1767787216000000000,"#,
            "FAIL outside golden 1767787215000000000 g-rides-fares",
        ),
        (
            "marked",
            r#""step":"g-agenda-clear","start":1771509632000000000,"#,
            r#""step":"g-agenda-clear","start":1771509633000000000,"#,
            "FAIL outside golden 1771509632000000000 g-agenda-clear",
        ),
    ];
    for (kind, from, to, line) in rows {
        let dir = copy_golden(&format!("outside-{kind}"));
        edit_labels(&dir, from, to);
        let o = run(&["check-labels", &s(&dir)]);
        assert_eq!(o.code, 1, "{kind}");
        let outside: Vec<&str> = o
            .stdout
            .lines()
            .filter(|l| l.starts_with("FAIL outside"))
            .collect();
        assert_eq!(outside, vec![line], "{kind}: {}", o.stdout);
    }
}
