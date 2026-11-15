use crate::Instant;

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const SECONDS_PER_DAY: i64 = 86_400;

/// The largest offset from UTC, in whole seconds either way, that a runner takes (`--utc-offset`, contracts.md
/// A6.1): 14 hours.
pub const UTC_OFFSET_S_MAX: i32 = 50_400;

/// The day of `at` on a clock `utc_offset_s` seconds east of UTC, in days since 1970-01-01 of that clock: the whole
/// seconds of `at`, rounded down, plus the offset, divided by a day and rounded down; a day before 1970-01-01 is 0.
pub fn day_of(at: Instant, utc_offset_s: i32) -> u32 {
    let local = at.0.div_euclid(NANOS_PER_SECOND) + i64::from(utc_offset_s);
    u32::try_from(local.div_euclid(SECONDS_PER_DAY)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = SECONDS_PER_DAY * NANOS_PER_SECOND;
    const HOUR: i64 = 3_600 * NANOS_PER_SECOND;
    const MAX: i32 = UTC_OFFSET_S_MAX;

    #[test]
    fn the_day_moves_with_the_offset_at_the_edges_of_its_range() {
        let noon = Instant(20_404 * DAY + 12 * HOUR);
        assert_eq!(day_of(noon, 0), 20_404);
        assert_eq!(day_of(noon, MAX), 20_405);
        assert_eq!(day_of(noon, -MAX), 20_403);
        assert_eq!(day_of(noon, 11 * 3_600 + 3_599), 20_404);
        assert_eq!(day_of(noon, 12 * 3_600), 20_405);
        assert_eq!(day_of(noon, -12 * 3_600), 20_404);
        assert_eq!(day_of(noon, -12 * 3_600 - 1), 20_403);
    }

    #[test]
    fn a_day_runs_from_its_first_nanosecond_to_its_last_on_the_offset_clock() {
        let d = 20_404;
        for offset in [-MAX, -3_600, 0, 3_600, MAX] {
            let first = Instant(d * DAY - i64::from(offset) * NANOS_PER_SECOND);
            assert_eq!(day_of(first, offset), 20_404, "{offset}: first");
            assert_eq!(
                day_of(Instant(first.0 + DAY - 1), offset),
                20_404,
                "{offset}: last"
            );
            assert_eq!(
                day_of(Instant(first.0 - 1), offset),
                20_403,
                "{offset}: before"
            );
            assert_eq!(
                day_of(Instant(first.0 + DAY), offset),
                20_405,
                "{offset}: after"
            );
        }
    }

    #[test]
    fn an_instant_before_the_epoch_of_its_clock_is_day_0() {
        assert_eq!(day_of(Instant(-1), 0), 0);
        assert_eq!(day_of(Instant(-1), -MAX), 0);
        assert_eq!(day_of(Instant(-DAY), MAX), 0);
        assert_eq!(day_of(Instant(i64::MIN), -MAX), 0);
        assert_eq!(day_of(Instant(i64::MIN), MAX), 0);
        assert_eq!(day_of(Instant(-HOUR), MAX), 0);
        assert_eq!(day_of(Instant(DAY - HOUR), MAX), 1);
    }

    #[test]
    fn the_largest_instant_has_a_day_without_overflow() {
        assert_eq!(day_of(Instant(i64::MAX), 0), 106_751);
        assert_eq!(day_of(Instant(i64::MAX), MAX), 106_752);
        assert_eq!(day_of(Instant(i64::MAX), -MAX), 106_751);
        assert_eq!(day_of(Instant(i64::MAX), i32::MAX), 131_607);
        assert_eq!(day_of(Instant(i64::MIN), i32::MIN), 0);
    }

    #[test]
    fn at_offset_0_the_day_is_the_utc_day_of_the_instant() {
        for at in [
            0,
            1,
            DAY - 1,
            DAY,
            20_404 * DAY + 5,
            20_404 * DAY - 1,
            i64::MAX,
        ] {
            assert_eq!(
                i64::from(day_of(Instant(at), 0)),
                at.div_euclid(DAY),
                "{at}"
            );
        }
    }
}
