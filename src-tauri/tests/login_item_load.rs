#![cfg(target_os = "macos")]

use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const NAME: &str = "a_scratch_login_item_of_the_built_app_loads_in_launchd_and_goes_out_again";
const LAUNCHCTL: &str = "/bin/launchctl";
const NOT_LOADED: i32 = 113;
const GONE_WAIT: Duration = Duration::from_millis(halvo_control::STOP_WAIT_MS);

fn say(line: &str) {
    let _ = std::io::stderr().write_all(format!("{NAME}: {line}\n").as_bytes());
}

fn launchctl(args: &[&str]) -> std::io::Result<Output> {
    Command::new(LAUNCHCTL)
        .args(args)
        .stdin(Stdio::null())
        .output()
}

fn status_of(args: &[&str]) -> (Option<i32>, String) {
    let out = launchctl(args).unwrap_or_else(|e| panic!("{LAUNCHCTL} does not start: {e}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code(), text)
}

struct TakenOut(String);

impl Drop for TakenOut {
    fn drop(&mut self) {
        match launchctl(&["bootout", &self.0]) {
            Ok(out) => say(&format!(
                "bootout {} answered {:?}: {}",
                self.0,
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            )),
            Err(e) => say(&format!("bootout {} did not start: {e}", self.0)),
        }
    }
}

#[test]
fn a_scratch_login_item_of_the_built_app_loads_in_launchd_and_goes_out_again() {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
        say("not run: it loads a launch agent into the real launchd, on a hosted runner alone");
        return;
    }
    let app = PathBuf::from(env!("CARGO_BIN_EXE_halvo"));
    let pid = std::process::id();
    let label = format!("dev.halvo.test.login-item-{pid}");
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("login-item-load-{pid}"));
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)
        .unwrap_or_else(|e| panic!("{} cannot be made: {e}", dir.display()));
    let uid = std::fs::metadata(&dir)
        .expect("the scratch directory reads")
        .uid();
    let domain = format!("gui/{uid}");
    let target = format!("{domain}/{label}");
    let taken_out = TakenOut(target.clone());

    let wrote = Command::new(&app)
        .arg("--write-login-item")
        .arg("--login-label")
        .arg(&label)
        .arg("--data-dir")
        .arg(&dir)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("{} does not start: {e}", app.display()));
    assert!(wrote.status.success(), "the scratch form: {wrote:?}");
    let plist = dir.join(format!("{label}.plist"));
    assert!(plist.is_file(), "{} stands", plist.display());

    let (before, _) = status_of(&["print", &target]);
    assert_eq!(
        before,
        Some(NOT_LOADED),
        "print {target} before the bootstrap: the plist stands and launchd does not hold the label"
    );
    let (booted, text) = status_of(&[
        "bootstrap",
        &domain,
        plist.to_str().expect("the plist's path is UTF-8"),
    ]);
    assert_eq!(
        booted,
        Some(0),
        "bootstrap {domain} {}: {}",
        plist.display(),
        text.trim()
    );
    let (loaded, _) = status_of(&["print", &target]);
    assert_eq!(
        loaded,
        Some(0),
        "print {target} after the bootstrap: launchd holds the label"
    );

    drop(taken_out);
    let until = Instant::now() + GONE_WAIT;
    let after = loop {
        let (after, _) = status_of(&["print", &target]);
        if after == Some(NOT_LOADED) || Instant::now() >= until {
            break after;
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    assert_eq!(
        after,
        Some(NOT_LOADED),
        "print {target} after the bootout: the plist stands and launchd does not hold the label"
    );
    say(&format!(
        "ran on a hosted runner: print answered {NOT_LOADED} before the bootstrap, 0 after it and {NOT_LOADED} after the bootout of {target}"
    ));
}
