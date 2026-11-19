//! The four tests of a failed start and of a stop (CP-170 E15 (2), CP-186 N1), on macOS and on
//! Windows, with the crate's stand-in browser `web-standin` as the browser's program in an install
//! directory of each test's own and as the fenced guard. Each test ends, on every way out, the
//! processes whose program lies below its own directory and that began after this process.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use halvo_web::cdp::{CrashMeans, Executor, StartOptions, PROBE_HOST, PROBE_PORT};
use halvo_web::guard::GuardProgram;
use halvo_web::procs::{self, Proc};
use halvo_web::relay::{Names, Relay};

const STAND_IN: &str = env!("CARGO_BIN_EXE_web-standin");

/// The program name of a stand-in in the start's failure.
const STAND_IN_NAME: &str = if cfg!(windows) {
    "stand-in.exe"
} else {
    "stand-in"
};

/// One test's stand-ins: a directory of its own under the target's `tmp`, with a copy of the
/// stand-in as the browser's program in its install directory (an app bundle on macOS) and the
/// mode beside it. Dropping it ends every process whose program lies below the directory and
/// that began after this process.
struct Scene {
    dir: PathBuf,
    browser: PathBuf,
    profile: PathBuf,
    since_us: u64,
}

impl Scene {
    fn new(name: &str, mode: &str) -> Scene {
        let made = halvo_test_dir::make(
            Path::new(env!("CARGO_TARGET_TMPDIR")),
            &format!("halvo-web-{name}"),
        )
        .unwrap();
        let dir = if cfg!(windows) {
            made
        } else {
            std::fs::canonicalize(&made).unwrap()
        };
        let install = if cfg!(windows) {
            dir.join("Stand-in")
        } else {
            dir.join("Stand-in.app").join("Contents").join("MacOS")
        };
        std::fs::create_dir_all(&install).unwrap();
        let browser = install.join(STAND_IN_NAME);
        std::fs::copy(STAND_IN, &browser).unwrap();
        std::fs::write(install.join("stand-in.mode"), mode).unwrap();
        Scene {
            profile: dir.join("p"),
            dir,
            browser,
            since_us: procs::own().unwrap().started_us,
        }
    }

    /// The fenced guard of the tests: the stand-in, whose ask and forced end reach only a
    /// program below this test's directory.
    fn guard(&self) -> GuardProgram {
        GuardProgram {
            program: PathBuf::from(STAND_IN),
            prefix: vec!["--fence-below".into(), self.dir.clone().into_os_string()],
        }
    }

    fn options(&self, relay: &Relay, guard: GuardProgram) -> StartOptions {
        StartOptions {
            browser: self.browser.clone(),
            profile: self.profile.clone(),
            headless: true,
            relay: relay.handle(),
            guard,
            crash_means: CrashMeans::Product,
        }
    }

    /// The processes that began after this process and bear the run's profile among their
    /// arguments (`checks::bears_profile`).
    fn naming(&self) -> Vec<Proc> {
        let profile = self.profile.to_string_lossy().into_owned();
        procs::table()
            .unwrap()
            .into_iter()
            .filter(|p| p.started_us >= self.since_us && p.pid != std::process::id())
            .filter(|p| {
                halvo_web::checks::command_args(p.pid)
                    .is_some_and(|a| halvo_web::checks::bears_profile(&a, &profile))
            })
            .collect()
    }

    /// `naming`, read until `wait` has passed or one shows.
    async fn named_within(&self, wait: Duration) -> Vec<Proc> {
        let until = Instant::now() + wait;
        loop {
            let left = self.naming();
            if !left.is_empty() || Instant::now() > until {
                return left;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// The processes whose program lies below the test's directory and that began after this
    /// process: the test's own stand-ins.
    fn ours(&self) -> Vec<Proc> {
        let norm = |p: &Path| {
            let s = p.to_string_lossy().into_owned();
            if cfg!(windows) {
                s.replace('\\', "/").to_lowercase()
            } else {
                s
            }
        };
        let dir = format!("{}/", norm(&self.dir).trim_end_matches('/'));
        procs::table()
            .unwrap()
            .into_iter()
            .filter(|p| p.started_us >= self.since_us && p.started_us != 0)
            .filter(|p| p.exe.as_deref().is_some_and(|e| norm(e).starts_with(&dir)))
            .collect()
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        for p in self.ours() {
            procs::kill(&p);
        }
    }
}

/// CP-126 E1 and point 42: a failed start ends every process that names the profile: the
/// stand-in leaves a helper and ends before its first reply, which is `browser_start`, kept apart
/// from `no_channel` (CP-165 (9)); the failure reports both directories removed.
#[tokio::test]
async fn a_failed_start_ends_a_process_that_the_browser_left_with_its_profile() {
    let scene = Scene::new("leftover", "leave");
    let relay = Relay::start(Names::Fixture).await.unwrap();
    let started = Executor::start(&scene.options(&relay, scene.guard())).await;
    let left = scene.naming();
    let failed = started.err().expect("the start failed");
    assert_eq!(failed.cause, "browser_start", "{failed:?}");
    assert!(failed.removed, "{failed:?}");
    assert_eq!(
        left.len(),
        0,
        "processes left that name the profile: {left:?}"
    );
}

/// The review's point 3, finding (3): a start for which no guard could be started fails with the
/// cause `guard`, ends its browser and leaves no process that names the profile.
#[tokio::test]
async fn a_start_with_no_guard_fails_and_leaves_no_browser() {
    let scene = Scene::new("no-guard", "browser_slow_probe");
    let relay = Relay::start(Names::Fixture).await.unwrap();
    let none = GuardProgram {
        program: scene.dir.join("nowhere").join("guard"),
        prefix: Vec::new(),
    };
    let started = Executor::start(&scene.options(&relay, none)).await;
    let left = scene.named_within(Duration::from_millis(500)).await;
    let failed = started.err().expect("the start went on with no guard");
    assert_eq!(failed.cause, "guard", "{failed:?}");
    assert!(failed.removed, "{failed:?}");
    assert_eq!(
        left.len(),
        0,
        "processes left that name the profile: {left:?}"
    );
}

/// CP-158 (1), CP-165 (9), CP-189 G5: a browser that lives and gives no first reply within the
/// bound is `no_channel`, and its tree is ended by the guard's rule: nothing that bears the run's
/// mark is left. Its helper, which names no profile and outlives the test's wait, does not bear
/// the mark: it is left, and the start's failure counts and names it.
#[tokio::test]
async fn a_start_with_no_first_reply_ends_its_tree_and_is_no_channel() {
    let scene = Scene::new("no-reply", "hang");
    let relay = Relay::start(Names::Fixture).await.unwrap();
    let started = Executor::start(&scene.options(&relay, scene.guard())).await;
    let left = scene.named_within(Duration::from_millis(500)).await;
    let helpers = scene.ours();
    let failed = started.err().expect("the start failed");
    assert_eq!(failed.cause, "no_channel", "{failed:?}");
    assert!(failed.removed, "{failed:?}");
    assert_eq!(
        left.len(),
        0,
        "processes left that name the profile: {left:?}"
    );
    assert_eq!(helpers.len(), 1, "the helper without the mark: {helpers:?}");
    assert_eq!(
        (failed.left_unmarked, failed.left_unmarked_programs.clone()),
        (1, vec![STAND_IN_NAME.to_string()]),
        "{failed:?}"
    );
}

/// CP-126 E1 (1): a stand-in browser that answers the start and the stop on its pipe starts,
/// when it is asked to close, a process that names the profile and starts another one 300 ms
/// later, after the browser has ended. After the stop none of them is left. The stand-in asks
/// for the start's probe 500 ms after the navigation, and the start returned only after the
/// relay recorded its refusal.
#[tokio::test]
async fn a_normal_stop_ends_a_process_that_appears_after_the_browser_ended() {
    let scene = Scene::new("late", "browser_slow_probe");
    let relay = Relay::start(Names::Fixture).await.unwrap();
    let started = Executor::start(&scene.options(&relay, scene.guard())).await;
    let ex = started.expect("the stand-in starts");
    assert_eq!(relay.handle().refusals_of(PROBE_HOST, PROBE_PORT), 1);
    let stopped = ex.stop_and_remove().await;
    let left = scene.named_within(Duration::from_secs(2)).await;
    assert!(stopped.removed);
    assert_eq!(
        left.len(),
        0,
        "processes left after the stop that name the profile: {left:?}"
    );
}

/// CP-186 N8: the fence is on in this test target from its start: the stand-in and
/// `Names::Fixture` stand behind it.
#[test]
fn the_fence_is_on_in_this_test_target() {
    assert!(
        std::hint::black_box(cfg!(feature = "test-fence")),
        "the fence is off in this test target"
    );
}
