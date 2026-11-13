mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{golden, refused, run, s, scratch, write};

const CANARIES: &str = r#"["c-one.test","c-three.test","c-two.test"]"#;

fn layer_text() -> String {
    [
        "g-agenda-clear", "g-agenda-week", "g-budget-sheet", "g-canary-news", "g-cart-basket", "g-cart-confirm", "g-cart-pay",
        "g-cart-receipt", "g-cart-signin", "g-power-bill", "g-power-signin", "g-rides-fares", "g-rides-signin",
    ]
    .iter()
    .map(|st| format!("{{\"step\":\"{st}\",\"source\":\"render\",\"accessibility_text\":null,\"extracted_text\":null}}\n"))
    .collect()
}

/// The golden script built as a sealed tier with three canaries in its header and one noise item on a canary.
fn sealed(dir: &Path, tier: &str, sites: &str) -> PathBuf {
    let g = golden();
    let noise = "{\"line\":\"step\",\"step\":\"g-canary-news\",\"app\":\"org.chromium.Chromium\",\"site\":\"c-one.test\",\"titles\":[\"Morning paper\"],\"paths\":[\"/home\"],\"password_input\":false,\"card_title\":false}\n{\"line\":\"noise\",\"stream\":\"golden\",\"day\":60,\"item\":{\"step\":\"g-canary-news\",\"title\":0,\"start\":36000000000000,\"end\":36030000000000,\"keys\":0,\"clicks\":1,\"scrolls\":0}}\n";
    let fix = |t: String| {
        t.replace("\"seen\"", &format!("\"{sites}\""))
            .replace("\"canaries\":[]", &format!("\"canaries\":{CANARIES}"))
    };
    let a = dir.join(format!("{tier}-script.jsonl"));
    let b = dir.join(format!("{tier}-marked.jsonl"));
    write(
        &a,
        &format!(
            "{}{noise}",
            fix(fs::read_to_string(g.join("script.jsonl")).expect("g"))
        ),
    );
    write(
        &b,
        &fix(fs::read_to_string(g.join("marked.jsonl")).expect("g")),
    );
    write(&dir.join("layers.jsonl"), &layer_text());
    let out = dir.join("bodies").join(tier);
    let o = run(&[
        "build",
        "--tier",
        tier,
        "--layer-records",
        &s(&dir.join("layers.jsonl")),
        "--seed",
        "3",
        "--out",
        &s(&out),
        &s(&a),
        &s(&b),
    ]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    out
}

#[test]
fn a_clean_path_passes_and_prints_every_line() {
    let dir = scratch("clean");
    let t = sealed(&dir, "gen-check", "check");
    write(
        &dir.join("clean/readme.txt"),
        "nothing here but plain words\n",
    );
    let o = run(&["leak", &s(&t), &s(&dir.join("clean"))]);
    assert_eq!(o.code, 0, "{}{}", o.stdout, o.stderr);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert!(
        lines[0].starts_with("leak ") && lines[0].ends_with(" bodies=0 files=1 canaries_listed=3"),
        "{}",
        lines[0]
    );
    assert_eq!(lines[1], "shared streams=0 sites=0 templates=0 canaries=0");
    assert_eq!(lines[2], "tokens 0");
    assert_eq!(lines.len(), 3 + 11 + 1);
    assert_eq!(lines[3], "share utility_bill_payment 0 2");
    assert_eq!(lines[9], "share shop_order 0 4");
    assert_eq!(lines[13], "share other 0 0");
    assert_eq!(lines[14], "leak PASS");
}

#[test]
fn a_site_name_in_a_file_and_a_development_body_fail() {
    let dir = scratch("hits");
    let t = sealed(&dir, "gen-check", "check");
    write(
        &dir.join("repo/a/notes.md"),
        "We used Goldfinch-Cart once.\n",
    );
    write(&dir.join("repo/target/skip.txt"), "goldfinch-power\n");
    write(&dir.join("repo/.git/skip.txt"), "goldfinch-power\n");
    write(&dir.join("repo/b/target"), "goldfinch-rides\n");
    let o = run(&["leak", &s(&t), &s(&dir.join("repo"))]);
    assert_eq!(o.code, 1);
    assert!(
        o.stdout
            .contains(&format!("HIT site {}/b/target 1\n", s(&dir.join("repo")))),
        "a file named target is searched: {}",
        o.stdout
    );
    assert!(
        o.stdout
            .contains(&format!("HIT site {}/a/notes.md 1\n", s(&dir.join("repo")))),
        "{}",
        o.stdout
    );
    assert!(
        o.stdout
            .contains("shared streams=0 sites=2 templates=0 canaries=0\n"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.ends_with("leak FAIL\n"));
    let o = run(&["leak", &s(&t), &s(&golden())]);
    assert_eq!(o.code, 1);
    assert!(o.stdout.contains(" bodies=1 "), "{}", o.stdout);
    assert!(
        o.stdout
            .contains("shared streams=0 sites=4 templates=5 canaries=0\n"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains("share shop_order 4 4\n"));
}

#[test]
fn the_canaries_of_a_body_of_the_same_author_are_left_out() {
    let dir = scratch("canaries");
    let t = sealed(&dir, "gen-check", "check");
    let h = sealed(&dir, "gen-heldout", "heldout");
    let o = run(&["leak", &s(&t), &s(&h)]);
    assert!(o.stdout.contains("canaries=0\n"), "{}", o.stdout);
    assert!(!o.stdout.contains("HIT canary"));
    assert!(
        !o.stdout.contains("/manifest.toml "),
        "the canary names in the body's own manifest are left out: {}",
        o.stdout
    );
    write(&dir.join("stray/c.txt"), "c-two\n");
    let o = run(&["leak", &s(&t), &s(&h), &s(&dir.join("stray"))]);
    assert!(
        o.stdout
            .contains(&format!("HIT canary {}/c.txt 1\n", s(&dir.join("stray")))),
        "{}",
        o.stdout
    );
    assert!(o.stdout.contains(" canaries=1\n"), "{}", o.stdout);
    assert_eq!(o.code, 1);
    let o = run(&["leak", &s(&t), &s(&dir.join("bodies"))]);
    assert!(
        o.stdout.contains(" bodies=1 "),
        "the tier's own files are skipped: {}",
        o.stdout
    );
}

#[test]
fn leak_refusals() {
    let dir = scratch("refusals");
    let t = sealed(&dir, "gen-check", "check");
    refused(&run(&["leak", &s(&t)]), "usage");
    refused(&run(&["leak", &s(&t), &s(&dir.join("none"))]), "path");
    write(&dir.join("bad/script-manifest.jsonl"), "{}\n");
    refused(&run(&["leak", &s(&t), &s(&dir.join("bad"))]), "body");
    refused(
        &run(&["leak", &s(&golden()), &s(&dir.join("bad"))]),
        "tier-manifest",
    );
}

fn plain_stream(id: &str, sites: &str, site: &str, words: (&str, &str)) -> Vec<String> {
    let mut lines = vec![
        format!(r#"{{"line":"stream","stream":"{id}","sites":"{sites}"}}"#),
        common::step_line(&format!("{id}-a"), site, words.0, false),
        common::step_line(&format!("{id}-b"), site, words.1, false),
    ];
    for day in 1..=3 {
        lines.push(common::episode(
            id,
            &format!("{id}-e{day}"),
            &format!("{id}-t1"),
            "email",
            "planted",
            day,
            &[common::item(
                &format!("{id}-a"),
                0,
                30 * common::SEC,
                1,
                1,
                0,
            )],
        ));
    }
    lines.push(common::episode(
        id,
        &format!("{id}-e9"),
        &format!("{id}-t2"),
        "ride_order",
        "one_off",
        9,
        &[common::item(
            &format!("{id}-b"),
            0,
            30 * common::SEC,
            0,
            1,
            0,
        )],
    ));
    lines
}

fn script_file(path: &Path, lines: &[String]) -> String {
    let text: String = std::iter::once(common::HEADER.to_owned())
        .chain(lines.iter().cloned())
        .map(|l| format!("{l}\n"))
        .collect();
    write(path, &text);
    s(path)
}

#[test]
fn a_development_body_that_holds_a_copy_of_a_stream_fails_and_without_it_passes() {
    let dir = scratch("copied-stream");
    let tier_lines = plain_stream("t1", "check", "teal-one.test", ("Teal page", "Teal form"));
    let tier_script = script_file(&dir.join("tier.jsonl"), &tier_lines);
    write(
        &dir.join("layers.jsonl"),
        "{\"step\":\"t1-a\",\"source\":\"render\",\"accessibility_text\":null,\"extracted_text\":null}\n{\"step\":\"t1-b\",\"source\":\"render\",\"accessibility_text\":null,\"extracted_text\":null}\n",
    );
    let t = dir.join("bodies/gen-check");
    let o = run(&[
        "build",
        "--tier",
        "gen-check",
        "--layer-records",
        &s(&dir.join("layers.jsonl")),
        "--seed",
        "5",
        "--out",
        &s(&t),
        &tier_script,
    ]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let dev_lines = plain_stream("d1", "seen", "dusk-one.test", ("Dusk note", "Dusk list"));
    let dev_script = script_file(&dir.join("dev.jsonl"), &dev_lines);
    let copy_lines = plain_stream("t1", "seen", "teal-one.test", ("Teal page", "Teal form"));
    let copy_script = script_file(&dir.join("copy.jsonl"), &copy_lines);
    let without = dir.join("without");
    let with = dir.join("with");
    assert_eq!(
        run(&["build", "--seed", "8", "--out", &s(&without), &dev_script]).code,
        0
    );
    assert_eq!(
        run(&[
            "build",
            "--seed",
            "8",
            "--out",
            &s(&with),
            &dev_script,
            &copy_script
        ])
        .code,
        0
    );
    let o = run(&["leak", &s(&t), &s(&without)]);
    assert_eq!(o.code, 0, "{}", o.stdout);
    assert!(
        o.stdout
            .contains("shared streams=0 sites=0 templates=0 canaries=0\n"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.ends_with("leak PASS\n"));
    let o = run(&["leak", &s(&t), &s(&with)]);
    assert_eq!(o.code, 1, "{}", o.stdout);
    assert!(o.stdout.contains("shared streams=1 "), "{}", o.stdout);
    assert!(o.stdout.ends_with("leak FAIL\n"));
}

#[test]
fn a_path_below_a_directory_named_target_is_read_and_a_target_inside_a_walk_is_not() {
    let dir = scratch("target-paths").join("target").join("case");
    fs::create_dir_all(&dir).expect("the directory below target");
    assert!(dir.components().any(|c| c.as_os_str() == "target"));
    let tier_lines = plain_stream("t1", "check", "teal-one.test", ("Teal page", "Teal form"));
    let tier_script = script_file(&dir.join("tier.jsonl"), &tier_lines);
    write(
        &dir.join("layers.jsonl"),
        "{\"step\":\"t1-a\",\"source\":\"render\",\"accessibility_text\":null,\"extracted_text\":null}\n{\"step\":\"t1-b\",\"source\":\"render\",\"accessibility_text\":null,\"extracted_text\":null}\n",
    );
    let t = dir.join("bodies/gen-check");
    let o = run(&[
        "build",
        "--tier",
        "gen-check",
        "--layer-records",
        &s(&dir.join("layers.jsonl")),
        "--seed",
        "5",
        "--out",
        &s(&t),
        &tier_script,
    ]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let copy_lines = plain_stream("t1", "seen", "teal-one.test", ("Teal page", "Teal form"));
    let copy_script = script_file(&dir.join("copy.jsonl"), &copy_lines);
    let walk = dir.join("walk");
    let dev = walk.join("target").join("dev");
    assert_eq!(
        run(&["build", "--seed", "8", "--out", &s(&dev), &copy_script]).code,
        0
    );
    let o = run(&["leak", &s(&t), &s(&dev)]);
    assert_eq!(o.code, 1, "{}", o.stdout);
    assert!(o.stdout.contains(" bodies=1 "), "{}", o.stdout);
    assert!(o.stdout.contains("shared streams=1 "), "{}", o.stdout);
    let o = run(&["leak", &s(&t), &s(&walk)]);
    assert_eq!(o.code, 0, "{}", o.stdout);
    assert!(o.stdout.contains(" bodies=0 files=0 "), "{}", o.stdout);
    assert!(o.stdout.ends_with("leak PASS\n"));
}
