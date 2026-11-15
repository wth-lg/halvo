mod support;

/// CP-207 K5: `scratch` of tests/support, called while directories that hold a file stand under the next names of
/// its count (as an earlier process with the same process id leaves them), passes over each and gives a new, empty
/// directory, and leaves the standing ones as they were.
#[test]
fn scratch_passes_over_a_standing_name() {
    let tag = "checkpoint-standing";
    let first = support::scratch(tag);
    let n: u32 = first
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.rsplit('-').next())
        .and_then(|count| count.parse().ok())
        .expect("the count at the end of the name");
    let root = first.parent().expect("the root").to_path_buf();
    let standing: Vec<std::path::PathBuf> = (n + 1..=n + 64)
        .map(|k| {
            let dir = root.join(format!("runner-{tag}-{}-{k}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("a standing directory");
            std::fs::write(dir.join("left"), b"an earlier process").expect("a file in it");
            dir
        })
        .collect();
    let next = support::scratch(tag);
    assert!(
        !standing.contains(&next) && next != first,
        "a directory of its own: {}",
        next.display()
    );
    assert_eq!(
        std::fs::read_dir(&next).expect("list").count(),
        0,
        "the directory is made anew and empty"
    );
    for dir in &standing {
        assert!(
            dir.join("left").is_file(),
            "{} is left as it was",
            dir.display()
        );
    }
}
