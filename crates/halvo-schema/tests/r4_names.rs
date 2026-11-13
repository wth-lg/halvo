use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

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
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("r4-export-names-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("names.txt");
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
    std::fs::remove_dir_all(&dir).unwrap();
}
