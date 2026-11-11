//! Prices of contract T10: the compiled-in price file, `cost` and `hold`, in whole micro-units.
//!
//! A rate is micro-units per million tokens. Every sum and product of amounts is computed in
//! `u128`, and a result that does not fit a `u64` is `u64::MAX`.

use std::sync::OnceLock;

use crate::tier::Tier;

/// An amount in whole micro-units.
pub type Micro = u64;

/// Micro-units in one minor unit of a currency.
pub const MICRO_PER_MINOR: Micro = 10_000;

const HEADER: &str = "tier\tinput\toutput\tsource\tdate";
const CELLS: usize = 5;
const PER_MILLION: u128 = 1_000_000;

/// Why a price file is refused; `Prices::parse` returns the first of this order that fits the text.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PriceError {
    Header,
    Rows,
    Number,
    Source,
    Date,
}

impl PriceError {
    pub const fn as_str(self) -> &'static str {
        match self {
            PriceError::Header => "header",
            PriceError::Rows => "rows",
            PriceError::Number => "number",
            PriceError::Source => "source",
            PriceError::Date => "date",
        }
    }
}

/// The two rates of a tier, in micro-units per million tokens.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Rate {
    pub input: Micro,
    pub output: Micro,
}

/// The rates of the three tiers, read from a price file whose every rate has a source and a date,
/// or, from `Prices::release()` alone, the mark that the compiled-in file did not parse.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Prices {
    rates: Rates,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Rates {
    Read([Rate; 3]),
    Unread,
}

impl Prices {
    /// Reads a price file: the header, then one row per tier in the order of `Tier::ALL`.
    pub fn parse(text: &str) -> Result<Prices, PriceError> {
        let first = text.split('\n').next().unwrap_or_default();
        if first != HEADER {
            return Err(PriceError::Header);
        }
        if text.contains('\r') || !text.ends_with('\n') {
            return Err(PriceError::Rows);
        }
        let rows: Vec<Vec<&str>> = text[HEADER.len() + 1..]
            .split_terminator('\n')
            .map(|line| line.split('\t').collect())
            .collect();
        let tiers_in_order = rows.len() == Tier::ALL.len()
            && rows
                .iter()
                .zip(Tier::ALL)
                .all(|(cells, tier)| cells.len() == CELLS && cells[0] == tier.as_str());
        if !tiers_in_order {
            return Err(PriceError::Rows);
        }
        let mut rates = [Rate {
            input: 0,
            output: 0,
        }; 3];
        for ((rate, cells), tier) in rates.iter_mut().zip(&rows).zip(Tier::ALL) {
            let (Some(input), Some(output)) = (decimal(cells[1]), decimal(cells[2])) else {
                return Err(PriceError::Number);
            };
            if tier == Tier::OnDevice && (input, output) != (0, 0) {
                return Err(PriceError::Number);
            }
            *rate = Rate { input, output };
        }
        if rows
            .iter()
            .any(|cells| cells[3].is_empty() || cells[3] == "-" || cells[3].contains(['"', ' ']))
        {
            return Err(PriceError::Source);
        }
        if !rows.iter().all(|cells| is_date(cells[4])) {
            return Err(PriceError::Date);
        }
        Ok(Prices {
            rates: Rates::Read(rates),
        })
    }

    /// The compiled-in price file, `crates/halvo-route/data/prices.tsv`, read once. When it does
    /// not parse, `cost` and `hold` answer `u64::MAX` for each hosted tier, whatever the token
    /// counts, and 0 for `on_device`, so no hosted call fits a cap and none is free.
    pub fn release() -> &'static Prices {
        static RELEASE: OnceLock<Prices> = OnceLock::new();
        RELEASE.get_or_init(|| release_of(include_str!("../data/prices.tsv")))
    }

    /// The two rates of a tier; when the compiled-in file did not parse, `u64::MAX` for a hosted
    /// tier and 0 for `on_device`.
    pub const fn rate(&self, tier: Tier) -> Rate {
        match self.rates {
            Rates::Read(rates) => rates[tier as usize],
            Rates::Unread => match tier {
                Tier::OnDevice => Rate {
                    input: 0,
                    output: 0,
                },
                Tier::LowCostHosted | Tier::Frontier => Rate {
                    input: u64::MAX,
                    output: u64::MAX,
                },
            },
        }
    }

    /// `tokens_in` times the input rate plus `tokens_out` times the output rate, over one million,
    /// rounded up.
    pub const fn cost(&self, tier: Tier, tokens_in: u64, tokens_out: u64) -> Micro {
        self.amount(tier, tokens_in, tokens_out)
    }

    /// `body_bytes` times the input rate plus `max_output_tokens` times the output rate, over one
    /// million, rounded up: the amount that the gateway holds for one choice.
    pub const fn hold(&self, tier: Tier, body_bytes: u64, max_output_tokens: u64) -> Micro {
        self.amount(tier, body_bytes, max_output_tokens)
    }

    const fn amount(&self, tier: Tier, input: u64, output: u64) -> Micro {
        match (self.rates, tier) {
            (Rates::Read(rates), _) => {
                let rate = rates[tier as usize];
                per_million_rounded_up(input, rate.input, output, rate.output)
            }
            (Rates::Unread, Tier::OnDevice) => 0,
            (Rates::Unread, Tier::LowCostHosted | Tier::Frontier) => u64::MAX,
        }
    }
}

fn release_of(text: &str) -> Prices {
    Prices::parse(text).unwrap_or(Prices {
        rates: Rates::Unread,
    })
}

const fn per_million_rounded_up(a: u64, rate_a: u64, b: u64, rate_b: u64) -> Micro {
    let Some(total) = (a as u128 * rate_a as u128).checked_add(b as u128 * rate_b as u128) else {
        return u64::MAX;
    };
    let micro = total / PER_MILLION + if total % PER_MILLION == 0 { 0 } else { 1 };
    if micro > u64::MAX as u128 {
        u64::MAX
    } else {
        micro as u64
    }
}

fn decimal(cell: &str) -> Option<u64> {
    let canonical = !cell.is_empty()
        && cell.bytes().all(|b| b.is_ascii_digit())
        && (cell == "0" || !cell.starts_with('0'));
    if canonical {
        cell.parse().ok()
    } else {
        None
    }
}

fn is_date(cell: &str) -> bool {
    let bytes = cell.as_bytes();
    let form = bytes.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| i == 4 || i == 7 || b.is_ascii_digit());
    if !form {
        return false;
    }
    let number = |range: std::ops::Range<usize>| {
        bytes[range]
            .iter()
            .fold(0u32, |n, b| n * 10 + u32::from(b - b'0'))
    };
    let (year, month, day) = (number(0..4), number(5..7), number(8..10));
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_price_file_that_does_not_parse_makes_every_hosted_call_impossible() {
        let text = include_str!("../data/prices.tsv").replacen("2026-11-10", "2026-11-31", 1);
        assert_eq!(Prices::parse(&text), Err(PriceError::Date));
        let broken = release_of(&text);
        let counts = [0, 1, u64::MAX];
        for tokens_in in counts {
            for tokens_out in counts {
                for tier in [Tier::LowCostHosted, Tier::Frontier] {
                    assert_eq!(broken.cost(tier, tokens_in, tokens_out), u64::MAX);
                    assert_eq!(broken.hold(tier, tokens_in, tokens_out), u64::MAX);
                }
                assert_eq!(broken.cost(Tier::OnDevice, tokens_in, tokens_out), 0);
                assert_eq!(broken.hold(Tier::OnDevice, tokens_in, tokens_out), 0);
            }
        }
    }
}
