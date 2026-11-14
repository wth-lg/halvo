#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(args: &[&str]) -> Out {
    let o = Command::new(env!("CARGO_BIN_EXE_halvo-sample"))
        .args(args)
        .output()
        .expect("the binary runs");
    Out {
        code: o.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

pub fn run_in(dir: &Path, args: &[&str]) -> Out {
    let o = Command::new(env!("CARGO_BIN_EXE_halvo-sample"))
        .args(args)
        .current_dir(dir)
        .output()
        .expect("the binary runs");
    Out {
        code: o.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
    }
}

/// A new, empty directory under the target's temporary directory for one test. A name that an earlier process
/// with the same id left is skipped, so a repeated process id never hands a test another run's files.
pub fn scratch(name: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(base).expect("the target's temporary directory");
    let mut k = 0u64;
    loop {
        let dir = base.join(format!(
            "halvo-sample-{name}-{}-{n}-{k}",
            std::process::id()
        ));
        match fs::create_dir(&dir) {
            Ok(()) => return dir,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => k += 1,
            Err(e) => panic!("scratch directory: {e}"),
        }
    }
}

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn golden() -> PathBuf {
    repo().join("fixtures/r2/golden")
}

pub fn s(p: &Path) -> String {
    p.to_str().expect("utf-8 path").to_owned()
}

pub fn refused(o: &Out, cause: &str) {
    assert_eq!(o.code, 2, "stdout {} stderr {}", o.stdout, o.stderr);
    assert!(
        o.stdout.is_empty(),
        "nothing on standard output: {}",
        o.stdout
    );
    assert!(
        o.stderr.starts_with(&format!("REFUSED {cause}: ")),
        "cause {cause}: {}",
        o.stderr
    );
    assert_eq!(o.stderr.lines().count(), 1);
}

pub fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("copy target");
    for e in fs::read_dir(from).expect("read dir") {
        let e = e.expect("entry");
        let t = to.join(e.file_name());
        if e.file_type().expect("type").is_dir() {
            copy_dir(&e.path(), &t);
        } else {
            fs::copy(e.path(), &t).expect("copy");
        }
    }
}

/// Every file under `dir`, as paths below it with `/`, sorted.
pub fn files_under(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, d: &Path, out: &mut Vec<String>) {
        for e in fs::read_dir(d).expect("read dir") {
            let e = e.expect("entry");
            if e.file_type().expect("type").is_dir() {
                walk(base, &e.path(), out);
            } else {
                let rel = e.path().strip_prefix(base).expect("below").to_owned();
                out.push(
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/"),
                );
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

/// Builds the golden script into `dir/name` and returns the output directory.
pub fn build_golden(dir: &Path, name: &str) -> PathBuf {
    let out = dir.join(name);
    let g = golden();
    let o = run(&[
        "build",
        "--seed",
        "1",
        "--out",
        &s(&out),
        &s(&g.join("script.jsonl")),
        &s(&g.join("marked.jsonl")),
    ]);
    assert_eq!(o.code, 0, "{}", o.stderr);
    out
}

/// The header line of a test script.
pub const HEADER: &str = r#"{"line":"script","v":1,"day1":1767225600000000000,"canaries":[]}"#;

/// A one-step web line of a test script.
pub fn step_line(id: &str, site: &str, title: &str, password: bool) -> String {
    format!(
        r#"{{"line":"step","step":"{id}","app":"org.chromium.Chromium","site":"{site}","titles":["{title}"],"paths":["/p"],"password_input":{password},"card_title":false}}"#
    )
}

pub fn item(step: &str, start: i64, end: i64, keys: u64, clicks: u64, scrolls: u64) -> String {
    format!(
        r#"{{"step":"{step}","title":0,"start":{start},"end":{end},"keys":{keys},"clicks":{clicks},"scrolls":{scrolls}}}"#
    )
}

pub fn episode(
    stream: &str,
    id: &str,
    task: &str,
    category: &str,
    kind: &str,
    day: u32,
    items: &[String],
) -> String {
    format!(
        r#"{{"line":"episode","stream":"{stream}","episode":"{id}","task":"{task}","category":"{category}","kind":"{kind}","day":{day},"ranges":[{}]}}"#,
        items.join(",")
    )
}

pub fn write(path: &Path, text: &str) {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p).expect("parent");
    }
    fs::write(path, text).expect("write");
}

pub const SEC: i64 = 1_000_000_000;

/// Makes `link` a symbolic link to the directory `target`, which need not exist.
pub fn link_dir(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("a symbolic link");
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, link).expect("a symbolic link");
}
