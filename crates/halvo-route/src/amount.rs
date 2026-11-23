//! How Halvo writes an amount of money for a person (r7 8.1L; the coordinator's log line 17895, V1): the units with
//! exactly two decimals, a blank and the currency word in capitals, as "20.00 USD", with no sign and no thousands
//! mark. A limit comes in minor units; what is spent and what is held come in micro-units (`MICRO_PER_MINOR` to a
//! minor unit) and are rounded up to the next minor unit, so that a page never shows less than the count holds. This
//! is the one writer of an amount for a person: a page gets its texts and does no arithmetic.

use crate::cap::Currency;
use crate::prices::{Micro, MICRO_PER_MINOR};

/// An amount in the unit in which the count keeps it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Amount {
    /// Minor units of the currency, as a cap is kept.
    Minor(u64),
    /// Micro-units, as what is spent and what is held are kept.
    Micro(Micro),
}

/// The amount written for a person in `currency`: "20.00 USD" for `Amount::Minor(2000)`, "0.01 USD" for
/// `Amount::Micro(1)`.
pub fn written(amount: Amount, currency: Currency) -> String {
    let minor = match amount {
        Amount::Minor(minor) => u128::from(minor),
        Amount::Micro(micro) => u128::from(micro).div_ceil(u128::from(MICRO_PER_MINOR)),
    };
    format!("{}.{:02} {}", minor / 100, minor % 100, currency.code())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// r7 8.1L view, sentence (1): an amount is written with two decimals and the currency word, and what is spent
    /// or held is rounded up to the next minor unit.
    #[test]
    fn an_amount_is_written_with_two_decimals_and_micro_units_are_rounded_up() {
        let usd = Currency::Usd;
        let rows = [
            (Amount::Minor(0), "0.00 USD"),
            (Amount::Micro(0), "0.00 USD"),
            (Amount::Micro(1), "0.01 USD"),
            (Amount::Micro(10_000), "0.01 USD"),
            (Amount::Micro(10_001), "0.02 USD"),
            (Amount::Minor(2_000), "20.00 USD"),
            (Amount::Minor(100_000_000), "1000000.00 USD"),
            (Amount::Minor(u64::MAX), "184467440737095516.15 USD"),
            (Amount::Micro(u64::MAX), "18446744073709.56 USD"),
        ];
        for (amount, text) in rows {
            assert_eq!(written(amount, usd), text, "{amount:?}");
        }
    }
}
