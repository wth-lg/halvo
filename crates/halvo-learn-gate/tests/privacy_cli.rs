use std::path::{Path, PathBuf};
use std::process::Command;

fn fresh(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("privacy-cli");
    halvo_test_dir::make(&root, name).expect("a test directory")
}

#[test]
fn learn_outside_misses_when_the_temporary_directory_of_the_process_does_not_exist() {
    let data = fresh("data");
    let missing = fresh("temp").join("missing");
    assert!(!missing.exists());
    let out = Command::new(env!("CARGO_BIN_EXE_learn-gate"))
        .args(["privacy", "--data-dir", data.to_str().expect("a path")])
        .env("TMPDIR", &missing)
        .env("TMP", &missing)
        .env("TEMP", &missing)
        .output()
        .expect("learn-gate runs");
    let text = String::from_utf8(out.stdout).expect("UTF-8");
    assert_eq!(
        text,
        "privacy check=learn_listed result=not-run\n\
         privacy check=learn_deleted result=ok\n\
         privacy check=learn_scrub result=not-run\n\
         privacy check=learn_delete_all result=not-run\n\
         privacy check=learn_outside result=miss\n\
         privacy check=learn_expired result=not-run\n"
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(
        out.stderr.is_empty(),
        "{:?}",
        String::from_utf8_lossy(&out.stderr)
    );
}
