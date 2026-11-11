#![cfg(target_os = "macos")]

use r2_probes::sys::{version_of, Browser};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

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
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("browser-version");
    std::fs::create_dir_all(&dir).expect("test directory");
    let path = dir.join("browser");
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
