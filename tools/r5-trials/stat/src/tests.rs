use super::*;

const REPO_EXPECTED: &str = include_str!("../expected.txt");
const REPO_TARGETS: &str = include_str!("../../../../docs/r5/targets.toml");

fn passing() -> Verdict {
    Verdict {
        lines_equal: true,
        stub: Stub {
            same_per_10000: SAME_PER_10000_MAX,
            gain90_per_10000: GAIN90_PER_10000_MIN,
        },
        dev_critical: Match::Same,
        heldout_critical: Match::Same,
    }
}

#[test]
fn verdict_misses_when_the_lines_differ_from_the_expected_file() {
    let v = Verdict {
        lines_equal: false,
        ..passing()
    };
    assert!(!v.ok());
}

#[test]
fn verdict_misses_when_same_per_10000_is_over_500() {
    let mut v = passing();
    v.stub.same_per_10000 = SAME_PER_10000_MAX + 1;
    assert!(!v.ok());
}

#[test]
fn verdict_misses_when_gain90_per_10000_is_under_8000() {
    let mut v = passing();
    v.stub.gain90_per_10000 = GAIN90_PER_10000_MIN - 1;
    assert!(!v.ok());
}

#[test]
fn verdict_misses_when_a_target_key_differs() {
    let dev = Verdict {
        dev_critical: Match::Differ,
        ..passing()
    };
    let heldout = Verdict {
        heldout_critical: Match::Differ,
        ..passing()
    };
    assert!(!dev.ok());
    assert!(!heldout.ok());
}

#[test]
fn verdict_is_ok_at_both_bounds() {
    assert!(passing().ok());
}

#[test]
fn choose_sums_to_a_power_of_two() {
    for n in 0..=LAST_N {
        let sum: u128 = (0..=n).map(|k| choose(n, k)).sum();
        assert_eq!(sum, 1u128 << n);
    }
}

#[test]
fn whole_tail_is_one() {
    for n in 1..=LAST_N {
        assert_eq!(upper_tail(n, 0, HALF), Ratio::new(1, 1));
        assert_eq!(upper_tail(n, 0, NINE_TENTHS), Ratio::new(1, 1));
    }
}

#[test]
fn critical_counts_of_the_contract() {
    assert_eq!(critical(4), None);
    assert_eq!(critical(8), Some((7, Ratio::new(9, 256))));
    assert_eq!(critical(12), Some((10, Ratio::new(79, 4096))));
}

#[test]
fn critical_is_the_smallest_count_at_or_under_alpha() {
    for n in FIRST_N..=LAST_N {
        let (c, tail) = critical(n).unwrap();
        assert!(tail.at_most(ALPHA));
        assert!(!upper_tail(n, c - 1, HALF).at_most(ALPHA));
    }
}

#[test]
fn stub_figures_of_the_contract() {
    assert_eq!(
        stub(),
        Stub {
            same_per_10000: 192,
            gain90_per_10000: 8891
        }
    );
}

#[test]
fn lines_equal_the_committed_expected_file() {
    assert_eq!(parse_expected(REPO_EXPECTED).unwrap(), critical_lines());
}

#[test]
fn committed_targets_give_verdict_ok() {
    let targets = parse_targets(REPO_TARGETS).unwrap();
    let block = block(&parse_expected(REPO_EXPECTED).unwrap(), targets);
    assert!(block.verdict.ok());
    assert!(block.text.ends_with("verdict stat=ok\n"));
    assert_eq!(block.text.lines().count(), 23);
}

#[test]
fn a_changed_expected_line_gives_miss() {
    let changed = REPO_EXPECTED.replacen("n=8 c=7", "n=8 c=8", 1);
    let block = block(
        &parse_expected(&changed).unwrap(),
        parse_targets(REPO_TARGETS).unwrap(),
    );
    assert!(!block.verdict.lines_equal);
    assert!(block.text.ends_with("verdict stat=miss\n"));
}

#[test]
fn a_changed_target_key_gives_differ() {
    let changed = REPO_TARGETS.replacen("heldout_critical = 10", "heldout_critical = 9", 1);
    let block = block(
        &parse_expected(REPO_EXPECTED).unwrap(),
        parse_targets(&changed).unwrap(),
    );
    assert!(block
        .text
        .contains("targets dev_critical=same heldout_critical=differ\n"));
    assert!(block.text.ends_with("verdict stat=miss\n"));
}

fn refused(result: Result<impl fmt::Debug, Refusal>) -> String {
    match result {
        Err(Refusal::Input(detail)) => detail,
        other => panic!("not refused with input: {other:?}"),
    }
}

#[test]
fn expected_refuses_other_forms() {
    assert_eq!(
        refused(parse_expected(&REPO_EXPECTED.replacen(
            "critical n=24",
            "x",
            1
        ))),
        "expected line 20 is not of its form"
    );
    let short: String = REPO_EXPECTED
        .lines()
        .skip(1)
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(
        refused(parse_expected(&short)),
        "expected holds 19 lines, not 20"
    );
    assert_eq!(
        refused(parse_expected(&REPO_EXPECTED.replace('\n', "\r\n"))),
        "expected is not LF text with one LF at its end"
    );
    assert_eq!(
        refused(parse_expected(REPO_EXPECTED.trim_end())),
        "expected is not LF text with one LF at its end"
    );
    assert_eq!(
        refused(parse_expected(&REPO_EXPECTED.replacen("c=5", "c=05", 1))),
        "expected line 1 is not of its form"
    );
}

#[test]
fn targets_refuses_other_forms() {
    let line_of =
        |from: &str, to: &str| refused(parse_targets(&REPO_TARGETS.replacen(from, to, 1)));
    assert_eq!(
        line_of("k_clean = 2", "k_clean=2"),
        "targets line 2 is not of its form"
    );
    assert_eq!(line_of("[rule]\n", ""), "targets line 1 is not of its form");
    assert_eq!(
        line_of("dev_fault = 8", "dev_benign = 8"),
        "targets line 8 is not of its form"
    );
    assert_eq!(
        line_of("[memory]", "[gate]"),
        "targets line 23 is not of its form"
    );
    assert_eq!(
        line_of("dev_critical = 7", "dev_critical = -7"),
        "targets line 9 is not of its form"
    );
    assert_eq!(
        line_of("dev_critical = 7\n", ""),
        "targets key dev_critical is absent"
    );
    assert_eq!(
        line_of("heldout_critical = 10\n", ""),
        "targets key heldout_critical is absent"
    );
    assert_eq!(
        line_of("[gate]", "[gates]"),
        "targets line 6 is not of its form"
    );
    assert_eq!(
        refused(parse_targets(&format!("\u{feff}{REPO_TARGETS}"))),
        "targets is not LF text with one LF at its end"
    );
}

#[test]
fn the_output_refusal_is_one_line_of_its_cause() {
    assert_eq!(
        Refusal::Output.to_string(),
        "REFUSED output: standard output cannot be written"
    );
}

#[test]
fn targets_refuses_an_extra_key() {
    assert_eq!(
        refused(parse_targets(&REPO_TARGETS.replacen(
            "k_clean = 2\n",
            "k_clean = 2\nextra = 1\n",
            1
        ))),
        "targets line 3 is not of its form"
    );
}

#[test]
fn targets_refuses_a_missing_key_that_the_trial_does_not_read() {
    assert_eq!(
        refused(parse_targets(&REPO_TARGETS.replacen(
            "edits_max = 8\n",
            "",
            1
        ))),
        "targets key edits_max is absent"
    );
}

#[test]
fn targets_refuses_two_swapped_keys() {
    assert_eq!(
        refused(parse_targets(&REPO_TARGETS.replacen(
            "dev_benign = 8\ndev_fault = 8\n",
            "dev_fault = 8\ndev_benign = 8\n",
            1
        ))),
        "targets line 7 is not of its form"
    );
}

#[test]
fn targets_refuses_a_key_in_the_wrong_table() {
    assert_eq!(
        refused(parse_targets(&REPO_TARGETS.replacen(
            "calendar_days_min = 240\n[routine]\n",
            "[routine]\ncalendar_days_min = 240\n",
            1
        ))),
        "targets line 15 is not of its form"
    );
}

#[test]
fn targets_refuses_an_extra_table() {
    assert_eq!(
        refused(parse_targets(&format!("{REPO_TARGETS}[extra]\nkey = 1\n"))),
        "targets line 26 is not of its form"
    );
}

#[test]
fn targets_refuses_the_tables_in_another_order() {
    let (head, memory) = REPO_TARGETS.split_at(REPO_TARGETS.find("[memory]").unwrap());
    assert_eq!(
        refused(parse_targets(&format!("{memory}{head}"))),
        "targets line 1 is not of its form"
    );
}

#[test]
fn targets_refuses_gate_with_the_two_read_keys_alone() {
    assert_eq!(
        refused(parse_targets(
            "[gate]\ndev_critical = 7\nheldout_critical = 10\n"
        )),
        "targets line 1 is not of its form"
    );
}

#[test]
fn targets_accepts_other_values_and_compares_the_two_keys() {
    let shifted: String = REPO_TARGETS
        .lines()
        .map(|line| match line.split_once(" = ") {
            Some((key, value)) => format!("{key} = {}\n", value.parse::<u64>().unwrap() + 1),
            None => format!("{line}\n"),
        })
        .collect();
    let targets = parse_targets(&shifted).unwrap();
    assert_eq!(
        targets,
        Targets {
            dev_critical: 8,
            heldout_critical: 11
        }
    );
    let expected = parse_expected(REPO_EXPECTED).unwrap();
    let differ = block(&expected, targets);
    assert!(differ
        .text
        .contains("targets dev_critical=differ heldout_critical=differ\n"));
    assert!(differ.text.ends_with("verdict stat=miss\n"));
    let kept = shifted
        .replacen("dev_critical = 8", "dev_critical = 7", 1)
        .replacen("heldout_critical = 11", "heldout_critical = 10", 1);
    let same = block(&expected, parse_targets(&kept).unwrap());
    assert!(same.verdict.ok());
}

#[test]
fn a_changed_dev_critical_alone_gives_differ_and_miss() {
    let changed = REPO_TARGETS.replacen("dev_critical = 7", "dev_critical = 6", 1);
    let block = block(
        &parse_expected(REPO_EXPECTED).unwrap(),
        parse_targets(&changed).unwrap(),
    );
    assert!(block
        .text
        .contains("targets dev_critical=differ heldout_critical=same\n"));
    assert!(block.text.ends_with("verdict stat=miss\n"));
}

#[test]
fn two_swapped_expected_lines_give_miss() {
    let mut lines: Vec<&str> = REPO_EXPECTED.lines().collect();
    lines.swap(0, 1);
    let swapped: String = lines.iter().map(|l| format!("{l}\n")).collect();
    let block = block(
        &parse_expected(&swapped).unwrap(),
        parse_targets(REPO_TARGETS).unwrap(),
    );
    assert!(!block.verdict.lines_equal);
    assert!(block.text.ends_with("verdict stat=miss\n"));
}

#[test]
fn targets_refuses_a_spaced_header() {
    assert_eq!(
        refused(parse_targets(
            &REPO_TARGETS.replacen("[gate]", "[ gate ]", 1)
        )),
        "targets line 6 is not of its form"
    );
}

#[test]
fn targets_refuses_a_key_that_is_not_a_name() {
    assert_eq!(
        refused(parse_targets(&REPO_TARGETS.replacen(
            "dev_critical = 7",
            "Dev_critical = 7",
            1
        ))),
        "targets line 9 is not of its form"
    );
}

#[test]
fn targets_refuses_a_value_of_two_to_the_64() {
    assert_eq!(
        refused(parse_targets(&REPO_TARGETS.replacen(
            "dev_critical = 7",
            "dev_critical = 18446744073709551616",
            1
        ))),
        "targets line 9 is not of its form"
    );
}

#[test]
fn expected_refuses_a_sixth_word() {
    assert_eq!(
        refused(parse_expected(&REPO_EXPECTED.replacen('\n', " x\n", 1))),
        "expected line 1 is not of its form"
    );
}
