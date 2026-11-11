//! The scorer of contracts T6.2 and T6.3: a router's picks on a table, read against the
//! derivation (R1), the held-out rows (R2), the comparison policies, and the check of the tool
//! itself (R3).

use std::collections::BTreeMap;
use std::num::{NonZeroU32, NonZeroU64};

use halvo_route::line::{lower_pct, meets, Line, LINE_DEV_MIN, LINE_HELD_MIN};
use halvo_route::policy::{cells, Basis, PickCell, Policy};
use halvo_route::prices::{Micro, Prices};
use halvo_route::tier::{CallKind, CategoryId, Tier};

use crate::derive::{counted_tiers, derive, tier_rows, Counts};
use crate::generate::SplitMix64;
use crate::routers::RANDOM_SEED;
use crate::table::{Machine, Row, Seed, Split, Table};
use crate::{Router, Setup};

/// The draws of the comparison `random` when `--draws` is absent.
pub const DEFAULT_DRAWS: NonZeroU32 = NonZeroU32::new(1_000).expect("a thousand");

/// The line of one cell.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CellLine {
    pub category: CategoryId,
    pub kind: CallKind,
    pub pick: PickCell,
    pub basis: Basis,
    pub dev: Counts,
    pub held: Counts,
    pub lower_pct: u32,
    pub cost_micro: Micro,
    pub wall_s: u64,
}

/// The word `line` of a category line.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Standing {
    NotSeed,
    NoRuntime,
    Missed,
    Met,
    Shortfall,
    Untested,
}

impl Standing {
    pub const fn as_str(self) -> &'static str {
        match self {
            Standing::NotSeed => "not_seed",
            Standing::NoRuntime => "no_runtime",
            Standing::Missed => "missed",
            Standing::Met => "met",
            Standing::Shortfall => "shortfall",
            Standing::Untested => "untested",
        }
    }
}

/// The line of one category.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CategoryLine {
    pub id: CategoryId,
    pub compile: PickCell,
    pub recover: PickCell,
    pub write: PickCell,
    pub line: Standing,
    pub cost_micro: Micro,
}

/// One comparison line, computed on the held-out rows.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Versus {
    pub name: &'static str,
    pub attempts: u64,
    pub passed: u64,
    pub wasted: u64,
    pub handed_back: u64,
    pub cost_micro: Micro,
    pub wall_s: u64,
}

/// The verdict line.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Verdict {
    pub generated: bool,
    pub router: &'static str,
    pub routed: u32,
    pub handed_back: u32,
    pub derivation_same: bool,
    pub held_ok: bool,
}

impl Verdict {
    /// R1 and R2 hold.
    pub const fn pass(&self) -> bool {
        self.derivation_same && self.held_ok
    }
}

/// Everything that `route-gate score` prints for one router on one table.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Score {
    pub cells: Vec<CellLine>,
    pub categories: Vec<CategoryLine>,
    pub versus: Vec<Versus>,
    pub verdict: Verdict,
    pub hindsight_fault: bool,
}

impl Score {
    /// The score of these lines. R3 is decided from the versus lines `router` and `hindsight`: it
    /// fails when `hindsight` passes fewer attempts than `router` or wastes one, and when either
    /// line is missing.
    pub fn new(
        cells: Vec<CellLine>,
        categories: Vec<CategoryLine>,
        versus: Vec<Versus>,
        verdict: Verdict,
    ) -> Score {
        let named = |name: &str| versus.iter().find(|line| line.name == name);
        let hindsight_fault = match (named("router"), named("hindsight")) {
            (Some(router), Some(hindsight)) => {
                hindsight.passed < router.passed || hindsight.wasted > 0
            }
            _ => true,
        };
        Score {
            cells,
            categories,
            versus,
            verdict,
            hindsight_fault,
        }
    }

    /// The verdict passes and the tool's own check R3 holds: the command exits 0.
    pub const fn passed(&self) -> bool {
        self.verdict.pass() && !self.hindsight_fault
    }

    /// The lines of contract T6.2 in their order, with `gate fault=hindsight` after the verdict
    /// when R3 does not hold.
    pub fn lines(&self) -> Vec<String> {
        let mut lines =
            Vec::with_capacity(self.cells.len() + self.categories.len() + self.versus.len() + 2);
        for cell in &self.cells {
            lines.push(format!(
                "cell category={} kind={} pick={} basis={} dev_k={} dev_n={} held_k={} held_n={} lower_pct={} cost_micro={} wall_s={}",
                cell.category.as_str(),
                cell.kind.as_str(),
                cell.pick.as_str(),
                cell.basis.as_str(),
                cell.dev.k,
                cell.dev.n,
                cell.held.k,
                cell.held.n,
                cell.lower_pct,
                cell.cost_micro,
                cell.wall_s
            ));
        }
        for category in &self.categories {
            lines.push(format!(
                "category id={} compile={} recover={} write={} line={} cost_micro={}",
                category.id.as_str(),
                category.compile.as_str(),
                category.recover.as_str(),
                category.write.as_str(),
                category.line.as_str(),
                category.cost_micro
            ));
        }
        for versus in &self.versus {
            lines.push(format!(
                "versus name={} attempts={} passed={} wasted={} handed_back={} cost_micro={} wall_s={}",
                versus.name, versus.attempts, versus.passed, versus.wasted, versus.handed_back, versus.cost_micro, versus.wall_s
            ));
        }
        let verdict = &self.verdict;
        lines.push(format!(
            "gate table={} router={} cells={} routed={} handed_back={} derivation={} held={} result={}",
            if verdict.generated { "generated" } else { "recorded" },
            verdict.router,
            self.cells.len(),
            verdict.routed,
            verdict.handed_back,
            if verdict.derivation_same { "same" } else { "differs" },
            if verdict.held_ok { "ok" } else { "miss" },
            if verdict.pass() { "pass" } else { "fail" }
        ));
        if self.hindsight_fault {
            lines.push("gate fault=hindsight".to_owned());
        }
        lines
    }
}

fn clamp(amount: u128) -> u64 {
    u64::try_from(amount).unwrap_or(u64::MAX)
}

#[derive(Clone, Copy, Default)]
struct Tally {
    dev: Counts,
    held: Counts,
    cost: u128,
    wall: u128,
}

#[derive(Clone, Copy, Default)]
struct Sums {
    attempts: u128,
    passed: u128,
    wasted: u128,
    handed_back: u128,
    cost: u128,
    wall: u128,
}

impl Sums {
    fn attempt(&mut self, tally: &Tally) {
        self.attempts += u128::from(tally.held.n);
        self.passed += u128::from(tally.held.k);
        self.wasted += u128::from(tally.held.n - tally.held.k);
        self.cost += tally.cost;
        self.wall += tally.wall;
    }

    fn versus(&self, name: &'static str, over: u128) -> Versus {
        Versus {
            name,
            attempts: clamp(self.attempts / over),
            passed: clamp(self.passed / over),
            wasted: clamp(self.wasted / over),
            handed_back: clamp(self.handed_back / over),
            cost_micro: clamp(self.cost / over),
            wall_s: clamp(self.wall / over),
        }
    }
}

struct ModelCell {
    category: CategoryId,
    kind: CallKind,
    pick: PickCell,
    tallies: [Tally; 3],
}

impl ModelCell {
    fn has_held_rows(&self) -> bool {
        self.tallies.iter().any(|tally| tally.held.n > 0)
    }

    fn held_tiers(&self) -> Vec<Tier> {
        Tier::ALL
            .into_iter()
            .filter(|tier| self.tallies[*tier as usize].held.n > 0)
            .collect()
    }
}

fn tally(
    table: &Table,
    setup: &Setup,
    prices: &Prices,
    category: CategoryId,
    kind: CallKind,
    tier: Tier,
) -> Tally {
    let mut tally = Tally::default();
    for row in tier_rows(table, setup, category, kind, tier) {
        let counts = match row.split() {
            Split::Dev => &mut tally.dev,
            Split::Held => &mut tally.held,
        };
        counts.n += 1;
        counts.k += u32::from(row.pass);
        if row.split() == Split::Held {
            tally.cost += u128::from(prices.cost(tier, row.tokens_in, row.tokens_out));
            tally.wall += u128::from(row.wall_s);
        }
    }
    tally
}

/// Scores `router` on `table`, read with `setup`, costed with `prices`, with `draws` draws of the
/// comparison `random`.
pub fn score(
    table: &Table,
    setup: &Setup,
    router: &dyn Router,
    prices: &Prices,
    draws: NonZeroU32,
) -> Score {
    let derived = derive(table, setup);
    let tallies: Vec<[Tally; 3]> = cells()
        .map(|(category, kind)| {
            Tier::ALL.map(|tier| tally(table, setup, prices, category, kind, tier))
        })
        .collect();
    let picks: Vec<PickCell> = cells()
        .map(|(category, kind)| router.pick(category, kind))
        .collect();
    let counted = counted_tiers(table, setup);
    let cell_lines: Vec<CellLine> = cells()
        .zip(&tallies)
        .zip(picks.iter().zip(counted))
        .map(|((cell, tally), (pick, counted))| cell_line(cell, tally, *pick, counted, &derived))
        .collect();
    let with_rows: Vec<bool> = tallies
        .iter()
        .map(|tallies| tallies.iter().any(|tally| tally.dev.n + tally.held.n > 0))
        .collect();
    let categories = category_lines(&cell_lines, &with_rows, setup);
    let model_cells: Vec<ModelCell> = cell_lines
        .iter()
        .zip(&tallies)
        .filter(|(line, _)| line.category.is_seed() && line.kind != CallKind::Replay)
        .map(|(line, tallies)| ModelCell {
            category: line.category,
            kind: line.kind,
            pick: line.pick,
            tallies: *tallies,
        })
        .collect();
    let router_sums = route_sums(&model_cells, |cell| match cell.pick {
        PickCell::Tier(tier) => Some(tier),
        _ => None,
    });
    let mut versus = vec![router_sums.versus("router", 1)];
    for tier in Tier::ALL {
        if model_cells
            .iter()
            .any(|cell| cell.tallies[tier as usize].held.n > 0)
        {
            let name = match tier {
                Tier::OnDevice => "fixed_on_device",
                Tier::LowCostHosted => "fixed_low_cost_hosted",
                Tier::Frontier => "fixed_frontier",
            };
            versus.push(route_sums(&model_cells, |_| Some(tier)).versus(name, 1));
        }
    }
    versus.push(
        random_sums(&model_cells, router_sums.attempted_cells, draws)
            .versus("random", u128::from(draws.get())),
    );
    versus.push(hindsight_sums(table, setup, prices, &model_cells).versus("hindsight", 1));
    let verdict = verdict(table, &cell_lines, &derived, router.name());
    Score::new(cell_lines, categories, versus, verdict)
}

fn cell_line(
    (category, kind): (CategoryId, CallKind),
    tallies: &[Tally; 3],
    pick: PickCell,
    counted: Option<Tier>,
    derived: &Policy,
) -> CellLine {
    let basis = derived.row(category, kind).basis;
    let mut line = CellLine {
        category,
        kind,
        pick,
        basis,
        dev: Counts::default(),
        held: Counts::default(),
        lower_pct: 0,
        cost_micro: 0,
        wall_s: 0,
    };
    match pick {
        PickCell::Tier(tier) => {
            let tally = &tallies[tier as usize];
            let clean = tally.dev.k == tally.dev.n && tally.held.k == tally.held.n;
            line.dev = tally.dev;
            line.held = tally.held;
            line.lower_pct = if clean {
                lower_pct(tally.dev.n + tally.held.n)
            } else {
                0
            };
            line.cost_micro = clamp(tally.cost);
            line.wall_s = clamp(tally.wall);
        }
        PickCell::HandBack => {
            if let Some(tier) = counted {
                line.dev = tallies[tier as usize].dev;
                line.held = tallies[tier as usize].held;
            }
        }
        PickCell::None => {}
    }
    line
}

fn meets_both(line: &CellLine) -> bool {
    meets(line.dev.k, line.dev.n, LINE_DEV_MIN) == Line::Met
        && meets(line.held.k, line.held.n, LINE_HELD_MIN) == Line::Met
}

fn category_lines(cell_lines: &[CellLine], with_rows: &[bool], setup: &Setup) -> Vec<CategoryLine> {
    cell_lines
        .chunks(CallKind::ALL.len())
        .zip(with_rows.chunks(CallKind::ALL.len()))
        .map(|(chunk, with_rows)| {
            let (compile, recover, write) = (&chunk[0], &chunk[1], &chunk[2]);
            let id = compile.category;
            let models = [compile, recover, write];
            let routed = |line: &CellLine| matches!(line.pick, PickCell::Tier(_));
            let line = if !id.is_seed() {
                Standing::NotSeed
            } else if setup.recorded && !id.has_runtime() {
                Standing::NoRuntime
            } else if models.iter().any(|line| routed(line) && !meets_both(line)) {
                Standing::Missed
            } else if models.iter().any(|line| routed(line)) {
                Standing::Met
            } else if with_rows[..3].contains(&true) {
                Standing::Shortfall
            } else {
                Standing::Untested
            };
            let cost = models.iter().map(|line| u128::from(line.cost_micro)).sum();
            CategoryLine {
                id,
                compile: compile.pick,
                recover: recover.pick,
                write: write.pick,
                line,
                cost_micro: clamp(cost),
            }
        })
        .collect()
}

struct Routed {
    sums: Sums,
    attempted_cells: u64,
}

impl Routed {
    fn versus(&self, name: &'static str, over: u128) -> Versus {
        self.sums.versus(name, over)
    }
}

fn route_sums(model_cells: &[ModelCell], choose: impl Fn(&ModelCell) -> Option<Tier>) -> Routed {
    let mut routed = Routed {
        sums: Sums::default(),
        attempted_cells: 0,
    };
    for cell in model_cells.iter().filter(|cell| cell.has_held_rows()) {
        match choose(cell)
            .map(|tier| &cell.tallies[tier as usize])
            .filter(|tally| tally.held.n > 0)
        {
            Some(tally) => {
                routed.sums.attempt(tally);
                routed.attempted_cells += 1;
            }
            None => routed.sums.handed_back += 1,
        }
    }
    routed
}

fn random_sums(model_cells: &[ModelCell], attempted: u64, draws: NonZeroU32) -> Sums {
    let with_rows: Vec<&ModelCell> = model_cells
        .iter()
        .filter(|cell| cell.has_held_rows())
        .collect();
    let mut sums = Sums::default();
    let Some(attemptable) = NonZeroU64::new(with_rows.len() as u64) else {
        return sums;
    };
    let mut stream = SplitMix64::new(RANDOM_SEED);
    for _ in 0..draws.get() {
        for cell in &with_rows {
            if stream.below(attemptable) < attempted {
                let tiers = cell.held_tiers();
                let count = NonZeroU64::new(tiers.len() as u64)
                    .expect("a cell with held-out rows has a tier with held-out rows");
                let tier = tiers[stream.below(count) as usize];
                sums.attempt(&cell.tallies[tier as usize]);
            } else {
                sums.handed_back += 1;
            }
        }
    }
    sums
}

fn hindsight_sums(
    table: &Table,
    setup: &Setup,
    prices: &Prices,
    model_cells: &[ModelCell],
) -> Sums {
    let mut sums = Sums::default();
    for cell in model_cells.iter().filter(|cell| cell.has_held_rows()) {
        let mut best: BTreeMap<(Seed, Machine, NonZeroU32), (Tier, &Row)> = BTreeMap::new();
        for tier in Tier::ALL {
            for row in tier_rows(table, setup, cell.category, cell.kind, tier)
                .filter(|row| row.split() == Split::Held && row.pass)
            {
                best.entry((row.seed, row.machine, row.attempt))
                    .or_insert((tier, row));
            }
        }
        if best.is_empty() {
            sums.handed_back += 1;
        }
        for (tier, row) in best.into_values() {
            sums.attempts += 1;
            sums.passed += u128::from(row.pass);
            sums.wasted += u128::from(!row.pass);
            sums.cost += u128::from(prices.cost(tier, row.tokens_in, row.tokens_out));
            sums.wall += u128::from(row.wall_s);
        }
    }
    sums
}

fn verdict(
    table: &Table,
    cell_lines: &[CellLine],
    derived: &Policy,
    router: &'static str,
) -> Verdict {
    let model = |line: &&CellLine| line.category.is_seed() && line.kind != CallKind::Replay;
    let routed = cell_lines
        .iter()
        .filter(model)
        .filter(|line| matches!(line.pick, PickCell::Tier(_)))
        .count();
    let handed_back = cell_lines
        .iter()
        .filter(model)
        .filter(|line| line.pick == PickCell::HandBack)
        .count();
    let derivation_same = cell_lines
        .iter()
        .all(|line| line.pick == derived.row(line.category, line.kind).pick);
    let held_ok = cell_lines
        .iter()
        .filter(model)
        .filter(|line| matches!(line.pick, PickCell::Tier(_)))
        .all(|line| meets(line.held.k, line.held.n, LINE_HELD_MIN) == Line::Met);
    Verdict {
        generated: table.is_generated(),
        router,
        routed: u32::try_from(routed).unwrap_or(u32::MAX),
        handed_back: u32::try_from(handed_back).unwrap_or(u32::MAX),
        derivation_same,
        held_ok,
    }
}
