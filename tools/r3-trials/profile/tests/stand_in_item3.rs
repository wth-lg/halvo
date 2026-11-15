use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PROGRAM: &str = env!("CARGO_BIN_EXE_profile-trial");
const CELL: &str = "machine=mac browser=chrome";
const RUN_BOUND: Duration = Duration::from_secs(60);
const PORT_WAIT: Duration = Duration::from_secs(60);

const CHECK: &str = "#!/bin/sh
cat case/inner
cat case/err >&2
mkdir -p target/egress/1
cat case/denied >target/egress/1/denied.log
if [ -f case/signal ]; then
  kill -TERM \"$PPID\"
  exec sleep 20
fi
exit 0
";

struct Case {
    name: &'static str,
    inner: Vec<String>,
    denied: Vec<&'static str>,
    first: bool,
    sandbox_off: bool,
    signal: bool,
    want: Vec<String>,
    code: i32,
}

fn inner(rest: &str) -> String {
    format!("egress_run {CELL} {rest}")
}

fn err_line(denied: usize) -> String {
    format!("egress mode=sandbox list=x command=0 denied={denied} log=target/egress/1/denied.log\n")
}

fn case(name: &'static str, inner_lines: &[String], want: &[String], code: i32) -> Case {
    Case {
        name,
        inner: inner_lines.to_vec(),
        denied: Vec::new(),
        first: false,
        sandbox_off: false,
        signal: false,
        want: want.to_vec(),
        code,
    }
}

fn cases() -> Vec<Case> {
    let started_no = inner("browser_started=no task=miss cause=start");
    let ok = inner("browser_started=yes task=ok cause=-");
    let set = |s: &str| format!("egress_set {CELL} set={s}");
    let mut list = vec![
        case(
            "a_failed_start",
            std::slice::from_ref(&started_no),
            &[
                started_no.clone(),
                set("final"),
                "egress machine=mac mode=sandbox browser_started=no denied=0".into(),
            ],
            0,
        ),
        case(
            "a_fault_before_the_start",
            &[inner("browser_started=no task=notrun cause=fixture")],
            &[
                inner("browser_started=no task=notrun cause=fixture"),
                format!("notrun item=3 {CELL} set=final cause=fixture"),
            ],
            1,
        ),
        case(
            "a_missed_task",
            &[inner("browser_started=yes task=miss cause=steps")],
            &[
                inner("browser_started=yes task=miss cause=steps"),
                format!("notrun item=3 {CELL} set=final cause=task"),
            ],
            1,
        ),
        case(
            "a_start_with_its_task_ok",
            std::slice::from_ref(&ok),
            &[
                ok.clone(),
                set("final"),
                "egress machine=mac mode=sandbox browser_started=yes denied=0".into(),
            ],
            0,
        ),
        case(
            "no_inner_line",
            &[],
            &[format!("notrun item=3 {CELL} set=final cause=egress_check")],
            1,
        ),
        case(
            "the_last_inner_line_counts",
            &[ok.clone(), started_no.clone()],
            &[
                ok.clone(),
                started_no.clone(),
                set("final"),
                "egress machine=mac mode=sandbox browser_started=no denied=0".into(),
            ],
            0,
        ),
    ];
    let denial = |s: &str, rest: &str| format!("egress_denial {CELL} set={s} {rest}");
    let mut control = case(
        "the_control",
        std::slice::from_ref(&ok),
        &[
            ok.clone(),
            denial("final", "proto=udp port=5353 loopback=no"),
            denial("final", "proto=tcp port=443 loopback=yes"),
            denial("final", "proto=dns port=- loopback=unknown"),
            format!("egress_control {CELL} set=final sandbox=off mode=sandbox browser_started=yes denied=3"),
        ],
        0,
    );
    control.sandbox_off = true;
    control.denied = vec![
        "denied at=1 proto=udp dest=10.1.0.4:5353 pid=7 image=chrome.exe",
        "denied at=2 proto=tcp dest=[::1]:443 pid=7 image=chrome.exe",
        "denied at=3 proto=dns dest=resolver pid=7 image=chrome.exe",
    ];
    list.push(control);
    let mut first = case(
        "the_first_set",
        std::slice::from_ref(&ok),
        &[
            ok.clone(),
            denial("first", "proto=udp port=443 loopback=unknown"),
            denial("first", "proto=tcp port=9 loopback=yes"),
            set("first"),
            "egress machine=mac mode=sandbox browser_started=yes denied=2".into(),
        ],
        0,
    );
    first.first = true;
    first.denied = vec![
        "denied at=1 proto=udp dest=-:443 pid=5 image=Google_Chrome_Helper",
        "denied at=2 proto=tcp dest=127.0.0.1:9 pid=5 image=Google_Chrome_Helper",
    ];
    list.push(first);
    if cfg!(unix) {
        let mut signal = case(
            "a_signal",
            std::slice::from_ref(&ok),
            &[ok.clone(), format!("notrun item=3 {CELL} cause=signal")],
            1,
        );
        signal.signal = true;
        list.push(signal);
    }
    list
}

struct Run(Option<Child>);

impl Drop for Run {
    fn drop(&mut self) {
        if let Some(mut c) = self.0.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

fn stand_in() -> (PathBuf, PathBuf, PathBuf) {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("stand-in-item3");
    let repo = root.join("repo");
    let scratch = root.join("scratch");
    let browsers = root.join("browser");
    for d in [
        repo.join("scripts/egress"),
        repo.join("case"),
        scratch.clone(),
        browsers.clone(),
    ] {
        std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{}: {e}", d.display()));
    }
    write(&repo.join("scripts/egress/check.sh"), CHECK);
    let browser = browsers.join(if cfg!(windows) {
        "chrome.exe"
    } else {
        "Google Chrome"
    });
    write(&browser, "not started by item 3's outer program\n");
    (repo, scratch, browser)
}

fn run(c: &Case, repo: &Path, scratch: &Path, browser: &Path) -> (Vec<String>, i32) {
    let dir = repo.join("case");
    let mut text: String = c.inner.iter().map(|l| format!("{l}\n")).collect();
    write(&dir.join("inner"), &text);
    write(&dir.join("err"), &err_line(c.denied.len()));
    text = c.denied.iter().map(|l| format!("{l}\n")).collect();
    write(&dir.join("denied"), &text);
    let signal = dir.join("signal");
    if c.signal {
        write(&signal, "");
    } else if signal.exists() {
        std::fs::remove_file(&signal).unwrap_or_else(|e| panic!("{e}"));
    }
    let waited = Instant::now();
    loop {
        let mut cmd = Command::new(PROGRAM);
        cmd.args(["--item", "3", "--browser"])
            .arg(browser)
            .args(["--port", "18090", "--machine", "mac", "--repo"])
            .arg(repo)
            .arg("--scratch")
            .arg(scratch);
        if c.first {
            cmd.args(["--flags", "first"]);
        }
        if c.sandbox_off {
            cmd.arg("--sandbox-off");
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut run = Run(Some(cmd.spawn().expect("the program starts")));
        let began = Instant::now();
        let status = loop {
            let child = run.0.as_mut().expect("a child");
            if let Some(s) = child.try_wait().expect("a wait") {
                break s;
            }
            assert!(
                began.elapsed() < RUN_BOUND,
                "case {}: the program ran past {RUN_BOUND:?}",
                c.name
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        let child = run.0.take().expect("a child");
        let out = child.wait_with_output().expect("the output");
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        if stderr.starts_with("REFUSED port") && waited.elapsed() < PORT_WAIT {
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        let lines = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        return (lines, status.code().unwrap_or(-1));
    }
}

#[test]
fn the_whole_program_prints_each_line_of_item_3_through_a_stand_in_check() {
    let (repo, scratch, browser) = stand_in();
    let mut wrong = Vec::new();
    let all = cases();
    for c in &all {
        let got = run(c, &repo, &scratch, &browser);
        if got != (c.want.clone(), c.code) {
            wrong.push(format!(
                "case {}: got {:?} exit {}, want {:?} exit {}",
                c.name, got.0, got.1, c.want, c.code
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert_eq!(all.len(), if cfg!(unix) { 9 } else { 8 });
}
