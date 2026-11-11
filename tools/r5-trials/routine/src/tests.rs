use crate::files::{parse_seeds, parse_targets, Refusal};
use crate::rule::{routine_step, walk, window, Mark, RoutineState, Setting};
use crate::train::{grid, grid_days, train, Cadence, Cell, Draws, Kind, TRAIN_DAYS};
use crate::trains_of;
use crate::trial::{count_cell, detected, false_stale, judge, results, verdict, CellCount, Lines};

const REPO_SEEDS: &str = include_str!("../seeds.txt");
const REPO_TARGETS: &str = include_str!("../../../../docs/r5/targets.toml");

const TWO_NINE: Setting = Setting::new(2, 0, 2);
const LINES: Lines = Lines {
    false_stale_max: 30,
    detected_min: 45,
    detect_days: 120,
};

fn cell(cadence: Cadence, jitter: u32, miss_pct: u32) -> Cell {
    *grid()
        .iter()
        .find(|c| c.cadence == cadence && c.jitter == jitter && c.miss_pct == miss_pct)
        .expect("a cell of the grid")
}

fn step(setting: Setting, days: &[u32], today: u32) -> RoutineState {
    routine_step(setting, None, days, today)
}

#[test]
fn period_is_null_with_fewer_than_three_days() {
    let s = step(TWO_NINE, &[1, 8], 100);
    assert_eq!((s.period, s.next, s.missed), (None, None, 0));
    assert_eq!(s.mark, Mark::None);
}

#[test]
fn period_is_the_median_gap() {
    assert_eq!(step(TWO_NINE, &[1, 8, 15, 40], 40).period, Some(7));
}

#[test]
fn period_takes_the_lower_middle_of_an_even_number_of_gaps() {
    assert_eq!(step(TWO_NINE, &[1, 3, 13, 33, 63], 63).period, Some(10));
}

#[test]
fn period_and_first_keep_prev_with_fewer_days() {
    let prev = step(TWO_NINE, &[1, 8, 15], 15);
    let s = routine_step(TWO_NINE, Some(&prev), &[100, 107], 107);
    assert_eq!(
        (s.seen, s.first, s.last, s.period, s.next),
        (2, Some(100), Some(107), Some(7), Some(114))
    );
}

#[test]
fn first_and_last_keep_prev_when_days_are_empty() {
    let prev = step(TWO_NINE, &[1, 8, 15], 15);
    let s = routine_step(TWO_NINE, Some(&prev), &[], 200);
    assert_eq!((s.seen, s.first, s.last), (0, Some(1), Some(15)));
}

#[test]
fn first_cycle_counts_as_missed_the_day_after_next_plus_tolerance() {
    let days = [1, 8, 15];
    assert_eq!(step(TWO_NINE, &days, 24).missed, 0);
    assert_eq!(step(TWO_NINE, &days, 25).missed, 1);
    assert_eq!(step(TWO_NINE, &days, 33).missed, 1);
    assert_eq!(step(TWO_NINE, &days, 34).missed, 2);
}

#[test]
fn missed_is_zero_when_today_is_at_or_before_last() {
    let prev = step(TWO_NINE, &[30, 40, 50], 50);
    let s = routine_step(TWO_NINE, Some(&prev), &[], 40);
    assert_eq!((s.missed, s.mark), (0, Mark::None));
}

#[test]
fn mark_is_stale_when_missed_reaches_miss_cycles() {
    let days = [1, 8, 15];
    assert_eq!(step(TWO_NINE, &days, 33).mark, Mark::None);
    assert_eq!(step(TWO_NINE, &days, 34).mark, Mark::Stale);
    assert_eq!(step(Setting::new(3, 0, 2), &days, 34).mark, Mark::None);
}

#[test]
fn mark_is_never_stale_with_miss_cycles_zero() {
    assert_eq!(
        step(Setting::new(0, 0, 0), &[1, 8, 15], 700).mark,
        Mark::None
    );
}

#[test]
fn mark_waits_for_the_floor() {
    let days = [1, 8, 15];
    let floor = Setting::new(2, 60, 2);
    assert_eq!(step(floor, &days, 74).mark, Mark::None);
    assert_eq!(step(floor, &days, 75).mark, Mark::Stale);
}

#[test]
fn since_is_today_when_the_mark_turns_and_null_while_always_none() {
    let a = step(TWO_NINE, &[1, 8, 15], 20);
    assert_eq!(a.since, None);
    let b = routine_step(TWO_NINE, Some(&a), &[1, 8, 15], 34);
    assert_eq!(b.since, Some(34));
    let c = routine_step(TWO_NINE, Some(&b), &[1, 8, 15], 35);
    assert_eq!(c.since, Some(34));
    let d = routine_step(TWO_NINE, Some(&c), &[1, 8, 15, 36], 36);
    assert_eq!((d.mark, d.since), (Mark::None, Some(36)));
}

#[test]
fn window_holds_the_occurrences_of_the_last_93_days() {
    let days = [1, 10, 100, 102];
    assert_eq!(window(&days, 93), &[1, 10]);
    assert_eq!(window(&days, 94), &[10]);
    assert_eq!(window(&days, 101), &[10, 100]);
    assert_eq!(window(&days, 102), &[10, 100, 102]);
    assert_eq!(window(&days, 103), &[100, 102]);
}

#[test]
fn grid_days_start_on_day_1_and_stand_one_cadence_apart() {
    let days = grid_days(Cadence::Seven, None);
    assert_eq!(&days[..3], &[1, 8, 15]);
    assert_eq!((days.len(), days.last()), (105, Some(&729)));
}

#[test]
fn a_shift_keeps_the_first_grid_day_at_or_after_the_drawn_day() {
    let days = grid_days(Cadence::Fourteen, Some(200));
    let at = days.iter().position(|&d| d == 211).expect("211 stands");
    assert_eq!(&days[at - 1..at + 3], &[197, 211, 241, 271]);
    let days = grid_days(Cadence::Seven, Some(204));
    let at = days.iter().position(|&d| d == 204).expect("204 stands");
    assert_eq!(&days[at - 1..at + 3], &[197, 204, 218, 232]);
}

#[test]
fn thirty_shifts_to_fourteen() {
    let days = grid_days(Cadence::Thirty, Some(300));
    let at = days.iter().position(|&d| d == 301).expect("301 stands");
    assert_eq!(&days[at - 1..at + 3], &[271, 301, 315, 329]);
}

#[test]
fn trains_are_ascending_distinct_and_within_the_train() {
    for c in grid() {
        for t in trains_of(&c) {
            assert!(t.days.windows(2).all(|w| w[0] < w[1]));
            assert!(t.days.iter().all(|&d| (1..=TRAIN_DAYS).contains(&d)));
            assert_eq!(t, train(&c, t.number));
        }
    }
}

#[test]
fn trains_are_numbered_same_then_stop_then_shift() {
    let c = cell(Cadence::Seven, 0, 0);
    let kinds: Vec<Kind> = [1, 100, 101, 150, 151, 200]
        .iter()
        .map(|&n| train(&c, n).kind)
        .collect();
    use Kind::*;
    assert_eq!(kinds, [Same, Same, Stop, Stop, Shift, Shift]);
}

#[test]
fn a_cadence_7_train_leaves_out_the_two_grid_days_after_its_pause_day() {
    let c = cell(Cadence::Seven, 0, 0);
    let pause = Draws::new(&c, 1).from_to(60, 180);
    let mut want = grid_days(Cadence::Seven, None);
    let at = want
        .iter()
        .position(|&g| g > pause)
        .expect("a grid day after the pause");
    want.drain(at..at + 2);
    assert_eq!(train(&c, 1).days, want);
}

#[test]
fn a_stop_train_has_no_occurrence_after_its_drawn_day() {
    let c = cell(Cadence::Fourteen, 0, 0);
    let stop = Draws::new(&c, 101).from_to(200, 500);
    let want: Vec<u32> = grid_days(Cadence::Fourteen, None)
        .into_iter()
        .filter(|&g| g <= stop)
        .collect();
    assert_eq!(train(&c, 101).days, want);
}

#[test]
fn a_shift_train_of_a_still_cell_is_its_shifted_grid() {
    let c = cell(Cadence::Thirty, 0, 0);
    let from = Draws::new(&c, 151).from_to(200, 500);
    assert_eq!(train(&c, 151).days, grid_days(Cadence::Thirty, Some(from)));
}

#[test]
fn the_miss_rate_spares_the_first_three_grid_days() {
    let c = cell(Cadence::Thirty, 0, 20);
    for t in trains_of(&c) {
        assert_eq!(&t.days[..3], &[1, 31, 61]);
    }
}

#[test]
fn a_jitter_moves_each_occurrence_by_at_most_j_days_after_its_grid_day() {
    let c = cell(Cadence::Fourteen, 3, 0);
    let days = train(&c, 1).days;
    let grid = grid_days(Cadence::Fourteen, None);
    assert_eq!(days.len(), grid.len());
    assert!(days.iter().zip(&grid).all(|(&d, &g)| d >= g && d <= g + 3));
    assert!(days.iter().zip(&grid).any(|(&d, &g)| d != g));
}

#[test]
fn the_first_ten_draws_of_two_seeds_are_chacha20_from_seed_from_u64() {
    let first_ten = |c: &Cell, number: u32| -> Vec<u32> {
        let mut draws = Draws::new(c, number);
        (0..10).map(|_| draws.next_u32()).collect()
    };
    let first = first_ten(&grid()[0], 1);
    let last = first_ten(&grid()[26], 200);
    println!("seed=1001 {first:?}");
    println!("seed=27200 {last:?}");
    assert_eq!(
        first,
        [
            172556661, 1451268005, 3327414182, 1205703654, 4249298938, 156554512, 2474412041,
            3750262970, 1503314913, 1615651461
        ]
    );
    assert_eq!(
        last,
        [
            1239689792, 1423801149, 1106076785, 716582861, 3707725574, 3020602723, 873855913,
            3317445694, 4062816005, 4013876107
        ]
    );
}

#[test]
fn false_stale_counts_turns_from_none_to_stale() {
    let marks = |m: &[Mark]| -> Vec<RoutineState> {
        m.iter()
            .map(|&mark| RoutineState {
                mark,
                ..RoutineState::default()
            })
            .collect()
    };
    use Mark::*;
    assert_eq!(false_stale(&marks(&[None, Stale, Stale, None, Stale])), 2);
    assert_eq!(false_stale(&marks(&[Stale, None])), 1);
    assert_eq!(false_stale(&marks(&[None, None])), 0);
}

#[test]
fn detected_needs_stale_from_the_last_occurrence_to_detect_days_after_it() {
    let at = |stale_day: u32| -> Vec<RoutineState> {
        (1..=TRAIN_DAYS)
            .map(|day| RoutineState {
                mark: if day == stale_day {
                    Mark::Stale
                } else {
                    Mark::None
                },
                ..RoutineState::default()
            })
            .collect()
    };
    let days = [1, 100, 300];
    assert!(detected(&at(300), &days, 120));
    assert!(detected(&at(420), &days, 120));
    assert!(!detected(&at(421), &days, 120));
    assert!(!detected(&at(299), &days, 120));
}

fn counted(false_stale: u32, detected: u32) -> CellCount {
    CellCount {
        cell: grid()[0],
        false_stale,
        detected,
    }
}

#[test]
fn a_setting_meets_only_both_lines_in_every_cell() {
    let s = TWO_NINE;
    assert!(judge(s, vec![counted(30, 45), counted(0, 50)], LINES).meets);
    assert!(!judge(s, vec![counted(30, 45), counted(31, 50)], LINES).meets);
    assert!(!judge(s, vec![counted(30, 44), counted(0, 50)], LINES).meets);
    let r = judge(s, vec![counted(3, 47), counted(9, 46)], LINES);
    assert_eq!((r.worst_false_stale, r.worst_detected), (9, 46));
}

#[test]
fn the_verdict_is_the_first_setting_that_meets() {
    let fail = judge(TWO_NINE, vec![counted(31, 50)], LINES);
    let a = judge(Setting::new(3, 0, 5), vec![counted(0, 50)], LINES);
    let b = judge(Setting::new(3, 60, 5), vec![counted(0, 50)], LINES);
    assert_eq!(verdict(&[fail.clone(), a, b]), Some(Setting::new(3, 0, 5)));
    assert_eq!(verdict(&[fail]), None);
}

#[test]
fn cell_7_0_0_under_2_0_2_marks_every_pause() {
    let c = cell(Cadence::Seven, 0, 0);
    let count = count_cell(TWO_NINE, c, &trains_of(&c), 120);
    assert!(count.false_stale >= 150, "{count:?}");
    assert!(count.false_stale > LINES.false_stale_max);
}

#[test]
fn a_pause_of_21_days_passes_two_cycles_of_nine_days() {
    let c = cell(Cadence::Seven, 0, 0);
    let t = train(&c, 1);
    let states = walk(TWO_NINE, &t.days);
    let turns: Vec<u32> = (1..=TRAIN_DAYS)
        .filter(|&d| states[(d - 1) as usize].mark == Mark::Stale)
        .collect();
    let gap_start = t
        .days
        .windows(2)
        .find(|w| w[1] - w[0] == 21)
        .expect("a pause")[0];
    assert_eq!(turns, [gap_start + 19, gap_start + 20]);
}

#[test]
fn the_repository_seeds_are_the_grid() {
    assert_eq!(parse_seeds(Some(REPO_SEEDS)), Ok(grid()));
    assert_eq!(REPO_SEEDS.lines().next(), Some("7 0 0 1"));
    assert_eq!(REPO_SEEDS.lines().last(), Some("30 3 20 27"));
}

fn is_seed_refusal(r: Result<Vec<Cell>, Refusal>) -> bool {
    matches!(r, Err(Refusal::Seed(_)))
}

#[test]
fn seeds_are_refused_when_absent_out_of_form_or_another_grid() {
    assert!(is_seed_refusal(parse_seeds(None)));
    assert!(is_seed_refusal(parse_seeds(Some(
        &REPO_SEEDS.replacen("7 0 0 1", "7 0 0  1", 1)
    ))));
    assert!(is_seed_refusal(parse_seeds(Some(
        &REPO_SEEDS.replacen("7 0 0 1", "7 0 0 28", 1)
    ))));
    assert!(is_seed_refusal(parse_seeds(Some(&REPO_SEEDS.replacen(
        "7 0 0 1\n",
        "",
        1
    )))));
    assert!(is_seed_refusal(parse_seeds(Some(
        &REPO_SEEDS.replace('\n', "\r\n")
    ))));
}

#[test]
fn the_repository_targets_give_the_lines() {
    assert_eq!(
        parse_targets(Some(REPO_TARGETS)).and_then(|t| t.lines()),
        Ok(LINES)
    );
}

#[test]
fn targets_are_refused_when_absent_out_of_form_or_without_a_key() {
    let input = |text: Option<&str>| {
        matches!(
            parse_targets(text).and_then(|t| t.lines()),
            Err(Refusal::Input(_))
        )
    };
    assert!(input(None));
    assert!(input(Some(
        &REPO_TARGETS.replace("detect_days = 120\n", "")
    )));
    assert!(input(Some(
        &REPO_TARGETS.replace("detect_days = 120", "detect_days=120")
    )));
    assert!(input(Some(
        &REPO_TARGETS.replace("detect_days = 120", "detect_days = 0120")
    )));
    assert!(input(Some(&format!("{REPO_TARGETS}[routine]\n"))));
}

fn refused_as_input(text: &str) -> bool {
    matches!(parse_targets(Some(text)), Err(Refusal::Input(_)))
}

#[test]
fn targets_are_refused_with_an_extra_key() {
    assert!(refused_as_input(
        &REPO_TARGETS.replace("value_min = 4\n", "value_min = 4\nvalue_max = 9\n")
    ));
}

#[test]
fn targets_are_refused_without_a_key_that_the_trial_does_not_read() {
    assert!(refused_as_input(
        &REPO_TARGETS.replace("host_bytes_max = 67108864\n", "")
    ));
}

#[test]
fn targets_are_refused_with_two_keys_swapped() {
    assert!(refused_as_input(&REPO_TARGETS.replace(
        "dev_benign = 8\ndev_fault = 8\n",
        "dev_fault = 8\ndev_benign = 8\n"
    )));
}

#[test]
fn targets_are_refused_with_a_key_in_the_wrong_table() {
    let moved = REPO_TARGETS
        .replace("calendar_days_min = 240\n", "")
        .replace("[memory]\n", "[memory]\ncalendar_days_min = 240\n");
    assert!(refused_as_input(&moved));
}

#[test]
fn targets_are_refused_with_an_extra_table() {
    assert!(refused_as_input(&format!("{REPO_TARGETS}[extra]\n")));
    assert!(refused_as_input(&format!("[extra]\n{REPO_TARGETS}")));
}

#[test]
fn targets_are_refused_with_a_value_that_is_no_integer() {
    assert!(refused_as_input(
        &REPO_TARGETS.replace("k_clean = 2", "k_clean = two")
    ));
    assert!(refused_as_input(
        &REPO_TARGETS.replace("k_clean = 2", "k_clean = -2")
    ));
}

#[test]
fn targets_take_any_integer_in_a_key() {
    let changed = REPO_TARGETS.replace("increment_bytes_max = 8388608", "increment_bytes_max = 1");
    assert!(parse_targets(Some(&changed)).is_ok());
}

#[test]
fn readers_refuse_a_file_that_ends_in_two_line_ends() {
    assert!(refused_as_input(&format!("{REPO_TARGETS}\n")));
    assert!(is_seed_refusal(parse_seeds(Some(&format!(
        "{REPO_SEEDS}\n"
    )))));
}

#[test]
fn readers_refuse_a_number_with_a_sign() {
    assert!(refused_as_input(
        &REPO_TARGETS.replace("detect_days = 120", "detect_days = +120")
    ));
    assert!(is_seed_refusal(parse_seeds(Some(&REPO_SEEDS.replacen(
        "7 0 0 1\n",
        "7 0 0 +1\n",
        1
    )))));
}

#[test]
fn each_read_key_reaches_its_own_line() {
    let changed = REPO_TARGETS
        .replace("false_stale_max = 30", "false_stale_max = 7")
        .replace("detected_min = 45", "detected_min = 8")
        .replace("detect_days = 120", "detect_days = 9");
    let lines = parse_targets(Some(&changed)).and_then(|t| t.lines());
    assert_eq!(
        lines,
        Ok(Lines {
            false_stale_max: 7,
            detected_min: 8,
            detect_days: 9
        })
    );
}

#[test]
fn detect_days_bounds_the_days_on_which_a_stop_counts_as_detected() {
    let c = cell(Cadence::Thirty, 0, 0);
    let trains = vec![trains_of(&c)];
    let with = |detect_days: u32| {
        let lines = Lines {
            detect_days,
            ..LINES
        };
        results(&[c], &trains, lines)[6].cells[0].detected
    };
    assert_eq!(with(120), 50);
    assert_eq!(with(60), 0);
}

#[test]
fn a_huge_detect_days_stops_at_the_end_of_the_train() {
    let c = cell(Cadence::Thirty, 0, 0);
    let t = train(&c, 101);
    let states = walk(Setting::new(3, 60, 5), &t.days);
    assert!(detected(&states, &t.days, u32::MAX));
}
