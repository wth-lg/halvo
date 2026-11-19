use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

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
            let _ = std::io::Write::write_fmt(&mut std::io::stderr(), format_args!("{line}\n"));
        }
    }
}

fn remove(dir: &Path) -> Option<String> {
    let error = std::fs::remove_dir_all(dir).err()?;
    Some(format!("leaf not removed: {}: {error}", dir.display()))
}

fn leaf(word: &str) -> Leaf {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    match halvo_test_dir::make(tmp, word) {
        Ok(dir) => Leaf(dir),
        Err(e) => panic!("{word} under {}: {e}", tmp.display()),
    }
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
fn leaf_lies_under_the_target_directory_and_its_name_begins_with_the_word() {
    let held = leaf("r5-names-leaf");
    assert_eq!(
        held.path().parent(),
        Some(Path::new(env!("CARGO_TARGET_TMPDIR"))),
        "{}",
        held.path().display()
    );
    let name = held.path().file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("r5-names-leaf-"), "{name}");
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

#[test]
fn leaf_passes_over_the_directory_that_the_next_leaf_names() {
    if std::env::var(LEAF_CHILD).as_deref() != Ok("next") {
        leaf_child(
            "leaf_passes_over_the_directory_that_the_next_leaf_names",
            "next",
            false,
        );
        return;
    }
    let first = leaf("r5-names-leaf-next");
    let name = first.path().file_name().unwrap().to_str().unwrap();
    let (stem, count) = name.rsplit_once('-').unwrap();
    let count: u64 = count.parse().unwrap();
    let next = first.path().with_file_name(format!("{stem}-{}", count + 1));
    std::fs::create_dir(&next).unwrap();
    let _next = Leaf(next.clone());
    std::fs::write(next.join("kept"), "kept\n").unwrap();
    let second = leaf("r5-names-leaf-next");
    assert!(second.path().is_dir(), "{}", second.path().display());
    assert_ne!(second.path(), next);
    assert_ne!(second.path(), first.path());
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

#[test]
fn leaf_removal_line_reaches_the_standard_error_of_the_process() {
    if std::env::var(LEAF_CHILD).as_deref() == Ok("line") {
        let held = leaf("r5-names-leaf-line");
        std::fs::remove_dir_all(held.path()).unwrap();
        drop(held);
        return;
    }
    let place = Path::new(env!("CARGO_TARGET_TMPDIR")).join("r5-names-leaf-line-");
    for nocapture in [false, true] {
        let out = leaf_child(
            "leaf_removal_line_reaches_the_standard_error_of_the_process",
            "line",
            nocapture,
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        let lines: Vec<&str> = stderr
            .lines()
            .filter(|line| line.starts_with("leaf not removed: "))
            .collect();
        assert_eq!(lines.len(), 1, "nocapture {nocapture}: {stderr}");
        assert!(
            lines[0].contains(&place.display().to_string()),
            "{}",
            lines[0]
        );
    }
}
