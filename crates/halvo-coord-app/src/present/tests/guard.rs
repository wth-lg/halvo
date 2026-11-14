use crate::present::guard::Guard;
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
