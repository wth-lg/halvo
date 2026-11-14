#[path = "hosted/schedule.rs"]
mod schedule;

use halvo_coord::types::Limits;
use schedule::{due, slipped, From};
use std::time::{Duration, Instant};

fn arm() -> Duration {
    Duration::from_millis(Limits::PRODUCT.arm_ms)
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Drawn delays of the main queue in milliseconds, from 0 to 399, by a fixed linear congruence.
fn delays(count: usize) -> Vec<u64> {
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    (0..count)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) % 400
        })
        .collect()
}

#[test]
fn schedule_a_press_timed_from_the_press_before_counts_from_when_that_press_ran() {
    let report = Instant::now();
    for first_delay in delays(200) {
        let first_ran = due(report, None, From::Report, 300) + ms(first_delay);
        let second = due(report, Some(first_ran), From::Previous, 250);
        assert_eq!(
            second.saturating_duration_since(first_ran),
            ms(250),
            "the first press ran {first_delay} ms late"
        );
    }
}

#[test]
fn schedule_the_gap_slips_only_when_the_second_press_itself_ran_late_by_250_ms_or_more() {
    let report = Instant::now();
    let drawn = delays(400);
    for pair in drawn.chunks(2) {
        let (first_delay, second_delay) = (pair[0], pair[1]);
        let first_ran = due(report, None, From::Report, 300) + ms(first_delay);
        let second_ran = due(report, Some(first_ran), From::Previous, 250) + ms(second_delay);
        let line = slipped(first_ran, second_ran, arm());
        assert_eq!(
            line.is_some(),
            250 + second_delay >= Limits::PRODUCT.arm_ms,
            "delays {first_delay} and {second_delay} ms"
        );
        if let Some(line) = line {
            assert!(line.starts_with("the schedule slipped"), "{line}");
        }
    }
}

#[test]
fn schedule_a_press_timed_from_the_report_or_with_no_press_before_counts_from_the_report() {
    let report = Instant::now();
    let earlier = report + ms(40);
    assert_eq!(
        due(report, Some(earlier), From::Report, 300),
        report + ms(300)
    );
    assert_eq!(due(report, None, From::Previous, 250), report + ms(250));
}

#[test]
fn schedule_a_gap_just_under_arm_holds_and_one_at_arm_slips() {
    let first = Instant::now();
    assert_eq!(slipped(first, first + arm() - ms(1), arm()), None);
    assert!(slipped(first, first + arm(), arm()).is_some());
    assert_eq!(
        slipped(first + ms(10), first, arm()),
        None,
        "a clock read earlier"
    );
}
