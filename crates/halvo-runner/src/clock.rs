use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use halvo_schema::Instant;

const DAY_NANOS: i64 = 86_400 * 1_000_000_000;

/// The instant now, in nanoseconds since 1970-01-01T00:00:00Z.
pub(crate) fn now() -> Instant {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX));
    Instant(nanos)
}

/// The day of an instant, in days since 1970-01-01, in UTC.
pub(crate) fn day(at: Instant) -> u32 {
    u32::try_from(at.0.div_euclid(DAY_NANOS)).unwrap_or(0)
}

static DRAWN: AtomicU64 = AtomicU64::new(0);

/// 16 bytes for an id that the runner mints: two keyed hashes of a count, under keys that the standard library
/// takes from the operating system's generator.
pub(crate) fn random16() -> [u8; 16] {
    let n = DRAWN.fetch_add(1, Ordering::Relaxed);
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&RandomState::new().hash_one(n).to_le_bytes());
    out[8..].copy_from_slice(&RandomState::new().hash_one(n).to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_counts_from_the_epoch_in_utc() {
        assert_eq!(day(Instant(0)), 0);
        assert_eq!(day(Instant(DAY_NANOS - 1)), 0);
        assert_eq!(day(Instant(DAY_NANOS)), 1);
        assert_eq!(day(Instant(20_404 * DAY_NANOS + 5)), 20_404);
    }

    #[test]
    fn two_draws_differ() {
        let drawn: std::collections::HashSet<[u8; 16]> = (0..1000).map(|_| random16()).collect();
        assert_eq!(drawn.len(), 1000);
        assert_ne!(random16()[..8], random16()[8..]);
    }
}
