mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{golden, refused, run, s, scratch, write};

const CANARIES: &str = r#"["c-one.test","c-three.test","c-two.test"]"#;

fn layer_text() -> String {
    [
        "g-agenda-clear", "g-agenda-week", "g-budget-sheet", "g-cart-basket", "g-cart-confirm", "g-cart-pay",
        "g-cart-receipt", "g-cart-signin", "g-power-bill", "g-power-signin", "g-rides-fares", "g-rides-signin",
    ]
    .iter()
    .map(|st| format!("{{\"step\":\"{st}\",\"source\":\"render\",\"accessibility_text\":null,\"extracted_text\":null}}\n"))
    .collect()
}

/// The golden script built as a sealed tier with three canaries in its header.
fn sealed(dir: &Path, tier: &str, sites: &str) -> PathBuf {
    let g = golden();
    let fix = |t: String| {
        t.replace("\"seen\"", &format!("\"{sites}\""))
            .replace("\"canaries\":[]", &format!("\"canaries\":{CANARIES}"))
    };
    let a = dir.join(format!("{tier}-script.jsonl"));
    let b = dir.join(format!("{tier}-marked.jsonl"));
    write(
        &a,
        &fix(fs::read_to_string(g.join("script.jsonl")).expect("g")),
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
    let o = run(&["leak", &s(&t), &s(&dir.join("repo"))]);
    assert_eq!(o.code, 1);
    assert!(
        o.stdout
            .contains(&format!("HIT site {}/a/notes.md 1\n", s(&dir.join("repo")))),
        "{}",
        o.stdout
    );
    assert!(
        o.stdout
            .contains("shared streams=0 sites=1 templates=0 canaries=0\n"),
        "{}",
        o.stdout
    );
    assert!(o.stdout.ends_with("leak FAIL\n"));
    let o = run(&["leak", &s(&t), &s(&golden())]);
    assert_eq!(o.code, 1);
    assert!(o.stdout.contains(" bodies=1 "), "{}", o.stdout);
    assert!(
        o.stdout
            .contains("shared streams=1 sites=4 templates=5 canaries=0\n"),
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
