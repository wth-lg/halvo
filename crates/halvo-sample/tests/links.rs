mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{copy_dir, golden, link_dir, refused, run, s, scratch, write};

fn copy_golden(name: &str) -> PathBuf {
    let dir = scratch(name).join("g");
    copy_dir(&golden(), &dir);
    dir
}

#[test]
fn a_stream_directory_that_is_a_link_fails_with_its_line_first() {
    let dir = copy_golden("link-stream");
    let elsewhere = dir.parent().expect("parent").join("elsewhere");
    copy_dir(&dir.join("streams/golden"), &elsewhere.join("golden"));
    link_dir(&elsewhere.join("golden"), &dir.join("streams/golden-b"));
    link_dir(&elsewhere.join("none"), &dir.join("streams/golden-a"));
    for rule in ["generated", "recorded"] {
        let o = run(&["check-labels", "--rule", rule, &s(&dir)]);
        assert_eq!(o.code, 1, "{rule}: {}{}", o.stdout, o.stderr);
        let lines: Vec<&str> = o.stdout.lines().collect();
        assert_eq!(lines[0], format!("rule {rule}"));
        assert_eq!(lines[1], "FAIL link golden-a", "{}", o.stdout);
        assert_eq!(lines[2], "FAIL link golden-b", "{}", o.stdout);
        assert_eq!(
            o.stdout.lines().filter(|l| l.starts_with("FAIL")).count(),
            2,
            "{}",
            o.stdout
        );
        assert!(
            o.stdout.contains("checked streams=1 ") && o.stdout.contains(" failures=2\n"),
            "{}",
            o.stdout
        );
        assert!(o.stdout.ends_with("check-labels FAIL\n"), "{}", o.stdout);
    }
}

#[test]
fn a_streams_directory_that_is_a_link_is_refused_as_batches() {
    let dir = copy_golden("link-streams");
    let elsewhere = dir.parent().expect("parent").join("elsewhere");
    fs::rename(dir.join("streams"), &elsewhere).expect("move the streams");
    link_dir(&elsewhere, &dir.join("streams"));
    refused(&run(&["check-labels", &s(&dir)]), "batches");
}

#[test]
fn a_streams_directory_with_links_alone_fails_and_is_not_refused() {
    let dir = copy_golden("link-only");
    let elsewhere = dir.parent().expect("parent").join("elsewhere");
    fs::rename(dir.join("streams/golden"), &elsewhere).expect("move the stream");
    link_dir(&elsewhere, &dir.join("streams/golden"));
    let o = run(&["check-labels", &s(&dir)]);
    assert_eq!(o.code, 1, "{}{}", o.stdout, o.stderr);
    assert!(o.stdout.contains("FAIL link golden\n"), "{}", o.stdout);
    assert!(
        o.stdout.contains("checked streams=0 episodes=0 ranges=0 title_events=0 failures=1\n"),
        "{}",
        o.stdout
    );
}

/// The golden script built as a tier gen-check with three canaries, as the tests of leak build it.
fn tier(dir: &Path) -> PathBuf {
    let g = golden();
    let fix = |t: String| {
        t.replace("\"seen\"", "\"check\"")
            .replace("\"canaries\":[]", r#""canaries":["c-one.test","c-three.test","c-two.test"]"#)
    };
    write(
        &dir.join("a.jsonl"),
        &fix(fs::read_to_string(g.join("script.jsonl")).expect("golden")),
    );
    write(
        &dir.join("b.jsonl"),
        &fix(fs::read_to_string(g.join("marked.jsonl")).expect("golden")),
    );
    let steps: String = fs::read_to_string(g.join("script-manifest.jsonl"))
        .expect("golden")
        .lines()
        .map(|l| {
            let id = l.split('"').nth(3).expect("step id");
            format!("{{\"step\":\"{id}\",\"source\":\"render\",\"accessibility_text\":null,\"extracted_text\":null}}\n")
        })
        .collect();
    write(&dir.join("layers.jsonl"), &steps);
    let out = dir.join("bodies/gen-check");
    let o = run(&[
        "build",
        "--tier",
        "gen-check",
        "--layer-records",
        &s(&dir.join("layers.jsonl")),
        "--seed",
        "3",
        "--out",
        &s(&out),
        &s(&dir.join("a.jsonl")),
        &s(&dir.join("b.jsonl")),
    ]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    out
}

#[test]
fn a_path_argument_that_is_a_link_is_refused_in_each_of_its_forms() {
    let dir = scratch("link-path");
    let t = tier(&dir);
    write(&dir.join("real/notes.md"), "We used Goldfinch-Cart once.\n");
    let o = run(&["leak", &s(&t), &s(&dir.join("real"))]);
    assert_eq!(o.code, 1, "{}", o.stdout);
    link_dir(&dir.join("real"), &dir.join("linked"));
    let sep = std::path::MAIN_SEPARATOR;
    let linked = s(&dir.join("linked"));
    for form in [
        linked.clone(),
        format!("{linked}{sep}"),
        format!("{linked}{sep}{sep}"),
        format!("{linked}{sep}."),
        format!("{linked}{sep}.{sep}"),
    ] {
        refused(&run(&["leak", &s(&t), &form]), "path");
    }
    let o = run(&["leak", &s(&t), &s(&dir.join("real")), &linked]);
    refused(&o, "path");
}

#[test]
fn a_link_inside_a_walk_is_not_followed() {
    let dir = scratch("link-inside");
    let t = tier(&dir);
    write(&dir.join("real/notes.md"), "We used Goldfinch-Cart once.\n");
    write(&dir.join("walk/plain.txt"), "plain words\n");
    link_dir(&dir.join("real"), &dir.join("walk/linked"));
    let o = run(&["leak", &s(&t), &s(&dir.join("walk"))]);
    assert_eq!(o.code, 0, "{}", o.stdout);
    assert!(o.stdout.contains(" bodies=0 files=1 "), "{}", o.stdout);
}

#[test]
fn a_stream_directory_that_is_a_link_in_the_tier_or_in_a_body_is_refused() {
    let dir = scratch("link-bodies");
    let t = tier(&dir);
    let dev = common::build_golden(&dir, "dev");
    let moved = dir.join("moved-dev");
    fs::rename(dev.join("streams/golden"), &moved).expect("move the stream");
    link_dir(&moved, &dev.join("streams/golden"));
    refused(&run(&["leak", &s(&t), &s(&dev)]), "body");
    let dev2 = common::build_golden(&dir, "dev2");
    let moved = dir.join("moved-dev2");
    fs::rename(dev2.join("streams"), &moved).expect("move the streams");
    link_dir(&moved, &dev2.join("streams"));
    refused(&run(&["leak", &s(&t), &s(&dev2)]), "body");
    let stream = fs::read_dir(t.join("streams"))
        .expect("streams")
        .next()
        .expect("one stream")
        .expect("entry")
        .path();
    let moved = dir.join("moved-tier");
    fs::rename(&stream, &moved).expect("move the stream");
    link_dir(&moved, &stream);
    write(&dir.join("clean/readme.txt"), "plain words\n");
    refused(&run(&["leak", &s(&t), &s(&dir.join("clean"))]), "tier-files");
}
