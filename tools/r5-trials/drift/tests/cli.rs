use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("the repository root")
}

fn drift_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_r5-drift"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("the binary runs")
}

fn drift(args: &[&str]) -> Output {
    drift_in(&root(), args)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("UTF-8 output")
}

fn refused(output: &Output, cause: &str) -> String {
    assert_eq!(output.status.code(), Some(2), "{}", text(&output.stderr));
    assert!(output.stdout.is_empty(), "standard output holds bytes");
    let stderr = text(&output.stderr);
    assert_eq!(stderr.matches('\n').count(), 1, "{stderr}");
    assert!(
        stderr.starts_with(&format!("REFUSED {cause}: ")),
        "{stderr}"
    );
    stderr
}

fn refusals() -> PathBuf {
    root().join("tools/r5-trials/drift/tests/refusals")
}

#[test]
fn the_trial_prints_64_case_lines_and_a_verdict_line_and_exits_0() {
    let output = drift(&[]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    let stdout = text(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 65);
    assert!(lines[..64]
        .iter()
        .all(|line| line.starts_with("case name=")));
    assert!(lines[..64].iter().all(|line| line.contains(" failed=0 ")));
    assert!(lines[64] == "verdict rule=admit" || lines[64] == "verdict rule=plain");
    let arms: Vec<&str> = lines[..4]
        .iter()
        .map(|line| {
            let at = line.find(" arm=").expect("an arm");
            let end = line.find(" occurrences=").expect("occurrences");
            &line[at + 1..end]
        })
        .collect();
    assert_eq!(
        arms,
        [
            "arm=none sort=-",
            "arm=plain sort=-",
            "arm=admit sort=yes",
            "arm=admit sort=no"
        ]
    );
    let mut names: Vec<&str> = lines[..64]
        .iter()
        .step_by(4)
        .map(|line| line.split(' ').nth(1).expect("a name"))
        .collect();
    let printed = names.clone();
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(printed, names);
}

#[test]
fn every_refusal_file_exits_2_with_its_one_line_and_nothing_on_standard_output() {
    let expected = std::fs::read_to_string(refusals().join("expected.txt")).expect("expected.txt");
    for want in expected.lines() {
        let file = want
            .strip_prefix("REFUSED case: ")
            .and_then(|rest| rest.split_once(": "))
            .map(|(file, _)| file)
            .expect("a file name");
        let path = refusals().join(file);
        let output = drift(&["--case", path.to_str().expect("a path"), "--arm", "plain"]);
        let stderr = refused(&output, "case");
        assert_eq!(stderr, format!("{want}\n"));
    }
}

#[test]
fn case_prints_one_line_for_the_arms_none_and_plain() {
    let file = root().join("fixtures/r5/cases/dev/renamed-pay.jsonl");
    for arm in ["none", "plain"] {
        let output = drift(&["--case", file.to_str().expect("a path"), "--arm", arm]);
        assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
        let stdout = text(&output.stdout);
        assert_eq!(stdout.lines().count(), 1);
        assert!(stdout.starts_with("case name=renamed-pay kind=benign form=renamed "));
        assert!(stdout.contains(&format!(" arm={arm} sort=- ")));
    }
}

#[test]
fn count_prints_the_six_spread_lines_of_the_development_cases() {
    let dir = root().join("fixtures/r5/cases/dev");
    let output = drift(&["--count", dir.to_str().expect("a path")]);
    assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
    assert_eq!(
        text(&output.stdout),
        "spread form=renamed cases=2 shows1=0 shows2=0\n\
         spread form=renamed-twice cases=2 shows1=0 shows2=0\n\
         spread form=alternates cases=2 shows1=0 shows2=0\n\
         spread form=flaky cases=2 shows1=0 shows2=0\n\
         spread form=decoy cases=4 shows1=2 shows2=2\n\
         spread form=reused cases=4 shows1=2 shows2=2\n"
    );
}

#[test]
fn count_refuses_a_directory_that_holds_a_refused_case() {
    let output = drift(&["--count", refusals().to_str().expect("a path")]);
    refused(&output, "case");
}

#[test]
fn arguments_that_the_trial_does_not_take_are_refused_with_usage() {
    let file = root().join("fixtures/r5/cases/dev/renamed-pay.jsonl");
    let file = file.to_str().expect("a path");
    let calls: [&[&str]; 9] = [
        &["--help"],
        &["--case", file],
        &["--case", file, "--arm", "admit"],
        &["--case", file, "--arm", "none", "--count"],
        &["--arm", "none", "--case", file],
        &["--count"],
        &["--count", ""],
        &["--case", "", "--arm", "none"],
        &["extra"],
    ];
    for args in calls {
        refused(&drift(args), "usage");
    }
}

#[test]
fn a_missing_targets_file_is_refused_with_input() {
    let output = drift_in(&root().join("tools/r5-trials"), &[]);
    let stderr = refused(&output, "input");
    assert_eq!(stderr, "REFUSED input: the targets file cannot be read\n");
}

#[test]
fn a_missing_case_file_is_refused_with_case() {
    let file = refusals().join("absent.jsonl");
    let output = drift(&["--case", file.to_str().expect("a path"), "--arm", "none"]);
    let stderr = refused(&output, "case");
    assert_eq!(stderr, "REFUSED case: absent.jsonl: not a case file\n");
}

fn scratch(file: &str, bytes: &[u8]) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(file);
    std::fs::write(&path, bytes).expect("a scratch file");
    path
}

#[test]
fn a_case_file_of_1048577_bytes_is_refused_before_any_line_is_parsed() {
    let mut bytes = vec![b'x'; 1_048_576];
    bytes.push(b'\n');
    assert_eq!(bytes.len(), 1_048_577);
    let path = scratch("size-above.jsonl", &bytes);
    let output = drift(&["--case", path.to_str().expect("a path"), "--arm", "none"]);
    assert_eq!(
        refused(&output, "case"),
        "REFUSED case: size-above.jsonl: the file holds more than 1048576 bytes\n"
    );
}

#[test]
fn a_case_file_of_1048576_bytes_of_valid_lines_is_read_as_its_content_says() {
    let source = std::fs::read_to_string(root().join("fixtures/r5/cases/dev/renamed-pay.jsonl"))
        .expect("a development case");
    let filler = "\"test_id\":\"help-link\",\"passes\":[]";
    let lines = source.matches(filler).count();
    assert_eq!(lines, source.lines().count() - 1);
    let mut name = String::from("size-limit-");
    let named = |name: &str| {
        source.replacen(
            "\"case\":\"renamed-pay\"",
            &format!("\"case\":\"{name}\""),
            1,
        )
    };
    while (1_048_576 - named(&name).len()) % lines != 0 {
        name.push('a');
    }
    let pad = (1_048_576 - named(&name).len()) / lines;
    let items = if pad % 2 == 1 {
        format!("9{}", ",9".repeat(pad / 2))
    } else {
        format!("99{}", ",9".repeat(pad / 2 - 1))
    };
    assert_eq!(items.len(), pad);
    let padded = named(&name).replace(
        filler,
        &format!("\"test_id\":\"help-link\",\"passes\":[{items}]"),
    );
    assert_eq!(padded.len(), 1_048_576);
    let path = scratch(&format!("{name}.jsonl"), padded.as_bytes());
    let source_path = root().join("fixtures/r5/cases/dev/renamed-pay.jsonl");
    for arm in ["none", "plain"] {
        let output = drift(&["--case", path.to_str().expect("a path"), "--arm", arm]);
        assert_eq!(output.status.code(), Some(0), "{}", text(&output.stderr));
        let unpadded = drift(&[
            "--case",
            source_path.to_str().expect("a path"),
            "--arm",
            arm,
        ]);
        assert_eq!(
            text(&output.stdout),
            text(&unpadded.stdout).replacen("name=renamed-pay", &format!("name={name}"), 1)
        );
    }
}

#[test]
fn a_standard_output_that_cannot_be_written_is_refused_with_output() {
    let (reader, writer) = std::io::pipe().expect("a pipe");
    drop(reader);
    let output = Command::new(env!("CARGO_BIN_EXE_r5-drift"))
        .current_dir(root())
        .stdout(writer)
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("the binary runs");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        text(&output.stderr),
        "REFUSED output: standard output cannot be written\n"
    );
}
