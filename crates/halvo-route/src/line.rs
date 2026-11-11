//! The line of contract T4.1: a tier meets it for a cell on a set of rows when the set holds at
//! least a least number of attempts and every one of them passed.

/// What a set of rows shows against the line.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Line {
    Untested,
    Met,
    Missed,
}

impl Line {
    pub const ALL: [Line; 3] = [Line::Untested, Line::Met, Line::Missed];

    pub const fn as_str(self) -> &'static str {
        match self {
            Line::Untested => "untested",
            Line::Met => "met",
            Line::Missed => "missed",
        }
    }
}

/// The least number of development attempts.
pub const LINE_DEV_MIN: u32 = 4;

/// The least number of held-out attempts.
pub const LINE_HELD_MIN: u32 = 2;

/// `Untested` when `n` is under `min`, `Met` when `k` equals `n`, else `Missed`.
pub const fn meets(k: u32, n: u32, min: u32) -> Line {
    if n < min {
        Line::Untested
    } else if k == n {
        Line::Met
    } else {
        Line::Missed
    }
}

const LOWER_PCT: [u32; 36] = [
    5, 22, 36, 47, 54, 60, 65, 68, 71, 74, 76, 77, 79, 80, 81, 82, 83, 84, 85, 86, 86, 87, 87, 88,
    88, 89, 89, 89, 90, 90, 90, 91, 91, 91, 91, 92,
];

/// The floor of 100 times 0.05 to the power 1/n, the one-sided 95 percent lower bound in percent of
/// the pass rate after `n` attempts with no miss: 0 for `n` = 0, and 92 for every `n` above 36.
pub const fn lower_pct(n: u32) -> u32 {
    match n {
        0 => 0,
        1..=36 => LOWER_PCT[n as usize - 1],
        _ => 92,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn times(digits: &mut Vec<u32>, factor: u32) {
        let mut carry = 0u64;
        for digit in digits.iter_mut() {
            let product = u64::from(*digit) * u64::from(factor) + carry;
            *digit = u32::try_from(product % 1_000_000_000).expect("below 10^9");
            carry = product / 1_000_000_000;
        }
        while carry > 0 {
            digits.push(u32::try_from(carry % 1_000_000_000).expect("below 10^9"));
            carry /= 1_000_000_000;
        }
    }

    fn power_times(base: u32, exponent: u32, factor: u32) -> Vec<u32> {
        let mut digits = vec![factor];
        for _ in 0..exponent {
            times(&mut digits, base);
        }
        digits
    }

    fn at_most(a: &[u32], b: &[u32]) -> bool {
        if a.len() != b.len() {
            return a.len() < b.len();
        }
        a.iter().rev().cmp(b.iter().rev()) != std::cmp::Ordering::Greater
    }

    #[test]
    fn lower_table() {
        for n in 1..=36u32 {
            let float = (100.0 * 0.05f64.powf(1.0 / f64::from(n))).floor();
            assert_eq!(f64::from(lower_pct(n)), float, "n={n} in floating point");
            let v = lower_pct(n);
            let bar = power_times(100, n, 1);
            assert!(
                at_most(&power_times(v, n, 20), &bar),
                "n={n}: {v} is above the bound"
            );
            assert!(
                !at_most(&power_times(v + 1, n, 20), &bar),
                "n={n}: {} is not above the bound",
                v + 1
            );
        }
        assert_eq!(lower_pct(0), 0);
        assert_eq!(lower_pct(6), 60);
        assert_eq!(lower_pct(18), 84);
        for n in [37, 38, 100, 1_000, u32::MAX] {
            assert_eq!(lower_pct(n), 92);
        }
    }

    #[test]
    fn line_constants() {
        assert_eq!(LINE_DEV_MIN, 4);
        assert_eq!(LINE_HELD_MIN, 2);
        assert_eq!(Line::ALL.map(Line::as_str), ["untested", "met", "missed"]);
    }

    #[test]
    fn meets_rule() {
        assert_eq!(meets(3, 3, LINE_DEV_MIN), Line::Untested);
        assert_eq!(meets(0, 0, LINE_DEV_MIN), Line::Untested);
        assert_eq!(meets(4, 4, LINE_DEV_MIN), Line::Met);
        assert_eq!(meets(3, 4, LINE_DEV_MIN), Line::Missed);
        assert_eq!(meets(0, 4, LINE_DEV_MIN), Line::Missed);
        assert_eq!(meets(12, 12, LINE_DEV_MIN), Line::Met);
        assert_eq!(meets(1, 1, LINE_HELD_MIN), Line::Untested);
        assert_eq!(meets(2, 2, LINE_HELD_MIN), Line::Met);
        assert_eq!(meets(1, 2, LINE_HELD_MIN), Line::Missed);
        assert_eq!(meets(5, 4, LINE_DEV_MIN), Line::Missed);
    }
}
