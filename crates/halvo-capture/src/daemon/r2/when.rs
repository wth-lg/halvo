//! When the analysis worker may run: the rule over three readings of the system, and the driver that gives the worker
//! slot its yes or no. The readings are the system's own figures, not the capture sources: the person's idle time,
//! whether the computer draws its power from the mains, and whether low-power mode is on.
//!
//! - THE RULE. A worker may run only when all of these hold: the idle time is 240 s or more ([`IDLE`]), counted inside
//!   this life of the daemon (the smaller of the system's figure and the loop's time); the computer is on mains power
//!   (`AC Power` on macOS, `ACLineStatus` 1 on Windows: a machine with no battery is on mains, one on the battery of
//!   a UPS is not); low-power mode is off (Low Power Mode on macOS, the battery saver on Windows); and the app gave
//!   its first liveness signal in this life ([`app_seen`]). A reading that fails or gives an unknown value is a no.
//! - THE DRIVER. [`Driver::turn`] reads the system at most once in 1 s of the loop's time ([`PERIOD`]), however
//!   often it is turned. After each reading it asks its [`Source`] for the origin and the watermark, and calls the
//!   slot's `pass` once with the loop's time: a yes carries the watermark, and the call names the origin that the
//!   source gave at that reading. Before the source has given any origin no worker can live, and a no makes no pass.
//! - THE THREAD. [`Driver::run`] moves the driver onto a thread of its own. The supervisor's tick hands that thread
//!   the loop's time ([`tick`]), so no reading and no end of a worker holds the tick up; the thread turns the driver
//!   with the newest time that it got.
//!
//! The newest readings live in memory alone: no file, line, record or status holds the idle time.

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use halvo_schema::Position;

use crate::daemon::r2::worker_slot::{Answer, MayRun, Origin, WorkerSlot};

#[cfg(target_os = "macos")]
use super::when_macos as sys;
#[cfg(windows)]
use super::when_windows as sys;

/// The shortest idle time at which a worker may run.
pub const IDLE: Duration = Duration::from_secs(240);

/// The shortest time of the loop between two readings of the system.
pub const PERIOD: Duration = Duration::from_secs(1);

/// One reading of the system. Each part is none where its reading failed or gave an unknown value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// The time since the person's last input to this computer, the system's own figure.
    pub idle: Option<Duration>,
    /// Whether the computer draws its power from the mains.
    pub mains: Option<bool>,
    /// Whether low-power mode is on.
    pub low_power: Option<bool>,
}

impl Reading {
    /// Whether a worker may run by this reading, at the loop's time `life` and with the app seen or not.
    fn may_run(&self, life: Duration, app_seen: bool) -> bool {
        let Some(idle) = self.idle else {
            return false;
        };
        app_seen
            && idle.min(life) >= IDLE
            && self.mains == Some(true)
            && self.low_power == Some(false)
    }
}

/// The readings of the system that the driver takes: the system's own in the daemon ([`SystemReadings`]), a
/// stand-in in a test.
pub trait Readings: Send {
    /// One reading of the system.
    fn read(&mut self) -> Reading;
}

/// The system's own readings: on macOS the seconds since the last input event of the HID system state, the
/// providing power source and Low Power Mode; on Windows the last input against the tick count, and the power status.
pub struct SystemReadings;

impl Readings for SystemReadings {
    fn read(&mut self) -> Reading {
        sys::read()
    }
}

/// The one source of what a yes carries and of where its worker reads from, which the driver's maker hands in.
pub trait Source: Send {
    /// The origin of a worker (the capture store's id and the committed position of the owner's newest report
    /// frame) and the watermark of the capture store that the cursor last read, both taken anew at each call; none
    /// while the capture store's id is not known.
    fn origin(&mut self) -> Option<(Origin, Option<Position>)>;
}

/// The slot that the driver passes: the worker slot, or a stand-in in a test.
pub trait Slot: Send {
    /// One pass of the slot, as [`WorkerSlot::pass`] has it.
    fn pass(&mut self, now: Instant, may_run: MayRun, origin: &Origin) -> Answer;

    /// Whether the person's switch of the search for repeated tasks is on; a slot with no switch is on.
    fn searching(&self) -> bool {
        true
    }
}

impl Slot for WorkerSlot {
    fn pass(&mut self, now: Instant, may_run: MayRun, origin: &Origin) -> Answer {
        WorkerSlot::pass(self, now, may_run, origin)
    }
}

/// Whether the app gave its first liveness signal in this life of the daemon.
struct Liveness(AtomicBool);

impl Liveness {
    const fn new() -> Liveness {
        Liveness(AtomicBool::new(false))
    }

    fn see(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn seen(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// The app's liveness in this life of the daemon.
static APP: Liveness = Liveness::new();

/// The app's first liveness signal in this life of the daemon: from this call on the driver may answer yes. The
/// capture service's run makes it at each status that claims the indicator (`service::seen`).
pub fn app_seen() {
    APP.see();
}

/// The sender of the driver's thread that [`tick`] turns: the newest that [`Driver::run`] started.
static TURNS: Mutex<Option<Sender<Duration>>> = Mutex::new(None);

/// Hands the loop's time `now` to the thread of the driver that [`Driver::run`] started; with none started it does
/// nothing. The supervisor's tick calls it.
pub fn tick(now: Duration) {
    if let Some(turns) = TURNS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = turns.send(now);
    }
}

/// The driver of the worker slot: the readings of the system, the source of the origin, the slot and the app's
/// liveness.
pub struct Driver<R, S, P> {
    readings: R,
    source: S,
    slot: P,
    app: &'static Liveness,
    /// The loop's time of the last reading.
    read_at: Option<Duration>,
    /// The clock's reading that stands for the loop's time zero.
    zero: Instant,
    /// The origin that the source last gave.
    origin: Option<Origin>,
}

impl<R: Readings, S: Source, P: Slot> Driver<R, S, P> {
    /// Makes the driver over the readings, the source and the slot. Nothing is read here.
    pub fn new(readings: R, source: S, slot: P) -> Driver<R, S, P> {
        Driver::over(readings, source, slot, &APP)
    }

    fn over(readings: R, source: S, slot: P, app: &'static Liveness) -> Driver<R, S, P> {
        Driver {
            readings,
            source,
            slot,
            app,
            read_at: None,
            zero: Instant::now(),
            origin: None,
        }
    }

    /// The slot.
    pub fn slot(&mut self) -> &mut P {
        &mut self.slot
    }

    /// One turn at the loop's time `now`. At the first turn, and once 1 s of the loop's time has passed since the
    /// last reading, it reads the system, asks the source and passes the slot once: a yes with the source's
    /// watermark, or a no. The slot's answer; none where the turn read nothing, or where the source has given no
    /// origin yet.
    pub fn turn(&mut self, now: Duration) -> Option<Answer> {
        if self
            .read_at
            .is_some_and(|read_at| now.saturating_sub(read_at) < PERIOD)
        {
            return None;
        }
        self.read_at = Some(now);
        let yes = self.readings.read().may_run(now, self.app.seen());
        let given = self.source.origin();
        let may_run = match &given {
            Some((_, watermark)) if yes => MayRun::Yes {
                watermark: *watermark,
            },
            _ => MayRun::No,
        };
        if let Some((origin, _)) = given {
            self.origin = Some(origin);
        }
        let origin = self.origin.as_ref()?;
        let at = self.zero.checked_add(now).unwrap_or(self.zero);
        Some(self.slot.pass(at, may_run, origin))
    }
}

impl<R, S, P> Driver<R, S, P>
where
    R: Readings + 'static,
    S: Source + 'static,
    P: Slot + 'static,
{
    /// Moves the driver onto a thread of its own, which [`tick`] turns from then on with the newest loop's time that
    /// it got. The thread of a driver that an earlier call started ends at its next wait, and drops its slot. The
    /// error where the thread did not start; the earlier driver is then still the one that [`tick`] turns.
    pub fn run(mut self) -> io::Result<()> {
        let (turns, times) = mpsc::channel::<Duration>();
        std::thread::Builder::new()
            .name("halvo-analysis-when".to_owned())
            .spawn(move || {
                while let Ok(mut now) = times.recv() {
                    while let Ok(newer) = times.try_recv() {
                        now = newer;
                    }
                    self.turn(now);
                }
            })?;
        *TURNS.lock().unwrap_or_else(PoisonError::into_inner) = Some(turns);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use halvo_schema::{Position, StoreId};

    use super::{Driver, Liveness, Reading, Readings, Slot, Source};
    use crate::daemon::r2::worker_slot::{Answer, MayRun, Origin};

    /// The watermark that the stand-in source gives.
    const WATERMARK: Position = Position { seg: 2, idx: 5 };

    /// What the stand-ins share with the test: the reading that they give, how often it was read, and every pass of
    /// the slot.
    #[derive(Default)]
    struct Seen {
        reading: Option<Reading>,
        reads: usize,
        passes: Vec<(MayRun, Origin)>,
        switched_off: bool,
    }

    #[derive(Clone, Default)]
    struct Shared(Arc<Mutex<Seen>>);

    impl Shared {
        fn set(&self, reading: Reading) {
            self.0.lock().unwrap().reading = Some(reading);
        }

        fn reads(&self) -> usize {
            self.0.lock().unwrap().reads
        }

        fn switch(&self, on: bool) {
            self.0.lock().unwrap().switched_off = !on;
        }

        /// The answers of the passes so far, and the check that each named the source's origin.
        fn answers(&self) -> Vec<MayRun> {
            let seen = self.0.lock().unwrap();
            seen.passes
                .iter()
                .map(|(may_run, origin)| {
                    assert_eq!(*origin, origin_of_source());
                    *may_run
                })
                .collect()
        }
    }

    impl Readings for Shared {
        fn read(&mut self) -> Reading {
            let mut seen = self.0.lock().unwrap();
            seen.reads += 1;
            seen.reading.expect("the test set no reading")
        }
    }

    impl Slot for Shared {
        fn pass(&mut self, _now: Instant, may_run: MayRun, origin: &Origin) -> Answer {
            self.0
                .lock()
                .unwrap()
                .passes
                .push((may_run, origin.clone()));
            match may_run {
                MayRun::Yes { .. } => Answer::Started,
                MayRun::No => Answer::Idle,
            }
        }

        fn searching(&self) -> bool {
            !self.0.lock().unwrap().switched_off
        }
    }

    struct Given;

    impl Source for Given {
        fn origin(&mut self) -> Option<(Origin, Option<Position>)> {
            Some((origin_of_source(), Some(WATERMARK)))
        }
    }

    fn origin_of_source() -> Origin {
        Origin {
            capture: StoreId::parse("0123456789abcdef0123456789abcdef").unwrap(),
            committed: None,
        }
    }

    const YES: MayRun = MayRun::Yes {
        watermark: Some(WATERMARK),
    };

    fn secs(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    /// A reading on mains, not in low-power mode, with the idle time `idle`.
    fn good(idle: Duration) -> Reading {
        Reading {
            idle: Some(idle),
            mains: Some(true),
            low_power: Some(false),
        }
    }

    /// A liveness of the test's own, not seen yet.
    fn liveness() -> &'static Liveness {
        Box::leak(Box::new(Liveness::new()))
    }

    /// A driver over the stand-ins, with the app seen.
    fn seen_driver(shared: &Shared) -> Driver<Shared, Given, Shared> {
        let app = liveness();
        app.see();
        Driver::over(shared.clone(), Given, shared.clone(), app)
    }

    /// Rule 1: on mains, with the app seen, at the loop's time 600 s, an idle time of 239 s is a no and one of
    /// 240 s a yes.
    #[test]
    fn idle_of_240_s_is_a_yes() {
        let shared = Shared::default();
        let mut driver = seen_driver(&shared);
        shared.set(good(secs(239)));
        assert_eq!(driver.turn(secs(600)), Some(Answer::Idle));
        shared.set(good(secs(240)));
        assert_eq!(driver.turn(secs(601)), Some(Answer::Started));
        assert_eq!(shared.answers(), [MayRun::No, YES]);
    }

    /// Rule 2: a reading of 900 s counts inside this life of the daemon: a no at the loop's time 239 s, a yes at
    /// 240 s.
    #[test]
    fn a_new_life_counts_its_own_idle() {
        let shared = Shared::default();
        let mut driver = seen_driver(&shared);
        shared.set(good(secs(900)));
        driver.turn(secs(239));
        driver.turn(secs(240));
        assert_eq!(shared.answers(), [MayRun::No, YES]);
    }

    /// Rule 3: at an idle time of 600 s, the battery, low-power mode and a power reading that fails or is unknown
    /// are each a no; mains with low-power mode off is a yes.
    #[test]
    fn battery_low_power_or_no_answer_is_a_no() {
        let shared = Shared::default();
        let mut driver = seen_driver(&shared);
        let mains = good(secs(600));
        let readings = [
            Reading {
                mains: Some(false),
                ..mains
            },
            Reading {
                low_power: Some(true),
                ..mains
            },
            Reading {
                mains: None,
                ..mains
            },
            Reading {
                low_power: None,
                ..mains
            },
            mains,
        ];
        for (second, reading) in (600..).zip(readings) {
            shared.set(reading);
            driver.turn(secs(second));
        }
        assert_eq!(
            shared.answers(),
            [MayRun::No, MayRun::No, MayRun::No, MayRun::No, YES]
        );
    }

    /// Rule 4: with every reading good the answer is no until the app's first liveness signal, and yes after it.
    /// The driver reads a liveness that the test owns, not the static that `Driver::new` reads: each status that claims
    /// the indicator sets that static, and another test of this process may serve one first. halvo-analysis tests the
    /// static's path in a process of its own (tests/worker_when.rs).
    #[test]
    fn no_yes_before_the_app_is_seen() {
        let shared = Shared::default();
        let app = liveness();
        let mut driver = Driver::over(shared.clone(), Given, shared.clone(), app);
        shared.set(good(secs(600)));
        driver.turn(secs(600));
        app.see();
        driver.turn(secs(601));
        assert_eq!(shared.answers(), [MayRun::No, YES]);
    }

    /// Rules 5 and 6: ten turns inside 1 s of the loop's time read the system once and pass the slot once; a turn
    /// 1 s after the reading reads and passes again.
    #[test]
    fn one_reading_a_second() {
        let shared = Shared::default();
        let mut driver = seen_driver(&shared);
        shared.set(good(secs(600)));
        for tenth in 0..10 {
            driver.turn(secs(600) + Duration::from_millis(100 * tenth));
        }
        assert_eq!((shared.reads(), shared.answers().len()), (1, 1));
        driver.turn(secs(601));
        assert_eq!((shared.reads(), shared.answers().len()), (2, 2));
    }

    /// While the search is switched off a turn reads nothing of the system and passes a no; the first turn behind a
    /// switch to on reads the system and decides by that reading.
    #[test]
    fn a_turn_reads_the_system_only_while_the_search_is_switched_on() {
        let shared = Shared::default();
        let mut driver = seen_driver(&shared);
        shared.set(good(secs(600)));
        shared.switch(false);
        driver.turn(secs(600));
        driver.turn(secs(601));
        assert_eq!(shared.reads(), 0, "two turns with the search switched off");
        assert_eq!(shared.answers(), [MayRun::No, MayRun::No]);
        shared.switch(true);
        driver.turn(secs(602));
        assert_eq!(shared.reads(), 1, "the first turn behind a switch to on");
        assert_eq!(shared.answers(), [MayRun::No, MayRun::No, YES]);
    }
}
