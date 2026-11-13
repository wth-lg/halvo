use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use halvo_cap::{model_lock_path, take_model_lock, LockError};

const ACCOUNT_VARIABLES: [&str; 4] = ["HOME", "LOCALAPPDATA", "APPDATA", "USERPROFILE"];

fn arg(key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    std::env::args().find_map(|a| a.strip_prefix(&prefix).map(str::to_owned))
}

#[test]
#[ignore = "a process that the other tests start"]
fn child() {
    let Some(role) = arg("halvo-role") else {
        return;
    };
    let mut out = std::io::stdout();
    let code = match role.as_str() {
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
        "hold-account" => match take_model_lock() {
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
        "try-account" => match take_model_lock() {
            Ok(_lock) => {
                writeln!(out, "taken").unwrap();
                0
            }
            Err(LockError::Held(_)) => {
                writeln!(out, "held-refused").unwrap();
                3
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

struct Started(Child);

impl Started {
    fn end(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl Drop for Started {
    fn drop(&mut self) {
        self.end();
    }
}

fn fresh_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("model-lock-accounts")
        .join(format!("{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn child_command(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", "child", "--nocapture", "--quiet"])
        .arg(format!("halvo-role={role}"))
        .stdin(Stdio::null())
        .stderr(Stdio::inherit());
    command
}

fn with_account_variables_in(mut command: Command, dir: &Path) -> Command {
    for name in ACCOUNT_VARIABLES {
        let value = dir.join(name);
        std::fs::create_dir_all(&value).unwrap();
        command.env(name, value);
    }
    command
}

fn first_line(command: &mut Command) -> (Option<i32>, String) {
    let output = command.output().unwrap();
    let text = String::from_utf8(output.stdout).unwrap();
    let line = text
        .lines()
        .find(|l| {
            ["path ", "error ", "taken", "held-refused"]
                .iter()
                .any(|start| l.starts_with(start))
        })
        .unwrap_or("")
        .to_string();
    (output.status.code(), line)
}

fn printed_path(command: &mut Command) -> Option<PathBuf> {
    let (code, line) = first_line(command);
    match (code, line.strip_prefix("path ")) {
        (Some(0), Some(path)) => Some(PathBuf::from(path)),
        _ => None,
    }
}

fn say(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

#[test]
fn two_children_with_other_account_variables_print_the_parents_lock_path() {
    let parent = model_lock_path().ok();
    let first = fresh_dir("first");
    let second = fresh_dir("second");
    let one = printed_path(&mut with_account_variables_in(
        child_command("path"),
        &first,
    ));
    let two = printed_path(&mut with_account_variables_in(
        child_command("path"),
        &second,
    ));
    assert!(parent.is_some(), "the parent got no lock path");
    assert!(
        one.is_some() && two.is_some(),
        "a child printed no lock path"
    );
    assert!(one == two, "the two children printed two lock paths");
    assert!(
        one == parent,
        "the children's lock path is not the parent's"
    );
    let one = one.unwrap();
    assert!(
        !one.starts_with(&first) && !one.starts_with(&second),
        "the lock path lies under a directory that a child's variables named"
    );
}

#[test]
fn children_with_other_cache_and_user_variables_and_working_directories_print_the_parents_lock_path(
) {
    let parent = model_lock_path().ok();
    for name in ["first", "second"] {
        let dir = fresh_dir(&format!("more-{name}"));
        let mut command = with_account_variables_in(child_command("path"), &dir);
        for variable in [
            "TMPDIR",
            "TMP",
            "TEMP",
            "XDG_CACHE_HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "CFFIXED_USER_HOME",
            "HOMEPATH",
            "PWD",
        ] {
            let value = dir.join(variable);
            std::fs::create_dir_all(&value).unwrap();
            command.env(variable, value);
        }
        for variable in ["USER", "LOGNAME", "USERNAME"] {
            command.env(variable, format!("someone-{name}"));
        }
        command.env("HOMEDRIVE", "Q:").current_dir(&dir);
        let got = printed_path(&mut command);
        assert!(got.is_some(), "a child printed no lock path");
        assert!(got == parent, "a child's lock path is not the parent's");
        assert!(
            !got.unwrap().starts_with(&dir),
            "the lock path lies under a directory of the child"
        );
    }
}

#[test]
fn a_child_with_none_of_the_account_variables_prints_the_parents_lock_path() {
    let parent = model_lock_path().ok();
    let mut command = child_command("path");
    command.env_clear();
    if cfg!(windows) {
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
    }
    let got = printed_path(&mut command);
    assert!(parent.is_some(), "the parent got no lock path");
    assert!(got.is_some(), "the child printed no lock path");
    assert!(got == parent, "the child's lock path is not the parent's");
}

#[test]
fn the_lock_path_is_the_path_below_the_accounts_own_directory() {
    let path = model_lock_path().unwrap();
    let below: PathBuf = if cfg!(windows) {
        ["halvo", "model.lock"].iter().collect()
    } else {
        ["Library", "Caches", "halvo", "model.lock"]
            .iter()
            .collect()
    };
    assert!(path.ends_with(&below), "the path below the base changed");
    let base = path
        .ancestors()
        .nth(below.components().count())
        .unwrap()
        .to_path_buf();
    if cfg!(target_os = "macos") {
        let output = Command::new("/usr/bin/id").arg("-P").output().unwrap();
        let entry = String::from_utf8(output.stdout).unwrap();
        let home = entry.trim_end().split(':').nth(8).map(PathBuf::from);
        assert_eq!(
            home.as_ref(),
            Some(&base),
            "the base is not the home directory of the account's entry"
        );
    }
    if on_a_hosted_runner() {
        let name = if cfg!(windows) {
            "LOCALAPPDATA"
        } else {
            "HOME"
        };
        let agree = std::env::var_os(name).map(PathBuf::from) == Some(base);
        say(&format!(
            "model-lock path: the base and the variable that builds before the change read agree: {agree}"
        ));
        assert!(agree, "the base is not the directory that {name} names");
    } else {
        say("model-lock path: the base is not compared with the variable here, because only on a hosted runner is the variable known to hold the account's directory");
    }
}

fn on_a_hosted_runner() -> bool {
    std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
}

#[test]
fn two_loaders_with_other_account_variables_meet_at_the_accounts_lock() {
    if !on_a_hosted_runner() {
        say("model-lock end-to-end: not run, because this is no hosted runner and the test takes the account's model lock");
        return;
    }
    let parent = model_lock_path().ok();
    let first = fresh_dir("hold-first");
    let second = fresh_dir("try-second");
    let mut holder = Started(
        with_account_variables_in(child_command("hold-account"), &first)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut lines = BufReader::new(holder.0.stdout.take().unwrap()).lines();
    let held = lines
        .find(|l| {
            l.as_ref()
                .map_or(true, |l| l.starts_with("held ") || l.starts_with("error "))
        })
        .and_then(Result::ok)
        .unwrap_or_default();
    let held_at = held.strip_prefix("held ").map(PathBuf::from);
    let (refused, _) = first_line(&mut with_account_variables_in(
        child_command("try-account"),
        &second,
    ));
    holder.end();
    let (after, _) = first_line(&mut with_account_variables_in(
        child_command("try-account"),
        &second,
    ));
    say(&format!(
        "model-lock end-to-end: ran, holder_at_parents_path={} second_loader_refused={} taken_after_holder_ended={}",
        held_at.is_some() && held_at == parent,
        refused == Some(3),
        after == Some(0)
    ));
    assert!(
        held_at.is_some() && held_at == parent,
        "the holder did not hold the parent's lock path"
    );
    assert_eq!(refused, Some(3), "a second loader was not refused");
    assert_eq!(
        after,
        Some(0),
        "no loader took the lock after the holder ended"
    );
}
