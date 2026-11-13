#![deny(clippy::undocumented_unsafe_blocks)]

mod common;

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use common::{wait_until, Script};

const SHORT: Duration = Duration::from_secs(15);

#[cfg(target_os = "macos")]
const LIST: &[&str] = &["HOME", "TMPDIR"];
#[cfg(windows)]
const LIST: &[&str] = &["LOCALAPPDATA", "SystemRoot", "TEMP", "TMP"];

#[cfg(target_os = "macos")]
const FROM_THE_LIBRARY: &str = "HOME";
#[cfg(windows)]
const FROM_THE_LIBRARY: &str = "LOCALAPPDATA";

#[cfg(target_os = "macos")]
const FROM_THE_SYSTEM: &[&str] = &["TMPDIR"];
#[cfg(windows)]
const FROM_THE_SYSTEM: &[&str] = &["SystemRoot", "TEMP", "TMP"];

const OTHER: &str = "HALVO_U49_OTHER";

struct Host {
    child: Child,
    dir: PathBuf,
}

impl Drop for Host {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        common::end_logged(&self.dir);
    }
}

fn marker() -> String {
    format!("u49-marker-{}", common::run_id())
}

/// Starts the library in a process of its own whose working directory is its data directory, with the test's
/// environment changed by `set` and `unset`, runs `steps` in its runner, and returns the scratch directory and the
/// runner's log once the run has ended.
fn run(name: &str, steps: &[String], set: &[(&str, &str)], unset: &[&str]) -> (PathBuf, String) {
    let dir = common::scratch(name);
    let data = dir.join("data");
    fs::create_dir_all(&data).unwrap();
    let mut lines = vec!["hello".to_owned()];
    lines.extend(steps.iter().cloned());
    lines.extend(["log done".to_owned(), "exit 0".to_owned()]);
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let script = Script::new(&dir, &refs);
    let mut cmd = Command::new(common::peer());
    cmd.arg("host")
        .arg(&data)
        .arg(&script.path)
        .arg(dir.join("owner.log"))
        .current_dir(&data)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (k, v) in set {
        cmd.env(k, v);
    }
    for k in unset {
        cmd.env_remove(k);
    }
    let mut child = cmd.spawn().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut host = Host {
        child,
        dir: dir.clone(),
    };
    let out = Arc::new(Mutex::new(Vec::new()));
    let o = Arc::clone(&out);
    thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdout.read(&mut buf) {
            if n == 0 {
                break;
            }
            o.lock().unwrap().extend_from_slice(&buf[..n]);
        }
    });
    let ended = || {
        String::from_utf8_lossy(&out.lock().unwrap())
            .lines()
            .any(|l| l.starts_with("end "))
    };
    assert!(
        wait_until(SHORT, || script.log().contains("\ndone") && ended()),
        "the runner logged its steps and the host reported the run's end: {}",
        script.log()
    );
    host.child.kill().unwrap();
    host.child.wait().unwrap();
    (dir, script.log())
}

fn logged<'a>(log: &'a str, key: &str) -> &'a str {
    log.lines()
        .find_map(|l| l.strip_prefix(key))
        .unwrap_or("<none>")
}

fn names(list: &[&str]) -> String {
    let mut v: Vec<&str> = list.to_vec();
    v.sort_unstable();
    v.join(",")
}

#[test]
fn the_runner_gets_the_names_of_the_list_of_its_system_and_no_other_variable_of_the_library() {
    let m = marker();
    let (_, log) = run(
        "environment-names",
        &[format!("environment {m}")],
        &[(OTHER, &m)],
        &[],
    );
    let got = logged(&log, "environment names=");
    assert!(
        !got.split(',').any(|n| n == OTHER),
        "a variable of the library that the list does not name reached the runner"
    );
    assert_eq!(got, names(LIST), "the names of the runner's environment");
}

#[test]
fn a_marker_in_the_temporary_directory_or_system_root_of_the_library_reaches_no_value_of_the_runner_and_one_in_home_or_local_app_data_does(
) {
    let m = marker();
    let system = format!("{m}-of-the-library");
    let mut set: Vec<(&str, &str)> = FROM_THE_SYSTEM
        .iter()
        .map(|n| (*n, system.as_str()))
        .collect();
    set.push((FROM_THE_LIBRARY, &m));
    let (_, log) = run(
        "environment-markers",
        &[format!("environment {m}")],
        &set,
        &[],
    );
    assert_eq!(
        logged(&log, "environment marked="),
        FROM_THE_LIBRARY,
        "the names of the runner's environment whose value holds the marker"
    );
    assert_eq!(
        logged(&log, "environment equal="),
        FROM_THE_LIBRARY,
        "the names of the runner's environment whose value is the library's own, unchanged"
    );
    assert_eq!(
        logged(&log, "environment temporary-directories="),
        FROM_THE_SYSTEM
            .iter()
            .filter(|n| **n != "SystemRoot")
            .count()
            .to_string(),
        "the temporary-directory variables of the runner that name a directory"
    );
}

#[cfg(target_os = "macos")]
fn answered_by_the_system() -> Vec<(&'static str, String)> {
    extern "C" {
        fn confstr(name: i32, buf: *mut u8, len: usize) -> usize;
    }
    const CS_DARWIN_USER_TEMP_DIR: i32 = 65537;
    let mut buf = vec![0u8; 1024];
    // SAFETY: `buf` is live and writable for `buf.len()` bytes, the length passed, and confstr writes at most that.
    let n = unsafe { confstr(CS_DARWIN_USER_TEMP_DIR, buf.as_mut_ptr(), buf.len()) };
    assert!(
        n > 1 && n <= buf.len(),
        "confstr answered the temporary directory"
    );
    buf.truncate(n - 1);
    vec![("TMPDIR", String::from_utf8(buf).unwrap())]
}

#[cfg(windows)]
fn answered_by_the_system() -> Vec<(&'static str, String)> {
    let var = |n: &str| std::env::var(n).unwrap();
    let temp = format!(r"{}\Temp", var("LOCALAPPDATA"));
    vec![
        ("SystemRoot", var("SystemRoot")),
        ("TEMP", temp.clone()),
        ("TMP", temp),
    ]
}

#[test]
fn the_temporary_directory_and_system_root_of_the_runner_are_what_the_system_answers() {
    let expected = answered_by_the_system();
    let steps: Vec<String> = expected
        .iter()
        .map(|(n, v)| format!("environment-is {n} {v}"))
        .collect();
    let (_, log) = run("environment-system", &steps, &[], &[]);
    for (name, _) in &expected {
        assert_eq!(
            logged(&log, &format!("environment-is {name} ")),
            "same",
            "the runner's {name} is what the system answers"
        );
    }
}

#[test]
fn a_library_whose_process_has_no_home_or_local_app_data_gives_the_runner_none() {
    let m = marker();
    let (_, log) = run(
        "environment-unset",
        &[format!("environment {m}")],
        &[],
        &[FROM_THE_LIBRARY],
    );
    let rest: Vec<&str> = LIST
        .iter()
        .copied()
        .filter(|n| *n != FROM_THE_LIBRARY)
        .collect();
    assert_eq!(
        logged(&log, "environment names="),
        names(&rest),
        "the names of the runner's environment"
    );
}

fn found_under(dir: &Path, name: &str) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        let path = e.path();
        e.file_name() == name || (path.is_dir() && found_under(&path, name))
    })
}

#[test]
fn the_working_directory_of_the_runner_is_the_system_directory_and_not_the_librarys() {
    let (_, log) = run("environment-directory", &["directory".to_owned()], &[], &[]);
    assert_eq!(
        logged(&log, "directory system="),
        "yes",
        "the runner's working directory is the system directory"
    );
}

#[test]
fn a_write_of_the_runner_by_a_relative_name_makes_no_file_and_leaves_nothing_under_the_data_directory(
) {
    let name = format!("u49-relative-{}", common::run_id());
    let (dir, log) = run(
        "environment-relative",
        &[format!("write-relative {name}")],
        &[],
        &[],
    );
    assert!(
        !found_under(&dir, &name),
        "a write by a relative name left a file under the library's data directory, its working directory"
    );
    assert!(
        !Path::new(&name).exists(),
        "a write by a relative name left a file in the test's working directory"
    );
    let made = logged(&log, "relative made=");
    eprintln!(
        "u49-relative: the write by a relative name made a file in the system directory: {made}; removed: {}",
        if log.contains("relative removed=yes") {
            "yes"
        } else {
            "no"
        }
    );
    if cfg!(windows) && made == "yes" {
        assert!(
            log.contains("relative removed=yes"),
            "the file that a relative write made in the system directory was removed by the stand-in"
        );
    } else {
        assert_eq!(made, "no", "a write by a relative name made a file");
    }
}

#[test]
fn the_runner_opens_a_loopback_socket() {
    let (_, log) = run("environment-socket", &["socket".to_owned()], &[], &[]);
    assert_eq!(
        logged(&log, "socket ok="),
        "yes",
        "the runner opened a loopback socket"
    );
}

#[cfg(windows)]
fn socket_by_hand(dir: &Path, system_root: bool) -> String {
    let script = Script::new(dir, &["socket", "log done", "exit 0"]);
    let mut cmd = Command::new(common::peer());
    cmd.arg("--run")
        .arg("socket")
        .arg("--browser")
        .arg(&script.path)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if system_root {
        cmd.env("SystemRoot", std::env::var_os("SystemRoot").unwrap());
    }
    let child = cmd.spawn().unwrap();
    let mut host = Host {
        child,
        dir: dir.to_path_buf(),
    };
    assert!(
        wait_until(SHORT, || host.child.try_wait().is_ok_and(|s| s.is_some())),
        "the stand-in ended: {}",
        script.log()
    );
    logged(&script.log(), "socket ok=").to_owned()
}

#[cfg(windows)]
#[test]
fn the_stand_in_started_by_hand_opens_a_loopback_socket_with_system_root_alone_and_none_without_it()
{
    let dir = common::scratch("environment-socket-control");
    assert_eq!(
        socket_by_hand(&dir, true),
        "yes",
        "the stand-in with SystemRoot alone opened a loopback socket"
    );
    assert_eq!(
        socket_by_hand(&dir, false),
        "no",
        "the stand-in with no SystemRoot opened a loopback socket"
    );
}

#[cfg(windows)]
#[test]
fn a_marker_in_the_profile_variables_of_the_library_reaches_no_temporary_directory_of_the_runner() {
    let m = marker();
    let profile = common::scratch("environment-profile").join(format!("{m}-profile"));
    fs::create_dir_all(profile.join("AppData").join("Local").join("Temp")).unwrap();
    let profile = profile.to_string_lossy().into_owned();
    let (drive, path) = profile.split_at(2);
    let (_, log) = run(
        "environment-profile-run",
        &[format!("environment {m}")],
        &[
            ("USERPROFILE", profile.as_str()),
            ("HOMEDRIVE", drive),
            ("HOMEPATH", path),
        ],
        &["LOCALAPPDATA"],
    );
    assert_eq!(
        logged(&log, "environment marked="),
        "",
        "the names of the runner's environment whose value holds the marker of the library's profile variables"
    );
    let rest: Vec<&str> = LIST
        .iter()
        .copied()
        .filter(|n| *n != "LOCALAPPDATA")
        .collect();
    assert_eq!(
        logged(&log, "environment names="),
        names(&rest),
        "the names of the runner's environment"
    );
}
