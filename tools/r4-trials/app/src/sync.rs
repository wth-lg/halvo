use std::sync::{Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;

/// The length of a change's payload, in bytes.
pub const PAYLOAD_BYTES: usize = 256;

/// One change of the sync trial: its `seq` from 1, the count `of` the send, a payload of
/// `PAYLOAD_BYTES` that differs from one `seq` to the next, and `hidden`: whether the core had
/// handled a hide and not yet the next show when it sent the change. The page counts a change
/// with `hidden` in `after_hide`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    pub seq: u64,
    pub of: u64,
    pub payload: String,
    pub hidden: bool,
}

/// The reply to one waiting call: the latest change, or `closed` when the window was hidden
/// or destroyed since the call began or is hidden now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    Change(Change),
    Closed,
}

/// The core's side of the sync ways: the latest change for the way `get`, whether that way is
/// closed, and whether the core has handled a hide since the last show.
pub struct Hub {
    state: Mutex<HubState>,
    changed: Condvar,
}

struct HubState {
    latest: Option<Change>,
    closed: bool,
    epoch: u64,
    hidden: bool,
}

/// The change `seq` of a send of `of` changes.
pub fn change(seq: u64, of: u64) -> Change {
    let mut state = seq;
    let mut payload = String::with_capacity(PAYLOAD_BYTES);
    while payload.len() < PAYLOAD_BYTES {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        payload.push_str(&format!("{z:016x}"));
    }
    Change {
        seq,
        of,
        payload,
        hidden: false,
    }
}

/// How many of `count` changes at `per_second` are due after `elapsed`.
pub fn due(count: u64, per_second: u64, elapsed: Duration) -> u64 {
    let due = elapsed.as_micros().saturating_mul(u128::from(per_second)) / 1_000_000;
    u64::try_from(due).unwrap_or(u64::MAX).min(count)
}

impl Next {
    /// The reply as the page reads it: the change, or `{"closed":"yes"}`.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            Next::Change(change) => serde_json::to_value(change).unwrap_or(serde_json::Value::Null),
            Next::Closed => serde_json::json!({"closed": "yes"}),
        }
    }
}

impl Default for Hub {
    fn default() -> Self {
        Hub {
            state: Mutex::new(HubState {
                latest: None,
                closed: false,
                epoch: 0,
                hidden: false,
            }),
            changed: Condvar::new(),
        }
    }
}

impl Hub {
    /// Makes `change` the latest and wakes every waiting call.
    pub fn publish(&self, change: Change) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.latest = Some(change);
        self.changed.notify_all();
    }

    /// Marks the window closed and answers every waiting call with `Next::Closed`.
    pub fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = true;
        state.epoch += 1;
        self.changed.notify_all();
    }

    /// Marks the window open again, and shown: a change sent from now on is not `hidden`.
    pub fn open(&self) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.closed = false;
        state.hidden = false;
    }

    /// Marks that the core handled a hide: a change sent from now until `open` is `hidden`.
    pub fn hide(&self) {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).hidden = true;
    }

    /// `change` with the mark of this moment, for a way that sends it now.
    pub fn stamp(&self, change: Change) -> Change {
        let hidden = self.state.lock().unwrap_or_else(|e| e.into_inner()).hidden;
        Change { hidden, ..change }
    }

    /// Waits until the latest change has a `seq` above `seq`, or the window closes; the change
    /// carries the mark of the moment of the reply.
    pub fn wait_next(&self, seq: u64) -> Next {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let epoch = state.epoch;
        loop {
            if state.closed || state.epoch != epoch {
                return Next::Closed;
            }
            if let Some(latest) = state.latest.as_ref().filter(|c| c.seq > seq) {
                return Next::Change(Change {
                    hidden: state.hidden,
                    ..latest.clone()
                });
            }
            state = self.changed.wait(state).unwrap_or_else(|e| e.into_inner());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{join, within};
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn a_change_has_its_seq_and_a_payload_of_256_bytes_that_changes() {
        within(|| {
            let a = change(1, 200_000);
            let b = change(2, 200_000);
            assert_eq!((a.seq, a.of), (1, 200_000));
            assert_eq!(a.payload.len(), PAYLOAD_BYTES);
            assert_eq!(b.payload.len(), PAYLOAD_BYTES);
            assert_ne!(a.payload, b.payload);
            assert_eq!(change(1, 200_000), a);
            assert!(a.payload.bytes().all(|c| c.is_ascii_hexdigit()));
            assert!(!a.hidden);
            let json = serde_json::to_value(&a).unwrap();
            assert_eq!(json["seq"], 1);
            assert_eq!(json["of"], 200_000);
            assert_eq!(json["hidden"], false);
        });
    }

    #[test]
    fn due_follows_the_rate_and_stops_at_the_count() {
        within(|| {
            assert_eq!(due(200_000, 1000, Duration::ZERO), 0);
            assert_eq!(due(200_000, 1000, Duration::from_micros(999)), 0);
            assert_eq!(due(200_000, 1000, Duration::from_millis(1)), 1);
            assert_eq!(due(200_000, 1000, Duration::from_millis(1500)), 1500);
            assert_eq!(due(200_000, 1000, Duration::from_secs(10_000)), 200_000);
            assert_eq!(due(10, u64::MAX, Duration::from_secs(1)), 10);
        });
    }

    #[test]
    fn the_replies_read_as_the_page_reads_them() {
        within(|| {
            assert_eq!(Next::Closed.to_json(), serde_json::json!({"closed": "yes"}));
            let c = change(3, 9);
            assert_eq!(
                Next::Change(c.clone()).to_json(),
                serde_json::to_value(c).unwrap()
            );
        });
    }

    #[test]
    fn a_call_behind_the_latest_answers_at_once() {
        within(|| {
            let hub = Hub::default();
            hub.publish(change(5, 10));
            assert_eq!(hub.wait_next(4), Next::Change(change(5, 10)));
            assert_eq!(hub.wait_next(0), Next::Change(change(5, 10)));
        });
    }

    #[test]
    fn a_waiting_call_gets_the_next_change() {
        within(|| {
            let hub = Arc::new(Hub::default());
            hub.publish(change(1, 10));
            let waiter = {
                let hub = hub.clone();
                thread::spawn(move || hub.wait_next(1))
            };
            thread::sleep(Duration::from_millis(50));
            assert!(!waiter.is_finished());
            hub.publish(change(2, 10));
            assert_eq!(join(waiter), Next::Change(change(2, 10)));
        });
    }

    #[test]
    fn closing_answers_a_waiting_call_and_every_call_while_closed() {
        within(|| {
            let hub = Arc::new(Hub::default());
            let waiter = {
                let hub = hub.clone();
                thread::spawn(move || hub.wait_next(0))
            };
            thread::sleep(Duration::from_millis(50));
            hub.close();
            assert_eq!(join(waiter), Next::Closed);
            hub.publish(change(1, 10));
            assert_eq!(hub.wait_next(0), Next::Closed);
            hub.open();
            assert_eq!(hub.wait_next(0), Next::Change(change(1, 10)));
        });
    }

    #[test]
    fn a_change_sent_after_the_hide_is_marked_hidden_until_the_show() {
        within(|| {
            let hub = Hub::default();
            assert!(!hub.stamp(change(1, 3)).hidden);
            hub.hide();
            assert!(hub.stamp(change(2, 3)).hidden);
            hub.open();
            assert!(!hub.stamp(change(3, 3)).hidden);
        });
    }

    #[test]
    fn a_reply_carries_the_mark_of_the_moment_it_is_sent() {
        within(|| {
            let hub = Arc::new(Hub::default());
            hub.publish(change(1, 10));
            assert_eq!(hub.wait_next(0), Next::Change(change(1, 10)));
            let waiter = {
                let hub = hub.clone();
                thread::spawn(move || hub.wait_next(1))
            };
            thread::sleep(Duration::from_millis(50));
            hub.hide();
            hub.publish(change(2, 10));
            let reply = join(waiter);
            assert_eq!(
                reply,
                Next::Change(Change {
                    hidden: true,
                    ..change(2, 10)
                })
            );
        });
    }

    #[test]
    fn a_call_that_began_before_a_close_and_open_is_closed() {
        within(|| {
            let hub = Arc::new(Hub::default());
            let waiter = {
                let hub = hub.clone();
                thread::spawn(move || hub.wait_next(0))
            };
            thread::sleep(Duration::from_millis(50));
            {
                hub.close();
                hub.open();
            }
            assert_eq!(join(waiter), Next::Closed);
        });
    }
}
