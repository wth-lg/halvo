//! The hold of the macOS keyboard source: every key event that the source
//! would record waits here until it is as old as the bound and every reading of
//! the secure-input state since its arrival was off and in time. Every doubt
//! drops every held key. Nothing here reads a clock or the system: the caller
//! hands in each reading with its time.

use std::collections::VecDeque;
use std::time::Duration;

use super::Reading;

/// How long a key is held before it may be recorded. A first value, the
/// owner's to change.
pub const HOLD_BOUND: Duration = Duration::from_millis(30_000);

/// The step of the timer that reads the secure-input state while keys are
/// held.
pub const READING_STEP: Duration = Duration::from_millis(10);

/// The widest gap between two readings that keeps the held keys: from the time
/// taken before the earlier read to the time taken after the later one.
pub const GAP_BOUND: Duration = Duration::from_millis(30);

/// The widest lag of a key at the callback, after its OS time, that keeps the
/// held keys. No change of the gap bound moves it.
pub const LAG_BOUND: Duration = Duration::from_millis(30);

/// The step at which the session is read again while keys are held.
pub const SESSION_STEP: Duration = Duration::from_millis(250);

/// The most keys that are held at once; one more drops them all.
pub const QUEUE_CAP: usize = 4_096;

/// A lag above which a key is a late key of the contract (C13.8).
pub const LATE_BOUND: Duration = Duration::from_millis(250);

/// A time in nanoseconds on CLOCK_MONOTONIC_RAW, which goes on while the Mac
/// sleeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Monotonic(pub u64);

/// A time in nanoseconds on CLOCK_UPTIME_RAW, the clock of an event's OS time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Uptime(pub u64);

/// The bounds that one hold keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub hold: Duration,
    pub gap: Duration,
    pub lag: Duration,
    pub cap: usize,
}

impl Bounds {
    /// The product's bounds.
    pub const PRODUCT: Self = Self {
        hold: HOLD_BOUND,
        gap: GAP_BOUND,
        lag: LAG_BOUND,
        cap: QUEUE_CAP,
    };
}

/// Why the held keys were dropped, or why an arriving key was not held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    On,
    Unknown,
    Gap,
    LateKey,
    UntimedKey,
    TapSwitchedOff,
    OwnTake,
    Full,
    Stop,
    Refused,
}

impl Cause {
    /// Whether a key dropped for this cause sets `keys_withheld`.
    pub fn withholds(self) -> bool {
        !matches!(self, Self::Stop | Self::Refused)
    }

    /// Whether a drop for this cause reads the source as impaired until a key
    /// is next released.
    pub fn timing_fault(self) -> bool {
        matches!(
            self,
            Self::Gap | Self::LateKey | Self::UntimedKey | Self::TapSwitchedOff | Self::Full
        )
    }
}

/// The two times of one reading: taken before the read of the state and
/// after it returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamps {
    pub before: Monotonic,
    pub after: Monotonic,
}

/// What one reading decides for the oldest held key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Release,
    Drop(Cause),
    Wait,
}

/// The deciding function: the arrival of the oldest held key, the time taken
/// before the read of the reading before this one, this reading with its two
/// times and the bounds in; release the oldest key, drop every held key, or
/// wait.
pub fn decide(
    oldest: Option<Monotonic>,
    previous: Option<Monotonic>,
    reading: Reading,
    at: Stamps,
    bounds: &Bounds,
) -> Verdict {
    let Some(oldest) = oldest else {
        return Verdict::Wait;
    };
    let in_time = previous.is_some_and(|previous| {
        at.after >= previous && Duration::from_nanos(at.after.0 - previous.0) <= bounds.gap
    });
    if !in_time {
        return Verdict::Drop(Cause::Gap);
    }
    match reading {
        Reading::On => Verdict::Drop(Cause::On),
        Reading::Unknown => Verdict::Drop(Cause::Unknown),
        Reading::Off
            if Duration::from_nanos(at.before.0.saturating_sub(oldest.0)) >= bounds.hold =>
        {
            Verdict::Release
        }
        Reading::Off => Verdict::Wait,
    }
}

/// How a key's OS time stands against its arrival at the callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timing {
    InTime { lag: Duration },
    Late { lag: Duration },
    Untimed,
}

/// The timing of a key whose OS time is `os_time` and which reached the
/// callback at `arrival`, both on CLOCK_UPTIME_RAW. An OS time of 0, after the
/// arrival or that could not be read is untimed.
pub fn timing(arrival: Uptime, os_time: Option<Uptime>, gap: Duration) -> Timing {
    match os_time {
        Some(os_time) if os_time.0 != 0 && os_time <= arrival => {
            let lag = Duration::from_nanos(arrival.0 - os_time.0);
            if lag > gap {
                Timing::Late { lag }
            } else {
                Timing::InTime { lag }
            }
        }
        _ => Timing::Untimed,
    }
}

/// What a reading came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Step<T> {
    Released(T),
    Dropped { keys: usize, cause: Cause },
    Waiting,
}

/// What became of an arriving key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Held,
    Suppressed,
    Withheld { cause: Cause, dropped: usize },
}

struct Held<T> {
    arrival: Monotonic,
    key: T,
}

/// The bytes of one slot of the queue for a held value of type `T`, its heap
/// aside.
pub fn slot_bytes<T>() -> usize {
    std::mem::size_of::<Held<T>>()
}

/// The held keys in the order of their arrival, with the time of the last
/// reading.
pub struct HeldKeys<T> {
    keys: VecDeque<Held<T>>,
    previous: Option<Monotonic>,
    bounds: Bounds,
}

impl<T> HeldKeys<T> {
    pub fn new(bounds: Bounds) -> Self {
        Self {
            keys: VecDeque::new(),
            previous: None,
            bounds,
        }
    }

    pub fn bounds(&self) -> Bounds {
        self.bounds
    }

    /// The time taken before the read of the last reading.
    pub fn previous(&self) -> Option<Monotonic> {
        self.previous
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Takes one reading: drops every held key on a gap, on or unknown, and
    /// otherwise releases the oldest key when it is due.
    pub fn reading(&mut self, reading: Reading, at: Stamps) -> Step<T> {
        let oldest = self.keys.front().map(|held| held.arrival);
        let verdict = decide(oldest, self.previous, reading, at, &self.bounds);
        self.previous = Some(at.before);
        match verdict {
            Verdict::Release => match self.keys.pop_front() {
                Some(held) => Step::Released(held.key),
                None => Step::Waiting,
            },
            Verdict::Drop(cause) => Step::Dropped {
                keys: self.drop_all(),
                cause,
            },
            Verdict::Wait => Step::Waiting,
        }
    }

    /// Takes the reading of a key event that came late or untimed: the
    /// callback ran late, so every held key is dropped and none is released.
    pub fn late_reading(&mut self, at: Stamps, cause: Cause) -> Step<T> {
        self.previous = Some(at.before);
        Step::Dropped {
            keys: self.drop_all(),
            cause,
        }
    }

    /// Admits a key that arrived at `arrival` under `reading`, the reading
    /// taken for its own event, with its timing.
    pub fn admit(
        &mut self,
        reading: Reading,
        timing: Timing,
        arrival: Monotonic,
        key: T,
    ) -> Admission {
        let refused = match (timing, reading) {
            (Timing::Untimed, _) => Cause::UntimedKey,
            (Timing::Late { .. }, _) => Cause::LateKey,
            (Timing::InTime { .. }, Reading::On) => {
                return Admission::Suppressed;
            }
            (Timing::InTime { .. }, Reading::Unknown) => Cause::Unknown,
            (Timing::InTime { .. }, Reading::Off) if self.keys.len() >= self.bounds.cap => {
                Cause::Full
            }
            (Timing::InTime { .. }, Reading::Off) => {
                self.keys.push_back(Held { arrival, key });
                return Admission::Held;
            }
        };
        Admission::Withheld {
            cause: refused,
            dropped: self.drop_all(),
        }
    }

    /// Drops every held key and answers how many there were.
    pub fn drop_all(&mut self) -> usize {
        let dropped = self.keys.len();
        self.keys.clear();
        dropped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;
    const IN_TIME: Timing = Timing::InTime {
        lag: Duration::ZERO,
    };

    fn at(ms: u64) -> Monotonic {
        Monotonic(ms * MS)
    }

    fn bounds() -> Bounds {
        Bounds {
            hold: Duration::from_millis(1_000),
            gap: Duration::from_millis(30),
            lag: Duration::from_millis(30),
            cap: 4,
        }
    }

    fn both(at: Monotonic) -> Stamps {
        Stamps {
            before: at,
            after: at,
        }
    }

    fn read(before_ms: u64, after_ms: u64) -> Stamps {
        Stamps {
            before: at(before_ms),
            after: at(after_ms),
        }
    }

    fn held(keys: &[u64]) -> HeldKeys<u64> {
        let mut hold = HeldKeys::new(bounds());
        for &key in keys {
            assert_eq!(hold.reading(Reading::Off, both(at(key))), Step::Waiting);
            assert_eq!(
                hold.admit(Reading::Off, IN_TIME, at(key), key),
                Admission::Held
            );
        }
        hold
    }

    #[test]
    fn the_gap_is_counted_from_before_the_earlier_read_to_after_the_later_read() {
        let mut hold = held(&[0]);
        assert_eq!(hold.reading(Reading::Off, read(1, 20)), Step::Waiting);
        assert_eq!(
            hold.reading(Reading::Off, read(25, 32)),
            Step::Dropped {
                keys: 1,
                cause: Cause::Gap
            },
            "the later read ended 31 ms after the earlier read began"
        );
        let mut hold = held(&[0]);
        assert_eq!(hold.reading(Reading::Off, read(2, 20)), Step::Waiting);
        assert_eq!(hold.reading(Reading::Off, read(25, 32)), Step::Waiting);
    }

    #[test]
    fn a_key_is_as_old_as_the_bound_by_the_time_before_its_releasing_read() {
        assert_eq!(
            decide(
                Some(at(0)),
                Some(at(990)),
                Reading::Off,
                read(999, 1_001),
                &bounds()
            ),
            Verdict::Wait
        );
        assert_eq!(
            decide(
                Some(at(0)),
                Some(at(990)),
                Reading::Off,
                read(1_000, 1_001),
                &bounds()
            ),
            Verdict::Release
        );
    }

    #[test]
    fn a_late_reading_drops_a_key_that_is_due_and_releases_none() {
        let mut hold = held(&[0, 1]);
        assert_eq!(
            hold.late_reading(both(at(1_000)), Cause::LateKey),
            Step::Dropped {
                keys: 2,
                cause: Cause::LateKey
            }
        );
        assert!(hold.is_empty());
        assert_eq!(hold.reading(Reading::Off, both(at(1_010))), Step::Waiting);
    }

    #[test]
    fn the_bounds_are_their_first_values() {
        assert_eq!(HOLD_BOUND, Duration::from_millis(30_000));
        assert_eq!(READING_STEP, Duration::from_millis(10));
        assert_eq!(GAP_BOUND, Duration::from_millis(30));
        assert_eq!(SESSION_STEP, Duration::from_millis(250));
        assert_eq!(QUEUE_CAP, 4_096);
        assert_eq!(LATE_BOUND, Duration::from_millis(250));
        assert_eq!(LAG_BOUND, Duration::from_millis(30));
        assert_eq!(
            Bounds::PRODUCT,
            Bounds {
                hold: Duration::from_millis(30_000),
                gap: Duration::from_millis(30),
                lag: Duration::from_millis(30),
                cap: 4_096
            }
        );
    }

    #[test]
    fn a_key_is_released_when_it_is_as_old_as_the_bound_and_not_before() {
        let previous = Some(at(985));
        assert_eq!(
            decide(
                Some(at(0)),
                previous,
                Reading::Off,
                both(at(999)),
                &bounds()
            ),
            Verdict::Wait
        );
        assert_eq!(
            decide(
                Some(at(0)),
                previous,
                Reading::Off,
                both(at(1_000)),
                &bounds()
            ),
            Verdict::Release
        );
    }

    #[test]
    fn a_reading_of_on_drops_the_held_keys() {
        assert_eq!(
            decide(
                Some(at(0)),
                Some(at(5)),
                Reading::On,
                both(at(10)),
                &bounds()
            ),
            Verdict::Drop(Cause::On)
        );
    }

    #[test]
    fn a_reading_of_unknown_drops_the_held_keys() {
        assert_eq!(
            decide(
                Some(at(0)),
                Some(at(5)),
                Reading::Unknown,
                both(at(10)),
                &bounds()
            ),
            Verdict::Drop(Cause::Unknown)
        );
    }

    #[test]
    fn a_reading_of_on_or_unknown_drops_even_a_key_that_is_due() {
        for reading in [Reading::On, Reading::Unknown] {
            assert!(matches!(
                decide(
                    Some(at(0)),
                    Some(at(1_990)),
                    reading,
                    both(at(2_000)),
                    &bounds()
                ),
                Verdict::Drop(_)
            ));
        }
    }

    #[test]
    fn a_gap_above_the_bound_drops_the_held_keys_before_a_due_key_is_released() {
        assert_eq!(
            decide(
                Some(at(0)),
                Some(at(1_000)),
                Reading::Off,
                both(at(1_031)),
                &bounds()
            ),
            Verdict::Drop(Cause::Gap)
        );
        assert_eq!(
            decide(
                Some(at(0)),
                Some(at(1_000)),
                Reading::Off,
                both(at(1_030)),
                &bounds()
            ),
            Verdict::Release
        );
    }

    #[test]
    fn a_reading_before_the_one_before_it_or_with_none_before_it_is_a_gap() {
        assert_eq!(
            decide(
                Some(at(0)),
                Some(at(20)),
                Reading::Off,
                both(at(10)),
                &bounds()
            ),
            Verdict::Drop(Cause::Gap)
        );
        assert_eq!(
            decide(Some(at(0)), None, Reading::Off, both(at(10)), &bounds()),
            Verdict::Drop(Cause::Gap)
        );
    }

    #[test]
    fn with_nothing_held_every_reading_waits() {
        for reading in [Reading::Off, Reading::On, Reading::Unknown] {
            assert_eq!(
                decide(None, None, reading, both(at(5_000)), &bounds()),
                Verdict::Wait
            );
        }
    }

    #[test]
    fn a_gap_before_a_key_arrives_onto_an_empty_hold_keeps_that_key() {
        let mut hold = HeldKeys::new(bounds());
        assert_eq!(hold.reading(Reading::Off, both(at(0))), Step::Waiting);
        assert_eq!(hold.reading(Reading::Off, both(at(60_000))), Step::Waiting);
        assert_eq!(
            hold.admit(Reading::Off, IN_TIME, at(60_000), 1),
            Admission::Held
        );
        assert_eq!(hold.len(), 1);
    }

    #[test]
    fn a_key_in_time_is_in_time_up_to_the_gap_bound() {
        let gap = Duration::from_millis(30);
        assert_eq!(
            timing(Uptime(100 * MS), Some(Uptime(70 * MS)), gap),
            Timing::InTime {
                lag: Duration::from_millis(30)
            }
        );
        assert_eq!(
            timing(Uptime(100 * MS), Some(Uptime(100 * MS)), gap),
            Timing::InTime {
                lag: Duration::ZERO
            }
        );
        assert_eq!(
            timing(Uptime(100 * MS), Some(Uptime(70 * MS - 1)), gap),
            Timing::Late {
                lag: Duration::from_nanos(30 * MS + 1)
            }
        );
    }

    #[test]
    fn a_key_with_an_os_time_of_0_after_its_arrival_or_unread_is_untimed() {
        let gap = Duration::from_millis(30);
        assert_eq!(
            timing(Uptime(100 * MS), Some(Uptime(0)), gap),
            Timing::Untimed
        );
        assert_eq!(
            timing(Uptime(100 * MS), Some(Uptime(100 * MS + 1)), gap),
            Timing::Untimed
        );
        assert_eq!(timing(Uptime(100 * MS), None, gap), Timing::Untimed);
    }

    #[test]
    fn a_late_key_is_dropped_and_drops_every_held_key() {
        let mut hold = held(&[0, 1, 2]);
        let late = Timing::Late {
            lag: Duration::from_millis(31),
        };
        assert_eq!(
            hold.admit(Reading::Off, late, at(3), 3),
            Admission::Withheld {
                cause: Cause::LateKey,
                dropped: 3
            }
        );
        assert!(hold.is_empty());
    }

    #[test]
    fn an_untimed_key_is_dropped_and_drops_every_held_key() {
        let mut hold = held(&[0, 1]);
        assert_eq!(
            hold.admit(Reading::Off, Timing::Untimed, at(2), 2),
            Admission::Withheld {
                cause: Cause::UntimedKey,
                dropped: 2
            }
        );
        assert!(hold.is_empty());
    }

    #[test]
    fn a_key_that_arrives_under_on_is_suppressed_and_not_held() {
        let mut hold = held(&[]);
        assert_eq!(
            hold.admit(Reading::On, IN_TIME, at(0), 0),
            Admission::Suppressed
        );
        assert!(hold.is_empty());
    }

    #[test]
    fn a_key_that_arrives_under_unknown_is_withheld_and_not_held() {
        let mut hold = held(&[]);
        assert_eq!(
            hold.admit(Reading::Unknown, IN_TIME, at(0), 0),
            Admission::Withheld {
                cause: Cause::Unknown,
                dropped: 0
            }
        );
        assert!(hold.is_empty());
    }

    #[test]
    fn a_key_beyond_the_cap_drops_every_held_key_and_itself() {
        let mut hold = held(&[0, 1, 2, 3]);
        assert_eq!(hold.len(), 4);
        assert_eq!(
            hold.admit(Reading::Off, IN_TIME, at(4), 4),
            Admission::Withheld {
                cause: Cause::Full,
                dropped: 4
            }
        );
        assert!(hold.is_empty());
    }

    #[test]
    fn a_drop_takes_the_oldest_and_the_newest_key_alike() {
        let mut hold = held(&[0, 10, 20]);
        assert_eq!(
            hold.reading(Reading::On, both(at(25))),
            Step::Dropped {
                keys: 3,
                cause: Cause::On
            }
        );
        assert!(hold.is_empty());
        let mut hold = held(&[0, 10, 20]);
        for step in 1..=200 {
            let _ = hold.reading(Reading::Off, both(at(20 + step * 10)));
        }
        assert!(hold.is_empty(), "every key is released or dropped");
    }

    #[test]
    fn one_reading_releases_one_key_and_the_keys_go_in_their_order() {
        let mut hold = held(&[0, 1, 2]);
        for ms in (10..1_000).step_by(10) {
            assert_eq!(
                hold.reading(Reading::Off, both(at(ms))),
                Step::Waiting,
                "{ms}"
            );
        }
        assert_eq!(
            hold.reading(Reading::Off, both(at(1_002))),
            Step::Released(0)
        );
        assert_eq!(
            hold.reading(Reading::Off, both(at(1_003))),
            Step::Released(1)
        );
        assert_eq!(
            hold.reading(Reading::Off, both(at(1_004))),
            Step::Released(2)
        );
        assert_eq!(hold.reading(Reading::Off, both(at(1_005))), Step::Waiting);
    }

    #[test]
    fn a_released_key_had_every_reading_off_and_in_time_from_its_arrival() {
        let mut hold = held(&[0]);
        let mut ms = 0;
        while ms < 990 {
            ms += 10;
            assert_eq!(hold.reading(Reading::Off, both(at(ms))), Step::Waiting);
        }
        assert_eq!(
            hold.reading(Reading::Unknown, both(at(1_000))),
            Step::Dropped {
                keys: 1,
                cause: Cause::Unknown
            }
        );
        assert_eq!(hold.reading(Reading::Off, both(at(1_010))), Step::Waiting);
    }

    #[test]
    fn the_causes_that_set_keys_withheld_and_the_timing_faults() {
        use Cause::*;
        let all = [
            On,
            Unknown,
            Gap,
            LateKey,
            UntimedKey,
            TapSwitchedOff,
            OwnTake,
            Full,
            Stop,
            Refused,
        ];
        let withholding: Vec<Cause> = all.into_iter().filter(|c| c.withholds()).collect();
        assert_eq!(
            withholding,
            [
                On,
                Unknown,
                Gap,
                LateKey,
                UntimedKey,
                TapSwitchedOff,
                OwnTake,
                Full
            ]
        );
        let timing: Vec<Cause> = all.into_iter().filter(|c| c.timing_fault()).collect();
        assert_eq!(timing, [Gap, LateKey, UntimedKey, TapSwitchedOff, Full]);
    }

    #[test]
    fn the_cap_holds_the_keys_of_the_bound_at_the_fastest_repeat_beside_a_fast_typist() {
        const FASTEST_REPEAT: Duration = Duration::from_millis(15);
        const FAST_TYPIST_KEYS_A_SECOND: u128 = 20;
        let bound_ms = HOLD_BOUND.as_millis();
        let repeats = bound_ms / FASTEST_REPEAT.as_millis();
        let typed = bound_ms * FAST_TYPIST_KEYS_A_SECOND / 1_000;
        assert_eq!((repeats, typed), (2_000, 600));
        assert!(repeats + typed < QUEUE_CAP as u128);
        let mut hold: HeldKeys<u64> = HeldKeys::new(Bounds::PRODUCT);
        let step = FASTEST_REPEAT.as_nanos() as u64 / 2;
        let mut now = 0;
        for key in 0..(repeats + typed) as u64 {
            now += step;
            assert_eq!(
                hold.reading(Reading::Off, both(Monotonic(now))),
                Step::Waiting
            );
            assert_eq!(
                hold.admit(Reading::Off, IN_TIME, Monotonic(now), key),
                Admission::Held,
                "key {key}"
            );
        }
    }
}
