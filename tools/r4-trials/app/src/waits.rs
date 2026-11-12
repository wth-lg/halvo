use std::sync::{Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// How a wait for a report ended: with the report, `Ended` when a newer token or a cancel ended
/// it, or `TimedOut` when its bound passed first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Waited<T> {
    Reported(T),
    Ended,
    TimedOut,
}

/// A report that the core asks the page for under a token: the first animation frame after a
/// show, or the page's counters. Only the report of the newest token is kept.
pub struct Reports<T> {
    state: Mutex<ReportState<T>>,
    arrived: Condvar,
}

struct ReportState<T> {
    token: u64,
    value: Option<T>,
    alone: bool,
}

impl<T> Default for Reports<T> {
    fn default() -> Self {
        Reports {
            state: Mutex::new(ReportState {
                token: 0,
                value: None,
                alone: false,
            }),
            arrived: Condvar::new(),
        }
    }
}

impl<T: Clone> Reports<T> {
    /// Starts a new token, from 1, and drops a report of an older one.
    pub fn begin(&self) -> u64 {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.token += 1;
        state.value = None;
        state.alone = false;
        self.arrived.notify_all();
        state.token
    }

    /// Starts a new token as `begin` does, unless the token of an earlier `begin_alone` has not
    /// ended yet: then it returns `None` and changes nothing. That token ends when its wait
    /// ends or at `cancel`.
    pub fn begin_alone(&self) -> Option<u64> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.alone {
            return None;
        }
        state.token += 1;
        state.value = None;
        state.alone = true;
        self.arrived.notify_all();
        Some(state.token)
    }

    /// Ends every wait with `Waited::Ended` and drops a later report of the current token.
    pub fn cancel(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.token += 1;
        state.value = None;
        state.alone = false;
        self.arrived.notify_all();
    }

    /// Keeps `value` when `token` is the newest; returns whether it was kept.
    pub fn deliver(&self, token: u64, value: T) -> bool {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if token == 0 || token != state.token {
            return false;
        }
        state.value = Some(value);
        self.arrived.notify_all();
        true
    }

    /// Waits for the report of `token` for at most `bound`.
    pub fn wait(&self, token: u64, bound: Duration) -> Waited<T> {
        let deadline = Instant::now()
            .checked_add(bound)
            .unwrap_or_else(Instant::now);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let waited = loop {
            if state.token != token {
                break Waited::Ended;
            }
            if let Some(value) = state.value.clone() {
                break Waited::Reported(value);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break Waited::TimedOut;
            }
            state = self
                .arrived
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        };
        if state.token == token {
            state.alone = false;
        }
        waited
    }
}

/// The page's two counters, `ticks` and `frames`, as the core reads them for `sample`: one
/// request under a token, and the page's report of that token. A sample without that report
/// has no counts; an earlier report never stands in for it.
#[derive(Default)]
pub struct Counts {
    reports: Reports<(u64, u64)>,
}

impl Counts {
    /// Asks the page for its counts with `ask(token)` and waits for the report of that token
    /// for at most `bound`.
    pub fn read(&self, ask: impl FnOnce(u64), bound: Duration) -> Option<(u64, u64)> {
        let token = self.reports.begin();
        ask(token);
        match self.reports.wait(token, bound) {
            Waited::Reported(counts) => Some(counts),
            Waited::Ended | Waited::TimedOut => None,
        }
    }

    /// Takes the page's report of `token`; returns whether `token` was the newest.
    pub fn deliver(&self, token: u64, counts: (u64, u64)) -> bool {
        self.reports.deliver(token, counts)
    }
}

/// Calls `done` every 20 ms until it returns `true` or `bound` has passed since the first call;
/// returns whether it returned `true`.
pub fn until(bound: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now()
        .checked_add(bound)
        .unwrap_or_else(Instant::now);
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{join, within};
    use std::sync::Arc;

    #[test]
    fn tokens_count_from_one() {
        within(|| {
            let r = Reports::<()>::default();
            assert_eq!(r.begin(), 1);
            assert_eq!(r.begin(), 2);
        });
    }

    #[test]
    fn a_report_of_the_newest_token_is_kept_and_read() {
        within(|| {
            let r = Reports::default();
            let t = r.begin();
            assert!(r.deliver(t, (3u64, 4u64)));
            assert_eq!(r.wait(t, Duration::ZERO), Waited::Reported((3, 4)));
        });
    }

    #[test]
    fn a_report_of_an_older_or_unknown_token_is_dropped() {
        within(|| {
            let r = Reports::default();
            let old = r.begin();
            let new = r.begin();
            assert!(!r.deliver(old, 1u8));
            assert!(!r.deliver(new + 1, 1u8));
            assert!(!r.deliver(0, 1u8));
            assert_eq!(r.wait(new, Duration::from_millis(20)), Waited::TimedOut);
            assert!(r.deliver(new, 2u8));
            assert_eq!(r.wait(old, Duration::ZERO), Waited::Ended);
            assert_eq!(r.wait(new, Duration::ZERO), Waited::Reported(2));
        });
    }

    #[test]
    fn a_report_before_any_token_is_dropped() {
        within(|| {
            let r = Reports::default();
            assert!(!r.deliver(1, ()));
            assert_eq!(r.begin(), 1);
            assert_eq!(r.wait(1, Duration::ZERO), Waited::TimedOut);
        });
    }

    #[test]
    fn a_newer_token_or_a_cancel_ends_a_wait_with_none() {
        within(|| {
            let r = Arc::new(Reports::<u8>::default());
            for end in ["begin", "cancel"] {
                let t = r.begin();
                let waiter = {
                    let r = r.clone();
                    thread::spawn(move || r.wait(t, Duration::from_secs(2)))
                };
                thread::sleep(Duration::from_millis(30));
                assert!(!waiter.is_finished());
                let ended = Instant::now();
                if end == "begin" {
                    r.begin();
                } else {
                    r.cancel();
                }
                assert_eq!(join(waiter), Waited::Ended, "{end}");
                assert!(
                    ended.elapsed() < Duration::from_secs(1),
                    "{end} did not wake the wait"
                );
                assert!(!r.deliver(t, 1));
            }
        });
    }

    #[test]
    fn a_wait_ends_at_the_report_or_at_its_timeout() {
        within(|| {
            let r = Arc::new(Reports::default());
            let t = r.begin();
            let started = Instant::now();
            assert_eq!(r.wait(t, Duration::from_millis(60)), Waited::TimedOut);
            assert!(started.elapsed() >= Duration::from_millis(60));
            let waiter = {
                let r = r.clone();
                thread::spawn(move || r.wait(t, Duration::from_secs(5)))
            };
            thread::sleep(Duration::from_millis(30));
            assert!(!waiter.is_finished());
            assert!(r.deliver(t, 9u32));
            assert_eq!(join(waiter), Waited::Reported(9));
        });
    }

    #[test]
    fn a_second_show_while_one_waits_is_busy() {
        within(|| {
            let r = Arc::new(Reports::<()>::default());
            let t = r.begin_alone().unwrap();
            let waiter = {
                let r = r.clone();
                thread::spawn(move || r.wait(t, Duration::from_secs(5)))
            };
            thread::sleep(Duration::from_millis(30));
            assert_eq!(r.begin_alone(), None);
            assert_eq!(r.begin_alone(), None);
            assert!(r.deliver(t, ()));
            assert_eq!(join(waiter), Waited::Reported(()));
            assert!(r.begin_alone().unwrap() > t);
        });
    }

    #[test]
    fn a_cancel_ends_a_show_wait_with_ended_and_frees_the_next_show() {
        within(|| {
            let r = Arc::new(Reports::<()>::default());
            let t = r.begin_alone().unwrap();
            let waiter = {
                let r = r.clone();
                thread::spawn(move || r.wait(t, Duration::from_secs(5)))
            };
            thread::sleep(Duration::from_millis(30));
            r.cancel();
            assert_eq!(join(waiter), Waited::Ended);
            assert!(r.begin_alone().is_some());
        });
    }

    #[test]
    fn a_show_wait_ends_at_its_bound_with_timed_out() {
        within(|| {
            let r = Arc::new(Reports::<()>::default());
            let t = r.begin_alone().unwrap();
            let started = Instant::now();
            let waiter = {
                let r = r.clone();
                thread::spawn(move || r.wait(t, Duration::from_millis(80)))
            };
            assert_eq!(join(waiter), Waited::TimedOut);
            assert!(started.elapsed() >= Duration::from_millis(80));
            assert!(r.begin_alone().is_some());
        });
    }

    #[test]
    fn a_sample_without_a_report_within_its_bound_gives_no_counts() {
        within(|| {
            let counts = Counts::default();
            let first = counts.read(
                |token| assert!(counts.deliver(token, (3, 9))),
                Duration::from_secs(5),
            );
            assert_eq!(first, Some((3, 9)));
            assert_eq!(counts.read(|_| {}, Duration::from_millis(30)), None);
        });
    }

    #[test]
    fn a_late_report_of_an_earlier_sample_is_not_counted() {
        within(|| {
            let counts = Counts::default();
            let mut asked = 0;
            assert_eq!(
                counts.read(|token| asked = token, Duration::from_millis(30)),
                None
            );
            assert!(!counts.deliver(asked + 1, (1, 1)));
            let second = counts.read(
                |token| {
                    assert!(!counts.deliver(asked, (5, 5)));
                    assert!(counts.deliver(token, (6, 7)));
                },
                Duration::from_secs(5),
            );
            assert_eq!(second, Some((6, 7)));
            assert_eq!(counts.read(|_| {}, Duration::ZERO), None);
        });
    }

    #[test]
    fn until_holds_at_once_later_or_not_within_its_bound() {
        within(|| {
            assert!(until(Duration::ZERO, || true));
            let mut calls = 0;
            assert!(until(Duration::from_secs(5), || {
                calls += 1;
                calls == 3
            }));
            assert_eq!(calls, 3);
            let started = Instant::now();
            assert!(!until(Duration::from_millis(60), || false));
            assert!(started.elapsed() >= Duration::from_millis(60));
            assert!(started.elapsed() < crate::testing::BOUND);
        });
    }
}
