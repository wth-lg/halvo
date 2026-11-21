#[path = "hosted/activation.rs"]
mod activation;
#[path = "hosted/reading.rs"]
mod reading;

use activation::{begun, late, run_loop, wait, Begun, Waited, LATE, LATE_MOST, RUN_LOOP_BOUND};
use std::cell::{Cell, RefCell};
use std::ffi::OsStr;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

const EVERY: Duration = Duration::from_secs(1);
const LIMIT: Duration = Duration::from_secs(20);

/// Drawn counts of asks before the app reads active, from 0 to 29, by a fixed congruence.
fn draws(count: usize) -> Vec<u32> {
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    (0..count)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 33) % 30) as u32
        })
        .collect()
}

/// Runs the wait over an app that reads active at its read number `after` (0 for the first read),
/// or never for `None`: the result, the asks made and the time passed.
fn run(after: Option<u32>) -> (Waited, u32, Duration) {
    let asks = Cell::new(0u32);
    let reads = Cell::new(0u32);
    let passed = Cell::new(Duration::ZERO);
    let result = wait(
        || asks.set(asks.get() + 1),
        || {
            let n = reads.get();
            reads.set(n + 1);
            after.is_some_and(|after| n >= after)
        },
        |d| passed.set(passed.get() + d),
        EVERY,
        LIMIT,
    );
    (result, asks.get(), passed.get())
}

#[test]
fn activation_an_app_active_at_once_is_asked_once_and_waits_nothing() {
    assert_eq!(
        run(Some(0)),
        (Waited::Active { asks: 1 }, 1, Duration::ZERO)
    );
}

#[test]
fn activation_an_app_active_after_n_asks_is_asked_every_second_until_it_is() {
    for after in draws(200) {
        let (result, asks, passed) = run(Some(after));
        if after <= 20 {
            assert_eq!(result, Waited::Active { asks: after + 1 }, "after {after}");
            assert_eq!(asks, after + 1, "after {after}");
            assert_eq!(passed, EVERY * after, "after {after}");
        } else {
            assert_eq!(result, Waited::Inactive { asks: 21 }, "after {after}");
            assert_eq!(passed, LIMIT, "after {after}");
        }
    }
}

#[test]
fn activation_an_app_never_active_is_inactive_after_twenty_seconds_and_21_asks() {
    assert_eq!(run(None), (Waited::Inactive { asks: 21 }, 21, LIMIT));
}

#[test]
fn late_unset_holds_nothing_back() {
    assert_eq!(late(None), Ok(None));
}

#[test]
fn late_a_whole_number_of_milliseconds_from_1_to_the_most_is_the_hold() {
    for ms in [1, 2_000, 12_000, LATE_MOST] {
        assert_eq!(
            late(Some(OsStr::new(&ms.to_string()))),
            Ok(Some(Duration::from_millis(ms))),
            "{ms}"
        );
    }
}

#[test]
fn late_every_other_value_is_refused_by_a_line_that_names_the_setting_and_runs_no_case() {
    let most_and_one = (LATE_MOST + 1).to_string();
    for value in [
        "",
        "0",
        &most_and_one,
        "18446744073709551616",
        "-5",
        "+5",
        "1.5",
        "2000ms",
        " 2000",
        "2000\n",
        "abc",
    ] {
        let refused = late(Some(OsStr::new(value))).expect_err(value);
        assert_eq!(
            refused,
            format!("start FAIL: {LATE} is set and is not a whole number of milliseconds from 1 to {LATE_MOST}; no case ran"),
            "{value:?}"
        );
    }
}

#[cfg(unix)]
#[test]
fn late_a_value_that_is_no_text_is_refused() {
    use std::os::unix::ffi::OsStrExt;
    assert!(late(Some(OsStr::from_bytes(&[0x32, 0xff]))).is_err());
}

#[cfg(windows)]
#[test]
fn late_a_value_that_is_no_text_is_refused() {
    use std::os::windows::ffi::OsStringExt;
    let value = std::ffi::OsString::from_wide(&[0x32, 0xd800]);
    assert!(late(Some(&value)).is_err());
}

#[test]
fn late_the_setting_is_its_own_and_none_that_check_46_takes_out_or_a_target_sets() {
    assert_eq!(LATE, "HALVO_HOSTED_LATE_RUN_MS");
    for other in [
        "GUARD_HOSTED_SCHEDULE",
        "GUARD_HOSTED_TALLY",
        "DIALOG_HOSTED_TEXT_SIZE",
    ] {
        assert_ne!(LATE, other);
    }
}

/// The bounds that the plain tests of `run_loop` hand in: the targets' own and one that is not it.
const BOUNDS: [Duration; 2] = [RUN_LOOP_BOUND, Duration::from_millis(7_001)];

#[test]
fn run_loop_the_sign_is_waited_for_with_the_bound_and_the_time_waited_is_kept() {
    for bound in BOUNDS {
        let given = RefCell::new(Vec::new());
        let result = run_loop(
            |limit| {
                given.borrow_mut().push(limit);
                Ok(())
            },
            || Duration::from_millis(2_003),
            bound,
        );
        assert_eq!(given.into_inner(), vec![bound], "{bound:?}");
        assert_eq!(
            result,
            Begun::After {
                waited: Duration::from_millis(2_003)
            },
            "{bound:?}"
        );
    }
}

#[test]
fn run_loop_with_no_sign_within_the_bound_has_not_begun() {
    for bound in BOUNDS {
        let given = RefCell::new(Vec::new());
        let result = run_loop(
            |limit| {
                given.borrow_mut().push(limit);
                Err(RecvTimeoutError::Timeout)
            },
            || bound,
            bound,
        );
        assert_eq!(given.into_inner(), vec![bound], "{bound:?}");
        assert_eq!(result, Begun::Not { bound }, "{bound:?}");
    }
}

#[test]
fn run_loop_a_channel_that_closed_with_no_sign_is_closed_after_the_time_waited_and_not_the_bound() {
    for bound in BOUNDS {
        let given = RefCell::new(Vec::new());
        let result = run_loop(
            |limit| {
                given.borrow_mut().push(limit);
                Err(RecvTimeoutError::Disconnected)
            },
            || Duration::from_millis(37),
            bound,
        );
        assert_eq!(given.into_inner(), vec![bound], "{bound:?}");
        assert_eq!(
            result,
            Begun::Closed {
                waited: Duration::from_millis(37)
            },
            "{bound:?}"
        );
    }
}

/// The time that the test's own clock gives the wait at every read.
const CLOCK: Duration = Duration::from_millis(2_003);

/// How the sign reaches the wait over a real channel.
#[derive(Clone, Copy)]
enum Sign {
    /// Another thread sends it and has ended before the wait begins.
    Sent,
    /// No sign comes, and the sender lives until the wait has ended.
    Never,
    /// The sender is dropped before the wait begins.
    Dropped,
}

/// Waits as the hosted targets do, on a real channel whose sign comes as `sign` says, with
/// `bound` and the test's clock `CLOCK`: the result, each limit that the wait was given, and the
/// waits and clock reads in their order.
fn over_a_channel(sign: Sign, bound: Duration) -> (Begun, Vec<Duration>, Vec<&'static str>) {
    let (ready, runs) = std::sync::mpsc::channel::<()>();
    let kept = match sign {
        Sign::Sent => {
            let sent = std::thread::spawn(move || ready.send(()).is_ok())
                .join()
                .expect("the sending thread ended");
            assert!(sent, "the sign was sent");
            None
        }
        Sign::Never => Some(ready),
        Sign::Dropped => {
            drop(ready);
            None
        }
    };
    let limits = RefCell::new(Vec::new());
    let steps = RefCell::new(Vec::new());
    let result = run_loop(
        |limit| {
            limits.borrow_mut().push(limit);
            steps.borrow_mut().push("wait");
            runs.recv_timeout(limit)
        },
        || {
            steps.borrow_mut().push("clock");
            CLOCK
        },
        bound,
    );
    drop(kept);
    (result, limits.into_inner(), steps.into_inner())
}

#[test]
fn run_loop_over_a_channel_goes_on_at_the_sign_and_ends_at_the_bound_without_it() {
    assert_eq!(
        over_a_channel(Sign::Sent, RUN_LOOP_BOUND),
        (
            Begun::After { waited: CLOCK },
            vec![RUN_LOOP_BOUND],
            vec!["wait", "clock"]
        )
    );
    let short = Duration::from_millis(5);
    assert_eq!(
        over_a_channel(Sign::Never, short),
        (Begun::Not { bound: short }, vec![short], vec!["wait"])
    );
}

#[test]
fn run_loop_over_a_channel_whose_sender_was_dropped_is_closed_and_not_the_bound() {
    assert_eq!(
        over_a_channel(Sign::Dropped, RUN_LOOP_BOUND),
        (
            Begun::Closed { waited: CLOCK },
            vec![RUN_LOOP_BOUND],
            vec!["wait", "clock"]
        )
    );
}

#[test]
fn begun_after_the_sign_the_cases_run_and_the_line_says_when_it_came() {
    for (target, waited, line) in [
        (
            "guard_hosted",
            2_003,
            "guard_hosted start: the test runtime's run loop gave its sign after 2003 ms, and the first show goes out after it",
        ),
        (
            "dialog_hosted",
            2_003,
            "dialog_hosted start: the test runtime's run loop gave its sign after 2003 ms, and the first show goes out after it",
        ),
        (
            "guard_hosted",
            0,
            "guard_hosted start: the test runtime's run loop gave its sign after 0 ms, and the first show goes out after it",
        ),
    ] {
        assert_eq!(
            begun(
                target,
                Begun::After {
                    waited: Duration::from_millis(waited)
                }
            ),
            (line.to_string(), true),
            "{target} {waited}"
        );
    }
}

#[test]
fn begun_with_no_sign_no_case_runs_and_the_line_names_the_sign_and_the_bound() {
    for (target, bound, line) in [
        (
            "dialog_hosted",
            RUN_LOOP_BOUND,
            "dialog_hosted start FAIL: the test runtime's run loop gave no sign that it runs within 10 s, so a show would run off the main thread; no case ran",
        ),
        (
            "guard_hosted",
            RUN_LOOP_BOUND,
            "guard_hosted start FAIL: the test runtime's run loop gave no sign that it runs within 10 s, so a show would run off the main thread; no case ran",
        ),
        (
            "guard_hosted",
            Duration::from_secs(7),
            "guard_hosted start FAIL: the test runtime's run loop gave no sign that it runs within 7 s, so a show would run off the main thread; no case ran",
        ),
    ] {
        assert_eq!(
            begun(target, Begun::Not { bound }),
            (line.to_string(), false),
            "{target} {bound:?}"
        );
    }
}

#[test]
fn begun_a_closed_channel_runs_no_case_and_the_line_names_the_closed_channel_and_the_time() {
    for (target, waited, line) in [
        (
            "guard_hosted",
            0,
            "guard_hosted start FAIL: the channel of the run loop's sign closed after 0 ms with no sign: the main thread dropped its end without the sign that the test runtime's run loop runs; no case ran",
        ),
        (
            "dialog_hosted",
            37,
            "dialog_hosted start FAIL: the channel of the run loop's sign closed after 37 ms with no sign: the main thread dropped its end without the sign that the test runtime's run loop runs; no case ran",
        ),
    ] {
        assert_eq!(
            begun(
                target,
                Begun::Closed {
                    waited: Duration::from_millis(waited)
                }
            ),
            (line.to_string(), false),
            "{target} {waited}"
        );
    }
}

#[test]
fn run_loop_the_bound_is_ten_seconds_and_the_longest_hold_a_minute() {
    assert_eq!(RUN_LOOP_BOUND, Duration::from_secs(10));
    assert_eq!(LATE_MOST, 60_000);
}

/// Runs the start over an app that reads active at its read number `after`, or never: the result
/// and the steps in the order in which they came.
fn started(after: Option<u32>) -> (Waited, Vec<&'static str>) {
    let steps = std::cell::RefCell::new(Vec::new());
    let reads = Cell::new(0u32);
    let result = activation::start(
        || steps.borrow_mut().push("front"),
        || steps.borrow_mut().push("ask"),
        || {
            steps.borrow_mut().push("read");
            let n = reads.get();
            reads.set(n + 1);
            after.is_some_and(|after| n >= after)
        },
        |_| steps.borrow_mut().push("pass"),
        || steps.borrow_mut().push("out"),
        EVERY,
        LIMIT,
    );
    (result, steps.into_inner())
}

#[test]
fn activation_the_start_brings_the_panel_forward_once_before_the_first_ask_and_out_after_the_last_read(
) {
    for after in [Some(0), Some(3), Some(20), None] {
        let (result, steps) = started(after);
        let label = format!("{after:?}: {steps:?}");
        assert_eq!(steps.first(), Some(&"front"), "{label}");
        assert_eq!(
            steps.iter().filter(|s| **s == "front").count(),
            1,
            "{label}"
        );
        assert_eq!(steps.get(1), Some(&"ask"), "{label}");
        assert_eq!(steps.last(), Some(&"out"), "{label}");
        assert_eq!(steps.iter().filter(|s| **s == "out").count(), 1, "{label}");
        let reads = steps.iter().filter(|s| **s == "read").count() as u32;
        match after {
            Some(n) => {
                assert_eq!(result, Waited::Active { asks: n + 1 }, "{label}");
                assert_eq!(reads, n + 1, "{label}");
            }
            None => assert_eq!(result, Waited::Inactive { asks: 21 }, "{label}"),
        }
    }
}

/// Entries of the main run loop's description as this Mac's Foundation prints them, for a timer
/// of 0.05 s that has fired and for one that never fired; the class is the dialog's.
const FIRED: &str = "\t0 : <CFRunLoopTimer 0x1032a4670 [0x1f15f8fc8]>{valid = Yes, firing = No, interval = 0.05, tolerance = 0, next fire date = 816400679 (0.0422188044 @ 12520909562612), callout = (NSTimer) [HalvoDialogHold reading:] (0x18ff1f3e8 / 0x102c05450) (/x/guard_hosted), context = <CFRunLoopTimer context 0x1032a4630>}";
const NEVER: &str = "\t2 : <CFRunLoopTimer 0x1050a84a0 [0x1f15f8fc8]>{valid = Yes, firing = No, interval = 0.05, tolerance = 0, next fire date = 816400687 (-0.960178971 @ 12521092795058), callout = (NSTimer) [HalvoDialogHold reading:] (0x18ff1f3e8 / 0x104bc9420) (/x/guard_hosted), context = <CFRunLoopTimer context 0x1032a4630>}";

#[test]
fn reading_a_timer_that_fired_has_run_and_one_that_never_fired_has_not() {
    let text = |entry: &str| {
        format!("<CFRunLoop 0x1 [0x2]>{{wakeup port = 0x3}}\n\ttimers = <CFArray>{{\n{entry}\n}}\n")
    };
    assert_eq!(
        reading::timer(&text(FIRED), 0.05),
        reading::Timer::Valid {
            next: 0.0422188044,
            run: true
        }
    );
    assert_eq!(
        reading::timer(&text(NEVER), 0.05),
        reading::Timer::Valid {
            next: -0.960178971,
            run: false
        }
    );
    assert!(reading::words(reading::timer(&text(FIRED), 0.05)).contains("has run"));
    assert!(reading::words(reading::timer(&text(NEVER), 0.05)).contains("has not run"));
}

#[test]
fn reading_no_entry_of_the_dialog_is_absent_and_a_broken_one_is_unread() {
    assert_eq!(
        reading::timer("<CFRunLoop 0x1>{}", 0.05),
        reading::Timer::Absent
    );
    let other = FIRED.replace("HalvoDialogHold", "GuardHostedWatch");
    assert_eq!(reading::timer(&other, 0.05), reading::Timer::Absent);
    let invalid = FIRED.replace("valid = Yes", "valid = No");
    assert_eq!(reading::timer(&invalid, 0.05), reading::Timer::Absent);
    let broken = FIRED.replace(" (0.0422188044 @", " (x @");
    assert_eq!(reading::timer(&broken, 0.05), reading::Timer::Unread);
}
