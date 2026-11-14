use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use halvo_schema::executable::names::{LocatorBy, Mode};
use halvo_schema::learning::names::{
    EditCause, Mark, RefinementKind, RunCause, EDIT_STATES, RUN_STATES,
};
use halvo_schema::learning::{ERROR_NAMES, NAMES};
use halvo_schema::site::DomainSource;
use halvo_schema::SeedCategory;

const LINES: [&str; 5] = [
    r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","edit":"5c4b3a29180f7e6d5c4b3a29180f7e6d","rev":1,"base":1,"n":4,"state":"provisional","cause":null,"on":null,"runs":["9f8e7d6c5b4a39281706f5e4d3c2b1a0"],"first":20767,"last":20767,"clean":0,"target":{"desc":"button \"Submit payment\"","locators":[{"by":"role","value":"button","name":"Submit payment"}]}}"#,
    r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","rev":3,"day":20767,"category":"utility_bill_payment","site":{"key":"northwind-power.test","source":"address_field"},"app":"com.google.Chrome","period":30,"seen":3,"first":20705,"last":20765,"next":20795,"missed":0,"mark":"none","since":null}"#,
    r#"{"v":1,"workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809","run":"9f8e7d6c5b4a39281706f5e4d3c2b1a0","rev":2,"day":20767,"mode":"replay","state":"verified","cause":null,"base":1,"assisted":1,"kept":1,"failed_step":null}"#,
    r#"{"op":"list_refinements","workflow":"0f1e2d3c4b5a69788796a5b4c3d2e1f0","agent":"1a2b3c4d5e6f708192a3b4c5d6e7f809"}"#,
    r#"{"refinements":[{"day":20767,"what":"target_learned"}]}"#,
];

const LEFT_OUT: [&str; 1] = ["role"];

fn keys_of(value: &serde_json::Value, out: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, inner) in map {
                out.insert(key.clone());
                keys_of(inner, out);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|item| keys_of(item, out)),
        _ => {}
    }
}

fn derived() -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in LINES {
        keys_of(&serde_json::from_str(line).unwrap(), &mut names);
    }
    let lists: Vec<Vec<&str>> = vec![
        vec![
            "learn",
            "learn_edit",
            "learn_run",
            "learn_routine",
            "list_refinements",
        ],
        RunCause::ALL.iter().map(|v| v.as_str()).collect(),
        EditCause::ALL.iter().map(|v| v.as_str()).collect(),
        Mark::ALL.iter().map(|v| v.as_str()).collect(),
        RefinementKind::ALL.iter().map(|v| v.as_str()).collect(),
        RUN_STATES.to_vec(),
        EDIT_STATES.to_vec(),
        Mode::ALL.iter().map(|v| v.as_str()).collect(),
        LocatorBy::ALL.iter().map(|v| v.as_str()).collect(),
    ];
    names.extend(lists.concat().into_iter().map(str::to_owned));
    for name in LEFT_OUT {
        names.remove(name);
    }
    names
}

fn committed() -> BTreeSet<String> {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/names.txt"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_names_list_holds_every_key_and_closed_list_name_of_the_three_kinds_and_the_body() {
    let listed: BTreeSet<String> = NAMES.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(listed.len(), NAMES.len(), "a name stands twice in NAMES");
    let derived = derived();
    let extra: Vec<&String> = listed.difference(&derived).collect();
    let missing: Vec<&String> = derived.difference(&listed).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "names that the rules do not give: {extra:?}; names that NAMES lacks: {missing:?}"
    );
    for name in LEFT_OUT.iter().chain(ERROR_NAMES) {
        assert!(!NAMES.contains(name), "{name}");
    }
}

#[test]
fn the_committed_names_file_holds_every_name_of_the_kinds_and_of_the_read_and_none_left_out() {
    let file = committed();
    for name in NAMES {
        assert!(file.contains(*name), "{name}");
    }
    let values_of_other_lists: Vec<&str> = SeedCategory::ALL
        .iter()
        .map(|v| v.as_str())
        .chain(DomainSource::ALL.iter().map(|v| v.as_str()))
        .chain(["title"])
        .collect();
    for name in values_of_other_lists {
        assert!(file.contains(name), "{name}");
    }
    for name in LEFT_OUT.iter().chain(ERROR_NAMES) {
        assert!(!file.contains(*name), "{name}");
    }
}

#[test]
fn the_names_tool_writes_the_names_of_the_learning_run() {
    let held = leaf("r5-export-names");
    let out = held.path().join("names.txt");
    let run = Command::new(env!("CARGO_BIN_EXE_export-names"))
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(0));
    let written: BTreeSet<String> = std::fs::read_to_string(&out)
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect();
    for name in NAMES {
        assert!(written.contains(*name), "{name}");
    }
    assert_eq!(written, committed());
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
    let first = leaf("r5-names-leaf");
    let second = leaf("r5-names-leaf");
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
    let first = leaf("r5-names-leaf");
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
    let held = leaf("r5-names-leaf");
    let path = held.path().to_path_buf();
    std::fs::write(path.join("file"), "x").unwrap();
    drop(held);
    assert!(!path.exists(), "{}", path.display());
}

#[test]
fn leaf_removal_that_fails_gives_one_line_with_the_path() {
    let held = leaf("r5-names-leaf");
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
    let held = leaf("r5-names-leaf");
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
    let held = leaf("r5-names-leaf-clock");
    let after = leaf_clock_now();
    let (word, pid, clock, _) = leaf_parts(held.path());
    assert_eq!(word, "r5-names-leaf-clock");
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
    let first = leaf("r5-names-leaf-next");
    let (word, pid, clock, count) = leaf_parts(first.path());
    let next = first
        .path()
        .with_file_name(format!("{word}-{pid}-{clock}-{}", count + 1));
    std::fs::create_dir(&next).unwrap();
    let _next = Leaf(next.clone());
    std::fs::write(next.join("kept"), "kept\n").unwrap();
    let caught = std::panic::catch_unwind(|| leaf("r5-names-leaf-next"));
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
    let held = leaf("r5-names-leaf-gone");
    std::fs::remove_dir_all(held.path()).unwrap();
    let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(held)));
    assert!(dropped.is_ok());
}
