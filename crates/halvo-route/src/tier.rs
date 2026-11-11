//! The closed lists of contract T1: `Tier`, `CallKind`, `Level` and `CategoryId`.
//!
//! Each list is complete and stands in its contract order in `ALL`, which is also the order of
//! `Ord`. `as_str` returns the list's word, and `parse` takes exactly those words and refuses every
//! other value with `ParseError`.

/// The refusal of every `parse` of a closed list: the text is no word of the list.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ParseError;

/// A role with one price class. `Tier::ALL` and `Ord` run from the cheapest class to the strongest.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Tier {
    OnDevice,
    LowCostHosted,
    Frontier,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::OnDevice, Tier::LowCostHosted, Tier::Frontier];

    pub const fn as_str(self) -> &'static str {
        match self {
            Tier::OnDevice => "on_device",
            Tier::LowCostHosted => "low_cost_hosted",
            Tier::Frontier => "frontier",
        }
    }

    pub fn parse(text: &str) -> Result<Tier, ParseError> {
        Tier::ALL
            .into_iter()
            .find(|tier| tier.as_str() == text)
            .ok_or(ParseError)
    }
}

/// What a step asks of a model; `replay` calls none.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum CallKind {
    Compile,
    Recover,
    Write,
    Replay,
}

impl CallKind {
    pub const ALL: [CallKind; 4] = [
        CallKind::Compile,
        CallKind::Recover,
        CallKind::Write,
        CallKind::Replay,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            CallKind::Compile => "compile",
            CallKind::Recover => "recover",
            CallKind::Write => "write",
            CallKind::Replay => "replay",
        }
    }

    pub fn parse(text: &str) -> Result<CallKind, ParseError> {
        CallKind::ALL
            .into_iter()
            .find(|kind| kind.as_str() == text)
            .ok_or(ParseError)
    }
}

/// The reasoning level that the attempts of a row ran at.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Level {
    Low,
    Medium,
    High,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Low, Level::Medium, Level::High];

    pub const fn as_str(self) -> &'static str {
        match self {
            Level::Low => "low",
            Level::Medium => "medium",
            Level::High => "high",
        }
    }

    pub fn parse(text: &str) -> Result<Level, ParseError> {
        Level::ALL
            .into_iter()
            .find(|level| level.as_str() == text)
            .ok_or(ParseError)
    }
}

/// One of the 11 task categories, in the order of the workflow run's list.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum CategoryId {
    UtilityBillPayment,
    TenantRent,
    LeaseRenewalOrChange,
    TenantApplicationReview,
    Email,
    SpreadsheetFill,
    ShopOrder,
    CalendarCleanup,
    RideOrder,
    DailyChartCheck,
    Other,
}

impl CategoryId {
    pub const ALL: [CategoryId; 11] = [
        CategoryId::UtilityBillPayment,
        CategoryId::TenantRent,
        CategoryId::LeaseRenewalOrChange,
        CategoryId::TenantApplicationReview,
        CategoryId::Email,
        CategoryId::SpreadsheetFill,
        CategoryId::ShopOrder,
        CategoryId::CalendarCleanup,
        CategoryId::RideOrder,
        CategoryId::DailyChartCheck,
        CategoryId::Other,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            CategoryId::UtilityBillPayment => "utility_bill_payment",
            CategoryId::TenantRent => "tenant_rent",
            CategoryId::LeaseRenewalOrChange => "lease_renewal_or_change",
            CategoryId::TenantApplicationReview => "tenant_application_review",
            CategoryId::Email => "email",
            CategoryId::SpreadsheetFill => "spreadsheet_fill",
            CategoryId::ShopOrder => "shop_order",
            CategoryId::CalendarCleanup => "calendar_cleanup",
            CategoryId::RideOrder => "ride_order",
            CategoryId::DailyChartCheck => "daily_chart_check",
            CategoryId::Other => "other",
        }
    }

    pub fn parse(text: &str) -> Result<CategoryId, ParseError> {
        CategoryId::ALL
            .into_iter()
            .find(|category| category.as_str() == text)
            .ok_or(ParseError)
    }

    /// False for `other` alone.
    pub const fn is_seed(self) -> bool {
        !matches!(self, CategoryId::Other)
    }

    /// True for the five categories whose preset has a runtime in the agent-setup run.
    pub const fn has_runtime(self) -> bool {
        matches!(
            self,
            CategoryId::UtilityBillPayment
                | CategoryId::Email
                | CategoryId::SpreadsheetFill
                | CategoryId::ShopOrder
                | CategoryId::CalendarCleanup
        )
    }
}

#[cfg(test)]
pub(crate) fn assert_closed<T: Copy + Eq + std::fmt::Debug>(
    words: &[(T, &str)],
    parse: fn(&str) -> Result<T, ParseError>,
) {
    let listed = |text: &str| {
        words
            .iter()
            .find(|(_, word)| *word == text)
            .map(|(value, _)| *value)
    };
    let check = |text: &str| match listed(text) {
        Some(value) => assert_eq!(parse(text), Ok(value), "{text:?}"),
        None => assert_eq!(parse(text), Err(ParseError), "{text:?}"),
    };
    for (i, (value, word)) in words.iter().enumerate() {
        assert!((1..=32).contains(&word.len()), "{word}");
        assert!(
            word.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
            "{word}"
        );
        assert!(
            words[..i]
                .iter()
                .all(|(other, w)| other != value && w != word),
            "{word} twice"
        );
        assert_eq!(parse(word), Ok(*value));
        let bytes = word.as_bytes();
        for at in 0..=bytes.len() {
            for byte in 0..=127u8 {
                let mut inserted = bytes.to_vec();
                inserted.insert(at, byte);
                check(std::str::from_utf8(&inserted).expect("ascii"));
                if at < bytes.len() {
                    let mut replaced = bytes.to_vec();
                    replaced[at] = byte;
                    check(std::str::from_utf8(&replaced).expect("ascii"));
                }
            }
            check(&word[..at]);
            check(&word[at..]);
        }
        for text in [
            word.to_uppercase(),
            format!(" {word}"),
            format!("{word} "),
            format!("{word}\n"),
            format!("{word}\u{e9}"),
            word.replace('_', "-"),
        ] {
            check(&text);
        }
    }
    for text in ["", "-", " ", "\t", "none ", "\u{feff}"] {
        check(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_list_is_closed() {
        assert_closed(&Tier::ALL.map(|v| (v, v.as_str())), Tier::parse);
        assert_eq!(
            Tier::ALL.map(Tier::as_str),
            ["on_device", "low_cost_hosted", "frontier"]
        );
        assert!(Tier::ALL.windows(2).all(|pair| pair[0] < pair[1]));
        for tier in Tier::ALL {
            match tier {
                Tier::OnDevice | Tier::LowCostHosted | Tier::Frontier => {}
            }
        }
    }

    #[test]
    fn call_kind_list_is_closed() {
        assert_closed(&CallKind::ALL.map(|v| (v, v.as_str())), CallKind::parse);
        assert_eq!(
            CallKind::ALL.map(CallKind::as_str),
            ["compile", "recover", "write", "replay"]
        );
        assert!(CallKind::ALL.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn level_list_is_closed() {
        assert_closed(&Level::ALL.map(|v| (v, v.as_str())), Level::parse);
        assert_eq!(Level::ALL.map(Level::as_str), ["low", "medium", "high"]);
    }

    #[test]
    fn category_list_is_closed() {
        assert_closed(&CategoryId::ALL.map(|v| (v, v.as_str())), CategoryId::parse);
        assert_eq!(
            CategoryId::ALL.map(CategoryId::as_str),
            [
                "utility_bill_payment",
                "tenant_rent",
                "lease_renewal_or_change",
                "tenant_application_review",
                "email",
                "spreadsheet_fill",
                "shop_order",
                "calendar_cleanup",
                "ride_order",
                "daily_chart_check",
                "other",
            ]
        );
        assert!(CategoryId::ALL.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn seed_and_runtime_lists() {
        let seed: Vec<&str> = CategoryId::ALL
            .into_iter()
            .filter(|c| !c.is_seed())
            .map(CategoryId::as_str)
            .collect();
        assert_eq!(seed, ["other"]);
        let runtime: Vec<&str> = CategoryId::ALL
            .into_iter()
            .filter(|c| c.has_runtime())
            .map(CategoryId::as_str)
            .collect();
        assert_eq!(
            runtime,
            [
                "utility_bill_payment",
                "email",
                "spreadsheet_fill",
                "shop_order",
                "calendar_cleanup"
            ]
        );
    }
}
