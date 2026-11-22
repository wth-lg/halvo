#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const NAME: &str =
    "the_app_runs_the_built_offline_delete_and_the_directory_reads_as_a_whole_delete_leaves_it";
const WITHIN: Duration = Duration::from_secs(60);

struct Own(Child);

impl Drop for Own {
    fn drop(&mut self) {
        if let Ok(None) = self.0.try_wait() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn output(program: &Path, args: &[&str], dir: &Path, verb: Option<&str>) -> Output {
    Command::new(program)
        .args(args)
        .arg(dir)
        .args(verb)
        .stdin(Stdio::null())
        .output()
        .unwrap_or_else(|e| panic!("{} does not start: {e}", program.display()))
}

fn answer_of(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {out:?}"))
}

fn roots(dir: &Path) -> usize {
    std::fs::read_dir(dir.join("roots")).map_or(0, Iterator::count)
}

#[test]
fn the_app_runs_the_built_offline_delete_and_the_directory_reads_as_a_whole_delete_leaves_it() {
    if std::env::var("GITHUB_ACTIONS").as_deref() != Ok("true") {
        eprintln!(
            "{NAME}: not run: it starts the built app and halvo-capture, on a hosted runner alone"
        );
        return;
    }
    let began = Instant::now();
    let app = PathBuf::from(env!("CARGO_BIN_EXE_halvo"));
    let daemon = app.with_file_name("halvo-capture");
    assert!(daemon.is_file(), "{} is not built", daemon.display());
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("offline-delete-{}", std::process::id()));
    std::os::unix::fs::DirBuilderExt::mode(std::fs::DirBuilder::new().recursive(true), 0o700)
        .create(&dir)
        .expect("the scratch data directory is made");

    let mut service = Own(Command::new(&daemon)
        .args(["--service-run", "--data-dir"])
        .arg(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .expect("the service run starts"));
    let ctl = ["ctl", "--data-dir"];
    let until = Instant::now() + WITHIN;
    while !String::from_utf8_lossy(&output(&daemon, &ctl, &dir, Some("status")).stdout)
        .lines()
        .any(|line| line == "store=open")
    {
        assert!(Instant::now() < until, "no store=open within {WITHIN:?}");
        std::thread::sleep(Duration::from_millis(200));
    }
    let stop = output(&daemon, &ctl, &dir, Some("stop"));
    assert!(stop.status.success(), "ctl stop: {stop:?}");
    assert!(service.0.wait().expect("the service run ends").success());
    assert!(roots(&dir) > 0, "a store stands before the delete");

    let deleted = output(&app, &["--delete-all", "--data-dir"], &dir, None);
    assert_eq!(deleted.status.code(), Some(0), "{deleted:?}");
    let answer = answer_of(&deleted);
    assert_eq!(answer["outcome"], "deleted", "{answer}");
    assert_eq!(answer["nothing_left"], true, "{answer}");
    assert_eq!(answer["looked_in"], dir.display().to_string(), "{answer}");
    assert_eq!(roots(&dir), 0, "no root file is left");
    let again = output(&app, &["--delete-all", "--data-dir"], &dir, None);
    assert_eq!(again.status.code(), Some(0), "{again:?}");
    assert_eq!(answer_of(&again)["outcome"], "no_store", "{again:?}");
    eprintln!(
        "{NAME}: ran on a hosted runner in {} s: deleted, nothing left, looked_in the directory, then no_store",
        began.elapsed().as_secs()
    );
}
