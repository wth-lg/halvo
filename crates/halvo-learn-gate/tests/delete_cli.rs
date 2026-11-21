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

/// The refusal of a command whose key does not open the derived store, as the gate writes it (src/main.rs).
const KEY_REFUSAL: &str =
    "REFUSED store: the derived store does not open: the key does not open the derived database\n";

/// The closed list of SQLCipher's own log lines that stand before KEY_REFUSAL on Windows, each after the library's
/// time stamp, as sqlcipher-src 419.0.6+sqlcipher-4.19.0 writes them for a key that does not open the store
/// (sqlcipher/sqlcipher.c): the texts at lines 112591, 114343 and 112257, with page 1, SQLITE_ERROR (1) and lock 0;
/// the words ERROR and CORE at line 113474; the stamp and the line end at lines 113510 and 113511, the line end CRLF
/// on the hosted Windows runner. The library's default target is the device log on Android and Apple systems and
/// standard error on every other platform (lines 111284 to 111302). What varies in a line of the list is its time
/// stamp alone, in its fixed form: seventeen digits, each any digit (a month 99 passes), between separators that stand
/// fixed. The reading takes form lines in any number and in any order before the refusal line; on Windows the test
/// holds three lines from the second command, each one of the forms (a form may repeat), as in all 200 rounds of run 6.
const SQLCIPHER_FORMS: [&str; 3] = [
    ": ERROR CORE sqlcipher_page_cipher: hmac check failed for pgno=1\r\n",
    ": ERROR CORE sqlite3Codec: error decrypting page 1 data: 1\r\n",
    ": ERROR CORE sqlcipher_codec_ctx_set_error: 1 lock=0\r\n",
];

/// The bytes of LINE after a time stamp of the library's fixed form (strftime's "%Y-%m-%d %H:%M:%S", then a full stop
/// and three digits of the millisecond), or None. It reads bytes and never cuts the text, so no line makes it panic.
fn after_stamp(line: &str) -> Option<&[u8]> {
    let form = b"dddd-dd-dd dd:dd:dd.ddd";
    let bytes = line.as_bytes();
    let fits = bytes
        .get(..form.len())?
        .iter()
        .zip(form)
        .all(|(byte, want)| {
            if *want == b'd' {
                byte.is_ascii_digit()
            } else {
                byte == want
            }
        });
    if fits {
        bytes.get(form.len()..)
    } else {
        None
    }
}

/// The four sentences of read_key_refusal's faults, with `#` in each place of a number.
const FAULT_SHAPES: [&str; 4] = [
    "standard error is empty",
    "line # of #, # bytes, is not the refusal line",
    "lines before the refusal line: #, where none may stand on this system",
    "line # of #, # bytes, is no form of the list",
];

/// FAULT with each run of ASCII digits put as one `#`.
fn shape(fault: &str) -> String {
    let mut out = String::new();
    let mut in_number = false;
    for c in fault.chars() {
        if c.is_ascii_digit() {
            if !in_number {
                out.push('#');
            }
            in_number = true;
        } else {
            out.push(c);
            in_number = false;
        }
    }
    out
}

/// Reads the standard error ERR of a command that the key refused, read as on Windows when WINDOWS holds: the last line
/// is exactly KEY_REFUSAL; when WINDOWS holds, every line before it is a time stamp and one form of SQLCIPHER_FORMS,
/// and otherwise no line may stand before it on this system. The fault is one of the sentences of FAULT_SHAPES, with
/// numbers alone for a line's place, the count of lines and a line's length, never a text of ERR.
fn read_key_refusal(err: &str, windows: bool) -> Result<(), String> {
    let lines: Vec<&str> = err.split_inclusive('\n').collect();
    let Some((last, before)) = lines.split_last() else {
        return Err("standard error is empty".to_owned());
    };
    if *last != KEY_REFUSAL {
        return Err(format!(
            "line {} of {}, {} bytes, is not the refusal line",
            lines.len(),
            lines.len(),
            last.len()
        ));
    }
    if !windows && !before.is_empty() {
        return Err(format!(
            "lines before the refusal line: {}, where none may stand on this system",
            before.len()
        ));
    }
    for (at, line) in before.iter().enumerate() {
        if !after_stamp(line)
            .is_some_and(|rest| SQLCIPHER_FORMS.iter().any(|form| form.as_bytes() == rest))
        {
            return Err(format!(
                "line {} of {}, {} bytes, is no form of the list",
                at + 1,
                lines.len(),
                line.len()
            ));
        }
    }
    Ok(())
}

/// BYTES in base64 with its padding.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let word = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for at in 0..4 {
            if at <= chunk.len() {
                text.push(char::from(ALPHABET[(word >> (18 - 6 * at)) as usize & 63]));
            } else {
                text.push('=');
            }
        }
    }
    text
}

#[test]
fn delete_a_second_command_on_the_derived_store_of_the_first_is_refused_by_its_own_key() {
    let stamp = |millis: u32| format!("2026-11-19 18:54:08.{millis:03}");
    let windows_lines = [
        format!("{}{}", stamp(555), SQLCIPHER_FORMS[0]),
        format!("{}{}", stamp(557), SQLCIPHER_FORMS[1]),
        format!("{}{}", stamp(557), SQLCIPHER_FORMS[2]),
    ];
    let with_first = |first: &str| {
        format!(
            "{first}{}{}{KEY_REFUSAL}",
            windows_lines[1], windows_lines[2]
        )
    };
    let seen_on_windows = with_first(&windows_lines[0]);
    assert_eq!(read_key_refusal(&seen_on_windows, true), Ok(()));
    assert_eq!(read_key_refusal(KEY_REFUSAL, false), Ok(()));

    let key: [u8; 32] = std::array::from_fn(|at| (at as u8).wrapping_mul(37).wrapping_add(11));
    let decimal: Vec<String> = key.iter().map(u8::to_string).collect();
    let form = windows_lines[0].trim_end_matches("\r\n");
    let added = |text: &str| format!("{form} {text}\r\n");
    let mut stand_ins: Vec<(u32, String)> = [
        (1, added("3f")),
        (2, added(&base64(&key))),
        (3, added(&decimal.join(" "))),
        (4, added(&SEEDED_LABELS[0][4..18])),
        (5, added(env!("CARGO_TARGET_TMPDIR"))),
        (6, added("DERIVED.DB")),
        (
            7,
            format!("{}: ERROR CORE a line of free text\r\n", stamp(555)),
        ),
        (8, added("REFUSED")),
        (9, windows_lines[0].replace("pgno=1", "pgno=2")),
        (10, format!("2026-11-19 18:54:08{}", SQLCIPHER_FORMS[0])),
        (11, format!("{}: ERROR codec: no CORE\r\n", stamp(555))),
        (12, format!("{}: ERROR CORE \r\n", stamp(555))),
        (13, form.to_owned()),
        (14, format!("x{}", windows_lines[0])),
        (15, format!("{}: ERROR CORE a\ttab\r\n", stamp(555))),
        (
            16,
            "REFUSED store: the derived store does not open\n".to_owned(),
        ),
        (
            17,
            format!("2026-11-19 18:54:08.55\u{e9}{}", SQLCIPHER_FORMS[0]),
        ),
    ]
    .into_iter()
    .map(|(number, first)| (number, with_first(&first)))
    .collect();
    let forms_alone = windows_lines.concat();
    stand_ins.extend([
        (18, forms_alone.clone()),
        (19, seen_on_windows.trim_end_matches('\n').to_owned()),
        (
            20,
            format!("{forms_alone}REFUSED store: the derived store is not in this build\n"),
        ),
    ]);
    let let_through: Vec<u32> = stand_ins
        .iter()
        .filter(|(_, err)| read_key_refusal(err, true).is_ok())
        .map(|(number, _)| *number)
        .collect();
    assert!(
        let_through.is_empty(),
        "the reading lets these stand-ins through: {let_through:?}"
    );
    let off_shape: Vec<u32> = stand_ins
        .iter()
        .filter(|(_, err)| match read_key_refusal(err, true) {
            Err(fault) => !FAULT_SHAPES.contains(&shape(&fault).as_str()),
            Ok(()) => false,
        })
        .map(|(number, _)| *number)
        .collect();
    assert!(
        off_shape.is_empty(),
        "the fault of these stand-ins is none of the reading's sentences: {off_shape:?}"
    );
    assert_eq!(
        read_key_refusal(&seen_on_windows, false).map_err(|fault| shape(&fault)),
        Err(FAULT_SHAPES[2].to_owned()),
        "on a system other than Windows no line may stand before the refusal line"
    );

    let dir = fresh("delete-two-commands");
    let first = derived_delete("workflow", &dir);
    assert_eq!(
        first.status.code(),
        Some(0),
        "the first command's exit status"
    );
    assert!(
        first.stderr.is_empty(),
        "the first command's standard error holds {} bytes",
        first.stderr.len()
    );
    let second = derived_delete("workflow", &dir);
    assert_eq!(
        second.status.code(),
        Some(2),
        "the second command's exit status"
    );
    assert!(
        second.stdout.is_empty(),
        "the second command's standard output holds {} bytes",
        second.stdout.len()
    );
    let Ok(err) = String::from_utf8(second.stderr) else {
        panic!("the second command's standard error is not UTF-8");
    };
    if let Err(fault) = read_key_refusal(&err, cfg!(windows)) {
        panic!(
            "each command makes a key of its own, so the second does not open the first one's store: {fault}"
        );
    }
    if cfg!(windows) {
        assert_eq!(
            err.split_inclusive('\n').count() - 1,
            0,
            "the lines before the refusal line: none, since the library starts with its log level at NONE"
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
