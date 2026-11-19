use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn data_dir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).expect("a test directory");
    dir
}

fn gate(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_learn-gate"))
        .args(args)
        .current_dir(repo())
        .output()
        .expect("learn-gate runs")
}

fn refused(out: &Output, cause: &str) {
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    let err = String::from_utf8(out.stderr.clone()).expect("UTF-8");
    assert!(err.starts_with(&format!("REFUSED {cause}: ")), "{err}");
}

#[test]
fn delete_workflow_takes_its_rows_and_leaves_the_other_served() {
    let dir = data_dir("delete-workflow");
    let dir = dir.to_str().expect("a path");
    let out = gate(&[
        "delete",
        "--what",
        "workflow",
        "--data-dir",
        dir,
        "--store",
        "memory",
    ]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let text = String::from_utf8(out.stdout).expect("UTF-8");
    let line = text.strip_suffix('\n').expect("one line");
    let pairs: Vec<(&str, &str)> = line
        .strip_prefix("delete ")
        .expect("a delete line")
        .split(' ')
        .map(|p| p.split_once('=').expect("a pair"))
        .collect();
    let keys: Vec<&str> = pairs.iter().map(|(k, _)| *k).collect();
    assert_eq!(
        keys,
        [
            "what",
            "store",
            "rows_left",
            "other_rows",
            "other_served",
            "result"
        ]
    );
    let value = |key: &str| {
        pairs
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| *v)
            .expect("a key")
    };
    assert_eq!((value("what"), value("store")), ("workflow", "memory"));
    assert_eq!(value("rows_left"), "0");
    value("other_rows").parse::<u32>().expect("a count");
    assert_eq!((value("other_served"), value("result")), ("yes", "ok"));
    assert_eq!(
        std::fs::read_dir(dir).expect("the data directory").count(),
        0
    );
}

#[test]
fn delete_all_on_the_memory_store_is_not_run() {
    let dir = data_dir("delete-all");
    let out = gate(&[
        "delete",
        "--what",
        "all",
        "--data-dir",
        dir.to_str().expect("a path"),
    ]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(
        String::from_utf8(out.stdout).expect("UTF-8"),
        "delete what=all store=memory exit=- files_left=- result=not-run\n"
    );
}

fn fresh(name: &str) -> PathBuf {
    halvo_test_dir::make(Path::new(env!("CARGO_TARGET_TMPDIR")), name).expect("a test directory")
}

fn line_of(out: &Output) -> String {
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stderr.is_empty(), "{out:?}");
    let text = String::from_utf8(out.stdout.clone()).expect("UTF-8");
    text.strip_suffix('\n').expect("one line").to_owned()
}

#[test]
fn delete_workflow_on_the_derived_store_takes_its_rows_by_the_decision_and_leaves_the_other_served()
{
    let memory = fresh("delete-workflow-memory");
    let derived = fresh("delete-workflow-derived");
    let temp = fresh("delete-workflow-temp");
    let call = |dir: &Path, store: &str| {
        Command::new(env!("CARGO_BIN_EXE_learn-gate"))
            .args([
                "delete",
                "--what",
                "workflow",
                "--data-dir",
                dir.to_str().expect("a path"),
                "--store",
                store,
            ])
            .current_dir(repo())
            .env("TMPDIR", &temp)
            .env("TMP", &temp)
            .env("TEMP", &temp)
            .output()
            .expect("learn-gate runs")
    };
    let memory_line = line_of(&call(&memory, "memory"));
    let derived_line = line_of(&call(&derived, "derived"));
    assert_eq!(
        std::fs::read_dir(&temp)
            .expect("the temporary directory")
            .count(),
        0,
        "the command writes nothing outside its data directory"
    );
    assert_eq!(
        derived_line,
        memory_line.replacen(" store=memory ", " store=derived ", 1)
    );
    assert!(
        derived_line.starts_with("delete what=workflow store=derived rows_left=0 other_rows="),
        "{derived_line}"
    );
    assert!(
        derived_line.ends_with(" other_served=yes result=ok"),
        "{derived_line}"
    );
    let entries: Vec<String> = std::fs::read_dir(&derived)
        .expect("the data directory")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(entries, ["derived"]);
}

#[test]
fn delete_all_on_the_derived_store_is_not_run() {
    let dir = fresh("delete-all-derived");
    let out = gate(&[
        "delete",
        "--what",
        "all",
        "--data-dir",
        dir.to_str().expect("a path"),
        "--store",
        "derived",
    ]);
    assert_eq!(
        line_of(&out),
        "delete what=all store=derived exit=- files_left=- result=not-run"
    );
}

fn derived_delete(what: &str, dir: &Path) -> Output {
    gate(&[
        "delete",
        "--what",
        what,
        "--data-dir",
        dir.to_str().expect("a path"),
        "--store",
        "derived",
    ])
}

/// Every path under DIR, relative to it, with the bytes of each file (None for a directory).
fn bytes_under(dir: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut found = BTreeMap::new();
    let mut open = vec![dir.to_path_buf()];
    while let Some(at) = open.pop() {
        for entry in std::fs::read_dir(&at).expect("a directory") {
            let path = entry.expect("an entry").path();
            let relative = path
                .strip_prefix(dir)
                .expect("a path under DIR")
                .to_path_buf();
            if path.is_dir() {
                found.insert(relative, None);
                open.push(path);
            } else {
                found.insert(relative, Some(std::fs::read(&path).expect("a file")));
            }
        }
    }
    found
}

#[test]
fn delete_a_second_command_on_the_derived_store_of_the_first_is_refused_by_its_own_key() {
    let dir = fresh("delete-two-commands");
    line_of(&derived_delete("workflow", &dir));
    let second = derived_delete("workflow", &dir);
    assert_eq!(second.status.code(), Some(2), "{second:?}");
    assert!(second.stdout.is_empty(), "{second:?}");
    assert_eq!(
        String::from_utf8(second.stderr).expect("UTF-8"),
        "REFUSED store: the derived store does not open: the key does not open the derived database\n",
        "each command makes a key of its own, so the second does not open the first one's store"
    );
}

#[test]
fn delete_all_on_the_derived_store_leaves_the_bytes_of_every_file_of_a_populated_directory() {
    let dir = fresh("delete-all-populated");
    line_of(&derived_delete("workflow", &dir));
    std::fs::write(
        dir.join("review-only-cell.txt"),
        b"Indigo disposable test cell",
    )
    .expect("a file of known bytes");
    let before = bytes_under(&dir);
    assert!(
        before.keys().any(|path| path.starts_with("derived")),
        "the derived store's files stand under the directory: {:?}",
        before.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        line_of(&derived_delete("all", &dir)),
        "delete what=all store=derived exit=- files_left=- result=not-run"
    );
    let after = bytes_under(&dir);
    let changed: Vec<&PathBuf> = before
        .keys()
        .chain(after.keys().filter(|path| !before.contains_key(*path)))
        .filter(|path| before.get(*path) != after.get(*path))
        .collect();
    assert!(
        changed.is_empty(),
        "delete --what all on the derived store changed the bytes of these paths, or added or removed them: {changed:?}"
    );
}

#[test]
fn delete_refuses_wrong_calls() {
    let dir = data_dir("delete-refusals");
    let dir = dir.to_str().expect("a path");
    refused(&gate(&["delete", "--what", "workflow"]), "usage");
    refused(&gate(&["delete", "--data-dir", dir]), "usage");
    refused(
        &gate(&["delete", "--what", "some", "--data-dir", dir]),
        "usage",
    );
    refused(
        &gate(&["delete", "--what", "workflow", "--data-dir", dir, "--force"]),
        "usage",
    );
}
