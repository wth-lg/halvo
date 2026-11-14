mod common;

use std::fs;

use common::{build_golden, files_under, golden, refused, run, s, scratch, write, HEADER, SEC};
use halvo_sample::build::record_id;
use halvo_sample::manifest::TierManifest;

#[test]
fn golden_build_reproduces_the_committed_files_byte_for_byte() {
    let dir = scratch("golden");
    let out = build_golden(&dir, "g");
    let g = golden();
    let built = files_under(&out);
    let committed: Vec<String> = files_under(&g)
        .into_iter()
        .filter(|f| f != "script.jsonl" && f != "marked.jsonl")
        .collect();
    assert_eq!(built, committed);
    for f in &built {
        assert_eq!(
            fs::read(out.join(f)).expect("built"),
            fs::read(g.join(f)).expect("committed"),
            "{f}"
        );
    }
    assert!(!out.join("manifest.toml").exists());
}

#[test]
fn two_builds_write_the_same_bytes_and_another_seed_other_ids() {
    let dir = scratch("twice");
    let a = build_golden(&dir, "a");
    let b = build_golden(&dir, "b");
    for f in files_under(&a) {
        assert_eq!(fs::read(a.join(&f)).ok(), fs::read(b.join(&f)).ok(), "{f}");
    }
    let g = golden();
    let c = dir.join("c");
    let o = run(&[
        "build",
        "--seed",
        "2",
        "--out",
        &s(&c),
        &s(&g.join("script.jsonl")),
        &s(&g.join("marked.jsonl")),
    ]);
    assert_eq!(o.code, 0);
    let day = "streams/golden/day-003.jsonl";
    assert_ne!(fs::read(a.join(day)).ok(), fs::read(c.join(day)).ok());
    assert_eq!(
        fs::read(a.join("streams/golden/labels.jsonl")).ok(),
        fs::read(c.join("streams/golden/labels.jsonl")).ok()
    );
}

#[test]
fn golden_build_prints_its_counts_rates_and_masks() {
    let dir = scratch("prints");
    let g = golden();
    let o = run(&[
        "build",
        "--seed",
        "1",
        "--out",
        &s(&dir.join("o")),
        &s(&g.join("script.jsonl")),
        &s(&g.join("marked.jsonl")),
    ]);
    let lines: Vec<&str> = o.stdout.lines().collect();
    assert_eq!(lines.len(), 3);
    assert!(lines[0].starts_with("counts streams=1 tasks=5 episodes=15 ranges=41 records="));
    assert!(lines[0].contains(" title_events=41 "));
    assert!(lines[1].starts_with("rates title_changes_per_hour="));
    assert_eq!(lines[2], "masks titles=12 changed=1 false=0");
}

#[test]
fn record_ids_are_version_7_with_the_millisecond_first() {
    let id = record_id(1_767_258_000_412_000_000, u64::MAX, 0);
    assert_eq!(id.len(), 36);
    assert_eq!(&id[14..15], "7");
    assert_eq!(&id[19..20], "8");
    assert_eq!(&id[..13], "019b78c9-0c1c");
    assert_eq!(&record_id(0, 0, u64::MAX)[19..], "bfff-ffffffffffff");
}

fn tier_script(sites: &str, negative: bool) -> String {
    let mut lines = vec![
        HEADER.to_owned(),
        format!(r#"{{"line":"stream","stream":"s1","sites":"{sites}"}}"#),
        common::step_line("pa", "alpha-one.test", "Bills - Alpha", false),
    ];
    for day in 1..=3 {
        lines.push(common::episode(
            "s1",
            &format!("e{day}"),
            "t1",
            "email",
            "planted",
            day,
            &[common::item("pa", 0, 30 * SEC, 1, 1, 1)],
        ));
    }
    if negative {
        lines.push(common::episode(
            "s1",
            "e9",
            "t2",
            "ride_order",
            "one_off",
            9,
            &[common::item("pa", 0, 30 * SEC, 0, 0, 0)],
        ));
    }
    lines.iter().map(|l| format!("{l}\n")).collect()
}

#[test]
fn a_tier_build_writes_its_manifest_with_counts_and_files() {
    let dir = scratch("tier");
    write(&dir.join("s.jsonl"), &tier_script("unseen", true));
    let o = run(&[
        "build",
        "--tier",
        "gen-unseen",
        "--seed",
        "9",
        "--out",
        &s(&dir.join("t")),
        &s(&dir.join("s.jsonl")),
    ]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let text = fs::read(dir.join("t/manifest.toml")).expect("manifest");
    let m = TierManifest::parse(&text).expect("a manifest of C9.3");
    assert_eq!(m.tier, "gen-unseen");
    assert_eq!(m.seed, 9);
    assert_eq!(m.streams[0].0, "gen-unseen-s1");
    assert_eq!(m.counts.planted[4], 1);
    assert_eq!(m.counts.one_off[8], 1);
    let listed: Vec<&str> = m.files.iter().map(|f| f.0.as_str()).collect();
    let mut on_disk = files_under(&dir.join("t"));
    on_disk.retain(|f| f != "manifest.toml");
    assert_eq!(listed, on_disk);
    let hash = halvo_sample::form::sha256_hex(&text);
    assert_eq!(
        o.stdout.lines().last(),
        Some(format!("manifest {hash}").as_str())
    );
}

#[test]
fn a_tier_build_refuses_other_sites_and_a_stream_with_no_negative() {
    let dir = scratch("tier-rules");
    write(&dir.join("a.jsonl"), &tier_script("seen", true));
    refused(
        &run(&[
            "build",
            "--tier",
            "gen-unseen",
            "--seed",
            "1",
            "--out",
            &s(&dir.join("x")),
            &s(&dir.join("a.jsonl")),
        ]),
        "script",
    );
    write(&dir.join("b.jsonl"), &tier_script("unseen", false));
    refused(
        &run(&[
            "build",
            "--tier",
            "gen-unseen",
            "--seed",
            "1",
            "--out",
            &s(&dir.join("y")),
            &s(&dir.join("b.jsonl")),
        ]),
        "script",
    );
    let o = run(&[
        "build",
        "--seed",
        "1",
        "--out",
        &s(&dir.join("z")),
        &s(&dir.join("b.jsonl")),
    ]);
    assert_eq!(o.code, 0, "without --tier the negative rule does not apply");
    assert!(!dir.join("x").exists() && !dir.join("y").exists());
}

#[test]
fn build_refusals() {
    let dir = scratch("refusals");
    let g = golden();
    let script = s(&g.join("script.jsonl"));
    let marked = s(&g.join("marked.jsonl"));
    let out = s(&dir.join("o"));
    refused(&run(&["build", "--seed", "1", &script]), "usage");
    refused(
        &run(&["build", "--seed", "-1", "--out", &out, &script]),
        "usage",
    );
    refused(
        &run(&[
            "build",
            "--seed",
            "9223372036854775808",
            "--out",
            &out,
            &script,
        ]),
        "usage",
    );
    refused(
        &run(&["build", "--seed", "01", "--out", &out, &script]),
        "usage",
    );
    refused(
        &run(&[
            "build", "--seed", "1", "--seed", "1", "--out", &out, &script,
        ]),
        "usage",
    );
    refused(
        &run(&[
            "build",
            "--tier",
            "gen-dev",
            "--layer-records",
            &script,
            "--seed",
            "1",
            "--out",
            &out,
            &script,
        ]),
        "usage",
    );
    refused(
        &run(&[
            "build",
            "--tier",
            "gen-check",
            "--seed",
            "1",
            "--out",
            &out,
            &script,
        ]),
        "usage",
    );
    refused(
        &run(&[
            "build", "--tier", "rec-dev", "--seed", "1", "--out", &out, &script,
        ]),
        "usage",
    );
    refused(
        &run(&[
            "build",
            "--seed",
            "1",
            "--out",
            &out,
            &s(&dir.join("missing.jsonl")),
        ]),
        "script",
    );
    write(
        &dir.join("bad.jsonl"),
        &format!("{HEADER}\n{{\"line\":\"stream\",\"stream\":\"S1\",\"sites\":\"seen\"}}\n"),
    );
    refused(
        &run(&[
            "build",
            "--seed",
            "1",
            "--out",
            &out,
            &s(&dir.join("bad.jsonl")),
        ]),
        "script",
    );
    write(&dir.join("layers.jsonl"), "{\"step\":\"g-cart-pay\",\"source\":\"render\",\"accessibility_text\":null,\"extracted_text\":null}\n");
    let tiered = dir.join("tiered.jsonl");
    write(
        &tiered,
        &fs::read_to_string(g.join("script.jsonl"))
            .expect("golden")
            .replace("\"seen\"", "\"check\""),
    );
    let tmarked = dir.join("tmarked.jsonl");
    write(
        &tmarked,
        &fs::read_to_string(g.join("marked.jsonl"))
            .expect("golden")
            .replace("\"seen\"", "\"check\""),
    );
    refused(
        &run(&[
            "build",
            "--tier",
            "gen-check",
            "--layer-records",
            &s(&dir.join("layers.jsonl")),
            "--seed",
            "1",
            "--out",
            &out,
            &s(&tiered),
            &s(&tmarked),
        ]),
        "layer-records",
    );
    refused(
        &run(&[
            "build",
            "--tier",
            "gen-check",
            "--layer-records",
            &s(&dir.join("none.jsonl")),
            "--seed",
            "1",
            "--out",
            &out,
            &s(&tiered),
            &s(&tmarked),
        ]),
        "layer-records",
    );
    assert!(!dir.join("o").exists());
    fs::create_dir_all(dir.join("o")).expect("dir");
    refused(
        &run(&["build", "--seed", "1", "--out", &out, &script, &marked]),
        "exists",
    );
}

#[test]
fn a_sealed_tier_copies_its_layer_records() {
    let dir = scratch("sealed");
    let g = golden();
    let tiered = dir.join("tiered.jsonl");
    write(
        &tiered,
        &fs::read_to_string(g.join("script.jsonl"))
            .expect("golden")
            .replace("\"seen\"", "\"check\""),
    );
    let tmarked = dir.join("tmarked.jsonl");
    write(
        &tmarked,
        &fs::read_to_string(g.join("marked.jsonl"))
            .expect("golden")
            .replace("\"seen\"", "\"check\""),
    );
    let steps = [
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
    let text: String = steps
        .iter()
        .map(|st| format!("{{\"step\":\"{st}\",\"source\":\"render\",\"accessibility_text\":\"a\",\"extracted_text\":null}}\n"))
        .collect();
    write(&dir.join("layers.jsonl"), &text);
    let o = run(&[
        "build",
        "--tier",
        "gen-check",
        "--layer-records",
        &s(&dir.join("layers.jsonl")),
        "--seed",
        "1",
        "--out",
        &s(&dir.join("t")),
        &s(&tiered),
        &s(&tmarked),
    ]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    assert_eq!(
        fs::read_to_string(dir.join("t/layers/steps.jsonl")).ok(),
        Some(text)
    );
    let m = TierManifest::parse(&fs::read(dir.join("t/manifest.toml")).expect("m")).expect("parse");
    assert!(m.files.iter().any(|f| f.0 == "layers/steps.jsonl"));
    assert_eq!(m.counts.marked[7], 1);
}

#[test]
fn a_click_comes_before_a_scroll_on_the_same_second() {
    let dir = scratch("same-second");
    let lines = [
        HEADER.to_owned(),
        r#"{"line":"stream","stream":"s1","sites":"seen"}"#.to_owned(),
        common::step_line("pa", "alpha-one.test", "Bills - Alpha", false),
        common::episode(
            "s1",
            "e1",
            "t1",
            "email",
            "one_off",
            1,
            &[common::item("pa", 0, 2 * SEC, 0, 1, 1)],
        ),
    ];
    write(
        &dir.join("s.jsonl"),
        &lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
    );
    let o = run(&[
        "build",
        "--seed",
        "1",
        "--out",
        &s(&dir.join("o")),
        &s(&dir.join("s.jsonl")),
    ]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let day = fs::read_to_string(dir.join("o/streams/s1/day-001.jsonl")).expect("day");
    let kinds: Vec<&str> = day
        .lines()
        .map(|l| {
            if l.contains("\"class\":\"click\"") {
                "click"
            } else if l.contains("\"class\":\"scroll\"") {
                "scroll"
            } else {
                "other"
            }
        })
        .collect();
    assert_eq!(kinds, vec!["other", "other", "click", "scroll"]);
    assert!(day
        .lines()
        .nth(2)
        .is_some_and(|l| l.contains("\"at\":1767225601000000000")));
    assert!(day
        .lines()
        .nth(3)
        .is_some_and(|l| l.contains("\"at\":1767225601000000000")));
}

#[test]
fn the_seed_mark_of_a_step_is_true_only_where_an_episode_of_a_seed_category_names_it() {
    let dir = scratch("seed-mark");
    let id = "s1";
    let item = |step: &str| common::item(step, 0, 30 * SEC, 0, 1, 0);
    let mut lines = vec![
        format!(r#"{{"line":"stream","stream":"{id}","sites":"seen"}}"#),
        common::step_line("s1-a", "amber-one.test", "Amber page", false),
        common::step_line("s1-b", "amber-one.test", "Amber form", false),
        common::step_line("s1-c", "amber-two.test", "Amber news", false),
        common::step_line("s1-d", "amber-three.test", "Amber feed", false),
    ];
    for day in 1..=3 {
        lines.push(common::episode(
            id,
            &format!("s1-e{day}"),
            "s1-t1",
            "email",
            "planted",
            day,
            &[item("s1-a")],
        ));
    }
    for day in 4..=6 {
        lines.push(common::episode(
            id,
            &format!("s1-e{day}"),
            "s1-t2",
            "other",
            "non_seed",
            day,
            &[
                common::item("s1-c", 0, 30 * SEC, 0, 1, 0),
                common::item("s1-a", 40 * SEC, 70 * SEC, 0, 1, 0),
            ],
        ));
    }
    lines.push(common::episode(
        id,
        "s1-e8",
        "s1-t3",
        "ride_order",
        "one_off",
        8,
        &[item("s1-b")],
    ));
    lines.push(format!(
        r#"{{"line":"noise","stream":"{id}","day":9,"item":{}}}"#,
        item("s1-d")
    ));
    let text: String = std::iter::once(HEADER.to_owned())
        .chain(lines)
        .map(|l| format!("{l}\n"))
        .collect();
    write(&dir.join("script.jsonl"), &text);
    let out = dir.join("out");
    let o = run(&[
        "build",
        "--seed",
        "1",
        "--out",
        &s(&out),
        &s(&dir.join("script.jsonl")),
    ]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let manifest = fs::read_to_string(out.join("script-manifest.jsonl")).expect("manifest");
    let marks: Vec<(String, bool)> = manifest
        .lines()
        .map(|l| {
            let step = l.split('"').nth(3).expect("step").to_owned();
            let mark = l.ends_with(",\"seed_category\":true}");
            assert!(mark || l.ends_with(",\"seed_category\":false}"), "{l}");
            (step, mark)
        })
        .collect();
    assert_eq!(
        marks,
        vec![
            ("s1-a".to_owned(), true),
            ("s1-b".to_owned(), true),
            ("s1-c".to_owned(), false),
            ("s1-d".to_owned(), false),
        ]
    );
    let o = run(&["check-labels", &s(&out)]);
    assert_eq!(o.code, 0, "{}", o.stdout);
}

#[test]
fn a_script_file_of_another_suffix_a_dot_name_or_a_directory_is_refused() {
    let dir = scratch("script-names");
    let g = golden();
    let script = fs::read_to_string(g.join("script.jsonl")).expect("golden");
    let marked = fs::read_to_string(g.join("marked.jsonl")).expect("golden");
    write(&dir.join("s/marked.jsonl"), &marked);
    for name in [
        "script.json",
        "script.jsonl.txt",
        "script.JSONL",
        ".script.jsonl",
        ".jsonl",
    ] {
        write(&dir.join("s").join(name), &script);
        let o = common::run_in(
            &dir.join("s"),
            &[
                "build",
                "--seed",
                "1",
                "--out",
                &s(&dir.join(format!("out-{name}"))),
                name,
                "marked.jsonl",
            ],
        );
        refused(&o, "script");
        assert_eq!(o.stderr, "REFUSED script: SCRIPT 1: not a script file\n");
        assert!(!dir.join(format!("out-{name}")).exists());
    }
    fs::create_dir_all(dir.join("s/sub.jsonl")).expect("a directory");
    let o = common::run_in(
        &dir.join("s"),
        &[
            "build",
            "--seed",
            "1",
            "--out",
            &s(&dir.join("out-sub")),
            "marked.jsonl",
            "sub.jsonl",
        ],
    );
    refused(&o, "script");
    assert_eq!(o.stderr, "REFUSED script: SCRIPT 2: not a script file\n");
    write(&dir.join("s/zz-mill.test.jsonl"), "not json\n");
    let o = common::run_in(
        &dir.join("s"),
        &[
            "build",
            "--seed",
            "1",
            "--out",
            &s(&dir.join("out-bad-line")),
            "marked.jsonl",
            "zz-mill.test.jsonl",
        ],
    );
    refused(&o, "script");
    assert_eq!(o.stderr, "REFUSED script: SCRIPT 2:1: S15\n");
    let o = common::run_in(
        &dir.join("s"),
        &[
            "build",
            "--seed",
            "1",
            "--out",
            &s(&dir.join("out-missing")),
            "marked.jsonl",
            "zz-gone.test.jsonl",
        ],
    );
    assert_eq!(o.stderr, "REFUSED script: SCRIPT 2: not a script file\n");
    refused(
        &common::run_in(
            &dir.join("s"),
            &[
                "build",
                "--seed",
                "1",
                "--out",
                &s(&dir.join("out-dir")),
                "sub.jsonl",
                "marked.jsonl",
            ],
        ),
        "script",
    );
    write(&dir.join("s/script.jsonl"), &script);
    let o = common::run_in(
        &dir.join("s"),
        &[
            "build",
            "--seed",
            "1",
            "--out",
            &s(&dir.join("out-good")),
            "./script.jsonl",
            "marked.jsonl",
        ],
    );
    assert_eq!(o.code, 0, "{}", o.stderr);
}
