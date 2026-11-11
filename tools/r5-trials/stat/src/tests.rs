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
        "targets key dev_critical is absent"
    );
    assert_eq!(
        refused(parse_targets(&format!("\u{feff}{REPO_TARGETS}"))),
        "targets is not LF text with one LF at its end"
    );
}
