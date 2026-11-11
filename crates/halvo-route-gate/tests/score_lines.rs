use std::num::NonZeroU32;

use halvo_route::policy::PickCell;
use halvo_route::prices::Prices;
use halvo_route::tier::{CallKind, CategoryId, Level, Tier};
use halvo_route_gate::generate::generate;
use halvo_route_gate::routers::{fixed, OneTier, PolicyRouter, Random};
use halvo_route_gate::score::{score, DEFAULT_DRAWS};
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
