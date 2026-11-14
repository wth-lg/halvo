use super::{pay, ALLOW, REJECT};
use crate::present::scripted::Scripted;
use crate::present::Dialog;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[test]
fn the_scripted_dialog_records_each_card_and_answers_each_press_through_the_mapping() {
    let scripted = Scripted::new(Duration::from_secs(10));
    scripted.press(1, ALLOW);
    scripted.press(2, REJECT);
    let on_screen = Mutex::new(0);
    let report = || *on_screen.lock().unwrap() += 1;
    assert_eq!(scripted.show(&pay(), &report), ALLOW);
    assert_eq!(scripted.show(&pay(), &report), REJECT);
    assert_eq!(*on_screen.lock().unwrap(), 2);
    assert_eq!(scripted.cards(), [pay(), pay()]);
}

#[test]
fn the_scripted_dialog_with_no_press_in_its_wait_answers_as_a_time_limit_does() {
    let scripted = Scripted::new(Duration::from_millis(50));
    let start = Instant::now();
    assert_eq!(scripted.show(&pay(), &|| {}), REJECT);
    assert!(start.elapsed() >= Duration::from_millis(50));
    assert_eq!(scripted.cards(), [pay()]);
}

#[test]
fn the_scripted_dialog_waits_for_a_press_that_comes_later() {
    let scripted = Arc::new(Scripted::new(Duration::from_secs(10)));
    let shown = scripted.clone();
    let answer = std::thread::spawn(move || shown.show(&pay(), &|| {}));
    assert!(scripted.wait_shown(1, Duration::from_secs(10)));
    assert!(!scripted.wait_shown(2, Duration::from_millis(20)));
    scripted.press(1, ALLOW);
    assert_eq!(answer.join().unwrap(), ALLOW);
}

#[test]
fn the_scripted_dialog_reports_on_screen_before_the_walk_sees_the_show() {
    let scripted = Arc::new(Scripted::new(Duration::from_secs(10)));
    let reported = Arc::new(Mutex::new(false));
    let (shown, flag) = (scripted.clone(), reported.clone());
    let answer = std::thread::spawn(move || {
        shown.show(&pay(), &|| {
            std::thread::sleep(Duration::from_millis(50));
            *flag.lock().unwrap() = true;
        })
    });
    assert!(scripted.wait_shown(1, Duration::from_secs(10)));
    assert!(*reported.lock().unwrap());
    scripted.press(1, REJECT);
    assert_eq!(answer.join().unwrap(), REJECT);
}

#[test]
fn a_press_answers_its_own_show_and_no_later_one() {
    let scripted = Scripted::new(Duration::from_millis(50));
    assert_eq!(scripted.show(&pay(), &|| {}), REJECT, "show 1 has no press");
    scripted.press(1, ALLOW);
    assert_eq!(scripted.show(&pay(), &|| {}), REJECT, "show 2 has no press");
    scripted.press(4, ALLOW);
    assert_eq!(scripted.show(&pay(), &|| {}), REJECT, "show 3 has no press");
    assert_eq!(scripted.show(&pay(), &|| {}), ALLOW, "show 4 has its own");
}

#[test]
fn a_show_once_more_after_an_early_answer_needs_a_press_of_its_own() {
    let scripted = Scripted::new(Duration::from_millis(50));
    scripted.press(1, ALLOW);
    assert_eq!(scripted.show(&pay(), &|| {}), ALLOW);
    assert_eq!(scripted.show(&pay(), &|| {}), REJECT);
    assert_eq!(scripted.cards().len(), 2);
}
