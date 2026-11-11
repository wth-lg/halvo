use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

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
        "hold" => match take_model_lock() {
            Ok(lock) => {
                writeln!(out, "held {}", lock.path().display()).unwrap();
                out.flush().unwrap();
                let mut rest = Vec::new();
                std::io::stdin().read_to_end(&mut rest).unwrap();
                0
            }
            Err(e) => {
                writeln!(out, "error {e}").unwrap();
                4
            }
        },
        "try" => {
            let begun = Instant::now();
            let taken = take_model_lock();
            let ms = begun.elapsed().as_millis();
            match taken {
                Ok(lock) => {
                    writeln!(out, "taken {}", lock.path().display()).unwrap();
                    0
                }
                Err(LockError::Held(path)) => {
                    writeln!(out, "held-refused ms={ms} {}", path.display()).unwrap();
                    3
                }
                Err(LockError::NoCacheDir(var)) => {
                    writeln!(out, "no-cache-dir {var}").unwrap();
                    5
                }
                Err(e) => {
                    writeln!(out, "error {e}").unwrap();
                    4
                }
            }
        }
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

fn loader_line(role: &str, home: &Path) -> (i32, String) {
    let output = loader_command(role, home).output().unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let line = text
        .lines()
        .find(|l| {
            l.starts_with("held")
                || l.starts_with("taken")
                || l.starts_with("no-cache-dir")
                || l.starts_with("path")
                || l.starts_with("error")
        })
        .unwrap_or("")
        .to_string();
    (output.status.code().unwrap_or(-1), line)
}

fn start_holder(home: &Path) -> Child {
    let mut holder = loader_command("hold", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(holder.stdout.take().unwrap()).lines();
    let first = lines
        .find(|l| {
            l.as_ref()
                .map_or(true, |l| l.starts_with("held") || l.starts_with("error"))
        })
        .unwrap()
        .unwrap();
    assert_eq!(first, format!("held {}", expected_lock(home).display()));
    holder
}

#[test]
fn a_second_loader_is_refused_at_once_and_a_killed_holder_frees_the_lock() {
    let home = fresh_home("model-lock");
    let lock = expected_lock(&home);
    assert!(!lock.exists());

    let mut holder = start_holder(&home);

    let (code, line) = loader_line("try", &home);
    assert_eq!(code, 3, "the second loader got: {line}");
    let ms: u64 = line
        .strip_prefix("held-refused ms=")
        .and_then(|rest| rest.split(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("unexpected line: {line}"));
    assert!(ms < 1000, "the second loader waited {ms} ms");

    holder.kill().unwrap();
    holder.wait().unwrap();

    let (code, line) = loader_line("try", &home);
    assert_eq!(code, 0, "after the holder was killed: {line}");
    assert_eq!(line, format!("taken {}", lock.display()));

    let meta = std::fs::metadata(&lock).unwrap();
    assert!(meta.is_file());
    assert_eq!(meta.len(), 0);
}

#[test]
fn the_lock_file_is_in_the_cache_directory_of_the_user() {
    let home = fresh_home("model-lock-path");
    let (code, line) = loader_line("path", &home);
    assert_eq!(code, 0, "{line}");
    assert_eq!(line, format!("path {}", expected_lock(&home).display()));
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
fn a_loader_that_exits_frees_the_lock() {
    let home = fresh_home("model-lock-exit");
    let (code, line) = loader_line("try", &home);
    assert_eq!(code, 0, "{line}");
    let (code, line) = loader_line("try", &home);
    assert_eq!(code, 0, "{line}");
}

#[cfg(windows)]
#[test]
fn the_held_lock_file_and_its_directory_cannot_be_moved() {
    let home = fresh_home("model-lock-move");
    let lock = expected_lock(&home);
    let mut holder = start_holder(&home);
    let moved = home.join("moved.lock");
    let file_moved = std::fs::rename(&lock, &moved).is_ok();
    let dir_moved = std::fs::rename(home.join("halvo"), home.join("halvo-moved")).is_ok();
    let (code, line) = loader_line("try", &home);
    holder.kill().unwrap();
    holder.wait().unwrap();
    assert!(!file_moved, "the held lock file was renamed");
    assert!(
        !dir_moved,
        "the directory of the held lock file was renamed"
    );
    assert_eq!(code, 3, "a second loader got: {line}");
}
