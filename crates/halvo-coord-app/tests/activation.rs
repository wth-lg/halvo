#[path = "hosted/activation.rs"]
mod activation;

use activation::{wait, Waited};
use std::cell::Cell;
use std::time::Duration;

const EVERY: Duration = Duration::from_secs(1);
const LIMIT: Duration = Duration::from_secs(20);

/// Drawn counts of asks before the app reads active, from 0 to 29, by a fixed congruence.
fn draws(count: usize) -> Vec<u32> {
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    (0..count)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 33) % 30) as u32
        })
        .collect()
}

/// Runs the wait over an app that reads active at its read number `after` (0 for the first read),
/// or never for `None`: the result, the asks made and the time passed.
fn run(after: Option<u32>) -> (Waited, u32, Duration) {
    let asks = Cell::new(0u32);
    let reads = Cell::new(0u32);
    let passed = Cell::new(Duration::ZERO);
    let result = wait(
        || asks.set(asks.get() + 1),
        || {
            let n = reads.get();
            reads.set(n + 1);
            after.is_some_and(|after| n >= after)
        },
        |d| passed.set(passed.get() + d),
        EVERY,
        LIMIT,
    );
    (result, asks.get(), passed.get())
}

#[test]
fn activation_an_app_active_at_once_is_asked_once_and_waits_nothing() {
    assert_eq!(
        run(Some(0)),
        (Waited::Active { asks: 1 }, 1, Duration::ZERO)
    );
}

#[test]
fn activation_an_app_active_after_n_asks_is_asked_every_second_until_it_is() {
    for after in draws(200) {
        let (result, asks, passed) = run(Some(after));
        if after <= 20 {
            assert_eq!(result, Waited::Active { asks: after + 1 }, "after {after}");
            assert_eq!(asks, after + 1, "after {after}");
            assert_eq!(passed, EVERY * after, "after {after}");
        } else {
            assert_eq!(result, Waited::Inactive { asks: 21 }, "after {after}");
            assert_eq!(passed, LIMIT, "after {after}");
        }
    }
}

#[test]
fn activation_an_app_never_active_is_inactive_after_twenty_seconds_and_21_asks() {
    assert_eq!(run(None), (Waited::Inactive { asks: 21 }, 21, LIMIT));
}
