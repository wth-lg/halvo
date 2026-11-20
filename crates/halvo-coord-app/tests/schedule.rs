#[path = "hosted/schedule.rs"]
mod schedule;

use halvo_coord::types::Limits;
use schedule::{
    due, line, next, retries_of, run_of, slipped, tally, times_of, verdict, From, Mode, Next, Run,
    ATTEMPTS, FLOOR_MACOS, FLOOR_WINDOWS, TOO_FEW,
};
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

/// The end line of one run of the target, with its failed cases and its slipped schedules.
fn end(failed: u64, slipped: u64) -> String {
    format!("guard_hosted end: cases ran 7 of 7, failed {failed}, schedule slipped {slipped}, helped 0, moments at a press 0, retries 0")
}

/// The lines of a run whose schedule case slipped with `gap` ms between its two presses.
fn slipped_run(gap: u64) -> String {
    format!(
        "guard_hosted case macos allow_click_at_300ms_and_250ms_after_it SLIPPED: the schedule slipped: the press ran {gap} ms after the press before it, and arm_ms is 500; not a pass and not a miss\n{}",
        end(0, 1)
    )
}

/// A loop of `kept` runs that kept the schedule and passed, then `slips` runs that slipped, then
/// `failed` runs that kept it and failed.
fn runs(kept: usize, slips: usize, failed: usize) -> Vec<String> {
    let mut texts = vec![end(0, 0); kept];
    texts.extend((0..slips).map(|n| slipped_run(500 + n as u64)));
    texts.extend(std::iter::repeat_n(end(1, 0), failed));
    texts
}

#[test]
fn schedule_once_a_slipped_case_runs_again_until_its_third_run_and_then_fails_by_name() {
    assert_eq!(ATTEMPTS, 3);
    assert_eq!(next(Mode::Once, 1, true), Next::Again);
    assert_eq!(next(Mode::Once, 2, true), Next::Again);
    assert_eq!(next(Mode::Once, 3, true), Next::TooFew);
    for attempt in 1..=3 {
        assert_eq!(
            next(Mode::Once, attempt, false),
            Next::Done,
            "run {attempt}"
        );
    }
    assert_eq!(TOO_FEW, "too few runs kept the schedule");
}

#[test]
fn schedule_in_a_loop_a_slipped_case_is_counted_apart_and_runs_once() {
    assert_eq!(next(Mode::Loop, 1, true), Next::Slipped);
    assert_eq!(next(Mode::Loop, 1, false), Next::Done);
}

#[test]
fn schedule_a_run_is_read_by_its_end_line_and_its_slipped_case_line() {
    assert_eq!(run_of(&end(0, 0)), (Run::Kept, vec![]));
    assert_eq!(run_of(&end(1, 0)), (Run::KeptFailed, vec![]));
    assert_eq!(run_of(&slipped_run(528)), (Run::Slipped, vec![528]));
    assert_eq!(run_of(&end(1, 1)), (Run::Missed, vec![]));
    assert_eq!(
        run_of("guard_hosted case macos x PASS"),
        (Run::Missed, vec![])
    );
    assert_eq!(
        run_of("guard_hosted end: cases ran 7 of 7, failed 0, helped 0, moments at a press 0, retries 0"),
        (Run::Missed, vec![]),
        "an end line with no count of slipped schedules"
    );
    for (line, kind) in [
        ("guard_hosted end: cases ran 7 of 7, failed 0, schedule slipped 0, helped 1, moments at a press 0, retries 0", Run::KeptFailed),
        ("guard_hosted end: cases ran 7 of 7, failed 0, schedule slipped 0, helped 0, moments at a press 1, retries 0", Run::KeptFailed),
        ("guard_hosted end: cases ran 6 of 7, failed 0, schedule slipped 0, helped 0, moments at a press 0, retries 0", Run::KeptFailed),
        ("guard_hosted end: cases ran 7 of 7, failed 0, schedule slipped 0, helped 0, moments at a press 0, retries 0, FAIL: not every case ran", Run::KeptFailed),
        ("guard_hosted end: cases ran 8 of 8, failed 0, schedule slipped 0, helped 0, moments at a press 0, retries 2", Run::Kept),
        ("guard_hosted end: cases ran 8 of 8, failed 0, schedule slipped 1, helped 1, moments at a press 0, retries 0", Run::Missed),
    ] {
        assert_eq!(run_of(line).0, kind, "{line}");
    }
}

#[test]
fn schedule_a_loop_passes_at_its_floor_and_fails_by_name_one_run_below_it() {
    for ((count, floor), system) in [(FLOOR_MACOS, "macos"), (FLOOR_WINDOWS, "windows")] {
        let at = tally(&runs(floor, count - floor, 0));
        assert_eq!(
            verdict(&at, (count, floor)),
            Vec::<String>::new(),
            "{system}"
        );
        let below = tally(&runs(floor - 1, count - floor + 1, 0));
        let failures = verdict(&below, (count, floor));
        assert_eq!(failures.len(), 1, "{system}: {failures:?}");
        assert!(failures[0].starts_with(TOO_FEW), "{system}: {failures:?}");
    }
    assert_eq!(FLOOR_MACOS, (60, 45));
    assert_eq!(FLOOR_WINDOWS, (20, 18));
}

#[test]
fn schedule_a_slipped_run_is_neither_a_pass_nor_a_miss() {
    let t = tally(&runs(53, 7, 0));
    assert_eq!((t.kept, t.slipped.len(), t.missed.len()), (53, 7, 0));
    assert_eq!(verdict(&t, FLOOR_MACOS), Vec::<String>::new());
    let t = tally(&runs(44, 16, 0));
    assert_eq!(
        t.kept, 44,
        "a slipped run is not a run that kept the schedule"
    );
    assert!(verdict(&t, FLOOR_MACOS)[0].starts_with(TOO_FEW));
}

#[test]
fn schedule_a_run_that_kept_the_schedule_and_failed_fails_the_loop_by_its_number() {
    let t = tally(&runs(59, 0, 1));
    assert_eq!(t.kept_failed, vec![60]);
    assert_eq!(
        verdict(&t, FLOOR_MACOS),
        vec!["a run that kept the schedule failed: run 60".to_string()]
    );
    let t = tally(&runs(19, 0, 1));
    assert_eq!(verdict(&t, FLOOR_WINDOWS).len(), 1);
}

#[test]
fn schedule_a_loop_of_another_length_or_with_a_missed_run_fails() {
    let t = tally(&runs(59, 0, 0));
    assert_eq!(
        verdict(&t, FLOOR_MACOS),
        vec!["the loop ran 59 runs, not 60".to_string()]
    );
    let mut texts = runs(59, 0, 0);
    texts.insert(3, "no end line".into());
    let t = tally(&texts);
    assert_eq!(t.missed, vec![4]);
    assert_eq!(verdict(&t, FLOOR_MACOS).len(), 1);
}

#[test]
fn schedule_a_loop_over_drawn_delays_keeps_the_runs_whose_gap_stayed_under_arm() {
    let drawn = delays(600);
    for chunk in drawn.chunks(60) {
        let texts: Vec<String> = chunk
            .iter()
            .map(|delay| {
                let gap = 250 + delay;
                if slipped(Instant::now(), Instant::now() + ms(gap), arm()).is_some() {
                    slipped_run(gap)
                } else {
                    end(0, 0)
                }
            })
            .collect();
        let kept = chunk
            .iter()
            .filter(|delay| 250 + **delay < Limits::PRODUCT.arm_ms)
            .count();
        let t = tally(&texts);
        assert_eq!(t.kept, kept);
        assert_eq!(t.slipped.len(), 60 - kept);
        assert!(t
            .slipped
            .iter()
            .all(|(_, gaps)| gaps.len() == 1 && gaps[0] >= 500));
        assert_eq!(
            verdict(&t, FLOOR_MACOS).is_empty(),
            kept >= 45,
            "kept {kept}"
        );
    }
}

#[test]
fn schedule_the_loop_line_gives_the_counts_and_each_gap() {
    let t = tally(&runs(2, 1, 1));
    assert_eq!(
        line(&t, (4, 3)),
        "guard_hosted loop: runs 4 of 4; kept the schedule 3 (the floor is 3): passed 2, failed 1; schedule slipped 1 (run 3 gap 500 ms); missed 0; retried an answering press 0"
    );
}

/// The lines of a run of the schedule case with its first two presses at `first` and `second`
/// ms, `retries` retries of its answering press, and the end line of `failed` and `slips`.
fn timed_run(first: u64, second: u64, retries: u64, failed: u64, slips: u64) -> String {
    format!(
        "guard_hosted schedule macos allow_click_at_300ms_and_250ms_after_it: the press before ran at +{first} ms, this one at +{second} ms, gap {} ms\n\
         guard_hosted schedule macos allow_click_at_300ms_and_250ms_after_it: the press before ran at +{second} ms, this one at +{} ms, gap 750 ms\n\
         guard_hosted end: cases ran 7 of 7, failed {failed}, schedule slipped {slips}, helped 0, moments at a press 0, retries {retries}",
        second - first,
        second + 750
    )
}

#[test]
fn schedule_each_run_of_a_loop_gives_its_first_two_press_times_and_their_gap_with_its_kind() {
    assert_eq!(
        times_of(&timed_run(1_317, 1_655, 0, 0, 0)),
        Some((1_317, 1_655, 338))
    );
    assert_eq!(times_of(&end(0, 0)), None);
    let texts = [
        timed_run(1_317, 1_655, 0, 0, 0),
        timed_run(1_300, 1_841, 0, 0, 1),
        timed_run(1_300, 1_560, 0, 1, 0),
        end(0, 0),
    ];
    let t = tally(&texts);
    assert_eq!(
        t.lines,
        [
            "guard_hosted loop run 1: the first press at +1317 ms, the second at +1655 ms, gap 338 ms; kept the schedule and passed",
            "guard_hosted loop run 2: the first press at +1300 ms, the second at +1841 ms, gap 541 ms; schedule slipped",
            "guard_hosted loop run 3: the first press at +1300 ms, the second at +1560 ms, gap 260 ms; kept the schedule and failed",
            "guard_hosted loop run 4: no schedule line; kept the schedule and passed",
        ]
    );
}

#[test]
fn schedule_a_run_that_retried_its_answering_press_is_counted_apart_and_still_passes() {
    assert_eq!(retries_of(&timed_run(1, 300, 2, 0, 0)), 2);
    assert_eq!(retries_of("no end line"), 0);
    let mut texts = runs(58, 0, 0);
    texts.push(timed_run(1, 300, 1, 0, 0));
    texts.push(timed_run(1, 300, 3, 0, 0));
    let t = tally(&texts);
    assert_eq!((t.kept, t.retried.clone()), (60, vec![59, 60]));
    assert_eq!(verdict(&t, FLOOR_MACOS), Vec::<String>::new());
    assert!(
        line(&t, FLOOR_MACOS).ends_with("; retried an answering press 2 (run 59, 60)"),
        "{}",
        line(&t, FLOOR_MACOS)
    );
}

#[test]
fn schedule_the_loop_line_says_passed_of_runs_that_kept_the_schedule_alone() {
    let t = tally(&runs(54, 6, 0));
    let text = line(&t, FLOOR_MACOS);
    assert!(
        text.contains(
            "kept the schedule 54 (the floor is 45): passed 54, failed 0; schedule slipped 6"
        ),
        "{text}"
    );
    assert!(!text.contains("passed 60"), "{text}");
}
