#![cfg(target_os = "macos")]

use r2_probes::sys::{version_of, Browser};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn write_browser(path: &Path, version: &str) {
    std::fs::write(
        path,
        format!("#!/bin/sh\necho 'Stand-in Browser {version}'\n"),
    )
    .expect("write the stand-in browser");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .expect("make the stand-in browser executable");
}

#[test]
fn a_browser_version_is_read_from_the_build_on_disk_at_each_call() {
    let held = leaf("r2-probes-browser-version");
    let path = held.path().join("browser");
    write_browser(&path, "157.0.8103.39");
    let browser = Browser {
        name: "chrome".to_string(),
        path: path.display().to_string(),
        version: "157.0.8103.39".to_string(),
    };
    assert_eq!(version_of(&browser), "157.0.8103.39");
    write_browser(&path, "157.0.8103.40");
    assert_eq!(version_of(&browser), "157.0.8103.40");
}

struct Leaf(PathBuf);

impl Leaf {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Leaf {
    fn drop(&mut self) {
        if let Some(line) = remove(&self.0) {
            let _ = std::io::Write::write_fmt(&mut std::io::stderr(), format_args!("{line}\n"));
        }
    }
}

fn remove(dir: &Path) -> Option<String> {
    let error = std::fs::remove_dir_all(dir).err()?;
    Some(format!("leaf not removed: {}: {error}", dir.display()))
}

fn leaf(word: &str) -> Leaf {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    match halvo_test_dir::make(tmp, word) {
        Ok(dir) => Leaf(dir),
        Err(e) => panic!("{word} under {}: {e}", tmp.display()),
    }
}

#[test]
fn leaf_two_calls_give_two_paths() {
    let first = leaf("r2-probes-browser-version-leaf");
    let second = leaf("r2-probes-browser-version-leaf");
    assert_ne!(first.path(), second.path());
    assert!(first.path().is_dir() && second.path().is_dir());
    let stem = |held: &Leaf| {
        let name = held
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        name[..name.rfind('-').unwrap()].to_owned()
    };
    assert_eq!(stem(&first), stem(&second));
}

#[test]
fn leaf_is_gone_after_its_drop() {
    let held = leaf("r2-probes-browser-version-leaf");
    let path = held.path().to_path_buf();
    std::fs::write(path.join("file"), "x").unwrap();
    drop(held);
    assert!(!path.exists(), "{}", path.display());
}

#[test]
fn leaf_removal_that_fails_gives_one_line_with_the_path() {
    let held = leaf("r2-probes-browser-version-leaf");
    let path = held.path().to_path_buf();
    drop(held);
    let line = remove(&path).expect("a line for a removal that fails");
    assert!(
        line.contains(&path.display().to_string()) && !line.contains('\n'),
        "{line}"
    );
}

#[test]
fn leaf_lies_under_the_target_directory_and_its_name_begins_with_the_word() {
    let held = leaf("r2-probes-browser-version-leaf");
    assert_eq!(
        held.path().parent(),
        Some(Path::new(env!("CARGO_TARGET_TMPDIR"))),
        "{}",
        held.path().display()
    );
    let name = held.path().file_name().unwrap().to_str().unwrap();
    assert!(
        name.starts_with("r2-probes-browser-version-leaf-"),
        "{name}"
    );
}
