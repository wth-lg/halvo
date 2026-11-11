use std::path::PathBuf;
use std::process::{Command, Output};

const EXPECTED: &str = include_str!("../expected.txt");
const TARGETS: &str = include_str!("../../../../docs/r5/targets.toml");

fn run_in(case: &str, targets: &str) -> Output {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("r5-stat-exit")
        .join(case);
    let write = |rel: &str, text: &str| {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write("tools/r5-trials/stat/expected.txt", EXPECTED);
    write("docs/r5/targets.toml", targets);
    Command::new(env!("CARGO_BIN_EXE_r5-stat"))
        .current_dir(&root)
        .output()
        .unwrap()
}

#[test]
fn the_binary_exits_0_at_ok() {
    let out = run_in("ok", TARGETS);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stdout.ends_with(b"verdict stat=ok\n"));
}

#[test]
fn the_binary_exits_1_at_miss() {
    let out = run_in(
        "miss",
        &TARGETS.replacen("dev_critical = 7", "dev_critical = 6", 1),
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.ends_with(b"verdict stat=miss\n"));
}

#[test]
fn the_binary_exits_2_at_a_refusal() {
    let out = run_in(
        "refusal",
        "[gate]\ndev_critical = 7\nheldout_critical = 10\n",
    );
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert_eq!(
        out.stderr,
        b"REFUSED input: targets line 1 is not of its form\n"
    );
}
