#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub const RUN: &str = "0123456789abcdef0123456789abcdef";

pub struct Scratch {
    pub dir: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("halvo-checkpoint")
            .join(format!("{name}-{}-{nanos}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("make the scratch directory");
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
