use halvo_route::prices::{Prices, Rate, MICRO_PER_MINOR};
use halvo_route::tier::Tier;

const DATA: &str = include_str!("../data/prices.tsv");

/// The cells of the row of `tier` in the compiled-in price file.
fn cells(tier: Tier) -> Vec<&'static str> {
    DATA.lines()
        .nth(1 + tier as usize)
        .expect("a row of the tier")
        .split('\t')
        .collect()
}

/// The compiled-in price file parses, `Prices::release()` is what it parses to, the rate of `on_device` is 0 and 0, and the
/// rate of each hosted tier is the two numbers of that tier's row, whatever they are: the test holds no rate of a host.
#[test]
fn price_data_parses() {
    let parsed = Prices::parse(DATA).expect("the compiled-in price file parses");
    assert_eq!(&parsed, Prices::release());
    assert_eq!(
        Prices::release().rate(Tier::OnDevice),
        Rate {
            input: 0,
            output: 0
        }
    );
    for tier in [Tier::LowCostHosted, Tier::Frontier] {
        let row = cells(tier);
        assert_eq!(row[0], tier.as_str());
        assert_eq!(
            Prices::release().rate(tier),
            Rate {
                input: row[1].parse().expect("an input rate"),
                output: row[2].parse().expect("an output rate"),
            },
            "{tier:?}"
        );
    }
}

/// The compiled-in price file has the form of contract T10.1: its header, the row `on_device` with the rates 0 and 0, the
/// source `device` and the date 2026-11-10, then one row of each hosted tier in that order; a minor unit is 10,000
/// micro-units. The rates, sources and dates of the hosted rows are the file's own (`price_data_parses`).
#[test]
fn price_data_is_the_text_of_the_contract() {
    let lines: Vec<&str> = DATA.lines().collect();
    assert_eq!(lines.len(), 4, "{DATA:?}");
    assert_eq!(lines[0], "tier\tinput\toutput\tsource\tdate");
    assert_eq!(lines[1], "on_device\t0\t0\tdevice\t2026-11-10");
    assert!(lines[2].starts_with("low_cost_hosted\t"), "{DATA:?}");
    assert!(lines[3].starts_with("frontier\t"), "{DATA:?}");
    assert!(DATA.ends_with('\n'));
    assert_eq!(MICRO_PER_MINOR, 10_000);
}
