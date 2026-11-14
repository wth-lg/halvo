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
        o.stdout
            .contains("checked streams=0 episodes=0 ranges=0 title_events=0 failures=1\n"),
        "{}",
        o.stdout
    );
}

fn tier(dir: &Path) -> PathBuf {
    let g = golden();
    let fix = |t: String| {
        t.replace("\"seen\"", "\"check\"").replace(
            "\"canaries\":[]",
            r#""canaries":["c-one.test","c-three.test","c-two.test"]"#,
        )
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
    fs::create_dir_all(dir.join("real/sub")).expect("a directory below");
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
        format!("{linked}{sep}sub{sep}.."),
        format!("{linked}{sep}sub{sep}..{sep}"),
    ] {
        refused(&run(&["leak", &s(&t), &form]), "path");
    }
    let o = run(&["leak", &s(&t), &s(&dir.join("real")), &linked]);
    refused(&o, "path");
    fs::create_dir_all(dir.join("below")).expect("a directory beside the link");
    let o = common::run_in(
        &dir.join("below"),
        &["leak", &s(&t), &format!("..{sep}linked{sep}")],
    );
    refused(&o, "path");
}

#[test]
fn a_path_argument_through_a_link_and_dot_dot_is_refused_where_it_names_a_link() {
    let dir = scratch("link-dot-dot");
    let t = tier(&dir);
    write(&dir.join("real/notes.md"), "We used Goldfinch-Cart once.\n");
    fs::create_dir_all(dir.join("phys/a/b")).expect("the physical target");
    link_dir(&dir.join("real"), &dir.join("phys/a/x"));
    fs::create_dir_all(dir.join("work")).expect("the working directory");
    link_dir(&dir.join("phys/a/b"), &dir.join("work/lnk"));
    assert!(!dir.join("work/x").exists());
    let sep = std::path::MAIN_SEPARATOR;
    let o = common::run_in(
        &dir.join("work"),
        &["leak", &s(&t), &format!("lnk{sep}..{sep}x")],
    );
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
    refused(
        &run(&["leak", &s(&t), &s(&dir.join("clean"))]),
        "tier-files",
    );
}

fn link_file(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("a symbolic link");
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(target, link).expect("a symbolic link");
}

fn files_made_links(stream: &Path, elsewhere: &Path, names: &[&str]) {
    fs::create_dir_all(elsewhere).expect("elsewhere");
    for name in names {
        fs::rename(stream.join(name), elsewhere.join(name)).expect("move the file");
        link_file(&elsewhere.join(name), &stream.join(name));
    }
}

#[test]
fn a_day_file_or_a_label_file_that_is_a_link_is_refused_by_check_labels() {
    let dir = copy_golden("link-files");
    let elsewhere = dir.parent().expect("parent").join("elsewhere");
    files_made_links(&dir.join("streams/golden"), &elsewhere, &["day-003.jsonl"]);
    refused(&run(&["check-labels", &s(&dir)]), "batches");
    let dir = copy_golden("link-labels");
    let elsewhere = dir.parent().expect("parent").join("elsewhere");
    files_made_links(&dir.join("streams/golden"), &elsewhere, &["labels.jsonl"]);
    refused(&run(&["check-labels", &s(&dir)]), "labels");
    for (name, cause) in [
        ("script-manifest.jsonl", "script-manifest"),
        ("manifest.toml", "tier-manifest"),
    ] {
        let dir = copy_golden(&format!("link-{name}"));
        let elsewhere = dir.parent().expect("parent").join("elsewhere");
        if name == "manifest.toml" {
            write(&elsewhere.join(name), "version = 1\n");
            link_file(&elsewhere.join(name), &dir.join(name));
        } else {
            files_made_links(&dir, &elsewhere, &[name]);
        }
        refused(&run(&["check-labels", &s(&dir)]), cause);
    }
}

#[test]
fn a_file_of_a_body_that_is_a_link_is_refused_by_leak_with_the_argument_alone() {
    let dir = scratch("link-body-files");
    let t = tier(&dir);
    let walk = dir.join("walk");
    let dev = common::build_golden(&walk, "zz-mill.test");
    files_made_links(
        &dev.join("streams/golden"),
        &dir.join("moved-day"),
        &["day-003.jsonl"],
    );
    let o = run(&["leak", &s(&t), &s(&walk)]);
    refused(&o, "body");
    assert_eq!(o.stderr, format!("REFUSED body: {}\n", s(&walk)));
    let walk = dir.join("walk-own");
    fs::create_dir_all(&walk).expect("walk");
    write(&walk.join("notes.md"), "c-two\n");
    link_file(
        &t.join("script-manifest.jsonl"),
        &walk.join("script-manifest.jsonl"),
    );
    link_file(&t.join("manifest.toml"), &walk.join("manifest.toml"));
    let o = run(&["leak", &s(&t), &s(&walk)]);
    refused(&o, "body");
    assert_eq!(o.stderr, format!("REFUSED body: {}\n", s(&walk)));
    let stream = fs::read_dir(t.join("streams"))
        .expect("streams")
        .next()
        .expect("one stream")
        .expect("entry")
        .path();
    files_made_links(&stream, &dir.join("moved-tier"), &["labels.jsonl"]);
    write(&dir.join("clean/readme.txt"), "plain words\n");
    let o = run(&["leak", &s(&t), &s(&dir.join("clean"))]);
    refused(&o, "tier-files");
    assert_eq!(o.stderr, format!("REFUSED tier-files: {}\n", s(&t)));
}

#[test]
fn the_link_lines_stand_in_byte_order_of_the_stream_id() {
    let dir = copy_golden("link-order");
    let names: Vec<String> = (0..12).rev().map(|k| format!("golden-l{k:02}")).collect();
    for n in &names {
        link_dir(&dir.join("none"), &dir.join("streams").join(n));
    }
    let o = run(&["check-labels", &s(&dir)]);
    let links: Vec<&str> = o
        .stdout
        .lines()
        .filter(|l| l.starts_with("FAIL link"))
        .collect();
    let mut want: Vec<String> = names.iter().map(|n| format!("FAIL link {n}")).collect();
    want.sort();
    assert_eq!(links, want, "{}", o.stdout);
}

#[test]
fn a_tier_manifest_that_is_a_link_is_refused_also_when_it_reads_well() {
    let dir = scratch("link-tier-manifest");
    let t = tier(&dir);
    assert_eq!(run(&["check-labels", &s(&t)]).code, 0);
    fs::rename(t.join("manifest.toml"), dir.join("manifest.toml")).expect("move the manifest");
    link_file(&dir.join("manifest.toml"), &t.join("manifest.toml"));
    refused(&run(&["check-labels", &s(&t)]), "tier-manifest");
}

#[test]
fn each_manifest_of_a_walked_body_that_is_a_link_is_refused_alone() {
    let dir = scratch("link-one-manifest");
    let t = tier(&dir);
    let walk = dir.join("walk-sm");
    fs::create_dir_all(&walk).expect("walk");
    link_file(
        &t.join("script-manifest.jsonl"),
        &walk.join("script-manifest.jsonl"),
    );
    let o = run(&["leak", &s(&t), &s(&walk)]);
    refused(&o, "body");
    let walk = dir.join("walk-mt");
    fs::create_dir_all(&walk).expect("walk");
    fs::copy(
        t.join("script-manifest.jsonl"),
        walk.join("script-manifest.jsonl"),
    )
    .expect("copy");
    link_file(&t.join("manifest.toml"), &walk.join("manifest.toml"));
    write(&walk.join("notes.md"), "c-two\n");
    let o = run(&["leak", &s(&t), &s(&walk)]);
    refused(&o, "body");
    let walk = dir.join("walk-dangling");
    fs::create_dir_all(&walk).expect("walk");
    link_file(&dir.join("gone.jsonl"), &walk.join("script-manifest.jsonl"));
    let o = run(&["leak", &s(&t), &s(&walk)]);
    refused(&o, "body");
}

#[test]
fn a_script_argument_that_is_a_link_is_refused_by_build() {
    let dir = scratch("link-script");
    let g = golden();
    write(
        &dir.join("s/real.jsonl"),
        &fs::read_to_string(g.join("script.jsonl")).expect("golden"),
    );
    write(
        &dir.join("s/marked.jsonl"),
        &fs::read_to_string(g.join("marked.jsonl")).expect("golden"),
    );
    link_file(&dir.join("s/real.jsonl"), &dir.join("s/linked.jsonl"));
    let o = common::run_in(
        &dir.join("s"),
        &[
            "build",
            "--seed",
            "1",
            "--out",
            &s(&dir.join("out")),
            "linked.jsonl",
            "marked.jsonl",
        ],
    );
    refused(&o, "script");
    assert_eq!(o.stderr, "REFUSED script: SCRIPT 1: not a script file\n");
    let o = common::run_in(
        &dir.join("s"),
        &[
            "build",
            "--seed",
            "1",
            "--out",
            &s(&dir.join("out-real")),
            "real.jsonl",
            "marked.jsonl",
        ],
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
}
