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

/// The labels that `delete` seeds into the rows of its two workflows (src/delete.rs MARKERS; an integration test cannot
/// import the binary's items).
const SEEDED_LABELS: [&str; 2] = ["Zq7-halvo-marker-learn-a", "Zq7-halvo-marker-learn-b"];

/// A line of SQLCipher's own log as the library writes it to standard error on Windows when a key does not open a
/// store: a date, a time with its fraction, ": ERROR CORE ", then the library's text in printable ASCII, and the line end.
fn sqlcipher_log_line(line: &str) -> bool {
    let Some(body) = line
        .strip_suffix("\r\n")
        .or_else(|| line.strip_suffix('\n'))
    else {
        return false;
    };
    let bytes = body.as_bytes();
    let digits = |from: usize, to: usize| {
        bytes
            .get(from..to)
            .is_some_and(|run| run.iter().all(u8::is_ascii_digit))
    };
    let stamp = bytes.len() > 20
        && digits(0, 4)
        && bytes[4] == b'-'
        && digits(5, 7)
        && bytes[7] == b'-'
        && digits(8, 10)
        && bytes[10] == b' '
        && digits(11, 13)
        && bytes[13] == b':'
        && digits(14, 16)
        && bytes[16] == b':'
        && digits(17, 19)
        && bytes[19] == b'.';
    if !stamp {
        return false;
    }
    let fraction = bytes[20..]
        .iter()
        .take_while(|b| b.is_ascii_digit())
        .count();
    fraction > 0
        && body[20 + fraction..]
            .strip_prefix(": ERROR CORE ")
            .is_some_and(|text| {
                !text.is_empty() && text.bytes().all(|b| (b' '..=b'~').contains(&b))
            })
}

/// The longest run of hex digits in LINE: a key printed as hex would make one of 64.
fn longest_hex_run(line: &str) -> usize {
    line.split(|c: char| !c.is_ascii_hexdigit())
        .map(str::len)
        .max()
        .unwrap_or(0)
}

#[test]
fn delete_a_second_command_on_the_derived_store_of_the_first_is_refused_by_its_own_key() {
    let seen_on_windows = [
        "2026-11-19 18:54:08.555: ERROR CORE sqlcipher_page_cipher: hmac check failed for pgno=1\r\n",
        "2026-11-19 18:54:08.557: ERROR CORE sqlite3Codec: error decrypting page 1 data: 1\r\n",
        "2026-11-19 18:54:08.557: ERROR CORE sqlcipher_codec_ctx_set_error: 1 lock=0\r\n",
    ];
    for line in seen_on_windows {
        assert!(sqlcipher_log_line(line), "{line:?}");
    }
    for line in [
        "2026-11-19 18:54:08: ERROR CORE a time without its fraction\r\n",
        "2026-11-19 18:54:08.555: ERROR codec: no CORE\r\n",
        "2026-11-19 18:54:08.555: ERROR CORE \r\n",
        "2026-11-19 18:54:08.555: ERROR CORE no line end",
        "x2026-11-19 18:54:08.555: ERROR CORE a byte before the date\r\n",
        "2026-11-19 18:54:08.555: ERROR CORE a\ttab\r\n",
        "REFUSED store: the derived store does not open\n",
    ] {
        assert!(!sqlcipher_log_line(line), "{line:?}");
    }

    let dir = fresh("delete-two-commands");
    line_of(&derived_delete("workflow", &dir));
    let second = derived_delete("workflow", &dir);
    assert_eq!(second.status.code(), Some(2), "{second:?}");
    assert!(second.stdout.is_empty(), "{second:?}");
    let err = String::from_utf8(second.stderr).expect("UTF-8");
    let lines: Vec<&str> = err.split_inclusive('\n').collect();
    let (last, before) = lines.split_last().expect("a line on standard error");
    assert_eq!(
        *last,
        "REFUSED store: the derived store does not open: the key does not open the derived database\n",
        "each command makes a key of its own, so the second does not open the first one's store: {err:?}"
    );
    let path = dir.to_str().expect("a path");
    let name = dir
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a name");
    for line in before {
        assert!(
            sqlcipher_log_line(line),
            "a line before the refusal that is not SQLCipher's own log: {line:?}"
        );
        assert!(!line.contains("REFUSED"), "{line:?}");
        assert!(
            !line.contains(path) && !line.contains(name) && !line.contains("derived.db"),
            "a path in SQLCipher's log line: {line:?}"
        );
        assert!(
            SEEDED_LABELS.iter().all(|label| !line.contains(label)),
            "a seeded label in SQLCipher's log line: {line:?}"
        );
        assert!(
            longest_hex_run(line) < 16,
            "a run of hex digits as long as a key's in SQLCipher's log line: {line:?}"
        );
    }
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
