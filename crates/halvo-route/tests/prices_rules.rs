use halvo_route::prices::{PriceError, Prices, Rate};
use halvo_route::tier::Tier;

const GOOD: &str = "tier\tinput\toutput\tsource\tdate\n\
on_device\t0\t0\tdevice\t2026-11-10\n\
low_cost_hosted\t3100000\t15700000\tfixture:B-3.2\t2026-11-10\n\
frontier\t15500000\t78500000\tfixture:B-3.2\t2026-11-10\n";

fn with_row(row: usize, line: &str) -> String {
    let mut lines: Vec<&str> = GOOD.lines().collect();
    lines[row + 1] = line;
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

fn refused(text: &str) -> PriceError {
    match Prices::parse(text) {
        Ok(prices) => panic!("the text parsed to {prices:?}: {text:?}"),
        Err(error) => error,
    }
}

#[test]
fn price_refusals() {
    let cases: Vec<(&str, String, PriceError)> = vec![
        ("empty text", String::new(), PriceError::Header),
        ("header of another order", GOOD.replacen("input\toutput", "output\tinput", 1), PriceError::Header),
        ("header with CR", GOOD.replacen("date\n", "date\r\n", 1), PriceError::Header),
        ("byte-order mark", format!("\u{feff}{GOOD}"), PriceError::Header),
        (
            "tiers out of order",
            "tier\tinput\toutput\tsource\tdate\non_device\t0\t0\tdevice\t2026-11-10\nfrontier\t15500000\t78500000\tfixture:B-3.2\t2026-11-10\nlow_cost_hosted\t3100000\t15700000\tfixture:B-3.2\t2026-11-10\n".to_owned(),
            PriceError::Rows,
        ),
        ("a tier twice", with_row(2, "low_cost_hosted\t15500000\t78500000\tfixture:B-3.2\t2026-11-10"), PriceError::Rows),
        ("an unknown tier", with_row(0, "device\t0\t0\tdevice\t2026-11-10"), PriceError::Rows),
        ("two rows", GOOD.replacen("frontier\t15500000\t78500000\tfixture:B-3.2\t2026-11-10\n", "", 1), PriceError::Rows),
        ("four rows", format!("{GOOD}frontier\t1\t1\tx\t2026-11-10\n"), PriceError::Rows),
        ("a blank line", format!("{GOOD}\n"), PriceError::Rows),
        ("a CR in a row", GOOD.replacen("device\t2026-11-10\n", "device\t2026-11-10\r\n", 1), PriceError::Rows),
        ("no last LF", GOOD.trim_end_matches('\n').to_owned(), PriceError::Rows),
        ("a row of four cells", with_row(0, "on_device\t0\t0\tdevice"), PriceError::Rows),
        ("a row of six cells", with_row(0, "on_device\t0\t0\tdevice\t2026-11-10\tx"), PriceError::Rows),
        ("a rate with a decimal point", with_row(1, "low_cost_hosted\t3.1\t15700000\tfixture:B-3.2\t2026-11-10"), PriceError::Number),
        ("a negative rate", with_row(1, "low_cost_hosted\t-3100000\t15700000\tfixture:B-3.2\t2026-11-10"), PriceError::Number),
        ("a rate with a sign", with_row(1, "low_cost_hosted\t+3100000\t15700000\tfixture:B-3.2\t2026-11-10"), PriceError::Number),
        ("a rate with a leading zero", with_row(1, "low_cost_hosted\t03100000\t15700000\tfixture:B-3.2\t2026-11-10"), PriceError::Number),
        ("a rate with digit groups", with_row(1, "low_cost_hosted\t3,100,000\t15700000\tfixture:B-3.2\t2026-11-10"), PriceError::Number),
        ("an empty rate", with_row(1, "low_cost_hosted\t\t15700000\tfixture:B-3.2\t2026-11-10"), PriceError::Number),
        ("a rate beyond u64", with_row(2, "frontier\t15500000\t18446744073709551616\tfixture:B-3.2\t2026-11-10"), PriceError::Number),
        ("a source of a dash", with_row(1, "low_cost_hosted\t3100000\t15700000\t-\t2026-11-10"), PriceError::Source),
        ("an empty source", with_row(1, "low_cost_hosted\t3100000\t15700000\t\t2026-11-10"), PriceError::Source),
        ("a source with a quote", with_row(1, "low_cost_hosted\t3100000\t15700000\t\"x\"\t2026-11-10"), PriceError::Source),
        ("an input rate on on_device", with_row(0, "on_device\t5\t0\tdevice\t2026-11-10"), PriceError::Number),
        ("an output rate on on_device", with_row(0, "on_device\t0\t7\tdevice\t2026-11-10"), PriceError::Number),
        ("a source of one space", with_row(1, "low_cost_hosted\t3100000\t15700000\t \t2026-11-10"), PriceError::Source),
        ("a source of two words", with_row(1, "low_cost_hosted\t3100000\t15700000\tfixture B-3.2\t2026-11-10"), PriceError::Source),
        ("a source with a quote inside", with_row(2, "frontier\t15500000\t78500000\tfixture:\"B-3.2\t2026-11-10"), PriceError::Source),
        ("a date of a dash", with_row(0, "on_device\t0\t0\tdevice\t-"), PriceError::Date),
        ("a date with slashes", with_row(0, "on_device\t0\t0\tdevice\t2026/11/10"), PriceError::Date),
        ("a date without zeros", with_row(0, "on_device\t0\t0\tdevice\t2026-1-10"), PriceError::Date),
        ("a date with a time", with_row(0, "on_device\t0\t0\tdevice\t2026-11-10T00:00"), PriceError::Date),
        ("month 13", with_row(0, "on_device\t0\t0\tdevice\t2026-13-10"), PriceError::Date),
        ("month 0", with_row(0, "on_device\t0\t0\tdevice\t2026-00-10"), PriceError::Date),
        ("day 0", with_row(0, "on_device\t0\t0\tdevice\t2026-11-00"), PriceError::Date),
        ("31 November", with_row(0, "on_device\t0\t0\tdevice\t2026-11-31"), PriceError::Date),
        ("29 February of a common year", with_row(0, "on_device\t0\t0\tdevice\t2026-02-29"), PriceError::Date),
        ("29 February of 1900", with_row(0, "on_device\t0\t0\tdevice\t1900-02-29"), PriceError::Date),
    ];
    for (name, text, want) in cases {
        assert_eq!(refused(&text), want, "case {name}");
    }
    assert!(Prices::parse(&with_row(0, "on_device\t0\t0\tdevice\t2028-02-29")).is_ok());
    assert!(Prices::parse(&with_row(0, "on_device\t0\t0\tdevice\t2000-02-29")).is_ok());
}

#[test]
fn price_refusal_order() {
    let header_and_rows = format!("x{GOOD}\n");
    assert_eq!(refused(&header_and_rows), PriceError::Header);
    let rows_and_number = format!("{}\n", with_row(0, "on_device\tx\t0\tdevice\t2026-11-10"));
    assert_eq!(refused(&rows_and_number), PriceError::Rows);
    let date_before_number =
        with_row(0, "on_device\t0\t0\tdevice\t2026-11-99").replacen("3100000", "3.1", 1);
    assert_eq!(refused(&date_before_number), PriceError::Number);
    let source_before_number_in_one_row = with_row(2, "frontier\tx\t78500000\t-\t2026-11-10");
    assert_eq!(
        refused(&source_before_number_in_one_row),
        PriceError::Number
    );
    let date_and_source =
        with_row(0, "on_device\t0\t0\tdevice\t2026-11-99").replacen("fixture:B-3.2", "-", 1);
    assert_eq!(refused(&date_and_source), PriceError::Source);
}

fn custom(low: (u64, u64), frontier: (u64, u64)) -> Prices {
    let text = format!(
        "tier\tinput\toutput\tsource\tdate\non_device\t0\t0\tdevice\t2026-11-10\nlow_cost_hosted\t{}\t{}\tx\t2026-11-10\nfrontier\t{}\t{}\tx\t2026-11-10\n",
        low.0, low.1, frontier.0, frontier.1
    );
    Prices::parse(&text).expect("the custom prices parse")
}

#[test]
fn cost_is_micro_units_per_million_tokens_rounded_up() {
    let prices = Prices::release();
    assert_eq!(prices.cost(Tier::LowCostHosted, 1_000, 1_000), 18_800);
    assert_eq!(prices.cost(Tier::LowCostHosted, 1_000_000, 0), 3_100_000);
    assert_eq!(prices.cost(Tier::LowCostHosted, 0, 1_000_000), 15_700_000);
    assert_eq!(prices.cost(Tier::LowCostHosted, 1, 0), 4);
    assert_eq!(prices.cost(Tier::LowCostHosted, 1, 1), 19);
    assert_eq!(prices.cost(Tier::LowCostHosted, 10, 0), 31);
    assert_eq!(prices.cost(Tier::Frontier, 1, 1), 94);
    assert_eq!(prices.cost(Tier::Frontier, 7, 3), 344);
    assert_eq!(prices.cost(Tier::Frontier, 0, 0), 0);
    assert_eq!(prices.cost(Tier::OnDevice, 1_000_000_000, 1_000_000_000), 0);
    assert_eq!(prices.cost(Tier::OnDevice, u64::MAX, u64::MAX), 0);
}

#[test]
fn cost_computes_in_u128_and_saturates() {
    let prices = Prices::release();
    assert_eq!(
        prices.cost(Tier::Frontier, 10_000_000_000_000, 0),
        155_000_000_000_000
    );
    assert_eq!(
        prices.cost(Tier::Frontier, 0, 10_000_000_000_000),
        785_000_000_000_000
    );
    assert_eq!(prices.cost(Tier::Frontier, u64::MAX, 0), u64::MAX);
    let top = custom((u64::MAX, u64::MAX), (1_000_000, 0));
    assert_eq!(top.cost(Tier::LowCostHosted, u64::MAX, u64::MAX), u64::MAX);
    assert_eq!(top.hold(Tier::LowCostHosted, u64::MAX, u64::MAX), u64::MAX);
    assert_eq!(top.cost(Tier::LowCostHosted, 1, 0), 18_446_744_073_710);
    assert_eq!(top.cost(Tier::Frontier, u64::MAX, 0), u64::MAX);
    let one = custom((1, 1), (999_999, 1_000_001));
    assert_eq!(one.cost(Tier::LowCostHosted, 1, 0), 1);
    assert_eq!(one.cost(Tier::LowCostHosted, 1_000_000, 0), 1);
    assert_eq!(one.cost(Tier::LowCostHosted, 1_000_001, 0), 2);
    assert_eq!(one.cost(Tier::Frontier, 1, 0), 1);
    assert_eq!(one.cost(Tier::Frontier, 0, 1), 2);
    assert_eq!(
        one.rate(Tier::Frontier),
        Rate {
            input: 999_999,
            output: 1_000_001
        }
    );
}

#[test]
fn hold_prices_the_body_bytes_and_the_output_limit() {
    let prices = Prices::release();
    assert_eq!(prices.hold(Tier::LowCostHosted, 131_072, 2_048), 438_477);
    assert_eq!(prices.hold(Tier::Frontier, 131_072, 2_048), 2_192_384);
    assert_eq!(prices.hold(Tier::LowCostHosted, 8_192, 256), 29_415);
    assert_eq!(prices.hold(Tier::OnDevice, 131_072, 2_048), 0);
    assert_eq!(prices.hold(Tier::Frontier, 0, 0), 0);
    for (bytes, tokens) in [(1, 1), (17, 0), (0, 9), (131_072, 2_048)] {
        for tier in Tier::ALL {
            assert_eq!(
                prices.hold(tier, bytes, tokens),
                prices.cost(tier, bytes, tokens)
            );
        }
    }
}
