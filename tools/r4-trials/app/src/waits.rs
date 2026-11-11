use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// A report that the core asks the page for under a token: the first animation frame after a
/// show, or the page's counters. Only the report of the newest token is kept.
pub struct Reports<T> {
    state: Mutex<ReportState<T>>,
    arrived: Condvar,
}

struct ReportState<T> {
    token: u64,
    value: Option<T>,
}

impl<T> Default for Reports<T> {
    fn default() -> Self {
        Reports {
            state: Mutex::new(ReportState {
                token: 0,
                value: None,
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
        state.token
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

    /// Waits for the report of `token`, at most `timeout` where one is given.
    pub fn wait(&self, token: u64, timeout: Option<Duration>) -> Option<T> {
        let deadline = timeout.map(|t| std::time::Instant::now() + t);
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if state.token != token {
                return None;
            }
            if let Some(value) = state.value.clone() {
                return Some(value);
            }
            match deadline {
                None => {
                    state = self.arrived.wait(state).unwrap_or_else(|e| e.into_inner());
                }
                Some(deadline) => {
                    let left = deadline.saturating_duration_since(std::time::Instant::now());
                    if left.is_zero() {
                        return None;
                    }
                    state = self
                        .arrived
                        .wait_timeout(state, left)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    #[test]
    fn tokens_count_from_one() {
        let r = Reports::<()>::default();
        assert_eq!(r.begin(), 1);
        assert_eq!(r.begin(), 2);
    }

    #[test]
    fn a_report_of_the_newest_token_is_kept_and_read() {
        let r = Reports::default();
        let t = r.begin();
        assert!(r.deliver(t, (3u64, 4u64)));
        assert_eq!(r.wait(t, Some(Duration::ZERO)), Some((3, 4)));
    }

    #[test]
    fn a_report_of_an_older_or_unknown_token_is_dropped() {
        let r = Reports::default();
        let old = r.begin();
        let new = r.begin();
        assert!(!r.deliver(old, 1u8));
        assert!(!r.deliver(new + 1, 1u8));
        assert!(!r.deliver(0, 1u8));
        assert_eq!(r.wait(new, Some(Duration::from_millis(20))), None);
        assert!(r.deliver(new, 2u8));
        assert_eq!(r.wait(old, Some(Duration::ZERO)), None);
        assert_eq!(r.wait(new, Some(Duration::ZERO)), Some(2));
    }

    #[test]
    fn a_report_before_any_token_is_dropped() {
        let r = Reports::default();
        assert!(!r.deliver(1, ()));
        assert_eq!(r.begin(), 1);
        assert_eq!(r.wait(1, Some(Duration::ZERO)), None);
    }

    #[test]
    fn a_wait_ends_at_the_report_or_at_its_timeout() {
        let r = Arc::new(Reports::default());
        let t = r.begin();
        let started = Instant::now();
        assert_eq!(r.wait(t, Some(Duration::from_millis(60))), None);
        assert!(started.elapsed() >= Duration::from_millis(60));
        let waiter = {
            let r = r.clone();
            thread::spawn(move || r.wait(t, None))
        };
        thread::sleep(Duration::from_millis(30));
        assert!(!waiter.is_finished());
        assert!(r.deliver(t, 9u32));
        assert_eq!(waiter.join().unwrap(), Some(9));
    }
}
