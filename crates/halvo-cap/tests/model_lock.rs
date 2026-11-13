use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use halvo_cap::{model_lock_path, take_model_lock, LockError};

const ROLE: &str = "HALVO_CAP_LOCK_ROLE";

#[cfg(target_os = "macos")]
const CACHE_VAR: &str = "HOME";
#[cfg(windows)]
const CACHE_VAR: &str = "LOCALAPPDATA";

#[test]
#[ignore = "a loader process that the other tests start"]
fn loader() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let mut out = std::io::stdout();
    let code = match role.as_str() {
        "try" => match take_model_lock() {
            Ok(lock) => {
                writeln!(out, "taken {}", lock.path().display()).unwrap();
                0
            }
            Err(LockError::NoCacheDir(var)) => {
                writeln!(out, "no-cache-dir {var}").unwrap();
                5
            }
            Err(e) => {
                writeln!(out, "error {e}").unwrap();
                4
            }
        },
        "path" => match model_lock_path() {
            Ok(path) => {
                writeln!(out, "path {}", path.display()).unwrap();
                0
            }
            Err(e) => {
                writeln!(out, "error {e}").unwrap();
                4
            }
        },
        _ => 6,
    };
    out.flush().unwrap();
    std::process::exit(code);
}

fn fresh_home(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("model-lock-homes")
        .join(format!("{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    home
}

fn expected_lock(home: &Path) -> PathBuf {
    if cfg!(windows) {
        home.join("halvo").join("model.lock")
    } else {
        home.join("Library")
            .join("Caches")
            .join("halvo")
            .join("model.lock")
    }
}

fn loader_command(role: &str, home: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", "loader", "--nocapture", "--quiet"])
        .env(ROLE, role)
        .env(CACHE_VAR, home)
        .stdin(Stdio::null())
        .stderr(Stdio::inherit());
    command
}

#[test]
fn the_lock_file_is_in_the_cache_directory_of_the_user() {
    let home = fresh_home("model-lock-path");
    let output = loader_command("path", &home).output().unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(0), "{text}");
    let want = format!("path {}", expected_lock(&home).display());
    assert!(text.lines().any(|l| l == want), "{text}");
}

#[test]
fn without_the_cache_variable_no_lock_is_taken() {
    let home = fresh_home("model-lock-no-var");
    let output = loader_command("try", &home)
        .env_remove(CACHE_VAR)
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(5), "{text}");
    assert!(
        text.lines()
            .any(|l| l == format!("no-cache-dir {CACHE_VAR}")),
        "{text}"
    );
}

#[test]
fn the_refusal_of_the_cache_variable_names_it() {
    let home = fresh_home("model-lock-words");
    let output = loader_command("path", &home)
        .env(CACHE_VAR, "relative-home")
        .current_dir(&home)
        .output()
        .unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(output.status.code(), Some(4), "{text}");
    let want =
        format!("error no model lock: the variable {CACHE_VAR} is not set to an absolute path");
    assert!(text.lines().any(|l| l == want), "{text}");
}

#[test]
fn a_cache_variable_that_is_empty_or_no_absolute_path_is_refused() {
    let home = fresh_home("model-lock-relative");
    for value in ["", "relative-home"] {
        let output = loader_command("try", &home)
            .env(CACHE_VAR, value)
            .current_dir(&home)
            .output()
            .unwrap();
        let text = String::from_utf8(output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(5), "{value:?}: {text}");
        assert!(
            text.lines()
                .any(|l| l == format!("no-cache-dir {CACHE_VAR}")),
            "{value:?}: {text}"
        );
    }
    assert!(!home.join("relative-home").exists());
}
