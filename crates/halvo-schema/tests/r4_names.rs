use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use halvo_schema::coordinator::names::{
    BindResult, CheckAnswer, ConfirmLapse, DoneAnswer, EffectAnswer, Event, HoldCause,
    HoldSetCause, ItemKind, OtherKind, OtherLapse, ReadyAnswer, RunEnd, SetupAnswer, Trigger,
};
use halvo_schema::coordinator::{ERROR_NAMES, KINDS, NAMES};

const LEFT_OUT: [&str; 1] = ["role"];

fn committed() -> BTreeSet<String> {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/names.txt"))
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn derived() -> BTreeSet<String> {
    let mut names: BTreeSet<String> = KINDS
        .iter()
        .flat_map(|k| {
            [k.name, k.family]
                .into_iter()
                .chain(k.fields.iter().map(|(key, _)| *key))
        })
        .map(str::to_owned)
        .collect();
    let lists: Vec<Vec<&str>> = vec![
        vec![
            "coord_log",
            "coord_history",
            "op",
            "records",
            "stored",
            "after",
            "last",
        ],
        Event::ALL.iter().map(|v| v.as_str()).collect(),
        ItemKind::ALL.iter().map(|v| v.as_str()).collect(),
        OtherKind::ALL.iter().map(|v| v.as_str()).collect(),
        Trigger::ALL.iter().map(|v| v.as_str()).collect(),
        RunEnd::ALL.iter().map(|v| v.as_str()).collect(),
        ConfirmLapse::ALL.iter().map(|v| v.as_str()).collect(),
        OtherLapse::ALL.iter().map(|v| v.as_str()).collect(),
        HoldSetCause::ALL.iter().map(|v| v.as_str()).collect(),
        HoldCause::ALL.iter().map(|v| v.as_str()).collect(),
        EffectAnswer::ALL.iter().map(|v| v.as_str()).collect(),
        DoneAnswer::ALL.iter().map(|v| v.as_str()).collect(),
        BindResult::ALL.iter().map(|v| v.as_str()).collect(),
        CheckAnswer::ALL.iter().map(|v| v.as_str()).collect(),
        SetupAnswer::ALL.iter().map(|v| v.as_str()).collect(),
        ReadyAnswer::ALL.iter().map(|v| v.as_str()).collect(),
    ];
    names.extend(lists.concat().into_iter().map(str::to_owned));
    for name in LEFT_OUT {
        names.remove(name);
    }
    names
}

#[test]
fn r4_names_list_holds_every_key_and_closed_list_name_of_the_kind_and_the_bodies() {
    let listed: BTreeSet<String> = NAMES.iter().map(|s| (*s).to_owned()).collect();
    assert_eq!(listed.len(), NAMES.len(), "a name stands twice in NAMES");
    let derived = derived();
    let extra: Vec<&String> = listed.difference(&derived).collect();
    let missing: Vec<&String> = derived.difference(&listed).collect();
    assert!(
        extra.is_empty() && missing.is_empty(),
        "names that the rules do not give: {extra:?}; names that NAMES lacks: {missing:?}"
    );
    assert!(!NAMES.contains(&"none"), "setup none stores no row");
    assert!(!NAMES.contains(&"binding"));
    for name in ERROR_NAMES {
        assert_eq!(NAMES.contains(name), *name == "seq", "{name}");
    }
}

#[test]
fn r4_names_the_committed_names_file_holds_every_name_of_the_run() {
    let file = committed();
    for name in NAMES {
        assert!(file.contains(*name), "{name}");
    }
    for name in LEFT_OUT.iter().chain(&["binding"]) {
        assert!(!file.contains(*name), "{name}");
    }
}

#[test]
fn r4_names_the_names_tool_writes_the_committed_file_with_the_names_of_the_interface_run() {
    let held = leaf("r4-export-names");
    let out = held.path().join("names.txt");
    let run = Command::new(env!("CARGO_BIN_EXE_export-names"))
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(0));
    let text = std::fs::read_to_string(&out).unwrap();
    let written: BTreeSet<String> = text.lines().map(str::to_owned).collect();
    for name in NAMES {
        assert!(written.contains(*name), "{name}");
    }
    let committed_text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/names.txt"),
    )
    .unwrap();
    assert_eq!(text, committed_text);
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
    let first = leaf("r4-names-leaf");
    let second = leaf("r4-names-leaf");
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
    let first = leaf("r4-names-leaf");
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
    let held = leaf("r4-names-leaf");
    let path = held.path().to_path_buf();
    std::fs::write(path.join("file"), "x").unwrap();
    drop(held);
    assert!(!path.exists(), "{}", path.display());
}

#[test]
fn leaf_removal_that_fails_gives_one_line_with_the_path() {
    let held = leaf("r4-names-leaf");
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
    let held = leaf("r4-names-leaf");
    assert!(
        held.path().starts_with(env!("CARGO_TARGET_TMPDIR")),
        "{}",
        held.path().display()
    );
}
