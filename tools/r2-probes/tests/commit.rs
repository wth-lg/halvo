use r2_probes::suite::extraction;
use std::process::Command;

fn git(args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?}");
    String::from_utf8(output.stdout)
        .expect("UTF-8")
        .trim()
        .to_string()
}

#[test]
fn an_extraction_record_names_the_commit_that_the_probe_was_built_from() {
    let record = extraction("list-1", &["row".to_string()], 1.0, 1.0, 1.0, (1920, 1080));
    let commit = record
        .fields
        .iter()
        .find(|field| field.key == "probe_commit")
        .and_then(|field| field.value.as_str())
        .expect("the record has probe_commit");
    let head = git(&["rev-parse", "HEAD"]);
    let changed = git(&[
        "status",
        "--porcelain",
        "--",
        "src",
        "Cargo.toml",
        "Cargo.lock",
        "build.rs",
    ]);
    let want = if changed.is_empty() {
        head
    } else {
        format!("{head}-dirty")
    };
    assert_eq!(commit, want);
}
