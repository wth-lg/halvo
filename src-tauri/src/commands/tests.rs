use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use halvo_control::client::{ClientError, Connector, NoTransport};

use super::control::{Control, Gated, MAX_CALLS_IN_FLIGHT, REPLY_BYTES_IN_FLIGHT};

const STEP_TIMEOUT: Duration = Duration::from_secs(20);
const ABSENT_STEP_WAIT: Duration = Duration::from_millis(300);

#[derive(Debug, Default)]
struct Gate {
    connects: Vec<String>,
    released: usize,
}

#[derive(Debug, Clone, Default)]
struct HeldUntilReleased {
    gate: Arc<(Mutex<Gate>, Condvar)>,
}

impl HeldUntilReleased {
    fn gate(&self) -> std::sync::MutexGuard<'_, Gate> {
        self.gate.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn connects(&self) -> Vec<String> {
        self.gate().connects.clone()
    }

    fn release_one(&self) {
        self.gate().released += 1;
        self.gate.1.notify_all();
    }

    fn wait_for_connects(&self, n: usize, limit: Duration) -> bool {
        let until = Instant::now() + limit;
        let mut gate = self.gate();
        while gate.connects.len() < n {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            gate = self
                .gate
                .1
                .wait_timeout(gate, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }
}

impl Connector for HeldUntilReleased {
    type Transport = NoTransport;

    fn endpoint(&self) -> String {
        "held".to_string()
    }

    fn connect(&self, _: Duration) -> Result<NoTransport, ClientError> {
        let name = std::thread::current().name().unwrap_or("-").to_string();
        let mut gate = self.gate();
        gate.connects.push(name);
        self.gate.1.notify_all();
        let until = Instant::now() + STEP_TIMEOUT;
        while gate.released == 0 && Instant::now() < until {
            let left = until.saturating_duration_since(Instant::now());
            gate = self
                .gate
                .1
                .wait_timeout(gate, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        gate.released = gate.released.saturating_sub(1);
        Err(ClientError::Unreachable {
            endpoint: "held".to_string(),
        })
    }
}

type Call = JoinHandle<Result<halvo_control::proto::DaemonStatus, ClientError>>;

fn status_call(gated: &Arc<Gated<HeldUntilReleased>>, name: &str) -> Call {
    let made = gated.places().make();
    let gated = Arc::clone(gated);
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || gated.status(made, false))
        .expect("a thread starts")
}

fn four_in_flight() -> (HeldUntilReleased, Arc<Gated<HeldUntilReleased>>, Vec<Call>) {
    let held = HeldUntilReleased::default();
    let gated = Arc::new(Gated::new(held.clone()));
    let calls = (0..4)
        .map(|n| status_call(&gated, &format!("in-flight-{n}")))
        .collect();
    assert!(
        held.wait_for_connects(4, STEP_TIMEOUT),
        "four calls connect"
    );
    assert_eq!(gated.places().in_flight(), 4);
    (held, gated, calls)
}

fn release_all(held: &HeldUntilReleased, calls: Vec<Call>) {
    for _ in 0..calls.len() + 2 {
        held.release_one();
    }
    for call in calls {
        let _ = call.join().expect("a call ends");
    }
}

#[test]
fn a_fifth_call_waits_until_one_returns() {
    assert_eq!(MAX_CALLS_IN_FLIGHT, 4);
    assert_eq!(halvo_control::wire::REPLY_MAX_BYTES, 262_144);
    assert_eq!(REPLY_BYTES_IN_FLIGHT, 1_048_576);
    let (held, gated, mut calls) = four_in_flight();
    calls.push(status_call(&gated, "fifth"));
    assert!(
        !held.wait_for_connects(5, ABSENT_STEP_WAIT),
        "a fifth call connected while four were in flight: {:?}",
        held.connects()
    );
    assert_eq!(gated.places().in_flight(), 4);
    assert_eq!(gated.places().waiting(), 1);
    held.release_one();
    assert!(
        held.wait_for_connects(5, STEP_TIMEOUT),
        "the fifth call connects once one returns"
    );
    assert_eq!(held.connects()[4], "fifth");
    release_all(&held, calls);
    assert_eq!(gated.places().in_flight(), 0);
    assert_eq!(gated.places().waiting(), 0);
}

#[test]
fn waiting_calls_take_the_free_places_in_the_order_in_which_they_were_made() {
    let (held, gated, mut calls) = four_in_flight();
    let first = gated.places().make();
    let second = gated.places().make();
    let third = gated.places().make();
    let spawn = |made, name: &str| {
        let gated = Arc::clone(&gated);
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || gated.status(made, false))
            .expect("a thread starts")
    };
    calls.push(spawn(third, "third"));
    calls.push(spawn(second, "second"));
    calls.push(spawn(first, "first"));
    assert!(!held.wait_for_connects(5, ABSENT_STEP_WAIT));
    for n in 5..=7 {
        held.release_one();
        assert!(held.wait_for_connects(n, STEP_TIMEOUT), "call {n} connects");
    }
    assert_eq!(held.connects()[4..], ["first", "second", "third"]);
    release_all(&held, calls);
}

#[test]
fn the_deadline_of_a_call_runs_from_the_moment_it_is_made_and_counts_the_wait() {
    let (held, gated, calls) = four_in_flight();
    let made = gated.places().make();
    let started = Instant::now();
    let answer = gated.status(made, false);
    let waited = started.elapsed();
    assert!(
        matches!(answer, Err(ClientError::Deadline { seconds: 2 })),
        "{answer:?}"
    );
    assert!(
        waited >= Duration::from_millis(2_000)
            && waited < Duration::from_millis(2_000) + STEP_TIMEOUT,
        "waited {waited:?}"
    );
    assert_eq!(held.connects().len(), 4, "the late call never connected");
    assert_eq!(gated.places().waiting(), 0, "the late call left the line");
    release_all(&held, calls);
}

#[test]
fn a_call_that_is_made_and_dropped_before_it_enters_leaves_the_line() {
    let (held, gated, mut calls) = four_in_flight();
    let dropped = gated.places().make();
    calls.push(status_call(&gated, "after"));
    drop(dropped);
    held.release_one();
    assert!(
        held.wait_for_connects(5, STEP_TIMEOUT),
        "the call after the dropped one connects"
    );
    assert_eq!(held.connects()[4], "after");
    release_all(&held, calls);
}

#[test]
fn the_ledger_line_of_the_unit_states_the_bytes_of_the_replies_in_flight() {
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../docs/baseline/ledger.d/app-2.txt");
    let text = std::fs::read_to_string(&file).expect("the ledger file of app-2 exists");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines,
        [format!(
            "ledger process=app name=client_replies bytes={REPLY_BYTES_IN_FLIGHT} kind=by-construction source=commands::tests::a_fifth_call_waits_until_one_returns"
        )]
    );
}

#[derive(Debug, Clone, Default)]
struct Timeouts(Arc<Mutex<Vec<Duration>>>);

impl Connector for Timeouts {
    type Transport = NoTransport;

    fn endpoint(&self) -> String {
        "timeouts".to_string()
    }

    fn connect(&self, timeout: Duration) -> Result<NoTransport, ClientError> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(timeout);
        Err(ClientError::Unreachable {
            endpoint: "timeouts".to_string(),
        })
    }
}

#[test]
fn the_client_gets_only_what_is_left_of_the_deadline_after_the_wait() {
    const AGED: Duration = Duration::from_millis(600);
    let timeouts = Timeouts::default();
    let gated = Gated::new(timeouts.clone());
    let made = gated.places().make();
    let aged_until = Instant::now() + AGED;
    while Instant::now() < aged_until {
        std::thread::park_timeout(aged_until.saturating_duration_since(Instant::now()));
    }
    let _ = gated.status(made, false);
    let seen = timeouts.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert_eq!(seen.len(), 1);
    assert!(
        seen[0] <= Duration::from_millis(2_000) - AGED,
        "the connect got {:?} of a 2 s deadline whose call was made {AGED:?} earlier",
        seen[0]
    );
}
