use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use halvo_schema::kinds::{RetentionClass, KINDS};
use halvo_schema::{category, ids, kinds, record, request, site, Category};

const BIN: &str = env!("CARGO_BIN_EXE_export-names");

fn scratch(test: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("export-names-{}-{test}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(BIN)
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn locked_names() -> String {
    let mut names: Vec<&str> = [
        category::NAMES,
        ids::NAMES,
        kinds::NAMES,
        record::NAMES,
        request::NAMES,
        site::NAMES,
    ]
    .concat();
    names.sort();
    names.dedup();
    names.iter().map(|n| format!("{n}\n")).collect()
}

fn refused(dir: &Path, args: &[&str], line: &str) {
    let out = run(dir, args);
    assert_eq!(out.status.code(), Some(2), "{args:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "", "{args:?}");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        format!("{line}\n"),
        "{args:?}"
    );
}

#[test]
fn export_names_writes_the_names_of_the_locked_lists_sorted_once_each_on_two_runs() {
    let dir = scratch("two-runs");
    let mut written = Vec::new();
    for file in ["first.txt", "second.txt"] {
        let out = run(&dir, &["--out", file]);
        assert_eq!(out.status.code(), Some(0));
        let text = std::fs::read_to_string(dir.join(file)).unwrap();
        let count = text.lines().count();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            format!("names {count}\n")
        );
        assert_eq!(String::from_utf8_lossy(&out.stderr), "");
        written.push(text);
    }
    assert_eq!(written[0], written[1]);
    let lines: Vec<&str> = written[0].lines().collect();
    for name in locked_names().lines() {
        assert!(lines.contains(&name), "{name}");
    }
    assert!(lines.windows(2).all(|w| w[0].as_bytes() < w[1].as_bytes()));
    assert!(lines.iter().all(|n| !n.is_empty()
        && n.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')));
    assert!(written[0].ends_with('\n') && !written[0].contains('\r'));
    assert_eq!(
        &lines[..3],
        ["accessibility_text", "account_link", "active"]
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn export_names_refuses_a_bad_argument_list_as_usage() {
    let dir = scratch("usage");
    refused(&dir, &[], "REFUSED usage: missing --out");
    refused(&dir, &["--out"], "REFUSED usage: --out");
    refused(&dir, &["--out", ""], "REFUSED usage: --out");
    refused(
        &dir,
        &["--out", "a.txt", "--out", "b.txt"],
        "REFUSED usage: --out",
    );
    refused(&dir, &["--out", "--out"], "REFUSED usage: --out");
    refused(&dir, &["--out", "a.txt", "extra"], "REFUSED usage: extra");
    refused(&dir, &["--help"], "REFUSED usage: --help");
    refused(&dir, &["--out=a.txt"], "REFUSED usage: --out=a.txt");
    refused(&dir, &["x", "--out", "a.txt"], "REFUSED usage: x");
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn export_names_refuses_a_file_that_exists_and_changes_nothing() {
    let dir = scratch("exists");
    std::fs::write(dir.join("names.txt"), "kept\n").unwrap();
    refused(&dir, &["--out", "names.txt"], "REFUSED exists: names.txt");
    assert_eq!(
        std::fs::read_to_string(dir.join("names.txt")).unwrap(),
        "kept\n"
    );
    std::fs::create_dir(dir.join("sub")).unwrap();
    refused(&dir, &["--out", "sub"], "REFUSED exists: sub");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn export_names_makes_no_directory_and_refuses_a_missing_one_as_output() {
    let dir = scratch("output");
    let file = Path::new("missing").join("names.txt");
    let shown = file.to_str().unwrap();
    refused(&dir, &["--out", shown], &format!("REFUSED output: {shown}"));
    assert!(!dir.join("missing").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_names_lists_of_category_and_kinds_hold_the_names_of_the_contract() {
    let ids: Vec<&str> = Category::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(category::NAMES, ids);
    let mut expected: Vec<&str> = KINDS.iter().flat_map(|k| [k.name, k.family]).collect();
    expected.extend(
        [
            RetentionClass::Window,
            RetentionClass::Bounded { days: 93 },
            RetentionClass::Kept,
        ]
        .map(RetentionClass::name),
    );
    expected.sort();
    expected.dedup();
    let mut names = kinds::NAMES.to_vec();
    names.sort();
    assert_eq!(names, expected);
}
