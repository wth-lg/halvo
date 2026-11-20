use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

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
        "hold-account" | "try-account" if !on_a_hosted_runner() => {
            writeln!(out, "refused: the role runs on a hosted runner alone").unwrap();
            5
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

fn fresh_dir(name: &str) -> (Leaf, PathBuf) {
    let held = leaf(&format!("model-lock-accounts-{name}"));
    let dir = held.path().to_path_buf();
    (held, dir)
}

#[test]
fn fresh_dir_two_calls_give_two_directories_and_each_is_gone_after_its_drop() {
    let (first, first_dir) = fresh_dir("fresh");
    let (second, second_dir) = fresh_dir("fresh");
    assert_ne!(first_dir, second_dir);
    assert!(first_dir.is_dir() && second_dir.is_dir());
    std::fs::write(first_dir.join("file"), "x").unwrap();
    drop(first);
    drop(second);
    assert!(!first_dir.exists(), "{}", first_dir.display());
    assert!(!second_dir.exists(), "{}", second_dir.display());
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
            ["path ", "error ", "taken", "held-refused", "refused: "]
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
    let (_fresh_first, first) = fresh_dir("first");
    let (_fresh_second, second) = fresh_dir("second");
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
        let (_fresh, dir) = fresh_dir(&format!("more-{name}"));
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

fn hosted_values(actions: Option<&str>, environment: Option<&str>) -> bool {
    actions == Some("true") && environment == Some("github-hosted")
}

fn on_a_hosted_runner() -> bool {
    hosted_values(
        std::env::var("GITHUB_ACTIONS").ok().as_deref(),
        std::env::var("RUNNER_ENVIRONMENT").ok().as_deref(),
    )
}

#[test]
fn the_hosted_gate_holds_both_hosted_values_and_nothing_else() {
    assert!(hosted_values(Some("true"), Some("github-hosted")));
    assert!(!hosted_values(Some("true"), None));
    assert!(!hosted_values(Some("true"), Some("self-hosted")));
    assert!(!hosted_values(None, None));
}

#[test]
fn the_library_half_of_lock_rs_reads_no_variable_asks_for_the_effective_user_and_has_four_public_items(
) {
    let lines: Vec<&str> = include_str!("../src/lock.rs")
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .collect();
    let end = lines
        .windows(2)
        .position(|w| {
            w[0] == "#[cfg(all(test, any(target_os = \"macos\", windows)))]"
                && w[1] == "mod tests {"
        })
        .expect("the test module of lock.rs");
    let code_of = |lines: &[&'static str]| -> Vec<&'static str> {
        lines
            .iter()
            .copied()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with("///") && !t.starts_with("//!")
            })
            .collect()
    };
    let code = code_of(&lines[..end]);
    for word in [
        "std::env",
        "env::var",
        "var_os",
        "getenv",
        "GetEnvironmentVariable",
        "ExpandEnvironmentStrings",
        "home_dir",
        "NSHomeDirectory",
        "SHGetFolderPath",
    ] {
        assert!(
            !code.iter().any(|l| l.contains(word)),
            "a code line of the library half of lock.rs names {word}"
        );
    }
    let joined = code.join("\n");
    assert_eq!(
        joined.matches("libc::geteuid()").count(),
        1,
        "libc::geteuid() must stand once in the library half of lock.rs"
    );
    for word in ["libc::getuid", "getlogin", "getpwnam"] {
        assert!(
            !joined.contains(word),
            "the library half of lock.rs names {word}"
        );
    }
    for (signature, body) in [
        (
            "pub fn take_model_lock() -> Result<ModelLock, LockError> {",
            "take_lock_from(account_dir())",
        ),
        (
            "pub fn model_lock_path() -> Result<PathBuf, LockError> {",
            "lock_path_from(account_dir())",
        ),
    ] {
        let bodies: Vec<&str> = code
            .windows(2)
            .filter(|w| w[0] == signature)
            .map(|w| w[1].trim())
            .collect();
        assert!(
            bodies.contains(&body)
                && bodies
                    .iter()
                    .all(|b| *b == body || *b == "Err(LockError::Unsupported)"),
            "{signature} is not {body}: {bodies:?}"
        );
    }
    let mut public: Vec<&str> = code
        .iter()
        .map(|l| l.trim())
        .filter(|l| l.starts_with("pub"))
        .collect();
    public.sort_unstable();
    public.dedup();
    assert_eq!(
        public,
        [
            "pub enum LockError {",
            "pub fn make_inheritable(&self) -> Result<(), LockError> {",
            "pub fn model_lock_path() -> Result<PathBuf, LockError> {",
            "pub fn path(&self) -> &Path {",
            "pub fn take_model_lock() -> Result<ModelLock, LockError> {",
            "pub struct ModelLock {",
        ],
        "the public items of the library half of lock.rs"
    );
    let lib: Vec<&str> = include_str!("../src/lib.rs")
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .collect();
    let of_lock: Vec<&str> = code_of(&lib)
        .into_iter()
        .map(str::trim)
        .filter(|l| l.contains("lock::") || l.contains("mod lock"))
        .collect();
    assert_eq!(
        of_lock,
        [
            "mod lock;",
            "pub use lock::{model_lock_path, take_model_lock, LockError, ModelLock};",
        ],
        "what lib.rs makes of the module lock"
    );
}

#[test]
fn a_child_whose_account_variable_names_a_directory_with_a_lock_file_prints_the_parents_lock_path()
{
    let parent = model_lock_path().ok();
    let (_fresh_dir, dir) = fresh_dir("lock-file-there");
    let layouts: [PathBuf; 2] = [
        ["Library", "Caches", "halvo", "model.lock"]
            .iter()
            .collect(),
        ["halvo", "model.lock"].iter().collect(),
    ];
    for layout in layouts {
        let file = dir.join(layout);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "").unwrap();
    }
    let mut sets: Vec<Vec<&str>> = ACCOUNT_VARIABLES.iter().map(|v| vec![*v]).collect();
    sets.push(ACCOUNT_VARIABLES.to_vec());
    for set in sets {
        let mut command = child_command("path");
        for name in &set {
            command.env(name, &dir);
        }
        let got = printed_path(&mut command);
        assert!(
            got.is_some(),
            "with {set:?}: the child printed no lock path"
        );
        assert!(
            got == parent,
            "with {set:?} naming a directory that holds a lock file, the child's lock path is not the parent's"
        );
    }
}

#[test]
fn two_loaders_with_other_account_variables_meet_at_the_accounts_lock() {
    if !on_a_hosted_runner() {
        say("model-lock end-to-end: not run, because this is no hosted runner and the test takes the account's model lock");
        return;
    }
    let mut refusals = 0;
    for role in ["hold-account", "try-account"] {
        let mut command = child_command(role);
        command
            .env_remove("GITHUB_ACTIONS")
            .env_remove("RUNNER_ENVIRONMENT");
        let (code, line) = first_line(&mut command);
        if code == Some(5) && line == "refused: the role runs on a hosted runner alone" {
            refusals += 1;
        }
    }
    let (after_refusals, _) = first_line(&mut child_command("try-account"));
    say(&format!(
        "model-lock end-to-end: the two roles without the hosted values refused with exit 5: {}; a gated take after them: {}",
        refusals == 2,
        after_refusals == Some(0)
    ));
    assert_eq!(
        refusals, 2,
        "a role without the hosted values did not refuse"
    );
    assert_eq!(
        after_refusals,
        Some(0),
        "a gated take after the refused roles did not take the lock"
    );
    let parent = model_lock_path().ok();
    let (_fresh_first, first) = fresh_dir("hold-first");
    let (_fresh_second, second) = fresh_dir("try-second");
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
    let first = leaf("model-lock-leaf");
    let second = leaf("model-lock-leaf");
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
    let held = leaf("model-lock-leaf");
    let path = held.path().to_path_buf();
    std::fs::write(path.join("file"), "x").unwrap();
    drop(held);
    assert!(!path.exists(), "{}", path.display());
}

#[test]
fn leaf_removal_that_fails_gives_one_line_with_the_path() {
    let held = leaf("model-lock-leaf");
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
    let held = leaf("model-lock-leaf");
    assert_eq!(
        held.path().parent(),
        Some(Path::new(env!("CARGO_TARGET_TMPDIR"))),
        "{}",
        held.path().display()
    );
    let name = held.path().file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("model-lock-leaf-"), "{name}");
}
