use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

const THREADS: usize = 64;

fn base() -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("halvo-test-dir");
    halvo_test_dir::make(&root, "threads").expect("a base of its own")
}

#[test]
fn two_threads_at_one_clock_value_get_two_directories() {
    let base = base();
    let start = Arc::new(Barrier::new(THREADS));
    let threads: Vec<_> = (0..THREADS)
        .map(|i| {
            let start = Arc::clone(&start);
            let base = base.clone();
            std::thread::spawn(move || {
                start.wait();
                let dir = halvo_test_dir::make(&base, "same").expect("a directory");
                fs::write(dir.join(format!("thread-{i}")), b"mine").expect("write its file");
                dir
            })
        })
        .collect();
    let mut dirs: Vec<PathBuf> = threads
        .into_iter()
        .map(|t| t.join().expect("the thread"))
        .collect();
    for (i, dir) in dirs.iter().enumerate() {
        let names: Vec<String> = fs::read_dir(dir)
            .expect("list the directory")
            .map(|e| {
                e.expect("an entry")
                    .file_name()
                    .into_string()
                    .expect("utf-8")
            })
            .collect();
        assert_eq!(names, vec![format!("thread-{i}")], "{}", dir.display());
    }
    dirs.sort();
    dirs.dedup();
    assert_eq!(dirs.len(), THREADS);
}

#[test]
fn make_gives_a_new_empty_directory_under_the_base_named_with_the_process_id() {
    let base = base();
    let dir = halvo_test_dir::make(&base, "one").expect("a directory");
    assert_eq!(dir.parent(), Some(base.as_path()));
    assert!(dir.is_dir());
    assert_eq!(fs::read_dir(&dir).expect("list").count(), 0);
    let name = dir.file_name().and_then(|n| n.to_str()).expect("a name");
    let prefix = format!("one-{}-", std::process::id());
    assert!(name.starts_with(&prefix), "{name}");
    assert!(name[prefix.len()..].parse::<u64>().is_ok(), "{name}");
}
