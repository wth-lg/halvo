mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{copy_dir, files_under, golden, refused, repo, run, s, scratch, write};
use halvo_sample::form::sha256_hex;
use halvo_sample::manifest::TierManifest;

const STEPS: [&str; 12] = [
    "g-agenda-clear",
    "g-agenda-week",
    "g-budget-sheet",
    "g-cart-basket",
    "g-cart-confirm",
    "g-cart-pay",
    "g-cart-receipt",
    "g-cart-signin",
    "g-power-bill",
    "g-power-signin",
    "g-rides-fares",
    "g-rides-signin",
];

fn layer_text() -> String {
    STEPS
        .iter()
        .map(|st| format!("{{\"step\":\"{st}\",\"source\":\"render\",\"accessibility_text\":\"a {st}\",\"extracted_text\":\"x {st}\"}}\n"))
        .collect()
}

/// The golden script built as a tier: gen-dev, or gen-check with layer records.
fn tier(dir: &Path, sealed: bool) -> PathBuf {
    let g = golden();
    let (name, sites) = if sealed {
        ("gen-check", "check")
    } else {
        ("gen-dev", "seen")
    };
    let a = dir.join("script.jsonl");
    let b = dir.join("marked.jsonl");
    write(
        &a,
        &fs::read_to_string(g.join("script.jsonl"))
            .expect("g")
            .replace("\"seen\"", &format!("\"{sites}\"")),
    );
    write(
        &b,
        &fs::read_to_string(g.join("marked.jsonl"))
            .expect("g")
            .replace("\"seen\"", &format!("\"{sites}\"")),
    );
    write(&dir.join("layers.jsonl"), &layer_text());
    let out = dir.join(name);
    let mut args = vec!["build".to_owned(), "--tier".to_owned(), name.to_owned()];
    if sealed {
        args.extend(["--layer-records".to_owned(), s(&dir.join("layers.jsonl"))]);
    }
    args.extend([
        "--seed".to_owned(),
        "4".to_owned(),
        "--out".to_owned(),
        s(&out),
        s(&a),
        s(&b),
    ]);
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    assert_eq!(run(&a).code, 0);
    out
}

fn app_title() -> String {
    s(&repo().join("docs/r2/input-sets/app-title.toml"))
}

#[test]
fn an_app_and_title_copy_holds_the_reduced_batches_and_nothing_else() {
    let dir = scratch("copy");
    let t = tier(&dir, false);
    let out = dir.join("copy");
    let o = run(&["input-copy", "--input-set", &app_title(), &s(&t), &s(&out)]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let hash = sha256_hex(&fs::read(t.join("manifest.toml")).expect("m"));
    assert_eq!(
        fs::read_to_string(out.join("manifest.sha256")).ok(),
        Some(format!("{hash}\n"))
    );
    let files = files_under(&out);
    let days: Vec<String> = files_under(&t)
        .into_iter()
        .filter(|f| f.contains("/day-"))
        .collect();
    let mut want = days.clone();
    want.insert(0, "manifest.sha256".to_owned());
    assert_eq!(files, want);
    let (mut read, mut written, mut keys_in_tier, mut sites_in_tier) = (0, 0, 0, 0);
    for d in &days {
        let original = fs::read_to_string(t.join(d)).expect("day");
        read += original.lines().count();
        keys_in_tier += original.matches("\"kind\":\"keystroke\"").count()
            + original.matches("\"kind\":\"pointer\"").count();
        sites_in_tier += original.matches("\"site\":").count();
        let copy = fs::read_to_string(out.join(d)).expect("copy");
        for line in copy.lines() {
            written += 1;
            let ok = line.contains("\"kind\":\"foreground_app\"")
                || line.contains("\"kind\":\"window_title\"")
                || line.contains("\"kind\":\"run_mark\"");
            assert!(ok, "{line}");
            assert!(
                !line.contains("\"cut\"")
                    && !line.contains("\"why\"")
                    && !line.contains("\"site\""),
                "{line}"
            );
            assert!(original.contains(&line[..60]), "envelope unchanged");
        }
    }
    assert!(
        keys_in_tier > 0 && sites_in_tier > 0,
        "the control: the tier holds what the set takes out"
    );
    assert_eq!(o.stdout, format!("input-copy {hash} streams=1 day_files={} records_read={read} records_written={written} layer_records=0\n", days.len()));
    refused(
        &run(&["input-copy", "--input-set", &app_title(), &s(&t), &s(&out)]),
        "exists",
    );
}

#[test]
fn a_layer_set_writes_records_by_stream_and_instant() {
    let dir = scratch("layers");
    let t = tier(&dir, true);
    let set = dir.join("set.toml");
    write(&set, "envelope = [\"app\"]\nlayers = [\"accessibility_text\"]\n[kinds]\nforeground_app = []\nwindow_title = [\"text\"]\n");
    let out = dir.join("copy");
    let o = run(&["input-copy", "--input-set", &s(&set), &s(&t), &s(&out)]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert!(o.stdout.ends_with(" layer_records=41\n"), "{}", o.stdout);
    let text = fs::read_to_string(out.join("layers/instants.jsonl")).expect("instants");
    assert_eq!(text.lines().count(), 41);
    let first = text.lines().next().expect("line");
    assert!(first.starts_with("{\"stream\":\"gen-check-golden\",\"at\":1767430800000000000,\"source\":\"render\",\"accessibility_text\":\"a g-cart-signin\"}"), "{first}");
    assert!(!text.contains("extracted_text") && !text.contains("\"step\""));
    let ats: Vec<i64> = text
        .lines()
        .map(|l| {
            l.split("\"at\":")
                .nth(1)
                .and_then(|r| r.split(',').next())
                .and_then(|n| n.parse().ok())
                .expect("at")
        })
        .collect();
    assert!(ats.windows(2).all(|w| w[0] < w[1]));
    for f in files_under(&out) {
        assert!(
            f == "manifest.sha256" || f == "layers/instants.jsonl" || f.contains("/day-"),
            "{f}"
        );
    }
}

#[test]
fn layers_from_a_directory_are_checked_against_the_layer_manifest() {
    let dir = scratch("layer-dir");
    let t = tier(&dir, false);
    let set = dir.join("set.toml");
    write(&set, "envelope = [\"app\"]\nlayers = [\"extracted_text\"]\n[kinds]\nforeground_app = []\nwindow_title = [\"text\"]\n");
    let layers = dir.join("dev-layers");
    write(&layers.join("gen-dev/steps.jsonl"), &layer_text());
    let hash = sha256_hex(layer_text().as_bytes());
    write(
        &layers.join("manifest.toml"),
        &format!("version = 1\n[[file]]\npath = \"gen-dev/steps.jsonl\"\nsha256 = \"{hash}\"\n"),
    );
    let o = run(&[
        "input-copy",
        "--input-set",
        &s(&set),
        "--layers",
        &s(&layers.join("gen-dev")),
        &s(&t),
        &s(&dir.join("a")),
    ]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    write(
        &layers.join("manifest.toml"),
        &format!(
            "version = 1\n[[file]]\npath = \"gen-dev/steps.jsonl\"\nsha256 = \"{}\"\n",
            "0".repeat(64)
        ),
    );
    refused(
        &run(&[
            "input-copy",
            "--input-set",
            &s(&set),
            "--layers",
            &s(&layers.join("gen-dev")),
            &s(&t),
            &s(&dir.join("b")),
        ]),
        "layers",
    );
    refused(
        &run(&[
            "input-copy",
            "--input-set",
            &s(&set),
            &s(&t),
            &s(&dir.join("c")),
        ]),
        "layers",
    );
    assert!(!dir.join("b").exists() && !dir.join("c").exists());
}

#[test]
fn input_copy_refusals() {
    let dir = scratch("refusals");
    let t = tier(&dir, false);
    refused(&run(&["input-copy", &s(&t), &s(&dir.join("o"))]), "usage");
    refused(
        &run(&["input-copy", "--input-set", &app_title(), &s(&t)]),
        "usage",
    );
    refused(
        &run(&[
            "input-copy",
            "--input-set",
            &s(&dir.join("none.toml")),
            &s(&t),
            &s(&dir.join("o")),
        ]),
        "input-set",
    );
    let changed = dir.join("changed");
    copy_dir(&t, &changed);
    let day = changed.join("streams/gen-dev-golden/day-003.jsonl");
    let text = fs::read_to_string(&day).expect("day");
    fs::write(&day, text.replacen("\"seq\":0", "\"seq\":9", 1)).expect("w");
    refused(
        &run(&[
            "input-copy",
            "--input-set",
            &app_title(),
            &s(&changed),
            &s(&dir.join("o")),
        ]),
        "tier-manifest",
    );
    let bad = text.replacen("\"v\":1", "\"v\":3", 1);
    fs::write(&day, &bad).expect("w");
    let mp = changed.join("manifest.toml");
    let mut m = TierManifest::parse(&fs::read(&mp).expect("m")).expect("parse");
    for f in &mut m.files {
        if f.0 == "streams/gen-dev-golden/day-003.jsonl" {
            f.1 = sha256_hex(bad.as_bytes());
        }
    }
    fs::write(&mp, m.write()).expect("w");
    refused(
        &run(&[
            "input-copy",
            "--input-set",
            &app_title(),
            &s(&changed),
            &s(&dir.join("o")),
        ]),
        "tier-files",
    );
    assert!(!dir.join("o").exists());
}

#[test]
fn an_input_copy_keeps_every_run_mark_of_its_tier() {
    let dir = scratch("marks");
    let t = tier(&dir, false);
    let out = dir.join("copy");
    let o = run(&["input-copy", "--input-set", &app_title(), &s(&t), &s(&out)]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let count = |root: &Path, edge: &str| -> usize {
        files_under(root)
            .iter()
            .filter(|f| f.contains("/day-"))
            .map(|f| {
                fs::read_to_string(root.join(f))
                    .expect("day")
                    .lines()
                    .filter(|l| {
                        l.contains("\"kind\":\"run_mark\"")
                            && l.contains(&format!("\"p\":{{\"edge\":\"{edge}\"}}"))
                    })
                    .count()
            })
            .sum()
    };
    for edge in ["start", "end"] {
        let in_tier = count(&t, edge);
        assert_eq!(in_tier, 1, "{edge}");
        assert_eq!(count(&out, edge), in_tier, "{edge}");
    }
}

fn rehash(tier: &Path, rel: &str, text: &str) {
    fs::write(tier.join(rel), text).expect("w");
    let mp = tier.join("manifest.toml");
    let mut m = TierManifest::parse(&fs::read(&mp).expect("m")).expect("parse");
    let entry = m.files.iter_mut().find(|f| f.0 == rel).expect("listed");
    entry.1 = sha256_hex(text.as_bytes());
    fs::write(&mp, m.write()).expect("w");
}

#[test]
fn the_tier_files_refusal_of_input_copy_names_the_tier_argument_alone() {
    let dir = scratch("tier-files-detail");
    let t = tier(&dir, true);
    let layer_set = dir.join("set.toml");
    write(&layer_set, "envelope = [\"app\"]\nlayers = [\"accessibility_text\"]\n[kinds]\nforeground_app = []\nwindow_title = [\"text\"]\n");
    let stream = files_under(&t)
        .into_iter()
        .find(|f| f.ends_with("/labels.jsonl"))
        .expect("a label file");
    let day = files_under(&t)
        .into_iter()
        .find(|f| f.starts_with("streams/") && f.contains("/day-"))
        .expect("a day file");
    let bad_day = fs::read_to_string(t.join(&day))
        .expect("day")
        .replacen("\"v\":1", "\"v\":3", 1);
    let cases: [(&str, &str, String, &Path); 4] = [
        (
            "labels",
            stream.as_str(),
            "{\"no\":\"row\"}\n".to_owned(),
            &layer_set,
        ),
        (
            "steps",
            "script-manifest.jsonl",
            "{\"no\":\"step\"}\n".to_owned(),
            &layer_set,
        ),
        ("day", day.as_str(), bad_day.clone(), Path::new("")),
        ("day-layers", day.as_str(), bad_day, &layer_set),
    ];
    for (name, rel, text, set) in cases {
        let c = dir.join(name);
        copy_dir(&t, &c);
        rehash(&c, rel, &text);
        let set = if set.as_os_str().is_empty() {
            app_title()
        } else {
            s(set)
        };
        let o = run(&[
            "input-copy",
            "--input-set",
            &set,
            &s(&c),
            &s(&dir.join("o")),
        ]);
        refused(&o, "tier-files");
        assert_eq!(
            o.stderr,
            format!("REFUSED tier-files: {}\n", s(&c)),
            "{name}"
        );
    }
    assert!(!dir.join("o").exists());
}

#[cfg(unix)]
#[test]
fn the_output_refusal_of_input_copy_and_build_names_the_dir_argument_alone() {
    let dir = scratch("output-detail");
    let t = tier(&dir, false);
    let umask = |args: &[String]| -> common::Out {
        let o = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("umask 0277 && exec \"$0\" \"$@\"")
            .arg(env!("CARGO_BIN_EXE_halvo-sample"))
            .args(args)
            .output()
            .expect("sh runs");
        common::Out {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }
    };
    let copy = dir.join("copy");
    let o = umask(&[
        "input-copy".to_owned(),
        "--input-set".to_owned(),
        app_title(),
        s(&t),
        s(&copy),
    ]);
    refused(&o, "output");
    assert_eq!(o.stderr, format!("REFUSED output: {}\n", s(&copy)));
    let built = dir.join("built");
    let o = umask(&[
        "build".to_owned(),
        "--tier".to_owned(),
        "gen-dev".to_owned(),
        "--seed".to_owned(),
        "4".to_owned(),
        "--out".to_owned(),
        s(&built),
        s(&dir.join("script.jsonl")),
        s(&dir.join("marked.jsonl")),
    ]);
    refused(&o, "output");
    assert_eq!(o.stderr, format!("REFUSED output: {}\n", s(&built)));
}

#[test]
fn the_layers_refusal_names_the_tier_or_layers_argument_alone() {
    let dir = scratch("layers-detail");
    let layer_set = dir.join("set.toml");
    write(&layer_set, "envelope = [\"app\"]\nlayers = [\"accessibility_text\"]\n[kinds]\nforeground_app = []\nwindow_title = [\"text\"]\n");
    let set = s(&layer_set);
    let expect = |o: common::Out, arg: &Path| {
        refused(&o, "layers");
        assert_eq!(o.stderr, format!("REFUSED layers: {}\n", s(arg)));
    };
    let open = tier(&dir.join("open"), false);
    let copy = |tier: &Path, extra: &[&str]| {
        let mut a = vec![
            "input-copy".to_owned(),
            "--input-set".to_owned(),
            set.clone(),
        ];
        a.extend(extra.iter().map(|e| (*e).to_owned()));
        a.extend([s(tier), s(&dir.join("o"))]);
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        run(&a)
    };
    expect(copy(&open, &[]), &open);
    let sealed = tier(&dir.join("sealed"), true);
    let broken = dir.join("broken");
    copy_dir(&sealed, &broken);
    rehash(&broken, "layers/steps.jsonl", "{\"no\":\"layer\"}\n");
    expect(copy(&broken, &[]), &broken);
    let absent = dir.join("absent-place").join("gen-dev");
    fs::create_dir_all(&absent).expect("an empty layer directory");
    expect(copy(&open, &["--layers", &s(&absent)]), &absent);
    let malformed = dir.join("malformed-place").join("gen-dev");
    write(&malformed.join("steps.jsonl"), "{\"no\":\"layer\"}\n");
    expect(copy(&open, &["--layers", &s(&malformed)]), &malformed);
    assert!(!dir.join("o").exists());
}

#[cfg(target_os = "macos")]
fn long_dir(base: &Path, total: usize) -> PathBuf {
    let mut dir = base.to_path_buf();
    while s(&dir).len() + 1 < total {
        let room = total - s(&dir).len() - 1;
        dir.push("d".repeat(room.min(200)));
    }
    assert_eq!(s(&dir).len(), total);
    dir
}

#[cfg(target_os = "macos")]
#[test]
fn a_stream_directory_that_cannot_be_made_is_refused_by_the_dir_argument_alone() {
    let dir = scratch("dir-make");
    let t = tier(&dir, false);
    let stream = files_under(&t)
        .into_iter()
        .find(|f| f.starts_with("streams/") && f.contains("/day-"))
        .and_then(|f| f.split('/').nth(1).map(str::to_owned))
        .expect("a stream directory");
    let fits = |d: &Path, rel: &str| s(&d.join(rel)).len() <= 1023;
    let copy = long_dir(&dir.join("c"), 1001);
    assert!(fits(&copy, "manifest.sha256") && !fits(&copy, &format!("streams/{stream}")));
    let o = run(&["input-copy", "--input-set", &app_title(), &s(&t), &s(&copy)]);
    refused(&o, "output");
    assert_eq!(o.stderr, format!("REFUSED output: {}\n", s(&copy)));
    assert!(copy.join("manifest.sha256").is_file());
    let built = long_dir(&dir.join("b"), 1001);
    assert!(fits(&built, "script-manifest.jsonl") && !fits(&built, &format!("streams/{stream}")));
    let o = run(&[
        "build",
        "--tier",
        "gen-dev",
        "--seed",
        "4",
        "--out",
        &s(&built),
        &s(&dir.join("script.jsonl")),
        &s(&dir.join("marked.jsonl")),
    ]);
    refused(&o, "output");
    assert_eq!(o.stderr, format!("REFUSED output: {}\n", s(&built)));
    assert!(built.join("script-manifest.jsonl").is_file());
}

fn without_entry(tier: &Path, rel: &str) {
    let mp = tier.join("manifest.toml");
    let mut m = TierManifest::parse(&fs::read(&mp).expect("m")).expect("parse");
    let before = m.files.len();
    m.files.retain(|f| f.0 != rel);
    assert_eq!(m.files.len() + 1, before, "{rel}");
    fs::write(&mp, m.write()).expect("w");
}

#[test]
fn the_tier_manifest_refusal_names_the_tier_argument_and_a_count_alone() {
    let dir = scratch("tier-detail");
    let t = tier(&dir, true);
    let walk = dir.join("walk");
    write(&walk.join("notes.md"), "nothing here\n");
    let layer_set = dir.join("set.toml");
    write(&layer_set, "envelope = [\"app\"]\nlayers = [\"accessibility_text\"]\n[kinds]\nforeground_app = []\nwindow_title = [\"text\"]\n");
    let expect = |tier: &Path, detail: &str| {
        let arg = s(tier);
        let want = format!("REFUSED tier-manifest: {arg}{detail}\n");
        for o in [
            run(&[
                "input-copy",
                "--input-set",
                &app_title(),
                &arg,
                &s(&dir.join("o")),
            ]),
            run(&["leak", &arg, &s(&walk)]),
        ] {
            refused(&o, "tier-manifest");
            assert_eq!(o.stderr, want);
        }
    };
    let listed: Vec<String> = files_under(&t)
        .into_iter()
        .filter(|f| f.starts_with("streams/"))
        .collect();
    assert!(listed.len() >= 3, "{listed:?}");
    let changed = dir.join("changed");
    copy_dir(&t, &changed);
    for f in &listed[..3] {
        let p = changed.join(f);
        let mut bytes = fs::read(&p).expect("listed file");
        bytes.push(b'\n');
        fs::write(&p, bytes).expect("w");
    }
    expect(&changed, " mismatched=3");
    let gone = dir.join("gone");
    copy_dir(&t, &gone);
    fs::rename(gone.join(&listed[0]), dir.join("moved-day")).expect("move a listed file");
    expect(&gone, " mismatched=1");
    let none = dir.join("none");
    copy_dir(&t, &none);
    fs::rename(none.join("manifest.toml"), dir.join("moved-manifest")).expect("move");
    expect(&none, "");
    let bad = dir.join("bad");
    copy_dir(&t, &bad);
    write(&bad.join("manifest.toml"), "tier = \n");
    expect(&bad, "");
    for (name, rel) in [
        ("no-steps", "script-manifest.jsonl"),
        ("no-layers", "layers/steps.jsonl"),
    ] {
        let c = dir.join(name);
        copy_dir(&t, &c);
        without_entry(&c, rel);
        let o = run(&[
            "input-copy",
            "--input-set",
            &s(&layer_set),
            &s(&c),
            &s(&dir.join("o")),
        ]);
        refused(&o, "tier-manifest");
        assert_eq!(o.stderr, format!("REFUSED tier-manifest: {}\n", s(&c)));
    }
    assert!(!dir.join("o").exists());
}
