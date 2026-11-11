use std::num::NonZeroU32;

use halvo_route::policy::PickCell;
use halvo_route::prices::Prices;
use halvo_route::tier::{CallKind, CategoryId, Level, Tier};
use halvo_route_gate::generate::generate;
use halvo_route_gate::routers::{fixed, OneTier, PolicyRouter, Random};
use halvo_route_gate::score::{score, Score, Versus, DEFAULT_DRAWS};
use halvo_route_gate::table::{Machine, Model, Row, Seed, Source, Table, Task};
use halvo_route_gate::{Router, Setup};

use CallKind::{Compile, Write};
use CategoryId::Email;
use Machine::{GenA, GenB};
use Seed::{One, Three, Two};
use Tier::{LowCostHosted, OnDevice};

fn row(
    kind: CallKind,
    tier: Tier,
    seed: Seed,
    machine: Machine,
    pass: bool,
    wall_s: u64,
    tokens: u64,
) -> Row {
    Row {
        task: Task::Gen,
        category: Email,
        kind,
        tier,
        model: Model::synthetic(),
        machine,
        seed,
        attempt: NonZeroU32::new(1).expect("one"),
        pass,
        steps: 3,
        wall_s,
        tokens_in: tokens,
        tokens_out: tokens,
        level: Level::Medium,
        source: Source::Generated,
    }
}

fn hand_table() -> Table {
    let mut rows = vec![
        row(Compile, OnDevice, One, GenA, true, 1, 50),
        row(Compile, OnDevice, One, GenB, true, 1, 50),
        row(Compile, OnDevice, Two, GenA, true, 1, 50),
        row(Compile, OnDevice, Two, GenB, true, 1, 50),
        row(Compile, OnDevice, Three, GenA, true, 10, 50),
        row(Compile, OnDevice, Three, GenB, true, 20, 50),
        row(Write, OnDevice, One, GenA, false, 1, 50),
        row(Write, OnDevice, One, GenB, true, 1, 50),
        row(Write, OnDevice, Two, GenA, true, 1, 50),
        row(Write, OnDevice, Two, GenB, true, 1, 50),
        row(Write, OnDevice, Three, GenA, true, 3, 50),
        row(Write, OnDevice, Three, GenB, false, 4, 50),
        row(Write, LowCostHosted, One, GenA, true, 1, 900),
        row(Write, LowCostHosted, One, GenB, true, 1, 900),
        row(Write, LowCostHosted, Two, GenA, true, 1, 900),
        row(Write, LowCostHosted, Two, GenB, true, 1, 900),
        row(Write, LowCostHosted, Three, GenA, true, 5, 1_000),
        row(Write, LowCostHosted, Three, GenB, true, 7, 1_000),
    ];
    rows.sort_by_cached_key(Row::to_line);
    Table::from_rows(&rows).expect("a table")
}

fn lines_of(router: &dyn Router, draws: u32) -> Vec<String> {
    let draws = NonZeroU32::new(draws).expect("not zero");
    score(
        &hand_table(),
        &Setup::generated(),
        router,
        Prices::release(),
        draws,
    )
    .lines()
}

fn line<'a>(lines: &'a [String], start: &str) -> &'a str {
    let found: Vec<&String> = lines
        .iter()
        .filter(|line| line.starts_with(start))
        .collect();
    assert_eq!(found.len(), 1, "one line that starts with {start:?}");
    found[0]
}

#[test]
fn derived_router_lines_by_hand() {
    let derived = PolicyRouter::derived(&hand_table(), &Setup::generated());
    let lines = lines_of(&derived, 1_000);
    assert_eq!(lines.len(), 44 + 11 + 5 + 1);
    let want = [
        "cell category=email kind=compile pick=on_device basis=line_met dev_k=4 dev_n=4 held_k=2 held_n=2 lower_pct=60 cost_micro=0 wall_s=30",
        "cell category=email kind=recover pick=hand_back basis=untested dev_k=0 dev_n=0 held_k=0 held_n=0 lower_pct=0 cost_micro=0 wall_s=0",
        "cell category=email kind=write pick=low_cost_hosted basis=line_met dev_k=4 dev_n=4 held_k=2 held_n=2 lower_pct=60 cost_micro=37600 wall_s=12",
        "cell category=email kind=replay pick=none basis=no_model dev_k=0 dev_n=0 held_k=0 held_n=0 lower_pct=0 cost_micro=0 wall_s=0",
        "cell category=utility_bill_payment kind=compile pick=hand_back basis=untested dev_k=0 dev_n=0 held_k=0 held_n=0 lower_pct=0 cost_micro=0 wall_s=0",
        "cell category=other kind=write pick=hand_back basis=not_seed dev_k=0 dev_n=0 held_k=0 held_n=0 lower_pct=0 cost_micro=0 wall_s=0",
        "category id=email compile=on_device recover=hand_back write=low_cost_hosted line=met cost_micro=37600",
        "category id=utility_bill_payment compile=hand_back recover=hand_back write=hand_back line=untested cost_micro=0",
        "category id=other compile=hand_back recover=hand_back write=hand_back line=not_seed cost_micro=0",
    ];
    for want in want {
        assert!(lines.iter().any(|line| line == want), "missing {want}");
    }
    assert_eq!(lines[0], "cell category=utility_bill_payment kind=compile pick=hand_back basis=untested dev_k=0 dev_n=0 held_k=0 held_n=0 lower_pct=0 cost_micro=0 wall_s=0");
    assert!(lines[43].starts_with("cell category=other kind=replay "));
    assert!(lines[44].starts_with("category id=utility_bill_payment "));
    assert_eq!(
        lines[55..],
        [
            "versus name=router attempts=4 passed=4 wasted=0 handed_back=0 cost_micro=37600 wall_s=42",
            "versus name=fixed_on_device attempts=4 passed=3 wasted=1 handed_back=0 cost_micro=0 wall_s=37",
            "versus name=fixed_low_cost_hosted attempts=2 passed=2 wasted=0 handed_back=1 cost_micro=37600 wall_s=12",
            "versus name=random attempts=4 passed=3 wasted=0 handed_back=0 cost_micro=18236 wall_s=39",
            "versus name=hindsight attempts=4 passed=4 wasted=0 handed_back=0 cost_micro=18800 wall_s=40",
            "gate table=generated router=derived cells=44 routed=2 handed_back=28 derivation=same held=ok result=pass",
        ]
    );
    assert_eq!(
        line(&lines_of(&derived, 10), "versus name=random "),
        "versus name=random attempts=4 passed=3 wasted=0 handed_back=0 cost_micro=15040 wall_s=39"
    );
    assert_eq!(
        line(&lines_of(&derived, 1), "versus name=random "),
        "versus name=random attempts=4 passed=4 wasted=0 handed_back=0 cost_micro=37600 wall_s=42"
    );
}

struct Handpicked;

impl Router for Handpicked {
    fn name(&self) -> &'static str {
        "handpicked"
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        match (category, kind) {
            (Email, Write) => PickCell::Tier(LowCostHosted),
            _ => fixed(category, kind, false).unwrap_or(PickCell::HandBack),
        }
    }
}

#[test]
fn a_router_that_hands_back_a_routed_cell_fails_r1_alone() {
    let lines = lines_of(&Handpicked, 1_000);
    assert_eq!(
        line(&lines, "cell category=email kind=compile "),
        "cell category=email kind=compile pick=hand_back basis=line_met dev_k=4 dev_n=4 held_k=2 held_n=2 lower_pct=0 cost_micro=0 wall_s=0"
    );
    assert_eq!(line(&lines, "category id=email "), "category id=email compile=hand_back recover=hand_back write=low_cost_hosted line=met cost_micro=37600");
    assert_eq!(
        line(&lines, "versus name=router "),
        "versus name=router attempts=2 passed=2 wasted=0 handed_back=1 cost_micro=37600 wall_s=12"
    );
    assert_eq!(
        line(&lines, "versus name=random "),
        "versus name=random attempts=1 passed=1 wasted=0 handed_back=1 cost_micro=9324 wall_s=19"
    );
    assert_eq!(line(&lines, "gate "), "gate table=generated router=handpicked cells=44 routed=1 handed_back=29 derivation=differs held=ok result=fail");
}

#[test]
fn one_tier_misses_the_held_out_rows() {
    let lines = lines_of(&OneTier { recorded: false }, 1_000);
    assert_eq!(
        line(&lines, "cell category=email kind=write "),
        "cell category=email kind=write pick=on_device basis=line_met dev_k=3 dev_n=4 held_k=1 held_n=2 lower_pct=0 cost_micro=0 wall_s=7"
    );
    assert_eq!(line(&lines, "category id=email "), "category id=email compile=on_device recover=on_device write=on_device line=missed cost_micro=0");
    assert_eq!(
        line(&lines, "category id=shop_order "),
        "category id=shop_order compile=on_device recover=on_device write=on_device line=missed cost_micro=0"
    );
    assert_eq!(
        line(&lines, "versus name=router "),
        "versus name=router attempts=4 passed=3 wasted=1 handed_back=0 cost_micro=0 wall_s=37"
    );
    assert_eq!(line(&lines, "gate "), "gate table=generated router=one_tier cells=44 routed=30 handed_back=0 derivation=differs held=miss result=fail");
}

#[test]
fn bad_routers_fail_for_their_reasons_on_the_specimen() {
    let table = generate(1);
    let setup = Setup::generated();
    let verdict =
        |router: &dyn Router| score(&table, &setup, router, Prices::release(), DEFAULT_DRAWS);
    let derived = verdict(&PolicyRouter::derived(&table, &setup));
    assert!(derived.passed());
    let peek = verdict(&PolicyRouter::peek(&table, &setup));
    assert!(!peek.verdict.derivation_same && peek.verdict.held_ok);
    let recover = |score: &halvo_route_gate::score::Score| {
        score
            .cells
            .iter()
            .find(|cell| {
                cell.category == CategoryId::UtilityBillPayment && cell.kind == CallKind::Recover
            })
            .map(|cell| cell.pick)
    };
    assert_eq!(recover(&peek), Some(PickCell::Tier(OnDevice)));
    assert_eq!(recover(&derived), Some(PickCell::Tier(Tier::Frontier)));
    let one_tier = verdict(&OneTier { recorded: false });
    assert!(!one_tier.verdict.derivation_same && !one_tier.verdict.held_ok);
    let compile = one_tier
        .cells
        .iter()
        .find(|cell| cell.category == CategoryId::UtilityBillPayment && cell.kind == Compile)
        .expect("cell");
    assert_eq!(
        (compile.pick, compile.basis.as_str()),
        (PickCell::Tier(OnDevice), "line_met")
    );
    let random = verdict(&Random::new(&table, &setup));
    assert!(!random.verdict.derivation_same);
    for score in [&derived, &peek, &one_tier, &random] {
        assert!(!score.hindsight_fault);
        let lines = score.lines();
        let hindsight = line(&lines, "versus name=hindsight ");
        assert!(hindsight.contains(" wasted=0 "), "{hindsight}");
    }
}

struct OneCellOff {
    derived: PolicyRouter,
    cell: (CategoryId, CallKind),
}

impl Router for OneCellOff {
    fn name(&self) -> &'static str {
        "one_cell_off"
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        if (category, kind) == self.cell {
            PickCell::Tier(OnDevice)
        } else {
            self.derived.pick(category, kind)
        }
    }
}

#[test]
fn a_router_that_routes_a_cell_with_no_model_fails_r1() {
    let table = generate(1);
    let setup = Setup::generated();
    for cell in [
        (CategoryId::Other, Compile),
        (Email, CallKind::Replay),
        (CategoryId::Other, CallKind::Replay),
    ] {
        let router = OneCellOff {
            derived: PolicyRouter::derived(&table, &setup),
            cell,
        };
        let result = score(&table, &setup, &router, Prices::release(), DEFAULT_DRAWS);
        assert!(!result.passed(), "{cell:?}");
        assert_eq!(
            result.lines().pop().as_deref(),
            Some("gate table=generated router=one_cell_off cells=44 routed=21 handed_back=9 derivation=differs held=ok result=fail"),
            "{cell:?}"
        );
    }
}

struct Turncoat {
    derived: PolicyRouter,
    asked: std::cell::Cell<u32>,
}

impl Router for Turncoat {
    fn name(&self) -> &'static str {
        "turncoat"
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        self.asked.set(self.asked.get() + 1);
        if self.asked.get() <= 44 {
            self.derived.pick(category, kind)
        } else {
            fixed(category, kind, false).unwrap_or(PickCell::Tier(OnDevice))
        }
    }
}

#[test]
fn the_scorer_asks_each_cell_once() {
    let table = hand_table();
    let setup = Setup::generated();
    let turncoat = Turncoat {
        derived: PolicyRouter::derived(&table, &setup),
        asked: std::cell::Cell::new(0),
    };
    let lines = score(&table, &setup, &turncoat, Prices::release(), DEFAULT_DRAWS).lines();
    assert_eq!(turncoat.asked.get(), 44);
    assert_eq!(
        line(&lines, "versus name=router "),
        "versus name=router attempts=4 passed=4 wasted=0 handed_back=0 cost_micro=37600 wall_s=42"
    );
}

#[test]
fn cells_with_development_rows_alone_count_in_no_comparison() {
    let mut rows = hand_table().rows().to_vec();
    for (seed, machine, pass) in [
        (One, GenA, true),
        (One, GenB, true),
        (Two, GenA, true),
        (Two, GenB, true),
    ] {
        rows.push(row(CallKind::Recover, OnDevice, seed, machine, pass, 1, 50));
        let mut shop = row(Compile, OnDevice, seed, machine, pass, 1, 50);
        shop.category = CategoryId::ShopOrder;
        shop.pass = seed == Two;
        rows.push(shop);
    }
    rows.sort_by_cached_key(Row::to_line);
    let table = Table::from_rows(&rows).expect("a table");
    let setup = Setup::generated();
    let derived = PolicyRouter::derived(&table, &setup);
    let lines = score(&table, &setup, &derived, Prices::release(), DEFAULT_DRAWS).lines();
    assert_eq!(
        line(&lines, "cell category=email kind=recover "),
        "cell category=email kind=recover pick=on_device basis=line_met dev_k=4 dev_n=4 held_k=0 held_n=0 lower_pct=47 cost_micro=0 wall_s=0"
    );
    assert_eq!(
        line(&lines, "cell category=shop_order kind=compile "),
        "cell category=shop_order kind=compile pick=hand_back basis=line_missed dev_k=2 dev_n=4 held_k=0 held_n=0 lower_pct=0 cost_micro=0 wall_s=0"
    );
    assert_eq!(
        lines[55..],
        [
            "versus name=router attempts=4 passed=4 wasted=0 handed_back=0 cost_micro=37600 wall_s=42",
            "versus name=fixed_on_device attempts=4 passed=3 wasted=1 handed_back=0 cost_micro=0 wall_s=37",
            "versus name=fixed_low_cost_hosted attempts=2 passed=2 wasted=0 handed_back=1 cost_micro=37600 wall_s=12",
            "versus name=random attempts=4 passed=3 wasted=0 handed_back=0 cost_micro=18236 wall_s=39",
            "versus name=hindsight attempts=4 passed=4 wasted=0 handed_back=0 cost_micro=18800 wall_s=40",
            "gate table=generated router=derived cells=44 routed=3 handed_back=27 derivation=same held=miss result=fail",
        ]
    );
    assert_eq!(
        line(&lines, "category id=email "),
        "category id=email compile=on_device recover=on_device write=low_cost_hosted line=missed cost_micro=37600"
    );
}

#[test]
fn a_cell_handed_back_shows_the_counts_of_the_tier_that_the_derivation_wrote() {
    let mut rows = hand_table().rows().to_vec();
    let recover = [
        (OnDevice, Three, GenA, true),
        (OnDevice, Three, GenB, true),
        (LowCostHosted, One, GenA, false),
        (LowCostHosted, One, GenB, true),
        (LowCostHosted, Two, GenA, true),
        (LowCostHosted, Two, GenB, true),
        (LowCostHosted, Three, GenA, true),
        (LowCostHosted, Three, GenB, false),
        (Tier::Frontier, One, GenA, true),
        (Tier::Frontier, One, GenB, true),
        (Tier::Frontier, Three, GenA, true),
        (Tier::Frontier, Three, GenB, true),
    ];
    for (tier, seed, machine, pass) in recover {
        rows.push(row(CallKind::Recover, tier, seed, machine, pass, 1, 50));
    }
    for machine in [GenA, GenB] {
        let mut shop = row(Write, OnDevice, Three, machine, true, 1, 50);
        shop.category = CategoryId::ShopOrder;
        rows.push(shop);
    }
    rows.sort_by_cached_key(Row::to_line);
    let table = Table::from_rows(&rows).expect("a table");
    let setup = Setup::generated();
    let derived = PolicyRouter::derived(&table, &setup);
    let lines = score(&table, &setup, &derived, Prices::release(), DEFAULT_DRAWS).lines();
    assert_eq!(
        line(&lines, "cell category=email kind=recover "),
        "cell category=email kind=recover pick=hand_back basis=line_missed dev_k=3 dev_n=4 held_k=1 held_n=2 lower_pct=0 cost_micro=0 wall_s=0"
    );
    assert_eq!(
        line(&lines, "cell category=shop_order kind=write "),
        "cell category=shop_order kind=write pick=hand_back basis=untested dev_k=0 dev_n=0 held_k=0 held_n=0 lower_pct=0 cost_micro=0 wall_s=0"
    );
    assert_eq!(
        line(&lines_of(&Handpicked, 1_000), "cell category=email kind=compile "),
        "cell category=email kind=compile pick=hand_back basis=line_met dev_k=4 dev_n=4 held_k=2 held_n=2 lower_pct=0 cost_micro=0 wall_s=0"
    );
}

struct NoneIn {
    derived: PolicyRouter,
    cells: Vec<(CategoryId, CallKind)>,
}

impl Router for NoneIn {
    fn name(&self) -> &'static str {
        "none_in"
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        if self.cells.contains(&(category, kind)) {
            PickCell::None
        } else {
            self.derived.pick(category, kind)
        }
    }
}

#[test]
fn a_category_with_rows_and_no_routed_cell_is_a_shortfall() {
    let setup = Setup::generated();
    let rows: Vec<Row> = generate(3)
        .rows()
        .iter()
        .filter(|row| row.category != CategoryId::TenantRent || row.kind == Compile)
        .cloned()
        .collect();
    let table = Table::from_rows(&rows).expect("a table");
    let router = NoneIn {
        derived: PolicyRouter::derived(&table, &setup),
        cells: vec![(CategoryId::TenantRent, Compile)],
    };
    let lines = score(&table, &setup, &router, Prices::release(), DEFAULT_DRAWS).lines();
    assert_eq!(
        line(&lines, "category id=tenant_rent "),
        "category id=tenant_rent compile=none recover=hand_back write=hand_back line=shortfall cost_micro=0"
    );
    let router = NoneIn {
        derived: PolicyRouter::derived(&hand_table(), &setup),
        cells: vec![(Email, Compile), (Email, Write)],
    };
    assert_eq!(
        line(&lines_of(&router, 1_000), "category id=email "),
        "category id=email compile=none recover=hand_back write=none line=shortfall cost_micro=0"
    );
    let mut rows = hand_table().rows().to_vec();
    for machine in [GenA, GenB] {
        let mut shop = row(Write, OnDevice, Three, machine, true, 1, 50);
        shop.category = CategoryId::ShopOrder;
        rows.push(shop);
        let mut calendar = row(Compile, OnDevice, One, machine, true, 1, 50);
        calendar.category = CategoryId::CalendarCleanup;
        rows.push(calendar);
    }
    rows.sort_by_cached_key(Row::to_line);
    let table = Table::from_rows(&rows).expect("a table");
    let derived = PolicyRouter::derived(&table, &setup);
    let lines = score(&table, &setup, &derived, Prices::release(), DEFAULT_DRAWS).lines();
    assert_eq!(
        line(&lines, "category id=shop_order "),
        "category id=shop_order compile=hand_back recover=hand_back write=hand_back line=shortfall cost_micro=0"
    );
    assert_eq!(
        line(&lines, "cell category=calendar_cleanup kind=compile "),
        "cell category=calendar_cleanup kind=compile pick=hand_back basis=untested dev_k=2 dev_n=2 held_k=0 held_n=0 lower_pct=0 cost_micro=0 wall_s=0"
    );
    assert_eq!(
        line(&lines, "category id=calendar_cleanup "),
        "category id=calendar_cleanup compile=hand_back recover=hand_back write=hand_back line=shortfall cost_micro=0"
    );
}

#[test]
fn a_breach_of_r3_prints_its_fault_after_the_verdict_and_fails() {
    let table = hand_table();
    let setup = Setup::generated();
    let derived = PolicyRouter::derived(&table, &setup);
    let result = score(&table, &setup, &derived, Prices::release(), DEFAULT_DRAWS);
    let lines = result.lines();
    assert!(result.passed() && !result.hindsight_fault);
    assert_eq!(
        line(&lines, "versus name=hindsight "),
        "versus name=hindsight attempts=4 passed=4 wasted=0 handed_back=0 cost_micro=18800 wall_s=40"
    );
    let gate = line(&lines, "gate ").to_owned();
    assert_eq!(lines.last(), Some(&gate));
    let router = result.versus[0];
    let hindsight = *result.versus.last().expect("the line hindsight");
    assert_eq!((router.name, hindsight.name), ("router", "hindsight"));
    let wastes = Versus {
        wasted: 1,
        ..hindsight
    };
    let passes_fewer = Versus {
        passed: router.passed - 1,
        ..hindsight
    };
    for breach in [wastes, passes_fewer] {
        let mut versus = result.versus.clone();
        *versus.last_mut().expect("the line hindsight") = breach;
        let breached = Score::new(
            result.cells.clone(),
            result.categories.clone(),
            versus,
            result.verdict,
        );
        let lines = breached.lines();
        assert_eq!(
            lines[lines.len() - 2..],
            [gate.clone(), "gate fault=hindsight".to_owned()],
            "{breach:?}"
        );
        assert!(breached.hindsight_fault && !breached.passed(), "{breach:?}");
    }
}

#[test]
fn a_routed_tier_with_one_held_out_attempt_misses_r2() {
    let rows: Vec<Row> = hand_table()
        .rows()
        .iter()
        .filter(|row| !(row.kind == Compile && row.seed == Three && row.machine == GenB))
        .cloned()
        .collect();
    let table = Table::from_rows(&rows).expect("a table");
    let setup = Setup::generated();
    let derived = PolicyRouter::derived(&table, &setup);
    let result = score(&table, &setup, &derived, Prices::release(), DEFAULT_DRAWS);
    let lines = result.lines();
    assert_eq!(
        line(&lines, "cell category=email kind=compile "),
        "cell category=email kind=compile pick=on_device basis=line_met dev_k=4 dev_n=4 held_k=1 held_n=1 lower_pct=54 cost_micro=0 wall_s=10"
    );
    assert_eq!(
        lines.last().map(String::as_str),
        Some("gate table=generated router=derived cells=44 routed=2 handed_back=28 derivation=same held=miss result=fail")
    );
    assert!(!result.passed());
}

#[test]
fn cost_sums_beyond_u64_are_u64_max_on_the_cell_and_versus_lines() {
    let mut rows = hand_table().rows().to_vec();
    for row in rows
        .iter_mut()
        .filter(|row| row.kind == Write && row.tier == LowCostHosted && row.seed == Three)
    {
        row.tokens_in = u64::MAX;
        row.tokens_out = u64::MAX;
    }
    let table = Table::from_rows(&rows).expect("a table");
    let setup = Setup::generated();
    let derived = PolicyRouter::derived(&table, &setup);
    let lines = score(&table, &setup, &derived, Prices::release(), DEFAULT_DRAWS).lines();
    assert_eq!(
        line(&lines, "cell category=email kind=write "),
        "cell category=email kind=write pick=low_cost_hosted basis=line_met dev_k=4 dev_n=4 held_k=2 held_n=2 lower_pct=60 cost_micro=18446744073709551615 wall_s=12"
    );
    assert_eq!(
        line(&lines, "versus name=router "),
        "versus name=router attempts=4 passed=4 wasted=0 handed_back=0 cost_micro=18446744073709551615 wall_s=42"
    );
}

fn with_compile_held_miss() -> Table {
    let mut rows = hand_table().rows().to_vec();
    for row in rows
        .iter_mut()
        .filter(|row| row.kind == Compile && row.seed == Three && row.machine == GenB)
    {
        row.pass = false;
    }
    Table::from_rows(&rows).expect("a table")
}

#[test]
fn a_routed_cell_that_is_not_clean_has_no_lower_bound() {
    let table = with_compile_held_miss();
    let setup = Setup::generated();
    let derived = PolicyRouter::derived(&table, &setup);
    let lines = score(&table, &setup, &derived, Prices::release(), DEFAULT_DRAWS).lines();
    assert_eq!(
        line(&lines, "cell category=email kind=compile "),
        "cell category=email kind=compile pick=on_device basis=line_met dev_k=4 dev_n=4 held_k=1 held_n=2 lower_pct=0 cost_micro=0 wall_s=30"
    );
}

#[test]
fn random_attempts_nothing_on_a_table_with_no_held_out_row() {
    let rows: Vec<Row> = hand_table()
        .rows()
        .iter()
        .filter(|row| row.seed != Three)
        .cloned()
        .collect();
    let table = Table::from_rows(&rows).expect("a table");
    let setup = Setup::generated();
    let derived = PolicyRouter::derived(&table, &setup);
    let lines = score(&table, &setup, &derived, Prices::release(), DEFAULT_DRAWS).lines();
    assert_eq!(
        line(&lines, "versus name=random "),
        "versus name=random attempts=0 passed=0 wasted=0 handed_back=0 cost_micro=0 wall_s=0"
    );
}
