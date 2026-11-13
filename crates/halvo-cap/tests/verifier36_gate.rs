use std::ffi::OsString;
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use halvo_cap::{model_lock_path, take_model_lock, LockError};

const CHILD_ARGS: [&str; 5] = ["--ignored", "--exact", "child", "--nocapture", "--quiet"];
const GATE: [&str; 2] = ["GITHUB_ACTIONS", "RUNNER_ENVIRONMENT"];
const NAMES: &[&str] = &[
    "HOME",
    "LOCALAPPDATA",
    "APPDATA",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "HOMESHARE",
    "USERNAME",
    "USERDOMAIN",
    "USERDNSDOMAIN",
    "LOGONSERVER",
    "COMPUTERNAME",
    "ProgramData",
    "ALLUSERSPROFILE",
    "PUBLIC",
    "SystemDrive",
    "OneDrive",
    "TEMP",
    "TMP",
    "TMPDIR",
    "USER",
    "LOGNAME",
    "CFFIXED_USER_HOME",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
];
const KINDS: [&str; 6] = ["dir", "relative", "empty", "dotdot", "missing", "notutf8"];
const STARTS: [&str; 7] = [
    "path ",
    "error ",
    "taken",
    "held-refused",
    "held ",
    "execd",
    "not-hosted",
];

fn arg(key: &str) -> Option<String> {
    let prefix = format!("{key}=");
    std::env::args().find_map(|a| a.strip_prefix(&prefix).map(str::to_owned))
}

fn on_a_hosted_runner() -> bool {
    std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        && std::env::var("RUNNER_ENVIRONMENT").as_deref() == Ok("github-hosted")
}

fn say(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

fn wait_for_the_end_of_input() {
    let mut rest = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut rest);
}

#[cfg(target_os = "macos")]
fn hold_then_exec(out: &mut std::io::Stdout) -> i32 {
    use std::os::unix::process::CommandExt;
    match take_model_lock() {
        Ok(lock) => {
            if let Err(e) = lock.make_inheritable() {
                let _ = writeln!(out, "error {e}");
                return 4;
            }
            let failed = Command::new(std::env::current_exe().unwrap())
                .args(CHILD_ARGS)
                .arg("v36-role=wait-after-exec")
                .env_clear()
                .exec();
            let _ = writeln!(out, "error exec {failed}");
            4
        }
        Err(e) => {
            let _ = writeln!(out, "error {e}");
            4
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn hold_then_exec(out: &mut std::io::Stdout) -> i32 {
    let _ = writeln!(out, "error no exec on this system");
    6
}

#[test]
#[ignore = "a process that the other tests start"]
fn child() {
    let Some(role) = arg("v36-role") else {
        return;
    };
    let mut out = std::io::stdout();
    let takes = matches!(role.as_str(), "hold-account" | "try-account" | "hold-exec");
    if takes && !on_a_hosted_runner() {
        let _ = writeln!(out, "not-hosted");
        let _ = out.flush();
        std::process::exit(7);
    }
    let code = match role.as_str() {
        "path" => match model_lock_path() {
            Ok(path) => {
                let _ = writeln!(out, "path {}", path.display());
                0
            }
            Err(e) => {
                let _ = writeln!(out, "error {e}");
                4
            }
        },
        "hold-account" => match take_model_lock() {
            Ok(lock) => {
                let _ = writeln!(out, "held {}", lock.path().display());
                let _ = out.flush();
                wait_for_the_end_of_input();
                0
            }
            Err(e) => {
                let _ = writeln!(out, "error {e}");
                4
            }
        },
        "try-account" => match take_model_lock() {
            Ok(_lock) => {
                let _ = writeln!(out, "taken");
                0
            }
            Err(LockError::Held(_)) => {
                let _ = writeln!(out, "held-refused");
                3
            }
            Err(e) => {
                let _ = writeln!(out, "error {e}");
                4
            }
        },
        "hold-exec" => hold_then_exec(&mut out),
        "wait-after-exec" => {
            let _ = writeln!(out, "execd");
            let _ = out.flush();
            wait_for_the_end_of_input();
            0
        }
        _ => 6,
    };
    let _ = out.flush();
    std::process::exit(code);
}

struct Started(Child);

impl Started {
    fn end(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }

    fn code_within_a_minute(&mut self) -> String {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            match self.0.try_wait() {
                Ok(Some(status)) => {
                    return status
                        .code()
                        .map_or_else(|| "signal".to_string(), |c| c.to_string())
                }
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                _ => {
                    self.end();
                    return "deadline".to_string();
                }
            }
        }
    }

    fn end_by_exit(&mut self) -> String {
        drop(self.0.stdin.take());
        self.code_within_a_minute()
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
        .map_or(0, |d| d.as_nanos());
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("v36-gate")
        .join(format!("{name}-{}-{nanos}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn child_command(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(CHILD_ARGS)
        .arg(format!("v36-role={role}"))
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    command
}

#[cfg(unix)]
fn not_utf8() -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(b"/v36-\xff\xfe".to_vec())
}

#[cfg(windows)]
fn not_utf8() -> OsString {
    use std::os::windows::ffi::OsStringExt;
    OsString::from_wide(&[0x43, 0x3a, 0x5c, 0xd800])
}

fn value(kind: &str, dir: &Path, name: &str) -> OsString {
    match kind {
        "dir" => {
            let made = dir.join(name);
            let _ = std::fs::create_dir_all(&made);
            made.into_os_string()
        }
        "relative" => OsString::from("v36-relative"),
        "empty" => OsString::new(),
        "dotdot" => dir.join("..").join("..").into_os_string(),
        "missing" => dir.join("v36-missing").join(name).into_os_string(),
        _ => not_utf8(),
    }
}

fn hostile(mut command: Command, kind: &str, dir: &Path) -> Command {
    for name in NAMES {
        command.env(name, value(kind, dir, name));
    }
    command.current_dir(dir);
    command
}

fn cleared(mut command: Command) -> Command {
    command.env_clear();
    for name in GATE {
        if let Some(held) = std::env::var_os(name) {
            command.env(name, held);
        }
    }
    if cfg!(windows) {
        if let Some(root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", root);
        }
    }
    command
}

fn run(mut command: Command) -> (String, String) {
    command.stdout(Stdio::piped());
    let Ok(spawned) = command.spawn() else {
        return ("no-start".to_string(), String::new());
    };
    let mut started = Started(spawned);
    let code = started.code_within_a_minute();
    let mut bytes = Vec::new();
    if let Some(mut out) = started.0.stdout.take() {
        let _ = out.read_to_end(&mut bytes);
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let line = text
        .lines()
        .find(|l| STARTS.iter().any(|start| l.starts_with(start)))
        .unwrap_or("")
        .to_string();
    (code, line)
}

fn start_holder(mut command: Command) -> Option<(Started, String)> {
    command.stdin(Stdio::piped()).stdout(Stdio::piped());
    let mut started = Started(command.spawn().ok()?);
    let out = started.0.stdout.take()?;
    let line = BufReader::new(out)
        .lines()
        .map_while(Result::ok)
        .find(|l| STARTS.iter().any(|start| l.starts_with(start)))
        .unwrap_or_default();
    Some((started, line))
}

fn compared(parent: &str, code: &str, line: &str) -> String {
    if line == parent {
        "same".to_string()
    } else if line.starts_with("error ") {
        format!("differs(code={code},{line})")
    } else {
        format!("differs(code={code})")
    }
}

#[test]
fn v36_path_cases() {
    let parent = match model_lock_path() {
        Ok(path) => format!("path {}", path.display()),
        Err(e) => format!("error {e}"),
    };
    say(&format!(
        "v36 path parent_has_a_path={}",
        parent.starts_with("path ")
    ));
    let dir = fresh_dir("path");
    for name in NAMES {
        let mut results = Vec::new();
        for kind in KINDS {
            let mut command = child_command("path");
            command.env(name, value(kind, &dir, name));
            let (code, line) = run(command);
            results.push(format!("{kind}={}", compared(&parent, &code, &line)));
        }
        say(&format!("v36 path var={name} {}", results.join(" ")));
    }
    let mut results = Vec::new();
    for kind in KINDS {
        let (code, line) = run(hostile(child_command("path"), kind, &dir));
        results.push(format!("{kind}={}", compared(&parent, &code, &line)));
    }
    say(&format!(
        "v36 path var=ALL-{}-and-another-working-directory {}",
        NAMES.len(),
        results.join(" ")
    ));
    let (code, line) = run(cleared(child_command("path")));
    say(&format!(
        "v36 path cleared-but-the-gate-names-and-on-windows-SystemRoot {}",
        compared(&parent, &code, &line)
    ));
    let mut bare = child_command("path");
    bare.env_clear();
    let (code, line) = run(bare);
    say(&format!(
        "v36 path cleared-with-no-variable-at-all {}",
        compared(&parent, &code, &line)
    ));
}

fn old_path() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(|base| PathBuf::from(base).join("halvo").join("model.lock"))
    } else {
        std::env::var_os("HOME").map(|base| {
            PathBuf::from(base)
                .join("Library")
                .join("Caches")
                .join("halvo")
                .join("model.lock")
        })
    }
}

fn old_open(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(3);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    options.open(path)
}

fn lock_word(file: &File) -> &'static str {
    match file.try_lock() {
        Ok(()) => "locked",
        Err(TryLockError::WouldBlock) => "would-block",
        Err(TryLockError::Error(_)) => "error",
    }
}

fn two_loaders(case: &str, holder: Command, second: impl Fn() -> Command, by_kill: bool) {
    let parent =
        model_lock_path().map_or_else(|e| format!("error {e}"), |p| p.display().to_string());
    let Some((mut started, line)) = start_holder(holder) else {
        say(&format!("v36 e2e case={case} holder=no-start"));
        return;
    };
    let at_parents = line.strip_prefix("held ") == Some(parent.as_str());
    let (refused, _) = run(second());
    let ended = if by_kill {
        started.end();
        "killed".to_string()
    } else {
        started.end_by_exit()
    };
    let (after, _) = run(second());
    say(&format!(
        "v36 e2e case={case} holder_at_parents_path={at_parents} second_code_while_held={refused} holder_ended={ended} second_code_after={after}"
    ));
}

#[test]
fn v36_account_lock_cases() {
    if !on_a_hosted_runner() {
        say("v36 e2e: not run, because this is no hosted runner and the cases take the account's model lock");
        return;
    }
    let dir = fresh_dir("e2e");
    two_loaders(
        "E1-cleared-holder-hostile-second-ended-by-exit",
        cleared(child_command("hold-account")),
        || hostile(child_command("try-account"), "dir", &dir),
        false,
    );
    two_loaders(
        "E2-hostile-holder-cleared-second-ended-by-kill",
        hostile(child_command("hold-account"), "dir", &dir),
        || cleared(child_command("try-account")),
        true,
    );
    two_loaders(
        "E3-hostile-missing-holder-hostile-relative-second-ended-by-exit",
        hostile(child_command("hold-account"), "missing", &dir),
        || hostile(child_command("try-account"), "relative", &dir),
        false,
    );
    match old_path().map(|path| old_open(&path)) {
        Some(Ok(file)) => {
            let old = lock_word(&file);
            let (while_held, _) = run(hostile(child_command("try-account"), "dir", &dir));
            drop(file);
            let (after, _) = run(hostile(child_command("try-account"), "dir", &dir));
            say(&format!(
                "v36 e2e case=E4-holder-as-before-the-change-by-the-variable-open-for-writing old_holder={old} head_second_code_while_held={while_held} head_second_code_after={after}"
            ));
        }
        Some(Err(e)) => say(&format!(
            "v36 e2e case=E4-holder-as-before-the-change open_failed={:?}",
            e.kind()
        )),
        None => say("v36 e2e case=E4-holder-as-before-the-change no-variable"),
    }
    match start_holder(hostile(child_command("hold-account"), "dir", &dir)) {
        Some((mut started, line)) => {
            let old = old_path().map(|path| old_open(&path));
            let (while_held, after) = match &old {
                Some(Ok(file)) => {
                    let while_held = lock_word(file);
                    let ended = started.end_by_exit();
                    let after = lock_word(file);
                    (
                        while_held.to_string(),
                        format!("{after},holder_exit={ended}"),
                    )
                }
                _ => ("no-open".to_string(), "no-open".to_string()),
            };
            say(&format!(
                "v36 e2e case=E5-head-holder-second-as-before-the-change holder_held={} old_second_while_held={while_held} old_second_after={after}",
                line.starts_with("held ")
            ));
        }
        None => say("v36 e2e case=E5-head-holder-second-as-before-the-change holder=no-start"),
    }
    if cfg!(target_os = "macos") {
        match start_holder(hostile(child_command("hold-exec"), "dir", &dir)) {
            Some((mut started, line)) => {
                let (while_held, _) = run(cleared(child_command("try-account")));
                let ended = started.end_by_exit();
                let (after, _) = run(cleared(child_command("try-account")));
                say(&format!(
                    "v36 e2e case=E6-holder-became-the-loader-by-an-exec-into-a-cleared-environment execd={} second_code_while_held={while_held} holder_exit={ended} second_code_after={after}",
                    line == "execd"
                ));
            }
            None => say("v36 e2e case=E6-holder-became-the-loader-by-an-exec holder=no-start"),
        }
    } else {
        say(
            "v36 e2e case=E6-holder-became-the-loader-by-an-exec not-tried: no exec on this system",
        );
    }
}
