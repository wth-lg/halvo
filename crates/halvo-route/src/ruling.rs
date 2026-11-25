//! The ruling of contract T1.5: the owner's word on what may leave the machine, held as one line
//! and the table of hosts that the line `ruling=hosted` is read with.
//!
//! This file is pinned whole, its test module included, as lib.rs, leave.rs and tier.rs are: the
//! test `pinned_files_hold_their_hashes` of tests/no_grant_in_release.rs holds its SHA-256. A pin
//! stops no change. It makes each change of the file come with a new hash line in the same commit,
//! where the reviewer of that commit reads it.

use crate::tier::Tier;

const HEADER: &str = "tier\torigin\tmodel\tadapter\tsource\tdate";
const CELLS: usize = 6;
const WORD_MAX: usize = 64;

/// The refusal of `RulingLine::parse`: the line is not a ruling line.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct RulingError;

impl RulingError {
    pub const fn as_str(self) -> &'static str {
        "ruling"
    }
}

/// The two lines that a ruling file may hold. A line names no host and makes no ruling: a hosted
/// ruling is read from the line `ruling=hosted` with the table of hosts.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum RulingLine {
    None,
    Hosted,
}

impl RulingLine {
    /// `None` for the line `ruling=none`, `Hosted` for the line `ruling=hosted`, and `RulingError`
    /// for every other line.
    pub fn parse(line: &str) -> Result<RulingLine, RulingError> {
        match line {
            "ruling=none" => Ok(RulingLine::None),
            "ruling=hosted" => Ok(RulingLine::Hosted),
            _ => Err(RulingError),
        }
    }
}

/// Why a table of hosts is refused; `check_hosts` returns the first of this order that fits the
/// text.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum HostsError {
    Header,
    Rows,
    Tier,
    Origin,
    Word,
    Source,
    Date,
}

impl HostsError {
    pub const fn as_str(self) -> &'static str {
        match self {
            HostsError::Header => "header",
            HostsError::Rows => "rows",
            HostsError::Tier => "tier",
            HostsError::Origin => "origin",
            HostsError::Word => "word",
            HostsError::Source => "source",
            HostsError::Date => "date",
        }
    }
}

/// A row of a hosted ruling: a hosted tier, the one origin that a request of that tier may go to,
/// and the model word and the adapter word of its requests. Its fields are private, so no file but
/// this one makes a row; the row of a grant is a copy of a row of its ruling.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct HostRow {
    tier: Tier,
    origin: String,
    model: String,
    adapter: String,
}

impl HostRow {
    pub fn tier(&self) -> Tier {
        self.tier
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn adapter(&self) -> &str {
        &self.adapter
    }
}

/// What the owner has ruled, with two values in every build. Under `None` no tier but `on_device`
/// may be picked and no origin may be reached. `Hosted` holds the rows of a table of hosts, at
/// least one, and nothing else: no file but this one can name the payload's type `HostRows` or set
/// its rows, as the type stands in a module private to this file. Two functions make a hosted
/// value and no third does: `release::ruling()`, through `Ruling::read_with_table`, which is
/// private to the crate and takes no origin but `https://` and a host name, and
/// `Ruling::loopback_for_tests`, whose one origin is `http://127.0.0.1:<port>`. Other files can
/// name the variant `Ruling::Hosted`, and hold, clone or make again a value that one of the two
/// made.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum Ruling {
    None,
    Hosted(payload::HostRows),
}

mod payload {
    /// The payload of a hosted ruling: the rows of a table of hosts, at least one, and nothing
    /// else. No file but ruling.rs can name this type or set its one field.
    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    pub struct HostRows {
        pub(super) rows: Vec<super::HostRow>,
    }
}

impl Ruling {
    /// The tiers that a pick may name: `on_device` and each tier for which the ruling holds a row,
    /// so `on_device` alone for `None`.
    pub fn allowed(&self) -> &'static [Tier] {
        match (
            self.row(Tier::LowCostHosted).is_some(),
            self.row(Tier::Frontier).is_some(),
        ) {
            (true, true) => &Tier::ALL,
            (true, false) => &[Tier::OnDevice, Tier::LowCostHosted],
            (false, true) => &[Tier::OnDevice, Tier::Frontier],
            (false, false) => &[Tier::OnDevice],
        }
    }

    /// The rows that the ruling holds, in the order of its table: none for `None`.
    pub fn rows(&self) -> &[HostRow] {
        match self {
            Ruling::None => &[],
            Ruling::Hosted(hosted) => &hosted.rows,
        }
    }

    /// The row of `tier`, when the ruling holds one.
    pub fn row(&self, tier: Tier) -> Option<&HostRow> {
        self.rows().iter().find(|row| row.tier == tier)
    }

    /// The reader of a ruling line with the text of a table of hosts: `Hosted` with the rows of
    /// `table` when `line` is `ruling=hosted` and `table` is a table of hosts with at least one
    /// row, else `None`. So a line that does not parse, a table that does not parse,
    /// `ruling=hosted` over a table with no row and `ruling=none` over any table each read as
    /// `None`. It is private to the crate: a public one would make a hosted value from any text of
    /// a caller.
    pub(crate) fn read_with_table(line: &str, table: &str) -> Ruling {
        match (RulingLine::parse(line), host_rows(table)) {
            (Ok(RulingLine::Hosted), Ok(rows)) if !rows.is_empty() => {
                Ruling::Hosted(payload::HostRows { rows })
            }
            _ => Ruling::None,
        }
    }

    /// The one public maker of a hosted value, for a test that runs a sender against a stand-in on
    /// the loopback: one row for each hosted tier, each with the origin `http://127.0.0.1:<port>`,
    /// which is made from `port` alone, and with `model` and `adapter` as its model word and its
    /// adapter word. When `model` or `adapter` is not a word of the table (1 to 64 ASCII letters,
    /// digits, `-`, `.`, `_` or `:`), the value is `None`. It stands in every build, the product's
    /// too; that no product code names it is held by a rule of the tree alone, the test
    /// `the_loopback_maker_stands_only_in_its_own_file_and_listed_test_files` of
    /// tests/no_grant_in_release.rs.
    pub fn loopback_for_tests(port: u16, model: &str, adapter: &str) -> Ruling {
        if !is_word(model) || !is_word(adapter) {
            return Ruling::None;
        }
        let origin = format!("http://127.0.0.1:{port}");
        let rows = [Tier::LowCostHosted, Tier::Frontier]
            .into_iter()
            .map(|tier| HostRow {
                tier,
                origin: origin.clone(),
                model: model.to_owned(),
                adapter: adapter.to_owned(),
            })
            .collect();
        Ruling::Hosted(payload::HostRows { rows })
    }
}

/// The number of rows of a table of hosts, or the first refusal of `HostsError` that fits its
/// text. It gives back no ruling.
pub fn check_hosts(table: &str) -> Result<usize, HostsError> {
    host_rows(table).map(|rows| rows.len())
}

/// Reads a table of hosts: the header, then rows of six cells, each line ended by one LF, with no
/// CR. A row names a hosted tier, and a tier has one row at most; its origin is `https://` and a
/// host name alone (`is_origin`); its model and adapter cells are words (`is_word`); its source and
/// date cells follow the rule of src/prices.rs for its two columns of those names.
fn host_rows(text: &str) -> Result<Vec<HostRow>, HostsError> {
    let first = text.split('\n').next().unwrap_or_default();
    if first != HEADER {
        return Err(HostsError::Header);
    }
    if text.contains('\r') || !text.ends_with('\n') {
        return Err(HostsError::Rows);
    }
    let lines: Vec<Vec<&str>> = text[HEADER.len() + 1..]
        .split_terminator('\n')
        .map(|line| line.split('\t').collect())
        .collect();
    if lines.iter().any(|cells| cells.len() != CELLS) {
        return Err(HostsError::Rows);
    }
    let mut tiers = Vec::new();
    for cells in &lines {
        match Tier::parse(cells[0]) {
            Ok(tier) if tier != Tier::OnDevice && !tiers.contains(&tier) => tiers.push(tier),
            _ => return Err(HostsError::Tier),
        }
    }
    if !lines.iter().all(|cells| is_origin(cells[1])) {
        return Err(HostsError::Origin);
    }
    if !lines
        .iter()
        .all(|cells| is_word(cells[2]) && is_word(cells[3]))
    {
        return Err(HostsError::Word);
    }
    if lines
        .iter()
        .any(|cells| cells[4].is_empty() || cells[4] == "-" || cells[4].contains(['"', ' ']))
    {
        return Err(HostsError::Source);
    }
    if !lines.iter().all(|cells| is_date(cells[5])) {
        return Err(HostsError::Date);
    }
    Ok(lines
        .iter()
        .zip(tiers)
        .map(|(cells, tier)| HostRow {
            tier,
            origin: cells[1].to_owned(),
            model: cells[2].to_owned(),
            adapter: cells[3].to_owned(),
        })
        .collect())
}

/// `https://` and a host name alone: two labels or more, separated by dots, each of lower-case
/// ASCII letters, digits and hyphens, the last of letters alone and not `localhost`. So no origin
/// of a table has a port, a path, an address in digits, plain http or the loopback's name.
fn is_origin(cell: &str) -> bool {
    let Some(name) = cell.strip_prefix("https://") else {
        return false;
    };
    let labels: Vec<&str> = name.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
        && labels.last().is_some_and(|last| {
            last.bytes().all(|b| b.is_ascii_lowercase()) && *last != "localhost"
        })
}

/// A word of the table: 1 to 64 ASCII letters, digits, `-`, `.`, `_` or `:`.
fn is_word(cell: &str) -> bool {
    (1..=WORD_MAX).contains(&cell.len())
        && cell
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b':'))
}

/// A day of the calendar written `YYYY-MM-DD`, the rule of src/prices.rs for its date column.
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

/// `admit` under the hosted value that the private reader makes from a table of two rows, one for
/// each hosted tier, for the tests of admit.rs.
#[cfg(test)]
pub(crate) fn admit_under_test_value(
    room: Option<&crate::admit::Room>,
    estimate: crate::prices::Micro,
    running: &[crate::prices::Micro],
) -> crate::admit::Admission {
    let table = concat!(
        "tier\torigin\tmodel\tadapter\tsource\tdate\n",
        "low_cost_hosted\thttps://low.invalid\tlow-model\tadapter-a\tfixture:test\t2026-11-22\n",
        "frontier\thttps://top.invalid\ttop-model\tadapter-b\tfixture:test\t2026-11-22\n",
    );
    crate::admit::admit(
        &Ruling::read_with_table("ruling=hosted", table),
        room,
        estimate,
        running,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOW: &str =
        "low_cost_hosted\thttps://low.invalid\tlow-model\tadapter-a\tfixture:test\t2026-11-22";
    const TOP: &str =
        "frontier\thttps://top.invalid\ttop-model\tadapter-b\tfixture:test\t2026-11-22";

    fn table(rows: &[&str]) -> String {
        let mut text = format!("{HEADER}\n");
        for row in rows {
            text.push_str(row);
            text.push('\n');
        }
        text
    }

    fn refused_near(line: &str) -> Vec<String> {
        let bytes = line.as_bytes();
        let mut near = Vec::new();
        for at in 0..=bytes.len() {
            for byte in 0..=127u8 {
                let mut inserted = bytes.to_vec();
                inserted.insert(at, byte);
                near.push(String::from_utf8(inserted).expect("ascii"));
                if at < bytes.len() && bytes[at] != byte {
                    let mut replaced = bytes.to_vec();
                    replaced[at] = byte;
                    near.push(String::from_utf8(replaced).expect("ascii"));
                }
            }
            if at < bytes.len() {
                near.push(line[..at].to_owned());
                let mut removed = bytes.to_vec();
                removed.remove(at);
                near.push(String::from_utf8(removed).expect("ascii"));
            }
            near.push(line[at..].to_owned());
        }
        near.retain(|text| text != line);
        near
    }

    #[test]
    fn ruling_test_value() {
        let both = Ruling::read_with_table("ruling=hosted", &table(&[LOW, TOP]));
        assert_eq!(both.allowed(), &Tier::ALL);
        let rows: Vec<(Tier, &str, &str, &str)> = both
            .rows()
            .iter()
            .map(|row| (row.tier(), row.origin(), row.model(), row.adapter()))
            .collect();
        assert_eq!(
            rows,
            [
                (
                    Tier::LowCostHosted,
                    "https://low.invalid",
                    "low-model",
                    "adapter-a"
                ),
                (
                    Tier::Frontier,
                    "https://top.invalid",
                    "top-model",
                    "adapter-b"
                ),
            ]
        );
        assert_eq!(both.row(Tier::Frontier), Some(&both.rows()[1]));
        assert_eq!(both.row(Tier::OnDevice), None);
        let low = Ruling::read_with_table("ruling=hosted", &table(&[LOW]));
        assert_eq!(low.allowed(), &[Tier::OnDevice, Tier::LowCostHosted]);
        assert_eq!(low.row(Tier::Frontier), None);
        let top = Ruling::read_with_table("ruling=hosted", &table(&[TOP]));
        assert_eq!(top.allowed(), &[Tier::OnDevice, Tier::Frontier]);
        assert_eq!(top.row(Tier::LowCostHosted), None);
        assert_eq!(
            Ruling::read_with_table("ruling=none", &table(&[LOW, TOP])),
            Ruling::None
        );
        assert_eq!(check_hosts(&table(&[LOW, TOP])), Ok(2));

        assert_eq!(RulingLine::parse("ruling=none"), Ok(RulingLine::None));
        assert_eq!(RulingLine::parse("ruling=hosted"), Ok(RulingLine::Hosted));
        assert_eq!(Ruling::None.allowed(), &[Tier::OnDevice]);
        assert!(Ruling::None.rows().is_empty());
        assert_eq!(RulingError.as_str(), "ruling");

        let mut lines = refused_near("ruling=none");
        lines.extend(refused_near("ruling=hosted"));
        for line in [
            "ruling=none\n",
            "ruling=none\r",
            "ruling=none\r\n",
            "\u{feff}ruling=none",
            "ruling=None",
            "RULING=NONE",
            "ruling=test_hosted",
            "ruling=hosted\n",
            "ruling=Hosted",
            "ruling=all",
            "ruling = none",
            "ruling=none ruling=none",
            "ruling=hosted ruling=hosted",
            "ruling=none\u{e9}",
            "ruling=\u{ff4e}one",
        ] {
            lines.push(line.to_owned());
        }
        assert!(lines.len() > 4_000, "{}", lines.len());
        let open = table(&[LOW, TOP]);
        for line in &lines {
            assert_eq!(RulingLine::parse(line), Err(RulingError), "{line:?}");
            assert_eq!(
                Ruling::read_with_table(line, &open),
                Ruling::None,
                "{line:?}"
            );
        }
    }

    #[test]
    fn a_hosted_line_with_no_row_is_none() {
        let empty = table(&[]);
        assert_eq!(empty, "tier\torigin\tmodel\tadapter\tsource\tdate\n");
        assert_eq!(check_hosts(&empty), Ok(0));
        assert_eq!(
            Ruling::read_with_table("ruling=hosted", &empty),
            Ruling::None
        );
        let one = Ruling::read_with_table("ruling=hosted", &table(&[LOW]));
        assert_eq!(check_hosts(&table(&[LOW])), Ok(1));
        assert_eq!(one.rows().len(), 1);
        assert_eq!(one.allowed(), &[Tier::OnDevice, Tier::LowCostHosted]);
    }
}
