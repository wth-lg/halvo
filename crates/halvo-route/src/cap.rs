//! The default cap of contract T10.4: the line `default_cap amount=<n> period=<word> status=<word>`.

use crate::tier::ParseError;

/// The period of a cap.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Period {
    Week,
    Month,
}

impl Period {
    pub const ALL: [Period; 2] = [Period::Week, Period::Month];

    pub const fn as_str(self) -> &'static str {
        match self {
            Period::Week => "week",
            Period::Month => "month",
        }
    }

    pub fn parse(text: &str) -> Result<Period, ParseError> {
        Period::ALL
            .into_iter()
            .find(|period| period.as_str() == text)
            .ok_or(ParseError)
    }
}

/// Whether a recorded cost stands behind the cap.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CapStatus {
    Provisional,
    Recorded,
}

impl CapStatus {
    pub const ALL: [CapStatus; 2] = [CapStatus::Provisional, CapStatus::Recorded];

    pub const fn as_str(self) -> &'static str {
        match self {
            CapStatus::Provisional => "provisional",
            CapStatus::Recorded => "recorded",
        }
    }

    pub fn parse(text: &str) -> Result<CapStatus, ParseError> {
        CapStatus::ALL
            .into_iter()
            .find(|status| status.as_str() == text)
            .ok_or(ParseError)
    }
}

/// Why a cap line is refused, in the order in which `DefaultCap::parse` checks.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CapError {
    Form,
    Amount,
    Period,
    Status,
}

impl CapError {
    pub const fn as_str(self) -> &'static str {
        match self {
            CapError::Form => "form",
            CapError::Amount => "amount",
            CapError::Period => "period",
            CapError::Status => "status",
        }
    }
}

/// The cap that a user has until the user sets one: `amount` minor units each `period`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct DefaultCap {
    pub amount: u64,
    pub period: Period,
    pub status: CapStatus,
}

impl DefaultCap {
    /// Reads `default_cap amount=<decimal digits> period=<word> status=<word>` with single spaces
    /// and returns the first error of this order: `form`, `amount` (outside 1 to 100,000,000),
    /// `period`, `status`.
    pub fn parse(line: &str) -> Result<DefaultCap, CapError> {
        let mut fields = line.split(' ');
        let (Some("default_cap"), Some(amount), Some(period), Some(status), None) = (
            fields.next(),
            fields
                .next()
                .and_then(|field| field.strip_prefix("amount=")),
            fields
                .next()
                .and_then(|field| field.strip_prefix("period=")),
            fields
                .next()
                .and_then(|field| field.strip_prefix("status=")),
            fields.next(),
        ) else {
            return Err(CapError::Form);
        };
        if !is_decimal(amount) || !is_word(period) || !is_word(status) {
            return Err(CapError::Form);
        }
        let amount = match amount.parse::<u64>() {
            Ok(amount) if (AMOUNT_MIN..=AMOUNT_MAX).contains(&amount) => amount,
            _ => return Err(CapError::Amount),
        };
        let period = Period::parse(period).map_err(|_| CapError::Period)?;
        let status = CapStatus::parse(status).map_err(|_| CapError::Status)?;
        Ok(DefaultCap {
            amount,
            period,
            status,
        })
    }
}

const AMOUNT_MIN: u64 = 1;
const AMOUNT_MAX: u64 = 100_000_000;

fn is_decimal(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'))
}

fn is_word(text: &str) -> bool {
    (1..=32).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tier::assert_closed;

    const LINE: &str = "default_cap amount=2000 period=month status=provisional";

    fn line_of(cap: &DefaultCap) -> String {
        format!(
            "default_cap amount={} period={} status={}",
            cap.amount,
            cap.period.as_str(),
            cap.status.as_str()
        )
    }

    #[test]
    fn cap_lists_are_closed() {
        assert_closed(&Period::ALL.map(|v| (v, v.as_str())), Period::parse);
        assert_eq!(Period::ALL.map(Period::as_str), ["week", "month"]);
        assert_closed(&CapStatus::ALL.map(|v| (v, v.as_str())), CapStatus::parse);
        assert_eq!(
            CapStatus::ALL.map(CapStatus::as_str),
            ["provisional", "recorded"]
        );
        assert_eq!(
            [
                CapError::Form,
                CapError::Amount,
                CapError::Period,
                CapError::Status
            ]
            .map(CapError::as_str),
            ["form", "amount", "period", "status"]
        );
    }

    #[test]
    fn default_cap_line() {
        assert_eq!(
            DefaultCap::parse(LINE),
            Ok(DefaultCap {
                amount: 2000,
                period: Period::Month,
                status: CapStatus::Provisional,
            })
        );
        for (line, amount, period, status) in [
            (
                "default_cap amount=1 period=week status=recorded",
                1,
                Period::Week,
                CapStatus::Recorded,
            ),
            (
                "default_cap amount=100000000 period=month status=recorded",
                100_000_000,
                Period::Month,
                CapStatus::Recorded,
            ),
            (
                "default_cap amount=99999999 period=week status=provisional",
                99_999_999,
                Period::Week,
                CapStatus::Provisional,
            ),
        ] {
            assert_eq!(
                DefaultCap::parse(line),
                Ok(DefaultCap {
                    amount,
                    period,
                    status
                }),
                "{line}"
            );
        }
    }

    #[test]
    fn default_cap_refusals() {
        use CapError::{Amount, Form, Period as P, Status as S};
        let cases: &[(&str, CapError)] = &[
            ("", Form),
            ("default_cap", Form),
            ("default_cap amount=2000 period=month", Form),
            ("default_cap amount=2000 period=month status=provisional x", Form),
            ("default_cap amount=2000 period=month status=provisional ", Form),
            (" default_cap amount=2000 period=month status=provisional", Form),
            ("default_cap  amount=2000 period=month status=provisional", Form),
            ("default_cap\tamount=2000 period=month status=provisional", Form),
            ("default_cap amount=2000 period=month status=provisional\n", Form),
            ("default_cap amount=2000 period=month status=provisional\r", Form),
            ("\u{feff}default_cap amount=2000 period=month status=provisional", Form),
            ("Default_cap amount=2000 period=month status=provisional", Form),
            ("default-cap amount=2000 period=month status=provisional", Form),
            ("cap amount=2000 period=month status=provisional", Form),
            ("default_cap period=month amount=2000 status=provisional", Form),
            ("default_cap amount=2000 status=provisional period=month", Form),
            ("default_cap amount 2000 period=month status=provisional", Form),
            ("default_cap amount= period=month status=provisional", Form),
            ("default_cap amount=02000 period=month status=provisional", Form),
            ("default_cap amount=00 period=month status=provisional", Form),
            ("default_cap amount=+2000 period=month status=provisional", Form),
            ("default_cap amount=-1 period=month status=provisional", Form),
            ("default_cap amount=2,000 period=month status=provisional", Form),
            ("default_cap amount=2000.0 period=month status=provisional", Form),
            ("default_cap amount=\u{0662}000 period=month status=provisional", Form),
            ("default_cap amount=2000 period= status=provisional", Form),
            ("default_cap amount=2000 period=month status=", Form),
            ("default_cap amount=2000 period=Month status=provisional", Form),
            ("default_cap amount=2000 period=month status=Provisional", Form),
            ("default_cap amount=2000 period=mon-th status=provisional", Form),
            ("default_cap amount=2000 period=month status=provisional=1", Form),
            (
                "default_cap amount=2000 period=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa status=provisional",
                Form,
            ),
            ("default_cap amount=0 period=x status=y", Amount),
            ("default_cap amount=0 period=month status=provisional", Amount),
            ("default_cap amount=100000001 period=month status=provisional", Amount),
            ("default_cap amount=4294967296 period=month status=provisional", Amount),
            (
                "default_cap amount=18446744073709551615 period=month status=provisional",
                Amount,
            ),
            (
                "default_cap amount=18446744073709551616 period=month status=provisional",
                Amount,
            ),
            (
                "default_cap amount=99999999999999999999999999 period=month status=provisional",
                Amount,
            ),
            ("default_cap amount=2000 period=day status=provisional", P),
            ("default_cap amount=2000 period=year status=y", P),
            ("default_cap amount=2000 period=weeks status=provisional", P),
            (
                "default_cap amount=2000 period=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa status=provisional",
                P,
            ),
            ("default_cap amount=2000 period=month status=final", S),
            ("default_cap amount=2000 period=week status=provisional_", S),
            ("default_cap amount=1 period=month status=0", S),
        ];
        for (line, error) in cases {
            assert_eq!(DefaultCap::parse(line), Err(*error), "{line:?}");
        }
    }

    #[test]
    fn default_cap_takes_only_its_form() {
        let bytes = LINE.as_bytes();
        let mut lines = Vec::new();
        for at in 0..=bytes.len() {
            for byte in 0..=127u8 {
                let mut inserted = bytes.to_vec();
                inserted.insert(at, byte);
                lines.push(inserted);
                if at < bytes.len() {
                    let mut replaced = bytes.to_vec();
                    replaced[at] = byte;
                    lines.push(replaced);
                }
            }
            if at < bytes.len() {
                let mut removed = bytes.to_vec();
                removed.remove(at);
                lines.push(removed);
                lines.push(bytes[..at].to_vec());
            }
        }
        let mut taken = std::collections::BTreeSet::new();
        for line in &lines {
            let line = std::str::from_utf8(line).expect("ascii");
            if let Ok(cap) = DefaultCap::parse(line) {
                assert_eq!(line_of(&cap), line);
                taken.insert(cap.amount);
            }
        }
        assert_eq!(taken.len(), 82);
        assert!(taken.iter().all(|amount| (200..=92_000).contains(amount)));
    }
}
