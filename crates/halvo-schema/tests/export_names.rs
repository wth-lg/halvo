use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use halvo_schema::kinds::{RetentionClass, KINDS};
use halvo_schema::{category, ids, kinds, record, request, site, Category};

const BIN: &str = env!("CARGO_BIN_EXE_export-names");

fn scratch(test: &str) -> Leaf {
    leaf(&format!("export-names-{test}"))
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
    let held = scratch("two-runs");
    let dir = held.path();
    let mut written = Vec::new();
    for file in ["first.txt", "second.txt"] {
        let out = run(dir, &["--out", file]);
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
}

#[test]
fn export_names_refuses_a_bad_argument_list_as_usage() {
    let held = scratch("usage");
    let dir = held.path();
    refused(dir, &[], "REFUSED usage: missing --out");
    refused(dir, &["--out"], "REFUSED usage: --out");
    refused(dir, &["--out", ""], "REFUSED usage: --out");
    refused(
        dir,
        &["--out", "a.txt", "--out", "b.txt"],
        "REFUSED usage: --out",
    );
    refused(dir, &["--out", "--out"], "REFUSED usage: --out");
    refused(dir, &["--out", "a.txt", "extra"], "REFUSED usage: extra");
    refused(dir, &["--help"], "REFUSED usage: --help");
    refused(dir, &["--out=a.txt"], "REFUSED usage: --out=a.txt");
    refused(dir, &["x", "--out", "a.txt"], "REFUSED usage: x");
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0);
}

#[test]
fn export_names_refuses_a_file_that_exists_and_changes_nothing() {
    let held = scratch("exists");
    let dir = held.path();
    std::fs::write(dir.join("names.txt"), "kept\n").unwrap();
    refused(dir, &["--out", "names.txt"], "REFUSED exists: names.txt");
    assert_eq!(
        std::fs::read_to_string(dir.join("names.txt")).unwrap(),
        "kept\n"
    );
    std::fs::create_dir(dir.join("sub")).unwrap();
    refused(dir, &["--out", "sub"], "REFUSED exists: sub");
}

#[test]
fn export_names_makes_no_directory_and_refuses_a_missing_one_as_output() {
    let held = scratch("output");
    let dir = held.path();
    let file = Path::new("missing").join("names.txt");
    let shown = file.to_str().unwrap();
    refused(dir, &["--out", shown], &format!("REFUSED output: {shown}"));
    assert!(!dir.join("missing").exists());
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

struct Leaf(PathBuf);

impl Leaf {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Leaf {
    fn drop(&mut self) {
        if let Some(line) = remove(&self.0) {
            eprintln!("{line}");
        }
    }
}

fn remove(dir: &Path) -> Option<String> {
    let error = std::fs::remove_dir_all(dir).err()?;
    Some(format!("leaf not removed: {}: {error}", dir.display()))
}

fn leaf_name(word: &str, pid: u32, clock: u128, count: u64) -> String {
    format!("{word}-{pid}-{clock}-{count}")
}

fn make_leaf(dir: PathBuf) -> Result<Leaf, String> {
    let parent = dir
        .parent()
        .ok_or_else(|| format!("no parent: {}", dir.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(Leaf(dir))
}

fn leaf(word: &str) -> Leaf {
    static CLOCK: OnceLock<u128> = OnceLock::new();
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let clock = *CLOCK.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    });
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    let name = leaf_name(word, std::process::id(), clock, count);
    make_leaf(Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)).unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn leaf_two_calls_give_two_paths() {
    let first = leaf("export-names-leaf");
    let second = leaf("export-names-leaf");
    assert_ne!(first.path(), second.path());
    assert!(first.path().is_dir() && second.path().is_dir());
    let stem = |held: &Leaf| {
        let name = held
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        name[..name.rfind('-').unwrap()].to_owned()
    };
    assert_eq!(stem(&first), stem(&second));
}

#[test]
fn leaf_name_differs_by_the_clock_reading_the_counter_and_the_process_id() {
    assert_ne!(leaf_name("w", 7, 1, 0), leaf_name("w", 7, 2, 0));
    assert_ne!(leaf_name("w", 7, 1, 0), leaf_name("w", 7, 1, 1));
    assert_ne!(leaf_name("w", 7, 1, 0), leaf_name("w", 8, 1, 0));
    assert!(leaf_name("w", 7, 1, 0).starts_with("w-7-"));
}

#[test]
fn leaf_that_exists_fails_with_its_path() {
    let first = leaf("export-names-leaf");
    let error = make_leaf(first.path().to_path_buf())
        .err()
        .expect("a second making of one leaf fails");
    assert!(
        error.contains(&first.path().display().to_string()),
        "{error}"
    );
}

#[test]
fn leaf_is_gone_after_its_drop() {
    let held = leaf("export-names-leaf");
    let path = held.path().to_path_buf();
    std::fs::write(path.join("file"), "x").unwrap();
    drop(held);
    assert!(!path.exists(), "{}", path.display());
}

#[test]
fn leaf_removal_that_fails_gives_one_line_with_the_path() {
    let held = leaf("export-names-leaf");
    let path = held.path().to_path_buf();
    drop(held);
    let line = remove(&path).expect("a line for a removal that fails");
    assert!(
        line.contains(&path.display().to_string()) && !line.contains('\n'),
        "{line}"
    );
}

#[test]
fn leaf_lies_under_the_target_directory() {
    let held = leaf("export-names-leaf");
    assert!(
        held.path().starts_with(env!("CARGO_TARGET_TMPDIR")),
        "{}",
        held.path().display()
    );
}

const LEAF_CHILD: &str = "HALVO_LEAF_CHILD";

fn leaf_child(test: &str, role: &str, nocapture: bool) -> std::process::Output {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([test, "--exact", "--test-threads=1"])
        .env(LEAF_CHILD, role);
    if nocapture {
        command.arg("--nocapture");
    }
    let out = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("test result: ok. 1 passed;"),
        "{role}: {}\n{stdout}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn leaf_parts(path: &Path) -> (String, u32, u128, u64) {
    let name = path.file_name().unwrap().to_str().unwrap();
    let mut parts = name.rsplitn(4, '-');
    let count = parts.next().unwrap_or_default().parse();
    let clock = parts.next().unwrap_or_default().parse();
    let pid = parts.next().unwrap_or_default().parse();
    let word = parts.next().unwrap_or_default().to_owned();
    match (pid, clock, count) {
        (Ok(pid), Ok(clock), Ok(count)) => (word, pid, clock, count),
        _ => panic!("not word-pid-clock-count: {name}"),
    }
}

fn leaf_clock_now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[test]
fn leaf_from_leaf_holds_the_word_the_process_id_and_the_clock() {
    if std::env::var(LEAF_CHILD).as_deref() != Ok("clock") {
        leaf_child(
            "leaf_from_leaf_holds_the_word_the_process_id_and_the_clock",
            "clock",
            false,
        );
        return;
    }
    let before = leaf_clock_now();
    let held = leaf("export-names-leaf-clock");
    let after = leaf_clock_now();
    let (word, pid, clock, _) = leaf_parts(held.path());
    assert_eq!(word, "export-names-leaf-clock");
    assert_eq!(pid, std::process::id());
    assert!(
        before <= clock && clock <= after,
        "{before} {clock} {after}"
    );
}

#[test]
fn leaf_refuses_the_directory_that_the_next_leaf_names() {
    if std::env::var(LEAF_CHILD).as_deref() != Ok("next") {
        leaf_child(
            "leaf_refuses_the_directory_that_the_next_leaf_names",
            "next",
            false,
        );
        return;
    }
    let first = leaf("export-names-leaf-next");
    let (word, pid, clock, count) = leaf_parts(first.path());
    let next = first
        .path()
        .with_file_name(format!("{word}-{pid}-{clock}-{}", count + 1));
    std::fs::create_dir(&next).unwrap();
    let _next = Leaf(next.clone());
    std::fs::write(next.join("kept"), "kept\n").unwrap();
    let caught = std::panic::catch_unwind(|| leaf("export-names-leaf-next"));
    let message = caught
        .err()
        .and_then(|payload| payload.downcast::<String>().ok())
        .expect("leaf() fails on the directory that it names");
    assert!(message.contains(&next.display().to_string()), "{message}");
    assert_eq!(
        std::fs::read_to_string(next.join("kept")).unwrap(),
        "kept\n"
    );
    assert_eq!(std::fs::read_dir(&next).unwrap().count(), 1);
}

#[test]
fn leaf_whose_path_is_gone_drops_without_a_panic() {
    let held = leaf("export-names-leaf-gone");
    std::fs::remove_dir_all(held.path()).unwrap();
    let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(held)));
    assert!(dropped.is_ok());
}
