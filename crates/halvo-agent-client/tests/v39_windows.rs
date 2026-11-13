#![cfg(windows)]
//! Verifier 39's own Windows cases for unit U4.9. Each prints counts and yes or no on standard error and fails on
//! a fault of its own rig alone; what the lines say is read from the job's log.

mod common;

use std::ffi::c_void;
use std::fs;
use std::io::{Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, AsRawSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;
use std::time::Duration;

use common::{wait_until, Script};

const SHORT: Duration = Duration::from_secs(20);

#[link(name = "kernel32")]
extern "system" {
    fn SetHandleInformation(handle: *mut c_void, mask: u32, flags: u32) -> i32;
}

fn say(line: &str) {
    let _ = writeln!(std::io::stderr(), "v39 {line}");
}

fn yes(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn logged(log: &str, key: &str) -> String {
    log.lines()
        .find_map(|l| l.strip_prefix(key))
        .unwrap_or("<none>")
        .to_owned()
}

/// A process that this file started, ended by its handle when the case ends, however it ends.
struct Ends(Child, PathBuf);

impl Drop for Ends {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        common::end_logged(&self.1);
    }
}

#[test]
fn v39_g_a_runner_holds_none_of_six_inheritable_handles_of_the_library() {
    let rig = common::Rig::new("v39-g");
    let marker = format!("v39g-{}", common::run_id());
    let file_path = rig.dir.join("held.txt");
    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file_path)
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut other = Ends(
        Command::new(common::peer())
            .arg("child")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
        rig.dir.clone(),
    );
    let other_in = other.0.stdin.take().unwrap();
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x0200_0000)
        .open(&rig.dir)
        .unwrap();
    let handles: Vec<(&str, usize)> = vec![
        ("file", file.as_raw_handle() as usize),
        ("socket", listener.as_raw_socket() as usize),
        ("pipe-of-another-child", other_in.as_raw_handle() as usize),
        ("directory", directory.as_raw_handle() as usize),
        ("own-stdout", std::io::stdout().as_raw_handle() as usize),
        ("own-stderr", std::io::stderr().as_raw_handle() as usize),
    ];
    let mut made = 0;
    for (name, h) in &handles {
        let ok = unsafe { SetHandleInformation(*h as *mut c_void, 1, 1) } != 0;
        say(&format!("g handle={name} made-inheritable={}", yes(ok)));
        if ok {
            made += 1;
        }
    }
    say(&format!("g inheritable-made={made} of={}", handles.len()));

    let mut lines: Vec<String> = vec!["hello".to_owned()];
    lines.extend(
        handles
            .iter()
            .map(|(_, h)| format!("write-inherited {h} {marker}")),
    );
    lines.extend([
        "pipes".to_owned(),
        "log probed".to_owned(),
        "idle".to_owned(),
    ]);
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let script = rig.script(&refs);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    let started = rig.sup.start(spec);
    say(&format!("g start-ok={}", yes(started.is_ok())));
    let probed = script.wait_log("probed", SHORT);
    say(&format!("g runner-probed={}", yes(probed)));
    let log = script.log();
    let mut absent = 0;
    for (name, h) in &handles {
        let got = logged(&log, &format!("inherited {h} "));
        if got == "absent" {
            absent += 1;
        }
        say(&format!(
            "g handle={name} absent-in-runner={}",
            yes(got == "absent")
        ));
    }
    say(&format!("g absent={absent} of={}", handles.len()));
    say(&format!(
        "g pipes-beyond-the-three-zero={}",
        yes(logged(&log, "pipes beyond=") == "0")
    ));
    let _ = rig.sup.kill(&run);
    let leaked = fs::read(&file_path)
        .unwrap_or_default()
        .windows(marker.len())
        .any(|w| w == marker.as_bytes());
    say(&format!(
        "g marker-in-held-file-after-library-start={}",
        yes(leaked)
    ));

    // The control: the same stand-in started by the standard library's start inherits the file handle.
    let control = common::scratch("v39-g-control");
    let file_handle = handles[0].1;
    let line = format!("write-inherited {file_handle} {marker}");
    let by_hand_script = Script::new(&control, &[line.as_str(), "log done", "exit 0"]);
    let by_hand = Ends(
        Command::new(common::peer())
            .arg("--run")
            .arg("control")
            .arg("--browser")
            .arg(&by_hand_script.path)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
        control.clone(),
    );
    let done = wait_until(SHORT, || by_hand_script.log().contains("done"));
    drop(by_hand);
    let got = logged(&by_hand_script.log(), &format!("inherited {file_handle} "));
    let reached = fs::read(&file_path)
        .unwrap_or_default()
        .windows(marker.len())
        .any(|w| w == marker.as_bytes());
    say(&format!(
        "g control-by-standard-start done={} wrote={} marker-in-held-file={}",
        yes(done),
        yes(got == "wrote"),
        yes(reached)
    ));
    drop(other_in);
    drop(directory);
    drop(listener);
    drop(file);
}

/// Starts the library in a process of its own with the environment changed, and returns the runner's log, or none
/// when the run did not end in time.
fn host_run(name: &str, steps: &[String], set: &[(&str, &str)], unset: &[&str]) -> Option<String> {
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
    let host = Ends(child, dir.clone());
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
    let ok = wait_until(SHORT, || script.log().contains("\ndone") && ended());
    drop(host);
    ok.then(|| script.log())
}

#[test]
fn v39_h_which_variables_of_the_library_move_a_value_of_a_windows_runner() {
    let m = format!("v39h-{}", common::run_id());
    let local = std::env::var("LOCALAPPDATA").unwrap();
    let temp = format!(r"{local}\Temp");
    let root = std::env::var("SystemRoot").unwrap();
    let base = common::scratch("v39-h-profile").join(format!("{m}-dir"));
    fs::create_dir_all(base.join("AppData").join("Local").join("Temp")).unwrap();
    let marked = base.to_string_lossy().into_owned();
    let (drive, path) = marked.split_at(2);
    let names = [
        "USERPROFILE",
        "HOMEDRIVE",
        "HOMEPATH",
        "APPDATA",
        "SystemDrive",
        "ProgramData",
        "ALLUSERSPROFILE",
        "PUBLIC",
        "USERNAME",
        "USERDOMAIN",
        "ProgramFiles",
        "CommonProgramFiles",
        "OneDrive",
        "TEMP",
        "TMP",
        "windir",
        "HOME",
        "TMPDIR",
        "PATH",
        "SystemRoot",
    ];
    let value = |n: &str| -> &str {
        match n {
            "HOMEDRIVE" => drive,
            "HOMEPATH" => path,
            _ => marked.as_str(),
        }
    };
    let steps = vec![
        format!("environment {m}"),
        format!("environment-is TEMP {temp}"),
        format!("environment-is TMP {temp}"),
        format!("environment-is SystemRoot {root}"),
    ];
    let judge = |label: &str, log: Option<String>, expected_names: &str| -> bool {
        let Some(log) = log else {
            say(&format!("h case={label} run-ended=no"));
            return false;
        };
        let names_ok = logged(&log, "environment names=") == expected_names;
        let marked_ok = logged(&log, "environment marked=").is_empty();
        let temp_ok = logged(&log, "environment-is TEMP ") == "same"
            && logged(&log, "environment-is TMP ") == "same";
        let root_ok = logged(&log, "environment-is SystemRoot ") == "same";
        let all = names_ok && marked_ok && temp_ok && root_ok;
        say(&format!(
            "h case={label} moves-nothing={} names={} unmarked={} temp={} root={}",
            yes(all),
            yes(names_ok),
            yes(marked_ok),
            yes(temp_ok),
            yes(root_ok)
        ));
        all
    };
    let mut held = 0;
    let mut cases = 0;
    cases += 1;
    if judge(
        "none-changed",
        host_run("v39-h-none", &steps, &[], &[]),
        "LOCALAPPDATA,SystemRoot,TEMP,TMP",
    ) {
        held += 1;
    }
    for n in names {
        cases += 1;
        if judge(
            n,
            host_run("v39-h-one", &steps, &[(n, value(n))], &[]),
            "LOCALAPPDATA,SystemRoot,TEMP,TMP",
        ) {
            held += 1;
        }
    }
    let all: Vec<(&str, &str)> = names
        .iter()
        .filter(|n| **n != "SystemRoot")
        .map(|n| (*n, value(n)))
        .collect();
    cases += 1;
    if judge(
        "all-but-SystemRoot-and-no-LOCALAPPDATA",
        host_run("v39-h-all", &steps, &all, &["LOCALAPPDATA"]),
        "SystemRoot,TEMP,TMP",
    ) {
        held += 1;
    }
    say(&format!("h cases={cases} moves-nothing={held}"));
}

#[test]
fn v39_i_arguments_with_spaces_quotes_and_backslashes_reach_a_runner_whose_path_has_a_space() {
    let bin = common::scratch("v39 i bin dir");
    let runner = bin.join("peer copy.exe");
    fs::copy(common::peer(), &runner).unwrap();
    let rig = common::Rig::with("v39 i sp ace", runner, Duration::from_secs(10));
    let models = [
        r"C:\m dir\x\",
        r"C:\m\tail\\",
        "a\"b",
        "a\\\\\"b c",
        "",
        " ",
        r"\\server\share\",
        "a\tb",
        r"C:\plain\model.gguf",
        "\"",
        r"\",
        "a b\\\\",
        "\"a b\"",
        r#"a\"b\\"#,
    ];
    let mut right = 0;
    for (i, model) in models.iter().enumerate() {
        let script = rig.script(&["hello", "idle"]);
        let mut spec = rig.spec(&script);
        spec.model = Some(PathBuf::from(model));
        spec.headed = true;
        let run = spec.run.clone();
        let started = rig.sup.start(spec);
        let seen = script.wait_log("args ", SHORT);
        let count = logged(&script.log(), "args ");
        let ok = started.is_ok() && seen && count == "13";
        if ok {
            right += 1;
        }
        say(&format!(
            "i argument={i} start-ok={} thirteen-arguments={}",
            yes(started.is_ok()),
            yes(count == "13")
        ));
        let _ = rig.sup.kill(&run);
    }
    say(&format!("i arguments={} right={right}", models.len()));
}

#[test]
fn v39_j_three_runners_started_at_the_same_moment_hold_no_pipe_beyond_their_own() {
    let rigs = [
        common::Rig::new("v39-j-a"),
        common::Rig::new("v39-j-b"),
        common::Rig::new("v39-j-c"),
    ];
    let rounds = 40;
    let mut starts = 0;
    let mut zero = 0;
    for _ in 0..rounds {
        let barrier = Barrier::new(rigs.len());
        let runs: Vec<_> = thread::scope(|s| {
            let handles: Vec<_> = rigs
                .iter()
                .map(|rig| {
                    let barrier = &barrier;
                    s.spawn(move || {
                        let script = rig.script(&["hello", "pipes", "log probed", "idle"]);
                        let spec = rig.spec(&script);
                        let run = spec.run.clone();
                        barrier.wait();
                        let started = rig.sup.start(spec);
                        (run, script, started)
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for ((run, script, started), rig) in runs.iter().zip(&rigs) {
            if started.is_ok() {
                starts += 1;
            }
            if script.wait_log("probed", SHORT) && logged(&script.log(), "pipes beyond=") == "0" {
                zero += 1;
            }
            let _ = rig.sup.kill(run);
        }
    }
    say(&format!(
        "j rounds={rounds} runners={} start-ok={starts} beyond-zero={zero}",
        rounds * rigs.len()
    ));
}

#[test]
fn v39_k_which_forms_of_a_windows_path_are_absolute() {
    // (form, absolute by verifier 39's model)
    let forms: [(&str, bool); 23] = [
        (r"C:\a", true),
        (r"c:\a b\c", true),
        ("C:/a", true),
        (r"Z:\", true),
        (r"\\server\share\a", true),
        (r"\\?\C:\a", true),
        (r"\\?\UNC\server\share\a", true),
        ("//server/share/a", true),
        (r"C:\a\..\b", true),
        (r"\\.\pipe\x", true),
        ("", false),
        ("a", false),
        (r"\a", false),
        ("/a", false),
        ("C:a", false),
        ("C:", false),
        (r"..\a", false),
        (r".\a", false),
        (r"\\", false),
        (r"\\server", false),
        (r"a\b", false),
        (r"1:\a", false),
        (r"\\server\", false),
    ];
    let mut same = 0;
    for (i, (form, expected)) in forms.iter().enumerate() {
        let got = Path::new(form).is_absolute();
        if got == *expected {
            same += 1;
        }
        say(&format!(
            "k form={i} absolute={} as-the-model={}",
            yes(got),
            yes(got == *expected)
        ));
    }
    say(&format!("k forms={} as-the-model={same}", forms.len()));
}
