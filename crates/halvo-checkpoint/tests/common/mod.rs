#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

pub const RUN: &str = "0123456789abcdef0123456789abcdef";

pub struct Scratch {
    pub dir: PathBuf,
}

#[test]
fn two_scratches_of_one_name_are_two_directories_and_the_second_is_empty() {
    let first = Scratch::new("one-name");
    fs::write(first.path("mine"), b"first").expect("write into the first");
    let second = Scratch::new("one-name");
    assert_ne!(first.dir, second.dir);
    assert_eq!(fs::read_dir(&second.dir).expect("list").count(), 0);
}

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join("halvo-checkpoint");
        let dir = halvo_test_dir::make(&base, name).expect("make the scratch directory");
        let dir = dir.canonicalize().expect("the canonical scratch directory");
        Scratch { dir }
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    pub fn write(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("make the parent directory");
        }
        fs::write(&path, bytes).expect("write the file");
        path
    }

    pub fn names(&self, rel: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.dir.join(rel))
            .expect("list the directory")
            .map(|entry| {
                entry
                    .expect("read the entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

#[cfg(unix)]
pub fn link_file(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("make the link");
}

#[cfg(windows)]
pub fn link_file(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_file(target, link).expect("make the link");
}

#[cfg(unix)]
pub fn link_dir(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("make the link");
}

#[cfg(windows)]
pub fn link_dir(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).expect("make the link");
}

pub fn folds_case(dir: &Path) -> bool {
    let probe = dir.join("case-probe.txt");
    fs::write(&probe, b"x").expect("write the probe");
    let folds = dir.join("CASE-PROBE.TXT").exists();
    fs::remove_file(&probe).expect("remove the probe");
    folds
}
