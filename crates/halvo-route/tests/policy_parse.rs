use halvo_route::line::LINE_DEV_MIN;
use halvo_route::policy::{cells, Basis, PickCell, Policy, PolicyError, Row};
use halvo_route::tier::{CallKind, CategoryId, Level, Tier};

const HEADER: &str = "category\tkind\tpick\tlevel\tbasis\tk\tn";

const FIRST_TABLE: &str = "category\tkind\tpick\tlevel\tbasis\tk\tn\n\
utility_bill_payment\tcompile\thand_back\t-\tuntested\t0\t0\n\
utility_bill_payment\trecover\thand_back\t-\tuntested\t0\t0\n\
utility_bill_payment\twrite\thand_back\t-\tuntested\t0\t0\n\
utility_bill_payment\treplay\tnone\t-\tno_model\t0\t0\n\
tenant_rent\tcompile\thand_back\t-\tno_runtime\t0\t0\n\
tenant_rent\trecover\thand_back\t-\tno_runtime\t0\t0\n\
tenant_rent\twrite\thand_back\t-\tno_runtime\t0\t0\n\
tenant_rent\treplay\tnone\t-\tno_model\t0\t0\n\
lease_renewal_or_change\tcompile\thand_back\t-\tno_runtime\t0\t0\n\
lease_renewal_or_change\trecover\thand_back\t-\tno_runtime\t0\t0\n\
lease_renewal_or_change\twrite\thand_back\t-\tno_runtime\t0\t0\n\
lease_renewal_or_change\treplay\tnone\t-\tno_model\t0\t0\n\
tenant_application_review\tcompile\thand_back\t-\tno_runtime\t0\t0\n\
tenant_application_review\trecover\thand_back\t-\tno_runtime\t0\t0\n\
tenant_application_review\twrite\thand_back\t-\tno_runtime\t0\t0\n\
tenant_application_review\treplay\tnone\t-\tno_model\t0\t0\n\
email\tcompile\thand_back\t-\tuntested\t0\t0\n\
email\trecover\thand_back\t-\tuntested\t0\t0\n\
email\twrite\thand_back\t-\tuntested\t0\t0\n\
email\treplay\tnone\t-\tno_model\t0\t0\n\
spreadsheet_fill\tcompile\thand_back\t-\tuntested\t0\t0\n\
spreadsheet_fill\trecover\thand_back\t-\tuntested\t0\t0\n\
spreadsheet_fill\twrite\thand_back\t-\tuntested\t0\t0\n\
spreadsheet_fill\treplay\tnone\t-\tno_model\t0\t0\n\
shop_order\tcompile\thand_back\t-\tuntested\t0\t0\n\
shop_order\trecover\thand_back\t-\tuntested\t0\t0\n\
shop_order\twrite\thand_back\t-\tuntested\t0\t0\n\
shop_order\treplay\tnone\t-\tno_model\t0\t0\n\
calendar_cleanup\tcompile\thand_back\t-\tuntested\t0\t0\n\
calendar_cleanup\trecover\thand_back\t-\tuntested\t0\t0\n\
calendar_cleanup\twrite\thand_back\t-\tuntested\t0\t0\n\
calendar_cleanup\treplay\tnone\t-\tno_model\t0\t0\n\
ride_order\tcompile\thand_back\t-\tno_runtime\t0\t0\n\
ride_order\trecover\thand_back\t-\tno_runtime\t0\t0\n\
ride_order\twrite\thand_back\t-\tno_runtime\t0\t0\n\
ride_order\treplay\tnone\t-\tno_model\t0\t0\n\
daily_chart_check\tcompile\thand_back\t-\tno_runtime\t0\t0\n\
daily_chart_check\trecover\thand_back\t-\tno_runtime\t0\t0\n\
daily_chart_check\twrite\thand_back\t-\tno_runtime\t0\t0\n\
daily_chart_check\treplay\tnone\t-\tno_model\t0\t0\n\
other\tcompile\thand_back\t-\tnot_seed\t0\t0\n\
other\trecover\thand_back\t-\tnot_seed\t0\t0\n\
other\twrite\thand_back\t-\tnot_seed\t0\t0\n\
other\treplay\tnone\t-\tno_model\t0\t0\n";

fn lines() -> Vec<String> {
    FIRST_TABLE.lines().map(str::to_owned).collect()
}

fn join(lines: &[String]) -> String {
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn with_row(row: usize, line: &str) -> String {
    let mut all = lines();
    all[row + 1] = line.to_owned();
    join(&all)
}

fn with_rows(edits: &[(usize, &str)]) -> String {
    let mut all = lines();
    for (row, line) in edits {
        all[row + 1] = (*line).to_owned();
    }
    join(&all)
}

const ALL: &[Tier] = &Tier::ALL;
const DEVICE: &[Tier] = &[Tier::OnDevice];

fn refused(text: &str, allowed: &[Tier]) -> PolicyError {
    match Policy::parse_for(text, allowed) {
        Ok(_) => panic!("the text parsed: {text:?}"),
        Err(error) => error,
    }
}

#[test]
fn policy_refusals() {
    let cases: Vec<(&str, String, &[Tier], PolicyError)> = vec![
        ("empty text", String::new(), ALL, PolicyError::Header),
        (
            "header alone without LF",
            HEADER.to_owned(),
            ALL,
            PolicyError::Rows,
        ),
        (
            "header with another cell",
            FIRST_TABLE.replacen("\tn\n", "\tn\tx\n", 1),
            ALL,
            PolicyError::Header,
        ),
        (
            "header with CR",
            FIRST_TABLE.replacen("\tn\n", "\tn\r\n", 1),
            ALL,
            PolicyError::Header,
        ),
        (
            "byte-order mark",
            format!("\u{feff}{FIRST_TABLE}"),
            ALL,
            PolicyError::Header,
        ),
        (
            "CR in a row",
            FIRST_TABLE.replacen("\t0\t0\n", "\t0\t0\r\n", 1),
            ALL,
            PolicyError::Rows,
        ),
        ("blank line", with_row(5, ""), ALL, PolicyError::Rows),
        (
            "blank line at the end",
            format!("{FIRST_TABLE}\n"),
            ALL,
            PolicyError::Rows,
        ),
        (
            "no last LF",
            FIRST_TABLE.trim_end_matches('\n').to_owned(),
            ALL,
            PolicyError::Rows,
        ),
        (
            "header alone",
            format!("{HEADER}\n"),
            ALL,
            PolicyError::Rows,
        ),
        ("43 rows", join(&lines()[..44]), ALL, PolicyError::Rows),
        (
            "45 rows",
            format!("{FIRST_TABLE}other\treplay\tnone\t-\tno_model\t0\t0\n"),
            ALL,
            PolicyError::Rows,
        ),
        (
            "six cells",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tuntested\t0",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "eight cells",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tuntested\t0\t0\t0",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "unknown category",
            with_row(0, "utility_bill\tcompile\thand_back\t-\tuntested\t0\t0"),
            ALL,
            PolicyError::Name,
        ),
        (
            "unknown kind",
            with_row(
                0,
                "utility_bill_payment\tcompiled\thand_back\t-\tuntested\t0\t0",
            ),
            ALL,
            PolicyError::Name,
        ),
        (
            "unknown pick",
            with_row(
                0,
                "utility_bill_payment\tcompile\thandback\t-\tuntested\t0\t0",
            ),
            ALL,
            PolicyError::Name,
        ),
        (
            "unknown level",
            with_row(
                0,
                "utility_bill_payment\tcompile\ton_device\tmid\tline_met\t4\t4",
            ),
            ALL,
            PolicyError::Name,
        ),
        (
            "unknown basis",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tuntried\t0\t0",
            ),
            ALL,
            PolicyError::Name,
        ),
        (
            "pick of a dash",
            with_row(0, "utility_bill_payment\tcompile\t-\t-\tuntested\t0\t0"),
            ALL,
            PolicyError::Name,
        ),
        (
            "rows of two cells swapped",
            with_rows(&[
                (
                    0,
                    "utility_bill_payment\trecover\thand_back\t-\tuntested\t0\t0",
                ),
                (
                    1,
                    "utility_bill_payment\tcompile\thand_back\t-\tuntested\t0\t0",
                ),
            ]),
            ALL,
            PolicyError::Rows,
        ),
        (
            "row of another category",
            with_row(4, "email\tcompile\thand_back\t-\tuntested\t0\t0"),
            ALL,
            PolicyError::Rows,
        ),
        (
            "k no decimal integer",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tuntested\tx\t0",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "n negative",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tuntested\t0\t-1",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "k with a leading zero",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tuntested\t01\t4",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "n beyond u32",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tuntested\t0\t4294967296",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "k above n",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tline_missed\t5\t4",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "none on a row that is not replay",
            with_row(0, "utility_bill_payment\tcompile\tnone\t-\tno_model\t0\t0"),
            ALL,
            PolicyError::Shape,
        ),
        (
            "replay row handed back",
            with_row(
                3,
                "utility_bill_payment\treplay\thand_back\t-\tuntested\t0\t0",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "replay row with counts",
            with_row(3, "utility_bill_payment\treplay\tnone\t-\tno_model\t1\t1"),
            ALL,
            PolicyError::Shape,
        ),
        (
            "replay row on a tier",
            with_row(
                3,
                "utility_bill_payment\treplay\ton_device\tmedium\tline_met\t4\t4",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "tier with no level",
            with_row(
                0,
                "utility_bill_payment\tcompile\ton_device\t-\tline_met\t4\t4",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "hand_back with a level",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\tmedium\tuntested\t0\t0",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "none with a level",
            with_row(3, "utility_bill_payment\treplay\tnone\tlow\tno_model\t0\t0"),
            ALL,
            PolicyError::Shape,
        ),
        (
            "tier with basis untested",
            with_row(
                0,
                "utility_bill_payment\tcompile\ton_device\tmedium\tuntested\t4\t4",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "hand_back with basis line_met",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tline_met\t4\t4",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "hand_back with basis no_model",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tno_model\t0\t0",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "not_seed on a seed category",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tnot_seed\t0\t0",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "a tier on other",
            with_row(40, "other\tcompile\tfrontier\thigh\tline_met\t4\t4"),
            ALL,
            PolicyError::Shape,
        ),
        (
            "other with a basis but not_seed",
            with_row(40, "other\tcompile\thand_back\t-\tuntested\t0\t0"),
            ALL,
            PolicyError::Shape,
        ),
        (
            "line_met with a miss",
            with_row(
                0,
                "utility_bill_payment\tcompile\ton_device\tmedium\tline_met\t0\t4",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "line_met under the least number",
            with_row(
                0,
                "utility_bill_payment\tcompile\ton_device\tmedium\tline_met\t3\t3",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "line_missed with no miss",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tline_missed\t4\t4",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "line_missed under the least number",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tline_missed\t1\t3",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "untested at the least number",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tuntested\t4\t4",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "no_runtime on a category with a runtime",
            with_row(
                0,
                "utility_bill_payment\tcompile\thand_back\t-\tno_runtime\t0\t0",
            ),
            ALL,
            PolicyError::Shape,
        ),
        (
            "no_runtime with counts",
            with_row(4, "tenant_rent\tcompile\thand_back\t-\tno_runtime\t1\t1"),
            ALL,
            PolicyError::Shape,
        ),
        (
            "not_seed with counts",
            with_row(40, "other\tcompile\thand_back\t-\tnot_seed\t1\t1"),
            ALL,
            PolicyError::Shape,
        ),
        (
            "on_device that the ruling does not allow",
            with_row(
                0,
                "utility_bill_payment\tcompile\ton_device\tmedium\tline_met\t4\t4",
            ),
            &[],
            PolicyError::Ruling,
        ),
        (
            "frontier that the ruling does not allow",
            with_row(
                0,
                "utility_bill_payment\tcompile\tfrontier\thigh\tline_met\t4\t4",
            ),
            DEVICE,
            PolicyError::Ruling,
        ),
        (
            "low_cost_hosted that the ruling does not allow",
            with_row(16, "email\tcompile\tlow_cost_hosted\tlow\tline_met\t6\t6"),
            DEVICE,
            PolicyError::Ruling,
        ),
    ];
    for (name, text, allowed, want) in cases {
        assert_eq!(refused(&text, allowed), want, "case {name}");
    }
}

#[test]
fn policy_refusal_order() {
    let header_and_cr = FIRST_TABLE
        .replacen("tenant_rent\tcompile", "x\rtenant_rent\tcompile", 1)
        .replacen("category", "kategory", 1);
    assert_eq!(refused(&header_and_cr, ALL), PolicyError::Header);
    let rows_and_name = with_row(2, "x\tx\tx\tx\tx\tx\tx").replacen("\t0\t0\n", "\t0\t0\r\n", 1);
    assert_eq!(refused(&rows_and_name, ALL), PolicyError::Rows);
    let ruling_then_shape = with_rows(&[
        (
            0,
            "utility_bill_payment\tcompile\tfrontier\thigh\tline_met\t4\t4",
        ),
        (1, "utility_bill_payment\trecover\thand_back"),
    ]);
    assert_eq!(refused(&ruling_then_shape, DEVICE), PolicyError::Ruling);
    let name_then_rows = with_row(0, "shop_order\trecover\thand_back\tmid\tuntested\t0\t0");
    assert_eq!(refused(&name_then_rows, ALL), PolicyError::Name);
    let rows_then_shape = with_row(0, "shop_order\trecover\thand_back\t-\tuntested\t5\t4");
    assert_eq!(refused(&rows_then_shape, ALL), PolicyError::Rows);
    let shape_then_ruling = with_row(
        0,
        "utility_bill_payment\tcompile\tfrontier\thigh\tline_met\t5\t4",
    );
    assert_eq!(refused(&shape_then_ruling, DEVICE), PolicyError::Shape);
    let shape_cells_then_name = with_row(0, "utility_bill\tcompile\thand_back\t-\tuntested\t0");
    assert_eq!(refused(&shape_cells_then_name, ALL), PolicyError::Shape);
    let first_row_wins = with_rows(&[
        (
            1,
            "utility_bill_payment\trecover\thand_back\t-\tuntested\t9\t4",
        ),
        (
            2,
            "utility_bill_payment\twrite\thand_back\t-\tuntried\t0\t0",
        ),
    ]);
    assert_eq!(refused(&first_row_wins, ALL), PolicyError::Shape);
}

#[test]
fn policy_round_trip() {
    let tiers = with_rows(&[
        (
            0,
            "utility_bill_payment\tcompile\ton_device\tmedium\tline_met\t4\t4",
        ),
        (
            1,
            "utility_bill_payment\trecover\tlow_cost_hosted\tlow\tline_met\t12\t12",
        ),
        (
            2,
            "utility_bill_payment\twrite\tfrontier\thigh\tline_met\t4294967295\t4294967295",
        ),
        (16, "email\tcompile\thand_back\t-\tline_missed\t3\t4"),
        (17, "email\trecover\thand_back\t-\tuntested\t2\t3"),
    ]);
    for text in [FIRST_TABLE.to_owned(), tiers] {
        let policy = Policy::parse_for(&text, ALL).expect("the text parses");
        assert_eq!(policy.to_text(), text);
        assert_eq!(Policy::parse_for(&policy.to_text(), ALL), Ok(policy));
    }
    let mut state = 0x5eed_u64;
    let mut next = move |bound: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % bound
    };
    for _ in 0..500 {
        let mut rows = Vec::new();
        for (category, kind) in cells() {
            let row = if kind == CallKind::Replay {
                Row {
                    pick: PickCell::None,
                    level: None,
                    basis: Basis::NoModel,
                    k: 0,
                    n: 0,
                }
            } else if category == CategoryId::Other {
                Row {
                    pick: PickCell::HandBack,
                    level: None,
                    basis: Basis::NotSeed,
                    k: 0,
                    n: 0,
                }
            } else {
                let n = u32::try_from(next(20)).expect("small");
                match next(4) {
                    0 => Row {
                        pick: PickCell::HandBack,
                        level: None,
                        basis: Basis::Untested,
                        k: 0,
                        n: n % LINE_DEV_MIN,
                    },
                    1 => Row {
                        pick: PickCell::HandBack,
                        level: None,
                        basis: Basis::LineMissed,
                        k: n.max(LINE_DEV_MIN) / 2,
                        n: n.max(LINE_DEV_MIN),
                    },
                    2 if !category.has_runtime() => Row {
                        pick: PickCell::HandBack,
                        level: None,
                        basis: Basis::NoRuntime,
                        k: 0,
                        n: 0,
                    },
                    2 => Row {
                        pick: PickCell::HandBack,
                        level: None,
                        basis: Basis::Untested,
                        k: 0,
                        n: 0,
                    },
                    _ => Row {
                        pick: PickCell::Tier(Tier::ALL[usize::try_from(next(3)).expect("small")]),
                        level: Some(Level::ALL[usize::try_from(next(3)).expect("small")]),
                        basis: Basis::LineMet,
                        k: n.max(LINE_DEV_MIN),
                        n: n.max(LINE_DEV_MIN),
                    },
                }
            };
            rows.push(row);
        }
        let rows: [Row; 44] = rows.try_into().expect("44 rows");
        let policy = Policy::from_rows(rows, ALL).expect("the rows are of the form");
        let text = policy.to_text();
        assert_eq!(Policy::parse_for(&text, ALL), Ok(policy));
        assert_eq!(
            Policy::parse_for(&text, ALL).expect("parses").to_text(),
            text
        );
    }
}

#[test]
fn policy_hand_back_all() {
    assert_eq!(Policy::hand_back_all().to_text(), FIRST_TABLE);
    assert_eq!(
        Policy::parse_for(FIRST_TABLE, DEVICE),
        Ok(Policy::hand_back_all())
    );
    assert_eq!(
        Policy::parse_for(FIRST_TABLE, &[]),
        Ok(Policy::hand_back_all())
    );
}

#[test]
fn policy_row_reads_its_cell() {
    let text = with_rows(&[
        (
            1,
            "utility_bill_payment\trecover\tlow_cost_hosted\tlow\tline_met\t5\t5",
        ),
        (42, "other\twrite\thand_back\t-\tnot_seed\t0\t0"),
    ]);
    let policy = Policy::parse_for(&text, ALL).expect("parses");
    assert_eq!(
        *policy.row(CategoryId::UtilityBillPayment, CallKind::Recover),
        Row {
            pick: PickCell::Tier(Tier::LowCostHosted),
            level: Some(Level::Low),
            basis: Basis::LineMet,
            k: 5,
            n: 5
        }
    );
    for (i, (category, kind)) in cells().enumerate() {
        let line = text.lines().nth(i + 1).expect("a row");
        let row = policy.row(category, kind);
        let level = row.level.map_or("-", Level::as_str);
        let want = format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            category.as_str(),
            kind.as_str(),
            row.pick.as_str(),
            level,
            row.basis.as_str(),
            row.k,
            row.n
        );
        assert_eq!(line, want);
    }
}

#[test]
fn policy_from_rows_refuses_what_parse_refuses() {
    let mut rows = *Policy::hand_back_all().rows();
    rows[0] = Row {
        pick: PickCell::Tier(Tier::Frontier),
        level: Some(Level::High),
        basis: Basis::LineMet,
        k: 4,
        n: 4,
    };
    assert_eq!(Policy::from_rows(rows, DEVICE), Err(PolicyError::Ruling));
    rows[0].level = None;
    assert_eq!(Policy::from_rows(rows, ALL), Err(PolicyError::Shape));
    rows[0] = Row {
        pick: PickCell::None,
        level: None,
        basis: Basis::NoModel,
        k: 0,
        n: 0,
    };
    assert_eq!(Policy::from_rows(rows, ALL), Err(PolicyError::Shape));
    rows[0] = Row {
        pick: PickCell::HandBack,
        level: None,
        basis: Basis::LineMissed,
        k: 4,
        n: 3,
    };
    assert_eq!(Policy::from_rows(rows, ALL), Err(PolicyError::Shape));
}
