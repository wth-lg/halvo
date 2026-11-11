use halvo_route::prices::{Prices, Rate, MICRO_PER_MINOR};
use halvo_route::tier::Tier;

const DATA: &str = include_str!("../data/prices.tsv");

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
    assert_eq!(
        Prices::release().rate(Tier::LowCostHosted),
        Rate {
            input: 3_100_000,
            output: 15_700_000
        }
    );
    assert_eq!(
        Prices::release().rate(Tier::Frontier),
        Rate {
            input: 15_500_000,
            output: 78_500_000
        }
    );
}

#[test]
fn price_data_is_the_text_of_the_contract() {
    assert_eq!(
        DATA,
        "tier\tinput\toutput\tsource\tdate\n\
         on_device\t0\t0\tdevice\t2026-11-10\n\
         low_cost_hosted\t3100000\t15700000\tfixture:B-3.2\t2026-11-10\n\
         frontier\t15500000\t78500000\tfixture:B-3.2\t2026-11-10\n"
    );
    assert_eq!(MICRO_PER_MINOR, 10_000);
}
