//! Drawn sequences of the hold on stand-in clocks (ruling MA-24 (d)): each
//! sequence types one key among sleeps, ticks, pointer events, preemptions,
//! slow reads, readings of on and stops, and a model of the rules says whether
//! the key may be released and whether it must be. Three forms: the difference
//! kept at the start, no close pair at the start, and pairs that fail at any
//! reading.

use super::*;

#[derive(Clone, Copy, Debug)]
enum Inject {
    Preempt(u64),
    Sleep(u64),
}

thread_local! {
    static CALL: Cell<u32> = const { Cell::new(0) };
    static INJECT: Cell<Option<(u32, Inject)>> = const { Cell::new(None) };
    static NO_CLOSE: Cell<bool> = const { Cell::new(false) };
    static SESSION_WIDTH: Cell<u64> = const { Cell::new(0) };
    static READ_WIDTH: Cell<u64> = const { Cell::new(0) };
    static LAST_STAMP: Cell<u64> = const { Cell::new(0) };
    static READER_JUST: Cell<bool> = const { Cell::new(false) };
    static READINGS: RefCell<Vec<(u64, u64, Reading)>> = const { RefCell::new(Vec::new()) };
    static SLEEPS: Cell<u32> = const { Cell::new(0) };
    static FAILED_READINGS: Cell<u32> = const { Cell::new(0) };
}

fn hook() {
    let call = CALL.with(|call| {
        call.set(call.get() + 1);
        call.get()
    });
    if let Some((at, inject)) = INJECT.with(Cell::get) {
        if at == call {
            INJECT.with(|slot| slot.set(None));
            match inject {
                Inject::Preempt(ns) => stall(ns),
                Inject::Sleep(ns) => mac_sleep(ns),
            }
        }
    }
}

fn mac_sleep(ns: u64) {
    MONOTONIC.with(|now| now.set(now.get() + ns));
    SLEEPS.with(|sleeps| sleeps.set(sleeps.get() + 1));
}

fn drawn_stamps() -> Monotonic {
    hook();
    let now = MONOTONIC.with(Cell::get);
    if READER_JUST.with(|just| just.replace(false)) {
        READINGS.with(|readings| {
            if let Some(last) = readings.borrow_mut().last_mut() {
                last.1 = now;
            }
        });
    }
    LAST_STAMP.with(|stamp| stamp.set(now));
    Monotonic::from_nanos(now)
}

fn drawn_uptime() -> Uptime {
    if NO_CLOSE.with(Cell::get) {
        stall(NO_CLOSE_STEP_NS);
    }
    hook();
    Uptime::from_nanos(UPTIME.with(Cell::get))
}

fn drawn_session() -> Session {
    stall(SESSION_WIDTH.with(Cell::get));
    stand_in_session()
}

fn drawn_reader(session: Session) -> Reading {
    let before = LAST_STAMP.with(Cell::get);
    stall(READ_WIDTH.with(Cell::get));
    let answer = stand_in_reader(session);
    READINGS.with(|readings| readings.borrow_mut().push((before, 0, answer)));
    READER_JUST.with(|just| just.set(true));
    if NO_CLOSE.with(Cell::get) {
        FAILED_READINGS.with(|failed| failed.set(failed.get() + 1));
    }
    answer
}

const DRAWN_CLOCKS: Clocks = Clocks {
    stamps: drawn_stamps,
    os_time: drawn_uptime,
};

fn reset_drawn() {
    CALL.with(|call| call.set(0));
    INJECT.with(|slot| slot.set(None));
    NO_CLOSE.with(|no_close| no_close.set(false));
    SESSION_WIDTH.with(|width| width.set(0));
    READ_WIDTH.with(|width| width.set(0));
    LAST_STAMP.with(|stamp| stamp.set(0));
    READER_JUST.with(|just| just.set(false));
    READINGS.with(|readings| readings.borrow_mut().clear());
    SLEEPS.with(|sleeps| sleeps.set(0));
    FAILED_READINGS.with(|failed| failed.set(0));
}

fn rig(no_close_at_the_start: bool) -> (Held, DropCounts) {
    reset_stand_ins();
    reset_drawn();
    let (handle, writer) = sink::channel(64);
    let (fg_writer, reader) = foreground_state_for_test();
    let counters = InputCounters::new();
    let settings = TapSettings {
        secure: drawn_reader,
        session: drawn_session,
        clocks: DRAWN_CLOCKS,
        ..tap_settings()
    };
    let mut ctx = Box::new(tap_context(
        handle,
        reader,
        counters.clone(),
        &settings,
        Bounds {
            hold: Duration::from_millis(100),
            gap: Duration::from_millis(30),
            lag: Duration::from_millis(30),
            cap: 64,
        },
        Arc::new(AtomicBool::new(false)),
    ));
    let drops = DropCounts::new();
    ctx.drops = Some(drops.clone());
    NO_CLOSE.with(|no_close| no_close.set(no_close_at_the_start));
    ctx.start(&mut lock(&ctx.hold));
    NO_CLOSE.with(|no_close| no_close.set(false));
    (
        Held {
            ctx,
            writer,
            counters,
            fg_writer,
        },
        drops,
    )
}

struct Health<'a> {
    held: &'a Held,
    drops: &'a DropCounts,
    unclocked: u64,
    broken: &'a mut Vec<String>,
}

impl Health<'_> {
    fn after(&mut self, what: &str) {
        let unclocked = self.drops.of("unclocked");
        if unclocked > self.unclocked
            && !self.held.ctx.stop.load(Ordering::SeqCst)
            && !self.held.counters.stalled()
            && self.broken.len() < 6
        {
            self.broken.push(format!(
                "a key withheld or dropped for a failed pair read healthy after {what}"
            ));
        }
        self.unclocked = unclocked;
    }
}

fn tick_drawn(ctx: &TapContext, no_close: bool) {
    advance(10);
    CALL.with(|call| call.set(0));
    NO_CLOSE.with(|slot| slot.set(no_close));
    on_reading_tick(ctx);
    NO_CLOSE.with(|slot| slot.set(false));
}

fn pointer_drawn(ctx: &TapContext, no_close: bool) {
    advance(1);
    CALL.with(|call| call.set(0));
    NO_CLOSE.with(|slot| slot.set(no_close));
    let monotonic = (ctx.clocks.stamps)();
    let uptime = (ctx.clocks.os_time)();
    on_tap_event(
        ctx,
        TapEvent {
            etype: CGEventType::MouseMoved,
            flags: 0,
            os_time: uptime,
            entry: Entry { monotonic, uptime },
        },
    );
    NO_CLOSE.with(|slot| slot.set(false));
}

struct Draw(u64);

impl Draw {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn percent(&mut self, p: u64) -> bool {
        self.below(100) < p
    }
}

const SLEEPS_NS: [u64; 4] = [2 * MS, 31 * MS, 5_000 * MS, 60_000 * MS];

fn lag_draw(draw: &mut Draw) -> u64 {
    match draw.below(6) {
        0 => 0,
        1 => draw.below(29 * MS),
        2 => 30 * MS,
        3 => 30 * MS + 1,
        4 => 29 * MS + draw.below(2 * MS),
        _ => 30 * MS + draw.below(30 * MS),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Form {
    Kept,
    NoCloseAtTheStart,
    FailedPairs,
}

#[derive(Default)]
struct Tally {
    runs: u64,
    released: u64,
    must_release: u64,
    against_the_rules: Vec<String>,
    over_withheld: Vec<String>,
    after_a_stop: Vec<String>,
    health: Vec<String>,
    with_a_failed_pair: u64,
}

#[allow(clippy::too_many_lines)]
fn one_sequence(seed: u64, form: Form, tally: &mut Tally) {
    let mut draw = Draw(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let (held, drops) = rig(form == Form::NoCloseAtTheStart);
    let ctx = &held.ctx;
    let quiet = draw.percent(50);
    let failing = |draw: &mut Draw| form == Form::FailedPairs && draw.percent(15);
    let mut health_broken = Vec::new();
    let mut health = Health {
        held: &held,
        drops: &drops,
        unclocked: 0,
        broken: &mut health_broken,
    };
    let mut log: Vec<String> = vec![format!("seed={seed} quiet={quiet}")];
    let mut undetected = false;
    let mut clocked = form != Form::NoCloseAtTheStart;
    if quiet && !clocked {
        pointer_drawn(ctx, false);
        clocked = true;
        log.push("pointer".into());
    }
    for _ in 0..draw.below(4) {
        match draw.below(4) {
            0 => {
                let ms = 1 + draw.below(400);
                advance(ms);
                log.push(format!("wait {ms}ms"));
            }
            1 if !quiet => {
                let ns = SLEEPS_NS[draw.below(4) as usize];
                mac_sleep(ns);
                undetected = true;
                log.push(format!("sleep {ns}ns"));
            }
            2 => {
                let fails = failing(&mut draw);
                tick_drawn(ctx, fails);
                health.after("a tick");
                undetected = false;
                clocked |= !fails;
                log.push(format!("tick fails={fails}"));
            }
            _ => {
                let fails = failing(&mut draw);
                pointer_drawn(ctx, fails);
                health.after("a pointer event");
                undetected = false;
                clocked |= !fails;
                log.push(format!("pointer fails={fails}"));
            }
        }
    }
    stall(1_000);
    let mut typed_uptime = UPTIME.with(Cell::get);
    let typed_monotonic = MONOTONIC.with(Cell::get);
    let sleeps_at_the_key = SLEEPS.with(Cell::get);
    let failed_at_the_key = FAILED_READINGS.with(Cell::get);
    let untimed_draw = !quiet && draw.percent(4);
    let before_the_callback = if quiet {
        draw.below(29 * MS)
    } else {
        lag_draw(&mut draw)
    };
    stall(before_the_callback);
    log.push(format!(
        "typed; {before_the_callback}ns before the callback"
    ));
    for _ in 0..draw.below(3) {
        match draw.below(4) {
            0 if !quiet => {
                let ns = SLEEPS_NS[draw.below(4) as usize];
                mac_sleep(ns);
                log.push(format!("mid sleep {ns}ns"));
            }
            1 => {
                let fails = failing(&mut draw);
                tick_drawn(ctx, fails);
                health.after("a tick");
                log.push(format!("mid tick fails={fails}"));
            }
            2 => {
                let fails = failing(&mut draw);
                pointer_drawn(ctx, fails);
                health.after("a pointer event");
                log.push(format!("mid pointer fails={fails}"));
            }
            _ => {
                let ns = draw.below(if quiet { MS } else { 5 * MS });
                stall(ns);
                log.push(format!("mid wait {ns}ns"));
            }
        }
    }
    let session_width = if !quiet && draw.percent(30) {
        draw.below(40 * MS)
    } else {
        0
    };
    let read_width = if !quiet && draw.percent(30) {
        draw.below(40 * MS)
    } else {
        0
    };
    let inject = if !quiet && draw.percent(50) {
        let at = 1 + draw.below(12) as u32;
        let what = if draw.percent(50) {
            Inject::Preempt(draw.below(10 * MS))
        } else {
            Inject::Sleep([2 * MS, 5_000 * MS][draw.below(2) as usize])
        };
        Some((at, what))
    } else {
        None
    };
    let own_reading = if quiet {
        Reading::Off
    } else {
        match draw.below(10) {
            0 => Reading::On,
            1 => Reading::Unknown,
            _ => Reading::Off,
        }
    };
    let key_fails = failing(&mut draw);
    log.push(format!(
        "callback session={session_width} read={read_width} inject={inject:?} reading={own_reading:?} fails={key_fails}"
    ));
    SESSION_WIDTH.with(|width| width.set(session_width));
    READ_WIDTH.with(|width| width.set(read_width));
    ANSWERS.with(|answers| answers.borrow_mut().push(own_reading));
    let first_reading = READINGS.with(|readings| readings.borrow().len());
    CALL.with(|call| call.set(0));
    INJECT.with(|slot| slot.set(inject));
    NO_CLOSE.with(|slot| slot.set(key_fails));
    let entry_monotonic = drawn_stamps();
    let entry_uptime = drawn_uptime();
    if untimed_draw {
        typed_uptime = entry_uptime.nanos() + 1;
        log.push("an OS time after the entry".into());
    }
    on_tap_event(
        ctx,
        TapEvent {
            etype: CGEventType::KeyDown,
            flags: 0,
            os_time: Uptime::from_nanos(typed_uptime),
            entry: Entry {
                monotonic: entry_monotonic,
                uptime: entry_uptime,
            },
        },
    );
    NO_CLOSE.with(|slot| slot.set(false));
    health.after("the key's own event");
    INJECT.with(|slot| slot.set(None));
    SESSION_WIDTH.with(|width| width.set(0));
    READ_WIDTH.with(|width| width.set(0));
    let (own_before, own_after, _) = READINGS.with(|readings| readings.borrow()[first_reading]);
    let true_lag = own_after.saturating_sub(typed_monotonic);
    let untimed = typed_uptime == 0 || typed_uptime > entry_uptime.nanos();
    let code_lag = if untimed {
        u64::MAX
    } else {
        (entry_uptime.nanos() - typed_uptime) + own_after.saturating_sub(entry_monotonic.nanos())
    };
    let held_after = held_now(ctx);
    let mut on_seen = own_reading != Reading::Off;
    let mut stop_set = false;
    let mut released_at: Option<(u32, u32, bool, bool)> = None;
    let mut flags_at_the_stop: Option<bool> = None;
    let mut slept_after = false;
    let mut failed_after = false;
    for at in 0..25 {
        if !quiet && draw.percent(4) {
            let ns = SLEEPS_NS[draw.below(4) as usize];
            mac_sleep(ns);
            slept_after = true;
            log.push(format!("sleep {ns}ns before tick {at}"));
        }
        if !quiet && draw.percent(4) {
            ANSWERS.with(|answers| answers.borrow_mut().push(Reading::On));
            log.push(format!("on at tick {at}"));
        }
        if !quiet && !stop_set && draw.percent(2) {
            flags_at_the_stop = Some(held.counters.figures().keys_withheld);
            held.ctx.stop.store(true, Ordering::SeqCst);
            stop_set = true;
            log.push(format!("stop before tick {at}"));
        }
        let fails = !quiet && failing(&mut draw);
        failed_after |= fails;
        let readings_before = READINGS.with(|readings| readings.borrow().len());
        tick_drawn(ctx, fails);
        health.after("a tick");
        let tick_on = READINGS.with(|readings| {
            readings.borrow()[readings_before..]
                .iter()
                .any(|(_, _, value)| *value != Reading::Off)
        });
        if released_at.is_none() && keystrokes(&held.writer) > 0 {
            released_at = Some((
                SLEEPS.with(Cell::get),
                FAILED_READINGS.with(Cell::get),
                on_seen,
                stop_set,
            ));
        }
        on_seen |= tick_on;
    }
    let wide_gap = READINGS.with(|readings| {
        readings.borrow()[first_reading..]
            .windows(2)
            .any(|two| two[1].1.saturating_sub(two[0].0) > 30 * MS || two[1].1 < two[0].0)
    });
    tally.runs += 1;
    tally.released += u64::from(released_at.is_some());
    let why_not = |(sleeps, failed, on, stopped): (u32, u32, bool, bool)| -> Option<&'static str> {
        if true_lag > 30 * MS {
            Some("a true lag above 30 ms")
        } else if untimed {
            Some("untimed")
        } else if sleeps > sleeps_at_the_key {
            Some("a sleep between the key's time and its release")
        } else if failed > failed_at_the_key {
            Some("a failed pair between the key's time and its release")
        } else if on {
            Some("a reading of on or unknown between its arrival and its release")
        } else if stopped {
            Some("a release after a stop")
        } else {
            None
        }
    };
    if let Some(at) = released_at {
        if let Some(why) = why_not(at) {
            if tally.against_the_rules.len() < 6 {
                tally.against_the_rules.push(format!(
                    "{why}: true_lag={true_lag} code_lag={code_lag} held_after={held_after} | {}",
                    log.join("; ")
                ));
            }
        }
    }
    let must_release = !untimed
        && true_lag <= 30 * MS
        && code_lag <= 30 * MS
        && clocked
        && !undetected
        && SLEEPS.with(Cell::get) == sleeps_at_the_key
        && FAILED_READINGS.with(Cell::get) == failed_at_the_key
        && !slept_after
        && !failed_after
        && !on_seen
        && !stop_set
        && !wide_gap;
    if must_release {
        tally.must_release += 1;
        if released_at.is_none() && tally.over_withheld.len() < 6 {
            tally.over_withheld.push(format!(
                "a key that the rules release was withheld: true_lag={true_lag} code_lag={code_lag} \
                 own_before={own_before} held_after={held_after} | {}",
                log.join("; ")
            ));
        }
    }
    if let Some(at_the_stop) = flags_at_the_stop {
        let at_the_end = held.counters.figures().keys_withheld;
        if at_the_end != at_the_stop && tally.after_a_stop.len() < 6 {
            tally.after_a_stop.push(format!(
                "keys_withheld moved after a stop | {}",
                log.join("; ")
            ));
        }
    }
    tally.with_a_failed_pair += u64::from(drops.of("unclocked") > 0);
    for broken in health_broken {
        if tally.health.len() < 6 {
            tally.health.push(format!("{broken} | {}", log.join("; ")));
        }
    }
}

fn run_drawn(sequences: u64, form: Form, label: &str) -> Tally {
    let mut tally = Tally::default();
    for seed in 1..=sequences {
        one_sequence(seed, form, &mut tally);
    }
    println!(
        "drawn {label}: runs={} released={} must_release={} against_the_rules={} over_withheld={} \
         after_a_stop={} health={} with_a_failed_pair={}",
        tally.runs,
        tally.released,
        tally.must_release,
        tally.against_the_rules.len(),
        tally.over_withheld.len(),
        tally.after_a_stop.len(),
        tally.health.len(),
        tally.with_a_failed_pair
    );
    for line in tally
        .against_the_rules
        .iter()
        .chain(&tally.over_withheld)
        .chain(&tally.after_a_stop)
        .chain(&tally.health)
    {
        println!("drawn {label}: {line}");
    }
    tally
}

fn assert_clean(tally: &Tally, floor: u64) {
    assert!(
        tally.must_release >= floor,
        "the draws reach {} keys that the rules release, under the floor of {floor}",
        tally.must_release
    );
    assert!(
        tally.against_the_rules.is_empty(),
        "{:?}",
        tally.against_the_rules
    );
    assert!(tally.over_withheld.is_empty(), "{:?}", tally.over_withheld);
    assert!(tally.after_a_stop.is_empty(), "{:?}", tally.after_a_stop);
    assert!(tally.health.is_empty(), "{:?}", tally.health);
}

#[test]
fn drawn_sequences_with_the_difference_kept_at_the_start_follow_the_rules() {
    let tally = run_drawn(20_000, Form::Kept, "the difference kept at the start");
    assert_clean(&tally, 1_000);
}

#[test]
fn drawn_sequences_with_no_close_pair_at_the_start_follow_the_rules_and_release_the_keys_after_the_first_close_pair(
) {
    let tally = run_drawn(5_000, Form::NoCloseAtTheStart, "no close pair at the start");
    assert_clean(&tally, 1_000);
}

#[test]
fn drawn_sequences_with_failed_pairs_at_any_reading_follow_the_rules_and_read_impaired() {
    let tally = run_drawn(10_000, Form::FailedPairs, "failed pairs at any reading");
    assert_clean(&tally, 1_000);
    assert!(
        tally.with_a_failed_pair >= 1_000,
        "the draws reach {} sequences with a key withheld or dropped for a failed pair",
        tally.with_a_failed_pair
    );
}
