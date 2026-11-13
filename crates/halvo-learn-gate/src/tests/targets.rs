use crate::cli::Cause;
use crate::targets::{parse, Targets};

const FILE: &str = include_str!("../../../../docs/r5/targets.toml");

fn read(text: &str) -> Result<Targets, Cause> {
    parse(text.as_bytes()).map_err(|refusal| refusal.cause)
}

#[test]
fn constants_match_targets() {
    let targets = read(FILE).expect("docs/r5/targets.toml is read");
    let pairs: [(&str, &str, u64); 7] = [
        ("rule", "k_clean", u64::from(halvo_learn::K_CLEAN)),
        ("rule", "locators_max", halvo_learn::LOCATORS_MAX as u64),
        ("rule", "edits_max", u64::from(halvo_learn::EDITS_MAX)),
        ("rule", "value_min", halvo_learn::VALUE_MIN as u64),
        (
            "routine",
            "miss_cycles",
            u64::from(halvo_learn::MISS_CYCLES),
        ),
        (
            "routine",
            "stale_floor",
            u64::from(halvo_learn::STALE_FLOOR),
        ),
        ("routine", "tolerance", u64::from(halvo_learn::TOLERANCE)),
    ];
    for (table, key, constant) in pairs {
        assert_eq!(targets.get(table, key), constant, "{table}.{key}");
    }
    use halvo_schema::learning::{CLEAN_BOUND, LOCATORS_BOUND, RUNS_BOUND};
    assert_eq!(LOCATORS_BOUND, halvo_learn::LOCATORS_MAX, "LOCATORS_BOUND");
    assert_eq!(
        RUNS_BOUND as u64,
        1 + u64::from(halvo_learn::K_CLEAN),
        "RUNS_BOUND"
    );
    assert_eq!(CLEAN_BOUND, halvo_learn::K_CLEAN, "CLEAN_BOUND");
}

#[test]
fn targets_file_reads_every_key() {
    let targets = read(FILE).expect("docs/r5/targets.toml is read");
    assert_eq!(targets.get("gate", "dev_benign"), 8);
    assert_eq!(targets.get("gate", "dev_critical"), 7);
    assert_eq!(targets.get("gate", "heldout_cost_budget"), 2);
    assert_eq!(targets.get("gate", "calendar_days_min"), 240);
    assert_eq!(targets.get("memory", "host_bytes_max"), 67_108_864);
}

#[test]
fn targets_file_refuses_other_forms() {
    let changed = |from: &str, to: &str| {
        assert!(FILE.contains(from), "the file holds {from:?}");
        FILE.replacen(from, to, 1)
    };
    let cases = [
        changed("k_clean = 2", "k_clean = +2"),
        changed("k_clean = 2", "k_clean = 02"),
        changed("k_clean = 2", "k_clean = \"2\""),
        changed("k_clean = 2", "k_clean=2"),
        changed("k_clean = 2", "k_clean = 2 "),
        changed("k_clean = 2", "k_clean = 18446744073709551616"),
        changed("k_clean = 2", "k_clean = "),
        changed("k_clean = 2\n", ""),
        changed(
            "k_clean = 2\nlocators_max = 4\n",
            "locators_max = 4\nk_clean = 2\n",
        ),
        changed("[gate]\n", ""),
        changed("[gate]\n", "[gate]\n\n"),
        changed("[gate]\n", "[gates]\n"),
        changed("value_min = 4\n", "value_min = 4\nextra = 1\n"),
        changed("\n", "\r\n"),
        format!("\u{feff}{FILE}"),
        FILE.trim_end_matches('\n').to_owned(),
        format!("{FILE}\n"),
        format!("{FILE}[more]\n"),
        String::new(),
    ];
    for (at, case) in cases.iter().enumerate() {
        assert_eq!(read(case), Err(Cause::Targets), "case {at}: {case:?}");
    }
}

#[test]
fn targets_file_takes_the_widest_integer_and_zero() {
    let wide = FILE.replacen("k_clean = 2", "k_clean = 18446744073709551615", 1);
    assert_eq!(read(&wide).map(|t| t.get("rule", "k_clean")), Ok(u64::MAX));
    let zero = FILE.replacen("k_clean = 2", "k_clean = 0", 1);
    assert_eq!(read(&zero).map(|t| t.get("rule", "k_clean")), Ok(0));
}
