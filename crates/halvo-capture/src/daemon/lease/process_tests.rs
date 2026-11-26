use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use halvo_control::paths::DataDir;
use halvo_control::proto::SourceName;

use super::lifeline::start_or_status;
use super::Lease;
use crate::daemon::instance::tests::kept_dir;
use crate::daemon::main_loop::{self, MainLoop};
use crate::daemon::model_store::foreground_now;
use crate::daemon::sources::tests::Factory;
use crate::daemon::state::tests::TestWrite;
use crate::daemon::state::{Desired, StateDir, StateFile};
use crate::daemon::supervisor::SHUTDOWN_BUDGET_MS;
use crate::daemon::{serve_on, ServiceParts};
use crate::source::SourceContext;

const DIR: &str = "HALVO_LIFELINE_DIR";
const MARKS: &str = "HALVO_LIFELINE_MARKS";
const PARENT_OF: &str = "HALVO_LIFELINE_PARENT_OF";
const CHILD: &str = "daemon::lease::process_tests::lifeline_child";
const STAND_IN: &str = "daemon::lease::process_tests::lifeline_stand_in_parent";
const ENDS_FIRST: &str = "HALVO_LIFELINE_ENDS_FIRST";
const EVENTS: usize = 5;
const READY_WITHIN: Duration = Duration::from_secs(60);

struct SleepLoop(Instant);

impl MainLoop for SleepLoop {
    fn turn(&mut self, wait: Duration) {
        std::thread::sleep(wait);
    }

    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

fn ended_with(marks: &Path, status: i32) -> ! {
    let _ = std::fs::write(marks.join("status"), status.to_string());
    std::process::exit(status)
}

#[test]
fn lifeline_child() {
    let (Some(dir), Some(marks)) = (std::env::var_os(DIR), std::env::var_os(MARKS)) else {
        return;
    };
    let marks = PathBuf::from(marks);
    let ends_first = std::env::var(ENDS_FIRST).ok();
    if let Some(status) = ends_first.and_then(|status| status.parse().ok()) {
        ended_with(&marks, status);
    }
    let lifeline = match start_or_status(&mut std::io::stderr()) {
        Ok(lifeline) => lifeline,
        Err(status) => ended_with(&marks, status),
    };
    let emitted = marks.join("emitted");
    let factory = Factory::adjusting(move |source| {
        if source.name == SourceName::Foreground {
            let emitted = emitted.clone();
            source.on_begin = Some(Box::new(move |ctx: &SourceContext| {
                for _ in 0..EVENTS {
                    let _ = ctx.sink().emit_event(foreground_now());
                }
                let _ = std::fs::write(&emitted, b"");
            }));
        }
    });
    let persisted = marks.join("persisted");
    let parts = ServiceParts {
        lifeline: Some(lifeline),
        lease: Lease::never_lapsing(),
        settled: Some(Box::new(move |settled| {
            let count = settled.map_or(0, |settled| settled.events_persisted);
            let _ = std::fs::write(&persisted, count.to_string());
        })),
        ..ServiceParts::of(factory)
    };
    let stop = AtomicBool::new(false);
    let status = serve_on(
        &DataDir::from_flag(PathBuf::from(dir)).unwrap(),
        Box::new(TestWrite::default()),
        SleepLoop(Instant::now()),
        &stop,
        Arc::new(main_loop::ExitNow),
        &mut std::io::stderr(),
        parts,
    );
    ended_with(&marks, status)
}

#[test]
fn lifeline_stand_in_parent() {
    let Some(dir) = std::env::var_os(PARENT_OF) else {
        return;
    };
    let marks = std::env::var_os(MARKS).unwrap();
    let status = child_command(Path::new(&dir), Path::new(&marks))
        .stdin(Stdio::inherit())
        .status()
        .unwrap();
    std::process::exit(status.code().unwrap_or(1));
}

fn child_command(dir: &Path, marks: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", CHILD, "--test-threads=1"])
        .env(DIR, dir)
        .env(MARKS, marks)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    command
}

fn case_dirs(name: &str) -> (PathBuf, PathBuf) {
    let root = kept_dir("lifeline-tests-");
    let data = root.join("data");
    let marks = root.join(name);
    std::fs::create_dir(&marks).unwrap();
    std::fs::create_dir(&data).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&data, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut state = StateFile::open(
        StateDir::open(&data).unwrap(),
        Box::new(TestWrite::default()),
    );
    state
        .update(|state| state.desired = Desired::Running)
        .unwrap();
    (data, marks)
}

fn seen(data: &Path, marks: &Path) -> String {
    let mut names: Vec<String> = std::fs::read_dir(marks)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    let status = std::fs::read_to_string(marks.join("status")).ok();
    let key = data.join("store").join("store.key").exists();
    let made = std::fs::read_to_string(data.join("daemon-state.json"))
        .is_ok_and(|text| text.contains("\"store_made\":true"));
    format!("marks={names:?} status={status:?} store_key={key} store_made={made}")
}

fn wait_for(
    mut started_by_the_test: Option<&mut Child>,
    (data, marks): (&Path, &Path),
    mark: &str,
    within: Duration,
    what: &str,
) {
    let path = marks.join(mark);
    let started = Instant::now();
    while !path.exists() {
        let ended = started_by_the_test
            .as_mut()
            .and_then(|child| child.try_wait().unwrap());
        if let Some(status) = ended {
            if path.exists() {
                return;
            }
            panic!(
                "{what}: the process that the test started ended before it, with {status}; {}",
                seen(data, marks)
            );
        }
        let asked = if started_by_the_test.is_some() {
            "the process that the test started still ran"
        } else {
            "the test held no process to ask"
        };
        assert!(
            started.elapsed() < within,
            "{what} within {within:?}: {asked}; {}",
            seen(data, marks)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn exited_within(child: &mut Child, bound: Duration) -> Option<(i32, Duration)> {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return Some((status.code().unwrap_or(-1), started.elapsed()));
        }
        if started.elapsed() > bound {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn persisted(marks: &Path) -> String {
    std::fs::read_to_string(marks.join("persisted")).unwrap_or_default()
}

#[test]
fn a_lifeline_ends_the_daemon_with_0_inside_the_budget() {
    let budget = Duration::from_millis(SHUTDOWN_BUDGET_MS);

    let (data, marks) = case_dirs("closed");
    let (reader, writer) = std::io::pipe().unwrap();
    let mut child = child_command(&data, &marks).stdin(reader).spawn().unwrap();
    wait_for(
        Some(&mut child),
        (&data, &marks),
        "emitted",
        READY_WITHIN,
        "the child's source emitted",
    );
    let closed = Instant::now();
    drop(writer);
    let ended = exited_within(&mut child, budget + Duration::from_secs(10));
    let took = closed.elapsed();
    assert_eq!(
        ended.map(|(status, _)| status),
        Some(0),
        "the closed lifeline"
    );
    assert!(took < budget, "the child ended {took:?} after the close");
    assert_eq!(persisted(&marks), EVENTS.to_string(), "events persisted");

    let (data, marks) = case_dirs("parent");
    let (reader, writer) = std::io::pipe().unwrap();
    let mut stand_in = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", STAND_IN, "--test-threads=1"])
        .env(PARENT_OF, &data)
        .env(MARKS, &marks)
        .stdin(reader)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    wait_for(
        Some(&mut stand_in),
        (&data, &marks),
        "emitted",
        READY_WITHIN,
        "the child's source emitted",
    );
    let killed = Instant::now();
    stand_in.kill().unwrap();
    stand_in.wait().unwrap();
    wait_for(
        None,
        (&data, &marks),
        "status",
        budget + Duration::from_secs(10),
        "the child's status after its parent was killed",
    );
    let took = killed.elapsed();
    let status = std::fs::read_to_string(marks.join("status")).unwrap();
    drop(writer);
    assert_eq!(status, "0", "the child of a killed parent");
    assert!(
        took < budget,
        "the child ended {took:?} after its parent was killed"
    );

    let (data, marks) = case_dirs("null");
    let ended = child_command(&data, &marks)
        .stdin(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(ended.code(), Some(halvo_control::exit::USAGE));
}
