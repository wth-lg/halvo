use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const FILES: [&str; 4] = [
    "vectors.jsonl",
    "answers.jsonl",
    "body.schema.json",
    "tier-limits.tsv",
];

fn gateway() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/r6/gateway")
}

fn lines_of(file: &str) -> usize {
    fs::read_to_string(gateway().join(file))
        .unwrap()
        .lines()
        .count()
}

fn fresh(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("gateway-check");
    fs::create_dir_all(&root).unwrap();
    loop {
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = root.join(format!("{label}-{}-{n}", std::process::id()));
        if fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
}

fn fixture(label: &str) -> PathBuf {
    let dir = fresh(label);
    for file in FILES {
        fs::copy(gateway().join(file), dir.join(file)).unwrap();
    }
    dir
}

fn edit(dir: &Path, file: &str, change: impl FnOnce(String) -> String) {
    let path = dir.join(file);
    let text = fs::read_to_string(&path).unwrap();
    let changed = change(text.clone());
    assert_ne!(changed, text, "the edit of {file} changed nothing");
    fs::write(path, changed).unwrap();
}

fn edit_line(dir: &Path, file: &str, n: usize, change: impl FnOnce(&str) -> String) {
    edit(dir, file, |text| {
        let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        lines[n - 1] = change(&lines[n - 1]);
        lines.join("\n")
    });
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_gateway-check"))
        .args(args)
        .output()
        .unwrap()
}

fn check(dir: &Path) -> Output {
    run(&["--dir", dir.to_str().unwrap()])
}

fn check_names(dir: &Path, names: &str) -> Output {
    let list = fresh("names").join("names.txt");
    fs::write(&list, names).unwrap();
    run(&[
        "--dir",
        dir.to_str().unwrap(),
        "--names",
        list.to_str().unwrap(),
    ])
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn line(ok_shift: usize, answers_shift: usize, rest: &str) -> String {
    let vectors = lines_of("vectors.jsonl");
    let answers = lines_of("answers.jsonl");
    format!(
        "gateway vectors={vectors} ok={} answers={answers} answers_ok={} {rest}\n",
        vectors - ok_shift,
        answers - answers_shift
    )
}

fn passes(output: &Output, rest: &str) {
    assert_eq!(stdout(output), line(0, 0, rest));
    assert!(output.stderr.is_empty());
    assert_eq!(output.status.code(), Some(0));
}

fn misses(output: &Output, want: &str) {
    assert_eq!(stdout(output), want);
    assert!(output.stderr.is_empty());
    assert_eq!(output.status.code(), Some(1));
}

fn refused(output: &Output, want: &str) {
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!("REFUSED {want}\n")
    );
    assert!(output.stdout.is_empty());
    assert_eq!(output.status.code(), Some(2));
}

type Case = (
    &'static str,
    &'static str,
    usize,
    fn(&str) -> String,
    &'static str,
);

const PASS: &str = "closed=yes keys=6 forbidden=not_checked limits=ok";

#[test]
fn the_committed_files_pass_with_the_line_of_t11_7() {
    passes(&check(&gateway()), PASS);
    passes(&check(&fixture("copy")), PASS);
}

#[test]
fn a_changed_stated_result_is_a_miss() {
    let dir = fixture("flip-canon");
    edit_line(&dir, "vectors.jsonl", 1, |text| {
        text.replace("\"canon\":\"yes\"", "\"canon\":\"no\"")
    });
    misses(
        &check(&dir),
        &format!(
            "gateway miss file=vectors n=1 name=least\n{}",
            line(1, 0, PASS)
        ),
    );

    let dir = fixture("flip-error");
    let at = fs::read_to_string(gateway().join("vectors.jsonl"))
        .unwrap()
        .lines()
        .position(|text| text.contains("\"name\":\"limit_zero\""))
        .unwrap()
        + 1;
    edit_line(&dir, "vectors.jsonl", at, |text| {
        text.replace("\"error\":\"limit\"", "\"error\":\"size\"")
    });
    misses(
        &check(&dir),
        &format!(
            "gateway miss file=vectors n={at} name=limit_zero\n{}",
            line(1, 0, PASS)
        ),
    );

    let dir = fixture("flip-valid");
    edit_line(&dir, "vectors.jsonl", 2, |text| {
        text.replace("\"valid\":\"yes\"", "\"valid\":\"no\"")
    });
    edit_line(&dir, "answers.jsonl", 1, |text| {
        text.replace("\"cost\":1520}", "\"cost\":1521}")
    });
    edit_line(&dir, "answers.jsonl", 3, |text| {
        text.replace("\"expect\":\"over_cap\"", "\"expect\":\"no_cap\"")
    });
    misses(
        &check(&dir),
        &format!(
            "gateway miss file=vectors n=2 name=largest\ngateway miss file=answers n=1 name=ok\ngateway miss file=answers n=3 name=refused_over_cap\n{}",
            line(1, 2, PASS)
        ),
    );
}

#[test]
fn the_names_file_counts_the_five_keys_other_than_tier() {
    let dir = gateway();
    misses(
        &check_names(&dir, "alpha\ncontent\nzeta\n"),
        &line(0, 0, "closed=yes keys=6 forbidden=1 limits=ok"),
    );
    passes(
        &check_names(&dir, "alpha\ntier\nzeta\n"),
        "closed=yes keys=6 forbidden=0 limits=ok",
    );
    misses(
        &check_names(
            &dir,
            "content\neffort\nmax_output_tokens\nmessages\nrole\ntier\n",
        ),
        &line(0, 0, "closed=yes keys=6 forbidden=5 limits=ok"),
    );
    passes(
        &check_names(&dir, "contents\n role\n"),
        "closed=yes keys=6 forbidden=0 limits=ok",
    );
}

#[test]
fn the_schema_is_measured() {
    let dir = fixture("open-items");
    edit(&dir, "body.schema.json", |text| {
        text.replace(
            "\"required\":[\"role\",\"content\"],\"additionalProperties\":false",
            "\"required\":[\"role\",\"content\"]",
        )
    });
    misses(
        &check(&dir),
        &line(0, 0, "closed=no keys=6 forbidden=not_checked limits=ok"),
    );

    let dir = fixture("seventh-key");
    edit(&dir, "body.schema.json", |text| {
        text.replace(
            "\"maxLength\":131072}",
            "\"maxLength\":131072},\"name\":{\"type\":\"string\",\"enum\":[\"a\"]}",
        )
    });
    misses(
        &check(&dir),
        &line(0, 0, "closed=yes keys=7 forbidden=not_checked limits=ok"),
    );

    let dir = fixture("renamed-key");
    edit(&dir, "body.schema.json", |text| {
        text.replace("\"effort\":{", "\"reasoning_level\":{")
    });
    misses(
        &check(&dir),
        &line(0, 0, "closed=yes keys=6 forbidden=not_checked limits=ok"),
    );

    let dir = fixture("open-defs");
    edit(&dir, "body.schema.json", |text| {
        text.replacen(
            "\"type\":\"object\",",
            "\"$defs\":{\"m\":{\"type\":\"object\",\"properties\":{\"workflow\":{\"type\":\"string\",\"enum\":[\"w\"]}}}},\"type\":\"object\",",
            1,
        )
    });
    misses(
        &check(&dir),
        &line(0, 0, "closed=no keys=7 forbidden=not_checked limits=ok"),
    );

    let dir = fixture("open-all-of");
    edit(&dir, "body.schema.json", |text| {
        text.replacen(
            "\"type\":\"object\",",
            "\"allOf\":[{\"type\":\"object\"}],\"type\":\"object\",",
            1,
        )
    });
    misses(
        &check(&dir),
        &line(0, 0, "closed=no keys=6 forbidden=not_checked limits=ok"),
    );

    let dir = fixture("open-item-types");
    edit(&dir, "body.schema.json", |text| {
        text.replacen(
            "\"items\":{\"type\":\"object\",",
            "\"items\":{\"type\":[\"object\",\"null\"],",
            1,
        )
        .replacen(",\"additionalProperties\":false}}}", "}}}", 1)
    });
    misses(
        &check(&dir),
        &line(0, 0, "closed=no keys=6 forbidden=not_checked limits=ok"),
    );

    let dir = fixture("maximum");
    edit(&dir, "body.schema.json", |text| {
        text.replace("\"maximum\":2048", "\"maximum\":4096")
    });
    misses(
        &check(&dir),
        &line(0, 0, "closed=yes keys=6 forbidden=not_checked limits=miss"),
    );

    let dir = fixture("max-length");
    edit(&dir, "body.schema.json", |text| {
        text.replace("\"maxLength\":131072", "\"maxLength\":131071")
    });
    misses(
        &check(&dir),
        &line(0, 0, "closed=yes keys=6 forbidden=not_checked limits=miss"),
    );
}

#[test]
fn the_limits_file_is_measured() {
    let dir = fixture("deadline");
    edit(&dir, "tier-limits.tsv", |text| {
        text.replacen("90000", "90001", 1)
    });
    misses(
        &check(&dir),
        &line(0, 0, "closed=yes keys=6 forbidden=not_checked limits=miss"),
    );

    let dir = fixture("rows-swapped");
    edit(&dir, "tier-limits.tsv", |text| {
        let lines: Vec<&str> = text.lines().collect();
        format!("{}\n{}\n{}\n", lines[0], lines[2], lines[1])
    });
    misses(
        &check(&dir),
        &line(0, 0, "closed=yes keys=6 forbidden=not_checked limits=miss"),
    );
}

#[test]
fn bad_arguments_are_the_refusal_usage() {
    let dir = gateway();
    let dir = dir.to_str().unwrap();
    for args in [
        vec![],
        vec!["--dir"],
        vec!["--dir", dir, "--dir", dir],
        vec!["--dir", dir, "--names"],
        vec!["--dir", dir, "--all"],
        vec!["--names", "names.txt"],
        vec![dir],
    ] {
        let output = run(&args);
        let text = String::from_utf8_lossy(&output.stderr).to_string();
        assert!(
            text.starts_with("REFUSED usage: ") && text.ends_with('\n'),
            "{args:?}: {text}"
        );
        assert_eq!(text.lines().count(), 1, "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert_eq!(output.status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn a_file_not_of_its_form_is_the_refusal_file() {
    let missing = fresh("missing");
    for file in &FILES[1..] {
        fs::copy(gateway().join(file), missing.join(file)).unwrap();
    }
    refused(&check(&missing), "file: vectors.jsonl:0");
    refused(
        &check(&fresh("empty-dir").join("none")),
        "file: vectors.jsonl:0",
    );

    let cases: [Case; 9] = [
        (
            "cr",
            "answers.jsonl",
            3,
            |text| format!("{text}\r"),
            "file: answers.jsonl:3",
        ),
        (
            "order",
            "vectors.jsonl",
            2,
            |text| {
                text.replacen(
                    "{\"n\":2,\"name\":\"largest\"",
                    "{\"name\":\"largest\",\"n\":2",
                    1,
                )
            },
            "file: vectors.jsonl:2",
        ),
        (
            "space",
            "vectors.jsonl",
            5,
            |text| text.replacen("\"n\":5", "\"n\": 5", 1),
            "file: vectors.jsonl:5",
        ),
        (
            "twice",
            "vectors.jsonl",
            4,
            |text| text.replacen("\"name\":\"unicode_text\"", "\"name\":\"largest\"", 1),
            "file: vectors.jsonl:4",
        ),
        (
            "gap",
            "vectors.jsonl",
            3,
            |text| text.replacen("\"n\":3", "\"n\":4", 1),
            "file: vectors.jsonl:3",
        ),
        (
            "error-word",
            "vectors.jsonl",
            30,
            |text| text.replacen("\"error\":\"limit\"", "\"error\":\"big\"", 1),
            "file: vectors.jsonl:30",
        ),
        (
            "status",
            "answers.jsonl",
            1,
            |text| text.replacen("\"status\":200", "\"status\":70000", 1),
            "file: answers.jsonl:1",
        ),
        (
            "expect-word",
            "answers.jsonl",
            3,
            |text| text.replacen("\"expect\":\"over_cap\"", "\"expect\":\"over-cap\"", 1),
            "file: answers.jsonl:3",
        ),
        (
            "name-form",
            "answers.jsonl",
            2,
            |text| {
                text.replacen(
                    "\"name\":\"ok_space_and_other_order\"",
                    "\"name\":\"ok space\"",
                    1,
                )
            },
            "file: answers.jsonl:2",
        ),
    ];
    for (label, file, n, change, want) in cases {
        let dir = fixture(label);
        edit_line(&dir, file, n, change);
        refused(&check(&dir), want);
    }

    let dir = fixture("values-on-a-refusal");
    edit_line(&dir, "answers.jsonl", 3, |text| {
        text.replacen("\"content\":null", "\"content\":\"x\"", 1)
    });
    refused(&check(&dir), "file: answers.jsonl:3");

    let dir = fixture("no-values-on-ok");
    edit_line(&dir, "answers.jsonl", 1, |text| {
        text.replacen(
            "\"content\":\"Done.\",\"tokens_in\":120,\"tokens_out\":40,\"cost\":1520",
            "\"content\":null,\"tokens_in\":null,\"tokens_out\":null,\"cost\":null",
            1,
        )
    });
    refused(&check(&dir), "file: answers.jsonl:1");

    let dir = fixture("one-value-on-ok");
    edit_line(&dir, "answers.jsonl", 1, |text| {
        text.replacen("\"cost\":1520}", "\"cost\":null}", 1)
    });
    refused(&check(&dir), "file: answers.jsonl:1");

    let dir = fixture("no-final-lf");
    edit(&dir, "answers.jsonl", |text| {
        text.trim_end_matches('\n').to_string()
    });
    refused(
        &check(&dir),
        &format!("file: answers.jsonl:{}", lines_of("answers.jsonl")),
    );

    let dir = fixture("blank-last");
    edit(&dir, "vectors.jsonl", |text| format!("{text}\n"));
    refused(
        &check(&dir),
        &format!("file: vectors.jsonl:{}", lines_of("vectors.jsonl") + 1),
    );

    let dir = fixture("schema-not-json");
    edit(&dir, "body.schema.json", |text| text.replacen('}', "", 1));
    refused(&check(&dir), "file: body.schema.json:1");

    let dir = fixture("schema-two-lines");
    edit(&dir, "body.schema.json", |text| format!("{text}{{}}\n"));
    refused(&check(&dir), "file: body.schema.json:2");

    let dir = fixture("schema-key-twice");
    edit(&dir, "body.schema.json", |text| {
        text.replacen(
            "{\"$schema\"",
            "{\"additionalProperties\":true,\"$schema\"",
            1,
        )
    });
    refused(&check(&dir), "file: body.schema.json:1");

    let dir = fixture("tsv-cells");
    edit(&dir, "tier-limits.tsv", |text| {
        text.replacen("\tprovisional", "", 1)
    });
    refused(&check(&dir), "file: tier-limits.tsv:2");

    let dir = gateway();
    let output = run(&[
        "--dir",
        dir.to_str().unwrap(),
        "--names",
        fresh("no-names").join("names.txt").to_str().unwrap(),
    ]);
    refused(&output, "file: names.txt:0");
    refused(&check_names(&dir, "content\r\n"), "file: names.txt:1");
}

fn as_hex(line: &str) -> String {
    let value: serde_json::Value = serde_json::from_str(line).unwrap();
    let body = value["body"].as_str().unwrap();
    let digits: String = body.bytes().map(|byte| format!("{byte:02x}")).collect();
    line.replacen(
        &format!("\"body\":{}", serde_json::to_string(body).unwrap()),
        &format!("\"hex\":\"{digits}\""),
        1,
    )
}

#[test]
fn a_line_holds_its_bytes_under_body_or_under_hex() {
    let dir = fixture("hex-form");
    edit_line(&dir, "vectors.jsonl", 1, as_hex);
    edit_line(&dir, "answers.jsonl", 1, as_hex);
    passes(&check(&dir), PASS);

    let dir = fixture("hex-of-other-bytes");
    edit_line(&dir, "vectors.jsonl", 1, |text| {
        as_hex(text).replacen("\"hex\":\"7b", "\"hex\":\"20", 1)
    });
    misses(
        &check(&dir),
        &format!(
            "gateway miss file=vectors n=1 name=least\n{}",
            line(1, 0, PASS)
        ),
    );

    let cases: [Case; 6] = [
        (
            "both-keys",
            "vectors.jsonl",
            1,
            |text| format!("{},\"hex\":\"7b\"}}", &text[..text.len() - 1]),
            "file: vectors.jsonl:1",
        ),
        (
            "neither-key",
            "vectors.jsonl",
            1,
            |text| text.replacen("\"body\":", "\"text\":", 1),
            "file: vectors.jsonl:1",
        ),
        (
            "odd-digits",
            "vectors.jsonl",
            1,
            |text| as_hex(text).replacen("\"hex\":\"7b", "\"hex\":\"7", 1),
            "file: vectors.jsonl:1",
        ),
        (
            "upper-case-digit",
            "vectors.jsonl",
            1,
            |text| as_hex(text).replacen("\"hex\":\"7b", "\"hex\":\"7B", 1),
            "file: vectors.jsonl:1",
        ),
        (
            "digit-g",
            "answers.jsonl",
            1,
            |text| as_hex(text).replacen("\"hex\":\"7b", "\"hex\":\"7g", 1),
            "file: answers.jsonl:1",
        ),
        (
            "answer-both-keys",
            "answers.jsonl",
            1,
            |text| text.replacen(",\"expect\":", ",\"hex\":\"7b\",\"expect\":", 1),
            "file: answers.jsonl:1",
        ),
    ];
    for (label, file, n, change, want) in cases {
        let dir = fixture(label);
        edit_line(&dir, file, n, change);
        refused(&check(&dir), want);
    }
}

#[test]
fn the_tool_writes_no_file() {
    let dir = fixture("quiet");
    check(&dir);
    let mut names: Vec<String> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "answers.jsonl",
            "body.schema.json",
            "tier-limits.tsv",
            "vectors.jsonl"
        ]
    );
}
