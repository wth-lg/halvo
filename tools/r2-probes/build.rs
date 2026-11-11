use std::path::{Path, PathBuf};
use std::process::Command;

const CODE: [&str; 4] = ["src", "Cargo.toml", "Cargo.lock", "build.rs"];

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn main() {
    let dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    for path in CODE {
        println!("cargo:rerun-if-changed={path}");
    }
    let commit = match git(&dir, &["rev-parse", "HEAD"]) {
        Some(head) => {
            let mut status = vec!["status", "--porcelain", "--"];
            status.extend(CODE);
            let changed = git(&dir, &status).is_none_or(|out| !out.is_empty());
            if changed {
                format!("{head}-dirty")
            } else {
                head
            }
        }
        None => "unknown".to_string(),
    };
    let mut watched: Vec<PathBuf> = Vec::new();
    if let Some(git_dir) = git(&dir, &["rev-parse", "--git-dir"]) {
        watched.push(dir.join(git_dir).join("HEAD"));
    }
    if let Some(common) = git(&dir, &["rev-parse", "--git-common-dir"]) {
        let common = dir.join(common);
        watched.push(common.join("packed-refs"));
        if let Some(branch) = git(&dir, &["symbolic-ref", "-q", "HEAD"]) {
            watched.push(common.join(branch));
        }
    }
    for path in watched.iter().filter(|path| path.exists()) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    println!("cargo:rustc-env=R2_PROBES_COMMIT={commit}");
}
