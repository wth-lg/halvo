use crate::present::guard::{Guard, Show};
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};

const ARM: Duration = Duration::from_millis(500);

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn reported_at(t0: Instant) -> Guard {
    let mut guard = Guard::new(ARM);
    assert!(guard.active(t0), "the first activation reports the show");
    guard.reported(t0);
    guard
}

fn answers(guard: Guard, at: Instant) -> bool {
    let mut copy = guard;
    copy.press(at)
}

#[test]
fn guard_a_press_before_the_report_returned_answers_nothing_however_late() {
    let t0 = Instant::now();
    let mut guard = Guard::new(ARM);
    assert!(!guard.press(t0 + ms(10_000)), "no activation and no report");
    assert!(guard.active(t0));
    assert!(
        !guard.press(t0 + ms(10_000)),
        "active, the report not returned"
    );
}

#[test]
fn guard_every_press_inside_arm_after_the_report_answers_nothing() {
    let t0 = Instant::now();
    for at in [0, 1, 100, 250, 499] {
        assert!(!answers(reported_at(t0), t0 + ms(at)), "{at} ms");
    }
}

#[test]
fn guard_a_press_at_arm_after_the_report_answers() {
    let t0 = Instant::now();
    assert!(answers(reported_at(t0), t0 + ARM));
    assert!(answers(reported_at(t0), t0 + ms(800)));
}

#[test]
fn guard_a_refused_press_starts_the_hold_anew() {
    let t0 = Instant::now();
    let mut guard = reported_at(t0);
    assert!(!guard.press(t0 + ms(100)));
    assert!(
        !answers(guard, t0 + ms(550)),
        "past the hold of the report, inside the hold of the refused press"
    );
    assert!(!answers(guard, t0 + ms(599)));
    assert!(answers(guard, t0 + ms(600)));
    assert!(!guard.press(t0 + ms(550)));
    assert!(!answers(guard, t0 + ms(1_049)));
    assert!(answers(guard, t0 + ms(1_050)));
}

#[test]
fn guard_a_held_run_of_presses_answers_nothing_while_it_lasts() {
    let t0 = Instant::now();
    let mut guard = reported_at(t0);
    for step in 1..=50 {
        assert!(!guard.press(t0 + ms(step * 100)), "press {step}");
    }
    assert!(!answers(guard, t0 + ms(5_499)));
    assert!(answers(guard, t0 + ms(5_500)));
}

#[test]
fn guard_the_window_active_again_starts_the_hold_anew() {
    let t0 = Instant::now();
    let mut guard = reported_at(t0);
    assert!(!guard.active(t0 + ms(700)), "the report comes once");
    assert!(!answers(guard, t0 + ms(1_199)));
    assert!(answers(guard, t0 + ms(1_200)));
}

#[test]
fn guard_the_hold_counts_from_the_return_of_the_report() {
    let t0 = Instant::now();
    let mut guard = Guard::new(ARM);
    assert!(guard.active(t0));
    assert!(!guard.active(t0 + ms(100)), "while the report is made");
    guard.reported(t0 + ms(300));
    assert!(!answers(guard, t0 + ms(700)), "500 ms after the activation");
    assert!(!answers(guard, t0 + ms(799)));
    assert!(answers(guard, t0 + ms(800)));
}

#[test]
fn guard_an_instant_read_before_the_start_of_the_hold_moves_it_back_never() {
    let t0 = Instant::now() + ms(1_000);
    let mut guard = reported_at(t0);
    guard.reported(t0 - ms(400));
    assert!(!guard.active(t0 - ms(300)));
    assert!(!guard.press(t0 - ms(200)));
    assert!(!answers(guard, t0 + ms(499)));
    assert!(answers(guard, t0 + ARM));
}

#[test]
fn guard_an_answering_press_does_not_start_the_hold_anew() {
    let t0 = Instant::now();
    let mut guard = reported_at(t0);
    assert!(guard.press(t0 + ARM));
    assert!(guard.press(t0 + ARM + ms(1)));
}

#[test]
fn guard_the_first_activation_alone_reports_the_show() {
    let t0 = Instant::now();
    let mut guard = Guard::new(ARM);
    assert!(guard.active(t0));
    assert!(!guard.active(t0 + ms(1)));
    guard.reported(t0 + ms(2));
    assert!(!guard.active(t0 + ms(3)));
}

#[test]
fn guard_a_hold_of_zero_answers_the_first_press_after_the_report() {
    let t0 = Instant::now();
    let mut guard = Guard::new(Duration::ZERO);
    assert!(!guard.press(t0), "no report yet");
    assert!(guard.active(t0));
    guard.reported(t0);
    assert!(guard.press(t0));
}

struct Clock {
    t0: Instant,
    reads: RefCell<Vec<u64>>,
}

impl Clock {
    fn new(reads: &[u64]) -> Clock {
        Clock {
            t0: Instant::now(),
            reads: RefCell::new(reads.iter().rev().copied().collect()),
        }
    }

    fn read(&self) -> Instant {
        self.t0
            + ms(self
                .reads
                .borrow_mut()
                .pop()
                .expect("a clock read more often than the test gave"))
    }

    fn at(&self, n: u64) -> Instant {
        self.t0 + ms(n)
    }
}

#[test]
fn show_the_first_activation_reports_once_and_the_hold_counts_from_the_return() {
    let clock = Clock::new(&[0, 300]);
    let reports = Cell::new(0);
    let report = || reports.set(reports.get() + 1);
    let show = Show::new(ARM, &report);
    assert!(!show.press(clock.at(0)), "before the window became active");
    show.active(&|| clock.read());
    assert_eq!(reports.get(), 1);
    assert!(
        !show.press(clock.at(799)),
        "499 ms after the report returned"
    );
    let clock = Clock::new(&[0, 300]);
    let show = Show::new(ARM, &report);
    show.active(&|| clock.read());
    assert!(show.press(clock.at(800)));
}

#[test]
fn show_a_later_activation_reports_nothing_and_starts_the_hold_anew() {
    let clock = Clock::new(&[0, 0, 700]);
    let reports = Cell::new(0);
    let report = || reports.set(reports.get() + 1);
    let show = Show::new(ARM, &report);
    show.active(&|| clock.read());
    show.active(&|| clock.read());
    assert_eq!(reports.get(), 1);
    assert!(!show.press(clock.at(1_199)));
    assert!(
        !show.press(clock.at(1_600)),
        "the refused press started the hold anew"
    );
    assert!(show.press(clock.at(2_100)));
}

#[test]
fn show_the_hold_starts_at_a_reading_of_the_clock_after_the_report_returned() {
    let clock = Clock::new(&[0, 100, 300]);
    let report = || {
        let _ = clock.read();
    };
    let show = Show::new(ARM, &report);
    show.active(&|| clock.read());
    assert!(!show.press(clock.at(799)), "the hold counts from 300 ms");
    let clock = Clock::new(&[0, 100, 300]);
    let report = || {
        let _ = clock.read();
    };
    let show = Show::new(ARM, &report);
    show.active(&|| clock.read());
    assert!(show.press(clock.at(800)));
}

#[test]
fn show_a_report_that_panics_still_starts_the_hold_and_the_show_goes_on() {
    let clock = Clock::new(&[0, 100]);
    let report = || panic!("the report panicked");
    let show = Show::new(ARM, &report);
    show.active(&|| clock.read());
    assert!(!show.press(clock.at(599)));
    assert!(show.press(clock.at(1_099)));
}

#[test]
fn show_a_lost_subclass_lets_no_allow_through_and_closes_on_any_other_control() {
    let clock = Clock::new(&[0, 0]);
    let reports = Cell::new(0);
    let report = || reports.set(reports.get() + 1);
    let show = Show::new(ARM, &report);
    show.active(&|| clock.read());
    assert!(
        show.click(true, clock.at(600)),
        "a whole show answers an allow after arm"
    );
    show.lose();
    for at in [0, 100, 600, 10_000] {
        assert!(
            !show.click(true, clock.at(at)),
            "the allowing control at {at} ms"
        );
        assert!(
            show.click(false, clock.at(at)),
            "another control at {at} ms"
        );
    }
    assert_eq!(reports.get(), 1);
}

#[test]
fn show_a_whole_show_holds_every_control_alike() {
    let clock = Clock::new(&[0, 0]);
    let report = || {};
    let show = Show::new(ARM, &report);
    show.active(&|| clock.read());
    assert!(
        !show.click(false, clock.at(100)),
        "the rejecting control inside arm"
    );
    assert!(
        !show.click(true, clock.at(599)),
        "the allowing control inside the restarted hold"
    );
    assert!(show.click(false, clock.at(1_099)));
}
