//! The derivation of contract T2.2: the policy that a table's development rows give, and the row
//! sets that it and the scorer read.

use halvo_route::line::{meets, Line, LINE_DEV_MIN};
use halvo_route::policy::{cells, Basis, PickCell, Policy, Row as PolicyRow, CELL_COUNT};
use halvo_route::tier::{CallKind, CategoryId, Tier};

use crate::table::{Row, Split, Table};
use crate::Setup;

/// The rows of a cell that count for a tier (contract T5.5): those of the cell's kind, which is not
/// `replay`, and of that tier, whose model fills the tier for the kind in `setup`.
pub fn tier_rows<'a>(
    table: &'a Table,
    setup: &'a Setup,
    category: CategoryId,
    kind: CallKind,
    tier: Tier,
) -> impl Iterator<Item = &'a Row> + 'a {
    let model = setup.fill(kind);
    table.rows().iter().filter(move |row| {
        row.category == category
            && row.kind == kind
            && row.tier == tier
            && Some(row.model.as_str()) == model
    })
}

/// Passes and attempts of a set of rows.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct Counts {
    pub k: u32,
    pub n: u32,
}

impl Counts {
    pub fn of<'a>(rows: impl Iterator<Item = &'a Row>) -> Counts {
        rows.fold(Counts::default(), |counts, row| Counts {
            k: counts.k + u32::from(row.pass),
            n: counts.n + 1,
        })
    }
}

/// The derivation of contract T2.2 on the development rows with `LINE_DEV_MIN`.
pub fn derive(table: &Table, setup: &Setup) -> Policy {
    Policy::from_rows(
        derive_on(table, setup, Split::Dev, LINE_DEV_MIN),
        &setup.allowed,
    )
    .expect("the derivation makes rows of the policy form")
}

/// The rule of the derivation on the rows of one split with one least number of attempts, as rows
/// in the order of `cells()`; the router `peek` reads it on the held-out rows with
/// `LINE_HELD_MIN`. Its rows are no policy text: only those of `derive` make a `Policy`.
pub fn derive_on(table: &Table, setup: &Setup, split: Split, min: u32) -> [PolicyRow; CELL_COUNT] {
    let mut rows = *Policy::hand_back_all().rows();
    for (row, (category, kind)) in rows.iter_mut().zip(cells()) {
        *row = derive_cell(table, setup, category, kind, split, min).0;
    }
    rows
}

/// For each cell in the order of `cells()`, the tier whose development rows give the `k` and `n`
/// that `derive` writes for the cell, or none where it writes `0 0`.
pub fn counted_tiers(table: &Table, setup: &Setup) -> [Option<Tier>; CELL_COUNT] {
    let mut tiers = [None; CELL_COUNT];
    for (tier, (category, kind)) in tiers.iter_mut().zip(cells()) {
        *tier = derive_cell(table, setup, category, kind, Split::Dev, LINE_DEV_MIN).1;
    }
    tiers
}

fn derive_cell(
    table: &Table,
    setup: &Setup,
    category: CategoryId,
    kind: CallKind,
    split: Split,
    min: u32,
) -> (PolicyRow, Option<Tier>) {
    let hand_back = |basis, counts: Counts| PolicyRow {
        pick: PickCell::HandBack,
        level: None,
        basis,
        k: counts.k,
        n: counts.n,
    };
    if kind == CallKind::Replay {
        let row = PolicyRow {
            pick: PickCell::None,
            level: None,
            basis: Basis::NoModel,
            k: 0,
            n: 0,
        };
        return (row, None);
    }
    if !category.is_seed() {
        return (hand_back(Basis::NotSeed, Counts::default()), None);
    }
    if setup.recorded && !category.has_runtime() {
        return (hand_back(Basis::NoRuntime, Counts::default()), None);
    }
    let mut cheapest: Option<(Line, Counts, Tier)> = None;
    for tier in Tier::ALL
        .into_iter()
        .filter(|tier| setup.allowed.contains(tier))
    {
        let mut rows = tier_rows(table, setup, category, kind, tier)
            .filter(|row| row.split() == split)
            .peekable();
        let level = rows.peek().map(|row| row.level);
        let counts = Counts::of(rows);
        let line = meets(counts.k, counts.n, min);
        if let (Line::Met, Some(level)) = (line, level) {
            let row = PolicyRow {
                pick: PickCell::Tier(tier),
                level: Some(level),
                basis: Basis::LineMet,
                k: counts.k,
                n: counts.n,
            };
            return (row, Some(tier));
        }
        if counts.n > 0 && cheapest.is_none() {
            cheapest = Some((line, counts, tier));
        }
    }
    match cheapest {
        Some((Line::Missed, counts, tier)) => (hand_back(Basis::LineMissed, counts), Some(tier)),
        Some((_, counts, tier)) => (hand_back(Basis::Untested, counts), Some(tier)),
        None => (hand_back(Basis::Untested, Counts::default()), None),
    }
}
