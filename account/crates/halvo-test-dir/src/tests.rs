use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{make, make_with, NAME_MAX};

fn base(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary");
    let target = exe
        .ancestors()
        .nth(3)
        .expect("the target directory above deps");
    make(&target.join("tmp").join("halvo-test-dir-unit"), name).expect("a base of its own")
}

fn entries(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("list the directory")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .into_string()
                .expect("utf-8")
        })
        .collect();
    names.sort();
    names
}

#[test]
fn a_name_that_stands_is_passed_over_for_the_next_number() {
    let base = base("stands");
    let pid = std::process::id();
    let standing = base.join(format!("word-{pid}-7"));
    fs::create_dir(&standing).expect("the standing directory");
    fs::write(standing.join("kept"), b"of an earlier process").expect("its file");
    let next = AtomicU64::new(7);
    let got = make_with(&base, "word", &next).expect("a directory");
    assert_eq!(got, base.join(format!("word-{pid}-8")));
    assert!(got.is_dir());
    assert_eq!(entries(&got), Vec::<String>::new());
    assert_eq!(
        fs::read(standing.join("kept")).expect("the kept file"),
        b"of an earlier process"
    );
    assert_eq!(next.load(Ordering::SeqCst), 9);
}

#[test]
fn a_file_that_stands_at_the_name_is_passed_over_too() {
    let base = base("file-stands");
    let pid = std::process::id();
    fs::write(base.join(format!("word-{pid}-3")), b"a file").expect("the standing file");
    let next = AtomicU64::new(3);
    let got = make_with(&base, "word", &next).expect("a directory");
    assert_eq!(got, base.join(format!("word-{pid}-4")));
    assert!(got.is_dir());
}

#[test]
fn the_name_is_the_word_the_process_id_and_the_number_and_the_counter_moves_by_one() {
    let base = base("form");
    let next = AtomicU64::new(41);
    let got = make_with(&base, "Az09-_", &next).expect("a directory");
    assert_eq!(
        got.file_name().and_then(|n| n.to_str()),
        Some(format!("Az09-_-{}-41", std::process::id()).as_str())
    );
    assert_eq!(got.parent(), Some(base.as_path()));
    assert_eq!(next.load(Ordering::SeqCst), 42);
}

#[test]
fn a_missing_base_is_made_with_its_parents() {
    let base = base("missing").join("one").join("two");
    let next = AtomicU64::new(0);
    let got = make_with(&base, "deep", &next).expect("a directory");
    assert!(got.is_dir());
    assert_eq!(got.parent(), Some(base.as_path()));
}

#[test]
fn a_name_of_the_longest_length_is_taken_and_one_byte_more_is_refused() {
    let base = base("length");
    let next = AtomicU64::new(0);
    let longest = "a".repeat(NAME_MAX);
    assert!(make_with(&base, &longest, &next).is_ok());
    let error = make_with(&base, &"a".repeat(NAME_MAX + 1), &next).expect_err("too long");
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
}

#[test]
fn a_bad_name_is_refused_before_anything_is_made() {
    let base = base("refused");
    let inner = base.join("never");
    let next = AtomicU64::new(5);
    for name in [
        "", "a/b", "a\\b", "a:b", "a.", "a b", ".", "..", "a\u{e9}", "a\nb", "a\0b", "a*b",
    ] {
        let error = make_with(&inner, name, &next).expect_err(name);
        assert_eq!(error.kind(), ErrorKind::InvalidInput, "{name:?}");
    }
    assert!(!inner.exists());
    assert_eq!(next.load(Ordering::SeqCst), 5);
}

#[test]
fn a_base_that_is_a_file_is_an_error_and_takes_no_number() {
    let base = base("base-file").join("a-file");
    fs::write(&base, b"not a directory").expect("the file");
    let next = AtomicU64::new(0);
    assert!(make_with(&base, "word", &next).is_err());
    assert_eq!(next.load(Ordering::SeqCst), 0);
}

#[cfg(unix)]
#[test]
fn another_error_than_a_standing_name_is_returned_after_one_number() {
    use std::os::unix::fs::PermissionsExt;
    let base = base("read-only");
    fs::set_permissions(&base, fs::Permissions::from_mode(0o555)).expect("read-only");
    let (tx, rx) = std::sync::mpsc::channel();
    let at = base.clone();
    std::thread::spawn(move || {
        let next = AtomicU64::new(0);
        let got = make_with(&at, "word", &next).map(|_| ());
        let _ = tx.send((got.map_err(|e| e.kind()), next.load(Ordering::SeqCst)));
    });
    let got = rx.recv_timeout(std::time::Duration::from_secs(20));
    fs::set_permissions(&base, fs::Permissions::from_mode(0o755)).expect("writable again");
    assert_eq!(got, Ok((Err(ErrorKind::PermissionDenied), 1)));
}
