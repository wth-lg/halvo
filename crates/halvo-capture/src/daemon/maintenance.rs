//! The maintenance schedule of the owner: expiry at open and on a 60 s
//! deadline of the monotonic clock, eviction after each flush, one chunk of
//! shreds per loop pass, and the guard against a wall clock set ahead.

use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime};

use super::port::{ByteBound, MaintFault, MaintainStore, Pass};

/// The interval of the expiry deadline, on the monotonic clock.
pub const MAINTENANCE_INTERVAL: Duration = Duration::from_secs(60);

/// The most segments that one shred call may shred in the product.
pub const SHRED_CHUNK: usize = 64;

/// How long a forward step of the wall clock must hold, in monotonic time,
/// before expiry reads the stepped wall clock.
pub const CLOCK_STEP_GRACE: Duration = Duration::from_secs(600);

/// A difference between the wall and the monotonic clock over one loop pass
/// that counts as a step of the wall clock.
pub const CLOCK_STEP_TOLERANCE: Duration = Duration::from_secs(1);

/// The two clocks of the owner. Deadlines read the monotonic clock; stamps and
/// the expiry comparison read the wall clock.
pub trait Clocks: Send {
    /// A reading that never goes back, from an origin of the clock's own.
    fn monotonic(&self) -> Duration;
    fn wall(&self) -> SystemTime;

    fn now(&self) -> Instants {
        Instants {
            monotonic: self.monotonic(),
            wall: self.wall(),
        }
    }
}

/// The clocks of the system.
#[derive(Debug, Clone, Copy)]
pub struct SystemClocks {
    origin: Instant,
}

impl SystemClocks {
    pub fn new() -> SystemClocks {
        SystemClocks {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClocks {
    fn default() -> SystemClocks {
        SystemClocks::new()
    }
}

impl Clocks for SystemClocks {
    fn monotonic(&self) -> Duration {
        self.origin.elapsed()
    }

    fn wall(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// One reading of both clocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Instants {
    pub monotonic: Duration,
    pub wall: SystemTime,
}

/// One shred call that the schedule asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Job {
    /// Expire what is due at the wall time `now`.
    Expire {
        now: SystemTime,
    },
    Evict,
}

/// When the owner calls the shred passes.
#[derive(Debug)]
pub struct Schedule {
    chunk: usize,
    bound: ByteBound,
    expiry_at: Duration,
    expiry_more: bool,
    evict_due: bool,
    expiry_failed: bool,
    evict_failed: bool,
    previous: Option<Instants>,
    steps: VecDeque<(Duration, Duration)>,
}

fn deadline_after(now: Instants) -> Duration {
    now.monotonic + MAINTENANCE_INTERVAL
}

fn reached(now: Instants, deadline: Duration) -> bool {
    now.monotonic >= deadline
}

impl Schedule {
    /// A schedule of shred calls of `chunk` segments that evicts to `bound`.
    pub fn new(chunk: usize, bound: ByteBound) -> Schedule {
        Schedule {
            chunk,
            bound,
            expiry_at: Duration::ZERO,
            expiry_more: true,
            evict_due: false,
            expiry_failed: false,
            evict_failed: false,
            previous: None,
            steps: VecDeque::new(),
        }
    }

    /// A flush landed: eviction is due.
    pub fn flushed(&mut self) {
        self.evict_due = true;
    }

    /// The monotonic reading at which the next shred call is due, or `None`
    /// when one is due now.
    pub fn deadline(&self) -> Option<Duration> {
        (!self.expiry_more && !self.evict_due).then_some(self.expiry_at)
    }

    /// The one shred call of this loop pass at `now`, if any: expiry first,
    /// then eviction.
    pub fn next(&mut self, now: Instants) -> Option<Job> {
        let stepped = self.observe(now);
        let expiry_wall = self.expiry_wall(now);
        let deadline = reached(now, self.expiry_at);
        if deadline && self.evict_failed {
            self.evict_due = true;
        }
        if stepped || self.expiry_more || deadline {
            self.expiry_at = deadline_after(now);
            self.expiry_more = false;
            return Some(Job::Expire { now: expiry_wall });
        }
        if self.evict_due {
            self.evict_due = false;
            return Some(Job::Evict);
        }
        None
    }

    /// Records the answer of `job`: a pass that owes work or finds more due is
    /// called again at the next loop pass; a fault is called again at each
    /// deadline until the job succeeds.
    pub fn done(&mut self, job: Job, answer: &Result<Pass, MaintFault>) {
        let again = answer.as_ref().is_ok_and(|pass| pass.more_due || pass.owed);
        match job {
            Job::Expire { .. } => {
                self.expiry_more = again;
                self.expiry_failed = answer.is_err();
            }
            Job::Evict => {
                self.evict_due = again;
                self.evict_failed = answer.is_err();
            }
        }
    }

    /// Whether the last call of either job failed.
    pub fn is_failing(&self) -> bool {
        self.expiry_failed || self.evict_failed
    }

    /// Whether the wall clock stepped against the monotonic clock since the
    /// last loop pass. A forward step of more than one interval is held until
    /// it has stood for [`CLOCK_STEP_GRACE`]; a step back takes back the steps
    /// still held, the latest first.
    fn observe(&mut self, now: Instants) -> bool {
        let Some(previous) = self.previous.replace(now) else {
            return false;
        };
        let monotonic = now.monotonic.saturating_sub(previous.monotonic);
        let (ahead, behind) = match now.wall.duration_since(previous.wall) {
            Ok(wall) if wall >= monotonic => (wall - monotonic, Duration::ZERO),
            Ok(wall) => (Duration::ZERO, monotonic - wall),
            Err(back) => (Duration::ZERO, back.duration() + monotonic),
        };
        if ahead > MAINTENANCE_INTERVAL {
            self.steps.push_back((now.monotonic, ahead));
        }
        let mut undone = behind;
        while !undone.is_zero() {
            let Some(last) = self.steps.back_mut() else {
                break;
            };
            let taken = last.1.min(undone);
            last.1 -= taken;
            undone -= taken;
            if last.1.is_zero() {
                self.steps.pop_back();
            }
        }
        ahead > CLOCK_STEP_TOLERANCE || behind > CLOCK_STEP_TOLERANCE
    }

    /// The wall time that expiry reads at `now`: the wall clock less every
    /// forward step that has not yet stood for [`CLOCK_STEP_GRACE`], so each
    /// step waits its own grace.
    fn expiry_wall(&mut self, now: Instants) -> SystemTime {
        while self
            .steps
            .front()
            .is_some_and(|(since, _)| now.monotonic.saturating_sub(*since) >= CLOCK_STEP_GRACE)
        {
            self.steps.pop_front();
        }
        let held: Duration = self.steps.iter().map(|(_, size)| *size).sum();
        now.wall.checked_sub(held).unwrap_or(now.wall)
    }

    /// Runs `job` on `store`: the one call of the port's `expire_due` and the
    /// one call of its `evict_to`.
    pub fn run<S: MaintainStore>(&self, store: &mut S, job: Job) -> Result<Pass, MaintFault> {
        match job {
            Job::Expire { now } => store.expire_due(now, self.chunk),
            Job::Evict => store.evict_to(self.bound, self.chunk),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;
    use crate::daemon::model_store::{foreground, Call, Harness, AT_NOON};
    use crate::daemon::owner::{Command, Report};
    use crate::daemon::port::Ordinal;

    #[test]
    fn expire_runs_at_open_and_within_60_s_under_a_paced_stream() {
        let mut harness = Harness::new();
        let log = harness.store.log();
        let first_expiry = log
            .iter()
            .position(|call| matches!(call, Call::ExpireDue { .. }))
            .expect("expire_due was not called at open");
        assert!(log[..first_expiry]
            .iter()
            .all(|call| matches!(call, Call::Usage)));
        assert_eq!(harness.reports(), [Report::StoreReady]);
        harness.ask(Command::ClosePause);
        let start = harness.clocks.monotonic();
        let mut last_call = start;
        let mut calls = harness.store.expire_calls().len();
        while harness.clocks.monotonic() - start < Duration::from_secs(200) {
            harness.emit(foreground(&AT_NOON, Duration::ZERO));
            harness.pass();
            let now = harness.clocks.monotonic();
            if harness.store.expire_calls().len() > calls {
                calls = harness.store.expire_calls().len();
                last_call = now;
            }
            assert!(
                now - last_call <= MAINTENANCE_INTERVAL,
                "no expiry call for {:?} under a paced stream",
                now - last_call
            );
            harness.clocks.advance(Duration::from_millis(300));
        }
        assert!(calls >= 4, "{calls} expiry calls in 200 s");
        assert!(harness
            .store
            .log()
            .iter()
            .all(|call| !matches!(call, Call::ExpireDue { max, .. } if *max != SHRED_CHUNK)));
    }

    #[test]
    fn sixty_five_due_segments_take_two_chunks_with_a_drain_between() {
        let store = crate::daemon::model_store::ModelStore::new();
        let shredded = |from: u64, count: u64| (from..from + count).map(Ordinal::new).collect();
        store.script_expire(vec![
            Ok(Pass {
                shredded: shredded(1, 64),
                more_due: true,
                owed: false,
            }),
            Ok(Pass {
                shredded: shredded(65, 1),
                more_due: false,
                owed: false,
            }),
        ]);
        let mut harness = Harness::over(store, crate::daemon::model_store::open_floor(), 16);
        assert_eq!(
            harness.store.expire_calls().len(),
            1,
            "open made more than one chunk"
        );
        harness.emit(foreground(&AT_NOON, Duration::ZERO));
        harness.pass();
        assert_eq!(
            harness.store.expire_calls().len(),
            2,
            "one loop pass made other than one chunk"
        );
        assert_eq!(
            harness.owner.snapshot().events_received(),
            1,
            "the owner did not drain between the chunks"
        );
        harness.passes(3);
        assert_eq!(harness.store.expire_calls().len(), 2, "a third chunk ran");
    }

    #[test]
    fn one_loop_pass_runs_one_chunk_while_more_is_due() {
        let store = crate::daemon::model_store::ModelStore::new();
        let chunk = |more_due| {
            Ok(Pass {
                shredded: Vec::new(),
                more_due,
                owed: false,
            })
        };
        store.script_expire(vec![chunk(true), chunk(true), chunk(true), chunk(false)]);
        let mut harness = Harness::over(store, crate::daemon::model_store::open_floor(), 16);
        for pass in 1..=3 {
            harness.emit(foreground(&AT_NOON, Duration::ZERO));
            harness.pass();
            assert_eq!(harness.store.expire_calls().len(), 1 + pass, "pass {pass}");
        }
        harness.passes(2);
        assert_eq!(harness.store.expire_calls().len(), 4);
    }

    #[test]
    fn a_wall_clock_step_back_does_not_delay_expiry() {
        let mut harness = Harness::new();
        harness.ask(Command::ClosePause);
        harness.clocks.advance(Duration::from_secs(10));
        harness.pass();
        let calls = harness.store.expire_calls().len();
        harness.clocks.step_wall_back(Duration::from_secs(3600));
        let stepped_at = harness.clocks.monotonic();
        loop {
            harness.clocks.advance(Duration::from_secs(1));
            harness.pass();
            if harness.store.expire_calls().len() > calls {
                break;
            }
            assert!(
                harness.clocks.monotonic() - stepped_at <= MAINTENANCE_INTERVAL,
                "expiry waited past 60 s of monotonic time after a step back"
            );
        }
    }

    #[test]
    fn a_forward_step_of_15_days_expires_nothing_inside_the_grace_time() {
        let mut harness = Harness::new();
        harness.ask(Command::ClosePause);
        let start_wall = harness.clocks.wall();
        let start = harness.clocks.monotonic();
        harness
            .clocks
            .step_wall_forward(Duration::from_secs(15 * 86_400));
        let calls_before = harness.store.expire_calls().len();
        while harness.clocks.monotonic() - start < CLOCK_STEP_GRACE - Duration::from_secs(1) {
            harness.clocks.advance(Duration::from_secs(5));
            harness.pass();
        }
        let inside = harness.store.expire_calls();
        assert!(
            inside.len() > calls_before,
            "no expiry call inside the grace time"
        );
        for now in &inside[calls_before..] {
            let elapsed = now.duration_since(start_wall).unwrap();
            assert!(
                elapsed <= CLOCK_STEP_GRACE + MAINTENANCE_INTERVAL,
                "expiry read the stepped wall clock {elapsed:?} after the start"
            );
        }
        while harness.clocks.monotonic() - start < CLOCK_STEP_GRACE + 2 * MAINTENANCE_INTERVAL {
            harness.clocks.advance(Duration::from_secs(5));
            harness.pass();
        }
        let last = *harness.store.expire_calls().last().unwrap();
        assert!(
            last.duration_since(start_wall).unwrap() > Duration::from_secs(15 * 86_400),
            "the step was never taken"
        );
    }

    #[test]
    fn a_step_in_either_direction_makes_expiry_due() {
        let mut schedule = Schedule::new(2, ByteBound::raw(0));
        let mut now = Instants {
            monotonic: Duration::from_secs(1),
            wall: UNIX_EPOCH + Duration::from_secs(1_000_000),
        };
        assert!(matches!(schedule.next(now), Some(Job::Expire { .. })));
        now.monotonic += Duration::from_secs(1);
        now.wall += Duration::from_secs(1);
        assert_eq!(schedule.next(now), None);
        now.wall += Duration::from_secs(30);
        assert!(matches!(schedule.next(now), Some(Job::Expire { .. })));
        now.wall -= Duration::from_secs(90);
        assert!(matches!(schedule.next(now), Some(Job::Expire { .. })));
        assert_eq!(schedule.next(now), None);
    }
}
