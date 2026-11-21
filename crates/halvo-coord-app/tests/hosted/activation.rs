use std::ffi::OsStr;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

/// The setting of the hosted targets that holds the main thread's run call of the test runtime
/// back by its number of milliseconds, after the cases' thread has started: the forced form of
/// the first show's race (rulings RD-145 (d) and RD-146 (3)). No job sets it; unset, nothing is
/// held back.
pub const LATE: &str = "HALVO_HOSTED_LATE_RUN_MS";

/// The longest hold, in milliseconds, that the setting `LATE` may ask for.
pub const LATE_MOST: u64 = 60_000;

/// How long the cases' thread waits for the sign that the test runtime's run loop runs.
pub const RUN_LOOP_BOUND: Duration = Duration::from_secs(10);

/// The hold that the value `value` of the setting `LATE` asks for: none when it is not set, the
/// milliseconds when they are a whole number from 1 to `LATE_MOST`, and otherwise the line of
/// the refusal.
pub fn late(value: Option<&OsStr>) -> Result<Option<Duration>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let digits = value
        .to_str()
        .filter(|text| text.bytes().all(|b| b.is_ascii_digit()));
    match digits.and_then(|text| text.parse::<u64>().ok()) {
        Some(ms) if (1..=LATE_MOST).contains(&ms) => Ok(Some(Duration::from_millis(ms))),
        _ => Err(format!(
            "start FAIL: {LATE} is set and is not a whole number of milliseconds from 1 to {LATE_MOST}; no case ran"
        )),
    }
}

/// How the cases' thread's wait for the test runtime's run loop ended (ruling RD-146 (3)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Begun {
    /// The run loop gave its sign after `waited`.
    After { waited: Duration },
    /// No sign came within `bound`.
    Not { bound: Duration },
    /// The channel of the sign closed after `waited` with no sign: the main thread dropped its
    /// end without the sign.
    Closed { waited: Duration },
}

/// Waits for the sign that the test runtime's run loop runs, so that a show that the cases'
/// thread asks for after it runs on the main thread: `sign` waits for the sign at most the time
/// that it is given and gives what the sign's channel gave, `elapsed` reads the time since the
/// wait began, and the wait ends at `bound`.
pub fn run_loop(
    sign: impl FnOnce(Duration) -> Result<(), RecvTimeoutError>,
    elapsed: impl FnOnce() -> Duration,
    bound: Duration,
) -> Begun {
    match sign(bound) {
        Ok(()) => Begun::After { waited: elapsed() },
        Err(RecvTimeoutError::Timeout) => Begun::Not { bound },
        Err(RecvTimeoutError::Disconnected) => Begun::Closed { waited: elapsed() },
    }
}

/// The line that the target `target` prints for `begun`, and whether its cases run.
pub fn begun(target: &str, begun: Begun) -> (String, bool) {
    match begun {
        Begun::After { waited } => (
            format!(
                "{target} start: the test runtime's run loop gave its sign after {} ms, and the first show goes out after it",
                waited.as_millis()
            ),
            true,
        ),
        Begun::Not { bound } => (
            format!(
                "{target} start FAIL: the test runtime's run loop gave no sign that it runs within {} s, so a show would run off the main thread; no case ran",
                bound.as_secs()
            ),
            false,
        ),
        Begun::Closed { waited } => (
            format!(
                "{target} start FAIL: the channel of the run loop's sign closed after {} ms with no sign: the main thread dropped its end without the sign that the test runtime's run loop runs; no case ran",
                waited.as_millis()
            ),
            false,
        ),
    }
}

/// How the hosted target's wait for its app to be active ended (ruling RD-107).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waited {
    /// The app read active after `asks` asks.
    Active { asks: u32 },
    /// The app never read active within the limit, after `asks` asks.
    Inactive { asks: u32 },
}

/// Asks once for the app to be active and reads whether it is; while it is not, passes `every`
/// through `pass` and asks and reads again, until `limit` has passed in all.
pub fn wait(
    mut ask: impl FnMut(),
    mut active: impl FnMut() -> bool,
    mut pass: impl FnMut(Duration),
    every: Duration,
    limit: Duration,
) -> Waited {
    let mut asks = 1;
    let mut passed = Duration::ZERO;
    ask();
    while !active() {
        if passed + every > limit {
            return Waited::Inactive { asks };
        }
        pass(every);
        passed += every;
        asks += 1;
        ask();
    }
    Waited::Active { asks }
}

/// The hosted target's start (ruling RD-107, its Addition 1): brings its own panel forward once
/// through `front`, waits as `wait` does over `ask`, `active` and `pass`, then orders the panel
/// out through `out`, whatever the wait gave.
pub fn start(
    front: impl FnOnce(),
    ask: impl FnMut(),
    active: impl FnMut() -> bool,
    pass: impl FnMut(Duration),
    out: impl FnOnce(),
    every: Duration,
    limit: Duration,
) -> Waited {
    front();
    let waited = wait(ask, active, pass, every, limit);
    out();
    waited
}
