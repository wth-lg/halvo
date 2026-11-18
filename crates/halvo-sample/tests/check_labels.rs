mod common;

use std::fs;
use std::path::Path;

use common::{build_golden, copy_dir, golden, link_dir, refused, run, s, scratch, write};
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
    assert!(
        o.stdout.contains(
            "FAIL range s1 l1 l3 1767430831000000000 1767430875000000000 titles=0 own=0\n"
        ),
        "{}",
        o.stdout
    );
    assert!(
        o.stdout
            .contains("FAIL outside s1 1767430830000000000 l3\n"),
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
    assert!(
        o.stdout.contains(
            "FAIL range s1 l40 l41 1770660000000000000 1770660120000000000 titles=0 own=0\n"
        ),
        "{}",
        o.stdout
    );
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
            "FAIL row s1 6 span",
            "FAIL row s1 8 reference",
            "FAIL row s1 11 span",
            "FAIL row s1 12 order",
            "FAIL row s1 15 overlap",
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
    assert!(
        o.stdout
            .contains("FAIL unmarked s1 l54 l56 1771509912000000000 1771509946000000000\n"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains("marked ranges=10 unmarked=1\n"));
    assert!(
        o.stdout
            .contains("FAIL keys s1 l1 l2 1767430800000000000 1767430835000000000 key_events=1\n"),
        "{}",
        o.stdout
    );
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
            "FAIL range s1 l10 l11 1767787200000000000 1767787216000000000 titles=2 own=1\n"
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
        o.stdout.contains("FAIL row s2 1 reference\n"),
        "{}",
        o.stdout
    );
    assert!(!o.stdout.contains("FAIL row s1 "), "{}", o.stdout);
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
            vec!["FAIL row s1 4 overlap", "FAIL row s1 5 overlap"],
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
            vec!["FAIL row s1 3 overlap", "FAIL row s1 5 overlap"],
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
        vec!["FAIL row s1 5 overlap", "FAIL row s1 6 order"],
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
        vec!["FAIL range s1 l40 l41 1770652800000000000 1770652920000000000 titles=1 own=0"],
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
            "FAIL outside s1 1770652800000000000 l41",
        ),
        (
            "twice",
            r#""step":"g-power-bill","start":1767607220000000000,"#,
            r#""step":"g-power-bill","start":1767607221000000000,"#,
            "FAIL outside s1 1767607220000000000 l9",
        ),
        (
            "near_miss",
            r#""step":"g-rides-fares","start":1767787215000000000,"#,
            r#""step":"g-rides-fares","start":1767787216000000000,"#,
            "FAIL outside s1 1767787215000000000 l12",
        ),
        (
            "marked",
            r#""step":"g-agenda-clear","start":1771509632000000000,"#,
            r#""step":"g-agenda-clear","start":1771509633000000000,"#,
            "FAIL outside s1 1771509632000000000 l44",
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

fn delete_task_rows(dir: &Path, task: &str) -> usize {
    let p = labels(dir);
    let text = fs::read_to_string(&p).expect("labels");
    let episodes: Vec<String> = text
        .lines()
        .filter(|l| l.contains(&format!("\"task\":\"{task}\"")))
        .map(|l| l.split('"').nth(7).expect("episode id").to_owned())
        .collect();
    assert!(!episodes.is_empty(), "{task}");
    let kept: String = text
        .lines()
        .filter(|l| {
            !episodes
                .iter()
                .any(|e| l.contains(&format!("\"episode\":\"{e}\"")))
        })
        .map(|l| format!("{l}\n"))
        .collect();
    fs::write(&p, kept).expect("write");
    episodes.len()
}

#[test]
fn case_210_every_row_of_a_one_off_task_deleted_fails() {
    every_row_deleted("210", "golden-t4", "one_off", &["g-budget-sheet"]);
}

#[test]
fn case_211_every_row_of_a_twice_task_deleted_fails() {
    every_row_deleted(
        "211",
        "golden-t2",
        "twice",
        &["g-power-bill", "g-power-signin"],
    );
}

#[test]
fn case_212_every_row_of_a_planted_task_deleted_fails() {
    every_row_deleted(
        "212",
        "golden-t1",
        "planted",
        &[
            "g-cart-basket",
            "g-cart-confirm",
            "g-cart-pay",
            "g-cart-receipt",
            "g-cart-signin",
        ],
    );
}

/// The place that an outside line prints for `step`: `l<n>` of the first range row of the stream's labels that
/// names it, else `m<n>` of its line in the script manifest.
fn step_place(dir: &Path, step: &str) -> String {
    let named = format!("\"step\":\"{step}\"");
    let rows = fs::read_to_string(labels(dir)).expect("labels");
    if let Some(n) = rows.lines().position(|l| l.contains(&named)) {
        return format!("l{}", n + 1);
    }
    let sm = fs::read_to_string(dir.join("script-manifest.jsonl")).expect("script manifest");
    let n = sm
        .lines()
        .position(|l| l.starts_with(&format!("{{{named},")))
        .expect("the step line");
    format!("m{}", n + 1)
}

fn every_row_deleted(case: &str, task: &str, kind: &str, steps: &[&str]) {
    {
        let dir = copy_golden(&format!("deleted-{case}"));
        let gone = delete_task_rows(&dir, task);
        let o = run(&["check-labels", &s(&dir)]);
        assert_eq!(o.code, 1, "case {case} ({kind}): {}", o.stdout);
        let outside: Vec<&str> = o
            .stdout
            .lines()
            .filter(|l| l.starts_with("FAIL outside"))
            .collect();
        assert!(!outside.is_empty(), "case {case}: {}", o.stdout);
        let places: Vec<String> = steps.iter().map(|st| step_place(&dir, st)).collect();
        assert!(
            outside
                .iter()
                .all(|l| places.iter().any(|p| l.ends_with(&format!(" {p}")))),
            "case {case}: {}",
            o.stdout
        );
        assert_eq!(
            o.stdout.lines().filter(|l| l.starts_with("FAIL")).count(),
            outside.len(),
            "case {case}: {}",
            o.stdout
        );
        assert!(gone >= 1);
    }
}

#[test]
fn a_seed_step_whose_mark_is_false_fails_with_a_seed_line_for_each_of_its_ranges() {
    let dir = copy_golden("seed-false");
    let p = dir.join("script-manifest.jsonl");
    let text = fs::read_to_string(&p).expect("script manifest");
    let line = text
        .lines()
        .find(|l| l.starts_with("{\"step\":\"g-power-bill\","))
        .expect("the step line");
    assert!(line.ends_with(",\"seed_category\":true}"), "{line}");
    fs::write(
        &p,
        text.replacen(
            line,
            &line.replace(",\"seed_category\":true}", ",\"seed_category\":false}"),
            1,
        ),
    )
    .expect("write");
    let rows = fs::read_to_string(labels(&dir)).expect("labels");
    let ranges: Vec<(usize, String)> = rows
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("\"step\":\"g-power-bill\""))
        .map(|(i, l)| (i + 1, l.to_owned()))
        .collect();
    assert_eq!(ranges.len(), 2);
    for rule in ["generated", "recorded"] {
        let o = run(&["check-labels", "--rule", rule, &s(&dir)]);
        assert_eq!(o.code, 1, "{rule}: {}", o.stdout);
        let fails: Vec<&str> = o.stdout.lines().filter(|l| l.starts_with("FAIL")).collect();
        assert_eq!(fails.len(), 2, "{rule}: {}", o.stdout);
        for (f, (line, r)) in fails.iter().zip(&ranges) {
            let field = |k: &str| {
                r.split(&format!("\"{k}\":"))
                    .nth(1)
                    .and_then(|v| v.split([',', '}']).next())
                    .expect("field")
                    .trim_matches('"')
                    .to_owned()
            };
            let episode_row = format!(
                "{{\"row\":\"episode\",\"episode\":\"{}\",",
                field("episode")
            );
            let episode_line = rows
                .lines()
                .position(|l| l.starts_with(&episode_row))
                .expect("the episode row")
                + 1;
            assert_eq!(
                *f,
                format!(
                    "FAIL seed s1 l{episode_line} l{line} {} {}",
                    field("start"),
                    field("end")
                ),
                "{rule}"
            );
        }
        assert!(o.stdout.contains(" failures=2\n"), "{rule}: {}", o.stdout);
    }
}

#[test]
fn a_script_manifest_line_without_its_seed_mark_is_refused() {
    let dir = copy_golden("seed-missing");
    let p = dir.join("script-manifest.jsonl");
    let text = fs::read_to_string(&p).expect("script manifest");
    fs::write(&p, text.replacen(",\"seed_category\":true}", "}", 1)).expect("write");
    refused(&run(&["check-labels", &s(&dir)]), "script-manifest");
}

#[test]
fn the_fail_lines_of_one_range_stand_in_the_order_range_seed_unmarked_keys() {
    let dir = copy_golden("one-range-order");
    let p = dir.join("script-manifest.jsonl");
    let text = fs::read_to_string(&p).expect("script manifest");
    let line = text
        .lines()
        .find(|l| l.starts_with("{\"step\":\"g-agenda-clear\","))
        .expect("the step line");
    let changed = line
        .replace("\"password_input\":false", "\"password_input\":true")
        .replace(",\"seed_category\":true}", ",\"seed_category\":false}");
    assert_ne!(changed, line);
    fs::write(&p, text.replacen(line, &changed, 1)).expect("write");
    let start = "1771509632000000000";
    let mut gone = 0;
    for e in fs::read_dir(dir.join("streams/golden")).expect("stream") {
        let path = e.expect("entry").path();
        if !path.to_string_lossy().contains("day-") {
            continue;
        }
        let day = fs::read_to_string(&path).expect("day file");
        let kept: String = day
            .lines()
            .filter(|l| {
                let title_here = l.contains("\"kind\":\"window_title\"")
                    && l.contains(&format!("\"at\":{start},"));
                let mark = l.contains("\"kind\":\"run_mark\"");
                if title_here || mark {
                    gone += 1;
                }
                !(title_here || mark)
            })
            .map(|l| format!("{l}\n"))
            .collect();
        fs::write(&path, kept).expect("write");
    }
    assert!(gone >= 3, "{gone}");
    let o = run(&["check-labels", &s(&dir)]);
    assert_eq!(o.code, 1, "{}", o.stdout);
    let kinds: Vec<&str> = o
        .stdout
        .lines()
        .filter(|l| l.contains(&format!(" s1 l42 l44 {start} ")))
        .map(|l| l.split(' ').nth(1).expect("kind"))
        .collect();
    assert_eq!(
        kinds,
        vec!["range", "seed", "unmarked", "keys"],
        "{}",
        o.stdout
    );
}

const MARKER: &str = "qzmq";

/// A copy of the golden directory whose stream, episode, task and step ids all hold the marker, with its stream
/// under `streams/qzmq-b` and a link `streams/qzmq-a` before it, so the stream is `s2`.
fn marked_golden(name: &str) -> std::path::PathBuf {
    let dir = copy_golden(name);
    let sm = dir.join("script-manifest.jsonl");
    let text = fs::read_to_string(&sm).expect("script manifest");
    fs::write(
        &sm,
        text.replace("{\"step\":\"g-", &format!("{{\"step\":\"{MARKER}-g-")),
    )
    .expect("write");
    let rows = labels(&dir);
    let text = fs::read_to_string(&rows).expect("labels");
    fs::write(
        &rows,
        text.replace("\"golden-", &format!("\"{MARKER}-golden-"))
            .replace("\"step\":\"g-", &format!("\"step\":\"{MARKER}-g-")),
    )
    .expect("write");
    let stream = dir.join(format!("streams/{MARKER}-b"));
    fs::rename(dir.join("streams/golden"), &stream).expect("rename");
    link_dir(&stream, &dir.join(format!("streams/{MARKER}-a")));
    dir
}

fn marked_labels(dir: &Path) -> std::path::PathBuf {
    dir.join(format!("streams/{MARKER}-b/labels.jsonl"))
}

fn edit_marked(dir: &Path, from: &str, to: &str) {
    let p = marked_labels(dir);
    let text = fs::read_to_string(&p).expect("labels");
    let (from, to) = (
        from.replace("\"g-", "\"qzmq-g-"),
        to.replace("\"g-", "\"qzmq-g-"),
    );
    assert_eq!(text.matches(&from).count(), 1, "{from}");
    fs::write(&p, text.replacen(&from, &to, 1)).expect("write");
}

fn place(token: &str, prefixes: &str) -> bool {
    let mut chars = token.chars();
    let first = chars.next().is_some_and(|c| prefixes.contains(c));
    let digits = chars.as_str();
    first
        && !digits.is_empty()
        && !digits.starts_with('0')
        && digits.bytes().all(|b| b.is_ascii_digit())
}

fn number(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit())
}

fn count(token: &str, key: &str) -> bool {
    token
        .strip_prefix(key)
        .and_then(|t| t.strip_prefix('='))
        .is_some_and(number)
}

/// True when a FAIL line of check-labels names its stream as `s<n>`, its episode and step as `l<n>` (or the step
/// of an outside line as `m<n>`), and holds nothing else but numbers and fixed words.
fn in_place_form(line: &str) -> bool {
    let t: Vec<&str> = line.split(' ').collect();
    let span = |a: usize| number(t[a]) && number(t[a + 1]);
    match (t.first(), t.get(1), t.len()) {
        (Some(&"FAIL"), Some(&"row"), 5) => {
            place(t[2], "s")
                && number(t[3])
                && ["order", "span", "overlap", "reference"].contains(&t[4])
        }
        (Some(&"FAIL"), Some(&"range"), 9) => {
            place(t[2], "s")
                && place(t[3], "l")
                && place(t[4], "l")
                && span(5)
                && count(t[7], "titles")
                && count(t[8], "own")
        }
        (Some(&"FAIL"), Some(&"outside"), 5) => {
            place(t[2], "s") && number(t[3]) && place(t[4], "lm")
        }
        (Some(&"FAIL"), Some(&"seed" | &"unmarked"), 7) => {
            place(t[2], "s") && place(t[3], "l") && place(t[4], "l") && span(5)
        }
        (Some(&"FAIL"), Some(&"keys"), 8) => {
            place(t[2], "s")
                && place(t[3], "l")
                && place(t[4], "l")
                && span(5)
                && count(t[7], "key_events")
        }
        (Some(&"FAIL"), Some(&"link"), 3) => place(t[2], "s"),
        _ => false,
    }
}

#[test]
fn over_a_tier_whose_ids_hold_a_marker_every_fail_line_names_places_and_no_line_holds_the_marker() {
    let mut kinds: Vec<String> = Vec::new();
    let mut check = |dir: &Path, rule: &str, want: &[&str]| {
        let o = run(&["check-labels", "--rule", rule, &s(dir)]);
        assert_eq!(o.code, 1, "{}{}", o.stdout, o.stderr);
        assert!(!o.stdout.contains(MARKER), "{}", o.stdout);
        assert!(o.stderr.is_empty(), "{}", o.stderr);
        let fails: Vec<&str> = o.stdout.lines().filter(|l| l.starts_with("FAIL")).collect();
        assert_eq!(fails.first(), Some(&"FAIL link s1"), "{}", o.stdout);
        for f in &fails {
            assert!(in_place_form(f), "{f}");
            kinds.push(f.split(' ').nth(1).expect("kind").to_owned());
        }
        for w in want {
            assert!(fails.contains(w), "{w}: {}", o.stdout);
        }
        assert!(
            o.stdout.contains(&format!(" failures={}\n", fails.len())),
            "{}",
            o.stdout
        );
    };
    let dir = marked_golden("marker-range");
    edit_marked(
        &dir,
        r#""step":"g-cart-basket","start":1767430830000000000,"#,
        r#""step":"g-cart-basket","start":1767430831000000000,"#,
    );
    check(
        &dir,
        "generated",
        &[
            "FAIL range s2 l1 l3 1767430831000000000 1767430875000000000 titles=0 own=0",
            "FAIL outside s2 1767430830000000000 l3",
        ],
    );
    let dir = marked_golden("marker-rows");
    let p = marked_labels(&dir);
    let text = fs::read_to_string(&p).expect("labels");
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    lines[0] = lines[0].replace("\"end\":1767431000000000000", "\"end\":1767431001000000000");
    lines.swap(10, 11);
    fs::write(
        &p,
        lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
    )
    .expect("write");
    check(
        &dir,
        "recorded",
        &[
            "FAIL row s2 6 span",
            "FAIL row s2 11 span",
            "FAIL row s2 12 order",
        ],
    );
    let dir = marked_golden("marker-marks");
    edit_marked(
        &dir,
        r#""step":"g-agenda-clear","start":1771509912000000000,"end":1771509942000000000"#,
        r#""step":"g-agenda-clear","start":1771509912000000000,"end":1771509946000000000"#,
    );
    edit_marked(
        &dir,
        r#""step":"g-cart-signin","start":1767430800000000000,"end":1767430830000000000"#,
        r#""step":"g-cart-signin","start":1767430800000000000,"end":1767430835000000000"#,
    );
    check(
        &dir,
        "recorded",
        &[
            "FAIL unmarked s2 l54 l56 1771509912000000000 1771509946000000000",
            "FAIL keys s2 l1 l2 1767430800000000000 1767430835000000000 key_events=1",
        ],
    );
    let dir = marked_golden("marker-seed");
    let sm = dir.join("script-manifest.jsonl");
    let text = fs::read_to_string(&sm).expect("script manifest");
    let line = text
        .lines()
        .find(|l| l.starts_with("{\"step\":\"qzmq-g-power-bill\","))
        .expect("the step line");
    fs::write(
        &sm,
        text.replacen(
            line,
            &line.replace(",\"seed_category\":true}", ",\"seed_category\":false}"),
            1,
        ),
    )
    .expect("write");
    check(&dir, "generated", &[]);
    let dir = marked_golden("marker-deleted");
    let p = marked_labels(&dir);
    let text = fs::read_to_string(&p).expect("labels");
    let episodes: Vec<String> = text
        .lines()
        .filter(|l| l.contains("\"task\":\"qzmq-golden-t1\""))
        .map(|l| l.split('"').nth(7).expect("episode id").to_owned())
        .collect();
    assert!(episodes.len() > 1);
    let kept: String = text
        .lines()
        .filter(|l| {
            !episodes
                .iter()
                .any(|e| l.contains(&format!("\"episode\":\"{e}\"")))
        })
        .map(|l| format!("{l}\n"))
        .collect();
    fs::write(&p, kept).expect("write");
    check(
        &dir,
        "generated",
        &["FAIL outside s2 1767430830000000000 m4"],
    );
    for kind in [
        "row", "range", "outside", "seed", "link", "unmarked", "keys",
    ] {
        assert!(kinds.iter().any(|k| k == kind), "{kind}: {kinds:?}");
    }
}

#[test]
fn a_refusal_of_check_labels_names_the_dir_argument_and_the_places_alone() {
    let refusal = |dir: &Path, cause: &str, after: &str| {
        let o = run(&["check-labels", &s(dir)]);
        refused(&o, cause);
        assert_eq!(o.stderr, format!("REFUSED {cause}: {}{after}\n", s(dir)));
    };
    let dir = marked_golden("marker-bad-row");
    let p = marked_labels(&dir);
    let text = fs::read_to_string(&p).expect("labels");
    fs::write(
        &p,
        text.replacen("\"row\":\"range\"", "\"row\":\"qzmq\"", 1),
    )
    .expect("write");
    refusal(&dir, "labels", " s2 l2");
    let dir = marked_golden("marker-no-labels");
    fs::rename(marked_labels(&dir), dir.join("streams/qzmq-b/labels.txt")).expect("mv");
    refusal(&dir, "labels", " s2");
    let dir = marked_golden("marker-labels-link");
    let p = marked_labels(&dir);
    fs::rename(&p, dir.join("streams/qzmq-b/labels.txt")).expect("mv");
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.join("streams/qzmq-b/labels.txt"), &p).expect("link");
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(dir.join("streams/qzmq-b/labels.txt"), &p).expect("link");
    refusal(&dir, "labels", " s2");
    let dir = marked_golden("marker-batches");
    let day = dir.join("streams/qzmq-b/day-003.jsonl");
    let text = fs::read_to_string(&day).expect("day");
    fs::write(&day, text.replacen("\"v\":1", "\"v\":2", 1)).expect("w");
    refusal(&dir, "batches", " s2");
    let dir = marked_golden("marker-sm");
    fs::write(dir.join("script-manifest.jsonl"), "{\"step\":\"qzmq\"}\n").expect("w");
    refusal(&dir, "script-manifest", "");
    let dir = marked_golden("marker-manifest");
    write(&dir.join("manifest.toml"), "version = 2\n");
    refusal(&dir, "tier-manifest", "");
    let dir = scratch("marker-no-streams");
    fs::write(dir.join("script-manifest.jsonl"), "").expect("w");
    refusal(&dir, "batches", "");
}

#[test]
fn a_range_whose_episode_has_no_episode_row_in_its_stream_names_its_own_row_for_the_episode() {
    let dir = copy_golden("no-episode-row");
    edit_labels(
        &dir,
        r#"{"row":"range","episode":"golden-e01","step":"g-cart-basket","start":1767430830000000000,"#,
        r#"{"row":"range","episode":"golden-e99","step":"g-cart-basket","start":1767430831000000000,"#,
    );
    let o = run(&["check-labels", &s(&dir)]);
    assert_eq!(o.code, 1, "{}", o.stdout);
    assert!(
        o.stdout.contains(
            "FAIL range s1 l3 l3 1767430831000000000 1767430875000000000 titles=0 own=0\n"
        ),
        "{}",
        o.stdout
    );
}

#[test]
fn every_failing_range_of_one_episode_names_the_episode_row_and_its_own_range_row() {
    let dir = copy_golden("episode-of-two-ranges");
    edit_labels(
        &dir,
        r#""step":"g-cart-basket","start":1767430830000000000,"#,
        r#""step":"g-cart-basket","start":1767430831000000000,"#,
    );
    edit_labels(
        &dir,
        r#""step":"g-cart-pay","start":1767430875000000000,"#,
        r#""step":"g-cart-pay","start":1767430876000000000,"#,
    );
    let o = run(&["check-labels", &s(&dir)]);
    assert_eq!(o.code, 1, "{}", o.stdout);
    let ranges: Vec<&str> = o
        .stdout
        .lines()
        .filter(|l| l.starts_with("FAIL range"))
        .collect();
    assert_eq!(
        ranges,
        vec![
            "FAIL range s1 l1 l3 1767430831000000000 1767430875000000000 titles=0 own=0",
            "FAIL range s1 l1 l4 1767430876000000000 1767430915000000000 titles=0 own=0",
        ],
        "{}",
        o.stdout
    );
}

#[test]
fn s_n_counts_the_directories_and_links_of_streams_and_no_plain_file() {
    let dir = copy_golden("stream-places");
    edit_labels(
        &dir,
        r#""step":"g-cart-basket","start":1767430830000000000,"#,
        r#""step":"g-cart-basket","start":1767430831000000000,"#,
    );
    write(&dir.join("streams/a-note"), "a plain file\n");
    link_dir(&dir.join("streams/golden"), &dir.join("streams/b-link"));
    let target = dir.join("script-manifest.jsonl");
    let file_link = dir.join("streams/c-file-link");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &file_link).expect("link");
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&target, &file_link).expect("link");
    let o = run(&["check-labels", &s(&dir)]);
    assert_eq!(o.code, 1, "{}", o.stdout);
    let fails: Vec<&str> = o.stdout.lines().filter(|l| l.starts_with("FAIL")).collect();
    assert_eq!(
        fails,
        vec![
            "FAIL link s1",
            "FAIL link s2",
            "FAIL outside s3 1767430830000000000 l3",
            "FAIL range s3 l1 l3 1767430831000000000 1767430875000000000 titles=0 own=0",
        ],
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains("checked streams=1 "), "{}", o.stdout);
}
