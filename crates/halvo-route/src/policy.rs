//! The policy table of contract T2: one row for each of the 44 cells, its parser and its writer.

use crate::tier::{CallKind, CategoryId, Level, ParseError, Tier};

const HEADER: &str = "category\tkind\tpick\tlevel\tbasis\tk\tn";
const CELLS: usize = 7;

/// The number of cells: the 11 categories times the 4 call kinds.
pub const CELL_COUNT: usize = CategoryId::ALL.len() * CallKind::ALL.len();

/// The cells in the order of the table: the categories in the order of `CategoryId::ALL`, and for
/// each the kinds in the order of `CallKind::ALL`.
pub fn cells() -> impl Iterator<Item = (CategoryId, CallKind)> {
    CategoryId::ALL
        .into_iter()
        .flat_map(|category| CallKind::ALL.into_iter().map(move |kind| (category, kind)))
}

const fn index(category: CategoryId, kind: CallKind) -> usize {
    category as usize * CallKind::ALL.len() + kind as usize
}

/// The pick of a row: `none` (the kind calls no model), a tier, or `hand_back`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PickCell {
    None,
    Tier(Tier),
    HandBack,
}

impl PickCell {
    pub const ALL: [PickCell; 5] = [
        PickCell::None,
        PickCell::Tier(Tier::OnDevice),
        PickCell::Tier(Tier::LowCostHosted),
        PickCell::Tier(Tier::Frontier),
        PickCell::HandBack,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            PickCell::None => "none",
            PickCell::Tier(tier) => tier.as_str(),
            PickCell::HandBack => "hand_back",
        }
    }

    pub fn parse(text: &str) -> Result<PickCell, ParseError> {
        PickCell::ALL
            .into_iter()
            .find(|pick| pick.as_str() == text)
            .ok_or(ParseError)
    }
}

/// Why a row has its pick.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Basis {
    NoModel,
    LineMet,
    LineMissed,
    Untested,
    NoRuntime,
    NotSeed,
}

impl Basis {
    pub const ALL: [Basis; 6] = [
        Basis::NoModel,
        Basis::LineMet,
        Basis::LineMissed,
        Basis::Untested,
        Basis::NoRuntime,
        Basis::NotSeed,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Basis::NoModel => "no_model",
            Basis::LineMet => "line_met",
            Basis::LineMissed => "line_missed",
            Basis::Untested => "untested",
            Basis::NoRuntime => "no_runtime",
            Basis::NotSeed => "not_seed",
        }
    }

    pub fn parse(text: &str) -> Result<Basis, ParseError> {
        Basis::ALL
            .into_iter()
            .find(|basis| basis.as_str() == text)
            .ok_or(ParseError)
    }
}

/// One row of the table: `k` and `n` are the passes and the attempts of the development rows
/// behind the cell.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Row {
    pub pick: PickCell,
    pub level: Option<Level>,
    pub basis: Basis,
    pub k: u32,
    pub n: u32,
}

/// Why a policy text is refused.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PolicyError {
    Header,
    Rows,
    Shape,
    Name,
    Ruling,
}

impl PolicyError {
    pub const fn as_str(self) -> &'static str {
        match self {
            PolicyError::Header => "header",
            PolicyError::Rows => "rows",
            PolicyError::Shape => "shape",
            PolicyError::Name => "name",
            PolicyError::Ruling => "ruling",
        }
    }
}

/// A table of 44 rows, each of the form that `Policy::parse_for` accepts for its tiers.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Policy {
    rows: [Row; CELL_COUNT],
}

const REPLAY: Row = Row {
    pick: PickCell::None,
    level: None,
    basis: Basis::NoModel,
    k: 0,
    n: 0,
};

impl Policy {
    /// Reads a policy text whose tier picks are all in `allowed`, and returns the first error in
    /// the order of contract T2.3: `header`, `rows`, then for each row in turn `shape` (cells),
    /// `name`, `rows` (the pair of its place), `shape` (values) and `ruling`.
    pub fn parse_for(text: &str, allowed: &[Tier]) -> Result<Policy, PolicyError> {
        let first = text.split('\n').next().unwrap_or_default();
        if first != HEADER {
            return Err(PolicyError::Header);
        }
        if text.contains('\r') || !text.ends_with('\n') {
            return Err(PolicyError::Rows);
        }
        let lines: Vec<&str> = text[HEADER.len() + 1..].split_terminator('\n').collect();
        if lines.len() != CELL_COUNT || lines.iter().any(|line| line.is_empty()) {
            return Err(PolicyError::Rows);
        }
        let mut rows = [REPLAY; CELL_COUNT];
        for ((slot, line), (category, kind)) in rows.iter_mut().zip(lines).zip(cells()) {
            let row = read_row(line, category, kind)?;
            check_row(category, kind, &row, allowed)?;
            *slot = row;
        }
        Ok(Policy { rows })
    }

    /// The table of these rows, in the order of `cells()`, checked as `parse_for` checks the values
    /// of each row.
    pub fn from_rows(rows: [Row; CELL_COUNT], allowed: &[Tier]) -> Result<Policy, PolicyError> {
        for (row, (category, kind)) in rows.iter().zip(cells()) {
            check_row(category, kind, row, allowed)?;
        }
        Ok(Policy { rows })
    }

    pub const fn row(&self, category: CategoryId, kind: CallKind) -> &Row {
        &self.rows[index(category, kind)]
    }

    /// The rows in the order of `cells()`.
    pub const fn rows(&self) -> &[Row; CELL_COUNT] {
        &self.rows
    }

    /// The text of the table in the form that `parse_for` reads.
    pub fn to_text(&self) -> String {
        let mut text = String::with_capacity(64 * (CELL_COUNT + 1));
        text.push_str(HEADER);
        text.push('\n');
        for (row, (category, kind)) in self.rows.iter().zip(cells()) {
            let level = row.level.map_or("-", Level::as_str);
            let line = [
                category.as_str(),
                kind.as_str(),
                row.pick.as_str(),
                level,
                row.basis.as_str(),
            ]
            .join("\t");
            text.push_str(&format!("{line}\t{}\t{}\n", row.k, row.n));
        }
        text
    }

    /// The table that routes no cell: `none` for `replay`, else `hand_back` with `not_seed` for
    /// `other`, `no_runtime` for a category with no runtime and `untested` for the rest.
    pub fn hand_back_all() -> Policy {
        let mut rows = [REPLAY; CELL_COUNT];
        for (row, (category, kind)) in rows.iter_mut().zip(cells()) {
            let basis = if !category.is_seed() {
                Basis::NotSeed
            } else if !category.has_runtime() {
                Basis::NoRuntime
            } else {
                Basis::Untested
            };
            if kind != CallKind::Replay {
                *row = Row {
                    pick: PickCell::HandBack,
                    level: None,
                    basis,
                    k: 0,
                    n: 0,
                };
            }
        }
        Policy { rows }
    }
}

fn read_row(line: &str, category: CategoryId, kind: CallKind) -> Result<Row, PolicyError> {
    let cells: Vec<&str> = line.split('\t').collect();
    if cells.len() != CELLS {
        return Err(PolicyError::Shape);
    }
    let level = match cells[3] {
        "-" => Ok(None),
        word => Level::parse(word).map(Some),
    };
    let (Ok(row_category), Ok(row_kind), Ok(pick), Ok(level), Ok(basis)) = (
        CategoryId::parse(cells[0]),
        CallKind::parse(cells[1]),
        PickCell::parse(cells[2]),
        level,
        Basis::parse(cells[4]),
    ) else {
        return Err(PolicyError::Name);
    };
    if (row_category, row_kind) != (category, kind) {
        return Err(PolicyError::Rows);
    }
    let (Some(k), Some(n)) = (count(cells[5]), count(cells[6])) else {
        return Err(PolicyError::Shape);
    };
    Ok(Row {
        pick,
        level,
        basis,
        k,
        n,
    })
}

fn count(cell: &str) -> Option<u32> {
    let canonical = !cell.is_empty()
        && cell.bytes().all(|b| b.is_ascii_digit())
        && (cell == "0" || !cell.starts_with('0'));
    if canonical {
        cell.parse().ok()
    } else {
        None
    }
}

fn check_row(
    category: CategoryId,
    kind: CallKind,
    row: &Row,
    allowed: &[Tier],
) -> Result<(), PolicyError> {
    let basis_fits = match row.pick {
        PickCell::None => row.basis == Basis::NoModel,
        PickCell::Tier(_) => row.basis == Basis::LineMet,
        PickCell::HandBack => !matches!(row.basis, Basis::NoModel | Basis::LineMet),
    };
    let level_fits = matches!(row.pick, PickCell::Tier(_)) == row.level.is_some();
    let replay_fits = (kind == CallKind::Replay) == (row.pick == PickCell::None)
        && (kind != CallKind::Replay || *row == REPLAY);
    let seed_fits = row.basis != Basis::NotSeed || category == CategoryId::Other;
    if row.k > row.n || !replay_fits || !level_fits || !basis_fits || !seed_fits {
        return Err(PolicyError::Shape);
    }
    match row.pick {
        PickCell::Tier(tier) if !allowed.contains(&tier) => Err(PolicyError::Ruling),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tier::assert_closed;

    #[test]
    fn pick_list_is_closed() {
        assert_closed(&PickCell::ALL.map(|v| (v, v.as_str())), PickCell::parse);
        assert_eq!(
            PickCell::ALL.map(PickCell::as_str),
            [
                "none",
                "on_device",
                "low_cost_hosted",
                "frontier",
                "hand_back"
            ]
        );
    }

    #[test]
    fn basis_list_is_closed() {
        assert_closed(&Basis::ALL.map(|v| (v, v.as_str())), Basis::parse);
        assert_eq!(
            Basis::ALL.map(Basis::as_str),
            [
                "no_model",
                "line_met",
                "line_missed",
                "untested",
                "no_runtime",
                "not_seed"
            ]
        );
    }

    #[test]
    fn cells_are_in_index_order() {
        assert_eq!(cells().count(), CELL_COUNT);
        for (i, (category, kind)) in cells().enumerate() {
            assert_eq!(index(category, kind), i);
        }
    }
}
