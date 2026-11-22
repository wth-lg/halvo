//! The deadline of the request `defer` (contracts.md C24.5 of the workflows run): while it holds, the pass of the
//! slot's driver turns a yes into a no, so the worker slot ends a living worker by its kill and starts none.
//!
//! - THE MARK. One reading of `Instant`, the end of the hold; none where no deadline is in force. The newest request
//!   replaces it, also with a nearer deadline.
//! - THE SET. At a request the mark reads the wall clock once: a deadline at or before that reading clears the mark
//!   and answers none; a later one holds for the time between the two, at most [`DEFER_CAP`], from the `Instant` of
//!   the request, and the answer is the wall reading plus that length, in nanoseconds since 1970.
//! - THE LAPSE. A pass compares `Instant::now()` with the mark, not the loop's time of the driver: a deadline that
//!   nobody renews lapses by itself, and a step of the wall clock after the request changes nothing.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::daemon::r2::worker_slot::MayRun;

/// The longest hold of one request: 300 s.
pub(super) const DEFER_CAP: Duration = Duration::from_secs(300);

/// The deadline in force: the `Instant` at which the hold ends, none where none holds.
pub(super) struct Deferral(Mutex<Option<Instant>>);

impl Deferral {
    pub(super) const fn new() -> Deferral {
        Deferral(Mutex::new(None))
    }

    fn end(&self) -> MutexGuard<'_, Option<Instant>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The set of a request with the deadline `until`, in nanoseconds since 1970, at the clocks' readings of this
    /// call: the deadline in force that the reply holds.
    pub(super) fn set(&self, until: i64) -> Option<i64> {
        self.set_at(until, SystemTime::now(), Instant::now())
    }

    /// The set of a request with the deadline `until` at the wall reading `wall` and the reading `at` of `Instant`:
    /// none, with the mark cleared, for a deadline at or before `wall`; else the mark holds for the time up to
    /// `until`, at most [`DEFER_CAP`], from `at`, and the answer is `wall` plus that length.
    fn set_at(&self, until: i64, wall: SystemTime, at: Instant) -> Option<i64> {
        let wall = nanos_since_1970(wall);
        let mut end = self.end();
        let ahead = i128::from(until) - i128::from(wall);
        if ahead <= 0 {
            *end = None;
            return None;
        }
        let length = u64::try_from(ahead)
            .map_or(DEFER_CAP, Duration::from_nanos)
            .min(DEFER_CAP);
        *end = Some(at + length);
        let length = i64::try_from(length.as_nanos()).expect("300 s in nanoseconds fit 64 bits");
        // The length is at most `until - wall`, so the sum is at most `until`.
        Some(wall + length)
    }

    /// The caller's answer `may_run` under the mark at the reading `at` of `Instant`: a no while the mark holds,
    /// which is strictly before its end, and `may_run` itself otherwise.
    pub(super) fn may_run(&self, may_run: MayRun, at: Instant) -> MayRun {
        if self.end().is_some_and(|end| at < end) {
            MayRun::No
        } else {
            may_run
        }
    }
}

/// The wall reading in nanoseconds since 1970, held to the range of 64 bits.
fn nanos_since_1970(wall: SystemTime) -> i64 {
    match wall.duration_since(UNIX_EPOCH) {
        Ok(after) => i64::try_from(after.as_nanos()).unwrap_or(i64::MAX),
        Err(before) => i64::try_from(before.duration().as_nanos()).map_or(i64::MIN, |nanos| -nanos),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use super::{Deferral, DEFER_CAP};
    use crate::daemon::r2::worker_slot::MayRun;

    /// The wall reading of the tests, 2027-01-15, and the same in nanoseconds since 1970.
    const WALL_SECONDS: u64 = 1_800_000_000;
    const WALL: i64 = 1_800_000_000_000_000_000;

    const SECOND: i64 = 1_000_000_000;

    const YES: MayRun = MayRun::Yes { watermark: None };

    fn wall() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(WALL_SECONDS)
    }

    fn after(at: Instant, seconds: u64) -> Instant {
        at + Duration::from_secs(seconds)
    }

    /// Ruling 4: 3,600 s ahead answers now plus 300 s and holds 300 s; a newer request 60 s ahead answers itself
    /// and replaces the hold, which then ends at 60 s; a deadline at now or before answers none and clears the mark.
    #[test]
    fn capped_replaced_cleared() {
        let at = Instant::now();
        let deferral = Deferral::new();
        assert_eq!(
            deferral.set_at(WALL + 3_600 * SECOND, wall(), at),
            Some(WALL + 300 * SECOND)
        );
        assert_eq!(DEFER_CAP, Duration::from_secs(300));
        assert_eq!(deferral.may_run(YES, after(at, 299)), MayRun::No);
        assert_eq!(deferral.may_run(YES, after(at, 300)), YES);

        assert_eq!(
            deferral.set_at(WALL + 60 * SECOND, wall(), at),
            Some(WALL + 60 * SECOND)
        );
        assert_eq!(deferral.may_run(YES, after(at, 59)), MayRun::No);
        assert_eq!(deferral.may_run(YES, after(at, 61)), YES, "the 300 s stand");

        for past in [WALL, WALL - 1, i64::MIN] {
            deferral.set_at(WALL + 60 * SECOND, wall(), at);
            assert_eq!(deferral.set_at(past, wall(), at), None, "{past}");
            assert_eq!(deferral.may_run(YES, at), YES, "{past}: the mark stands");
        }
    }

    /// Ruling 5: after a set of 60 s a yes becomes a no at 59 s and stays a yes at 60 s.
    #[test]
    fn holds_until_it_lapses() {
        let at = Instant::now();
        let deferral = Deferral::new();
        assert_eq!(deferral.may_run(YES, at), YES);
        deferral.set_at(WALL + 60 * SECOND, wall(), at);
        assert_eq!(deferral.may_run(YES, after(at, 59)), MayRun::No);
        assert_eq!(deferral.may_run(YES, after(at, 60)), YES);
    }
}
