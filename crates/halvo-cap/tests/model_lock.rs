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
        #[cfg(unix)]
        "hold-then-exec" => match take_model_lock() {
            Ok(lock) => {
                use std::os::unix::process::CommandExt;
                writeln!(out, "held {}", lock.path().display()).unwrap();
                out.flush().unwrap();
                if let Err(e) = lock.make_inheritable() {
                    writeln!(out, "error {e}").unwrap();
                    std::process::exit(4);
                }
                let error = Command::new("/bin/sleep").arg("60").exec();
                writeln!(out, "error {error}").unwrap();
                4
            }
            Err(e) => {
                writeln!(out, "error {e}").unwrap();
                4
            }
        },
        "start-child-and-exit" => match take_model_lock() {
            Ok(lock) => match start_loader_child("wait-stdin") {
                Ok(_) => {
                    writeln!(out, "held {}", lock.path().display()).unwrap();
                    0
                }
                Err(e) => {
                    writeln!(out, "error {e}").unwrap();
                    4
                }
            },
            Err(e) => {
                writeln!(out, "error {e}").unwrap();
                4
            }
        },
        #[cfg(target_os = "macos")]
        "hand-on-start-child-and-exit" => match take_model_lock() {
            Ok(lock) => match lock
                .make_inheritable()
                .map_err(|e| std::io::Error::other(e.to_string()))
                .and_then(|()| start_loader_child("wait-stdin"))
            {
                Ok(_) => {
                    writeln!(out, "held {}", lock.path().display()).unwrap();
                    0
                }
                Err(e) => {
                    writeln!(out, "error {e}").unwrap();
                    4
                }
            },
            Err(e) => {
                writeln!(out, "error {e}").unwrap();
                4
            }
        },
        "wait-stdin" => {
            let mut rest = Vec::new();
            std::io::stdin().read_to_end(&mut rest).unwrap();
            0
        }
        #[cfg(target_os = "macos")]
        "hold-and-start-unlocker" => match take_model_lock() {
            Ok(lock) => {
                let ended = start_loader_child("unlock-inherited").and_then(|mut c| c.wait());
                writeln!(out, "unlocker {ended:?}").unwrap();
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
        #[cfg(target_os = "macos")]
        "unlock-inherited" => {
            for fd in 3..256 {
                unsafe { libc::flock(fd, libc::LOCK_UN) };
            }
            0
        }
        "take-twice" => match take_model_lock() {
            Ok(_first) => match take_model_lock() {
                Ok(_) => {
                    writeln!(out, "second taken").unwrap();
                    0
                }
                Err(e) => {
                    writeln!(out, "second {e}").unwrap();
                    3
                }
            },
            Err(e) => {
                writeln!(out, "error {e}").unwrap();
                4
            }
        },
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

fn start_loader_child(role: &str) -> std::io::Result<Child> {
    Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "loader", "--nocapture", "--quiet"])
        .env(ROLE, role)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
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

fn loader_line(role: &str, home: &Path) -> (i32, String) {
    let output = loader_command(role, home).output().unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let line = text
        .lines()
        .find(|l| {
            l.starts_with("held")
                || l.starts_with("taken")
                || l.starts_with("second")
                || l.starts_with("no-cache-dir")
                || l.starts_with("path")
                || l.starts_with("error")
        })
        .unwrap_or("")
        .to_string();
    (output.status.code().unwrap_or(-1), line)
}

fn start_holder(home: &Path) -> Child {
    start_holder_as("hold", home)
}

fn start_holder_as(role: &str, home: &Path) -> Child {
    let mut holder = loader_command(role, home)
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
fn the_take_leaves_the_content_of_the_lock_file_as_it_was() {
    let home = fresh_home("model-lock-content");
    let lock = expected_lock(&home);
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(&lock, "left by an earlier run\n").unwrap();
    let (code, line) = loader_line("try", &home);
    assert_eq!(code, 0, "{line}");
    assert_eq!(
        std::fs::read_to_string(&lock).unwrap(),
        "left by an earlier run\n"
    );
}

#[test]
fn a_holder_that_starts_a_child_and_ends_leaves_the_lock_free_while_the_child_lives() {
    let home = fresh_home("model-lock-child");
    let mut holder = start_holder_as("start-child-and-exit", &home);
    let child_input = holder.stdin.take().unwrap();
    holder.wait().unwrap();
    let (code, line) = loader_line("try", &home);
    drop(child_input);
    assert_eq!(
        code, 0,
        "a loader after the holder ended, while the child that it started lives: {line}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_child_of_the_holder_cannot_release_the_lock() {
    let home = fresh_home("model-lock-unlock");
    let mut holder = start_holder_as("hold-and-start-unlocker", &home);
    let (code, line) = loader_line("try", &home);
    holder.kill().unwrap();
    holder.wait().unwrap();
    assert_eq!(
        code, 3,
        "a loader while the holder lives, after a child of the holder unlocked what it inherited: {line}"
    );
}

#[test]
fn a_held_lock_is_refused_with_words_that_name_no_other_process() {
    let home = fresh_home("model-lock-twice");
    let (code, line) = loader_line("take-twice", &home);
    assert_eq!(code, 3, "{line}");
    assert_eq!(
        line,
        format!(
            "second the model lock is held: {}",
            expected_lock(&home).display()
        )
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

#[cfg(target_os = "macos")]
#[test]
fn a_child_started_after_the_second_call_holds_the_lock_after_the_holder_has_ended() {
    let home = fresh_home("model-lock-hand-on");
    let mut holder = start_holder_as("hand-on-start-child-and-exit", &home);
    let child_input = holder.stdin.take().unwrap();
    holder.wait().unwrap();
    let (code, line) = loader_line("try", &home);
    drop(child_input);
    assert_eq!(
        code, 3,
        "a loader after the holder ended, while the child that it started after the second call lives: {line}"
    );
    let begun = Instant::now();
    loop {
        let (code, line) = loader_line("try", &home);
        if code == 0 {
            break;
        }
        assert!(
            begun.elapsed() < std::time::Duration::from_secs(10),
            "the lock stayed held after the child ended: {line}"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

#[test]
fn a_loader_that_exits_frees_the_lock() {
    let home = fresh_home("model-lock-exit");
    let (code, line) = loader_line("try", &home);
    assert_eq!(code, 0, "{line}");
    let (code, line) = loader_line("try", &home);
    assert_eq!(code, 0, "{line}");
}

#[cfg(unix)]
#[test]
fn a_holder_that_execs_keeps_the_lock_until_its_process_ends() {
    let home = fresh_home("model-lock-exec");
    let mut holder = start_holder_as("hold-then-exec", &home);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let (code, line) = loader_line("try", &home);
    holder.kill().unwrap();
    holder.wait().unwrap();
    assert_eq!(
        code, 3,
        "a second loader got the lock after the holder's exec: {line}"
    );
    let (code, line) = loader_line("try", &home);
    assert_eq!(code, 0, "after the holder's process ended: {line}");
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
