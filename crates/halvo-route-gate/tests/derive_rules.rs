use std::num::NonZeroU32;

use halvo_route::line::LINE_HELD_MIN;
use halvo_route::policy::{Basis, PickCell, Policy, Row as PolicyRow};
use halvo_route::tier::{CallKind, CategoryId, Level, Tier};
use halvo_route_gate::derive::{derive, derive_on};
use halvo_route_gate::generate::generate;
use halvo_route_gate::table::{Machine, Model, Row, Seed, Source, Split, Table, Task};
use halvo_route_gate::Setup;

const DEV: [(Seed, Machine); 4] = [
    (Seed::One, Machine::GenA),
    (Seed::One, Machine::GenB),
    (Seed::Two, Machine::GenA),
    (Seed::Two, Machine::GenB),
];
const HELD: [(Seed, Machine); 2] = [(Seed::Three, Machine::GenA), (Seed::Three, Machine::GenB)];

fn row(
    place: (CategoryId, CallKind, Tier),
    (seed, machine): (Seed, Machine),
    pass: bool,
    level: Level,
) -> Row {
    Row {
        task: Task::Gen,
        category: place.0,
        kind: place.1,
        tier: place.2,
        model: Model::synthetic(),
        machine,
        seed,
        attempt: NonZeroU32::new(1).expect("one"),
        pass,
        steps: 1,
        wall_s: 1,
        tokens_in: 1,
        tokens_out: 1,
        level,
        source: Source::Generated,
    }
}

fn rows(
    place: (CategoryId, CallKind, Tier),
    attempts: &[(Seed, Machine)],
    passes: &[bool],
) -> Vec<Row> {
    attempts
        .iter()
        .zip(passes)
        .map(|(attempt, pass)| row(place, *attempt, *pass, Level::Medium))
        .collect()
}

fn table(mut rows: Vec<Row>) -> Table {
    rows.sort_by_cached_key(Row::to_line);
    Table::from_rows(&rows).expect("the rows make a table")
}

fn met(tier: Tier, level: Level, k: u32, n: u32) -> PolicyRow {
    PolicyRow {
        pick: PickCell::Tier(tier),
        level: Some(level),
        basis: Basis::LineMet,
        k,
        n,
    }
}

fn hand_back(basis: Basis, k: u32, n: u32) -> PolicyRow {
    PolicyRow {
        pick: PickCell::HandBack,
        level: None,
        basis,
        k,
        n,
    }
}

use CallKind::{Compile, Recover, Replay, Write};
use CategoryId::{CalendarCleanup, Email, Other, ShopOrder, TenantRent, UtilityBillPayment};
use Tier::{Frontier, LowCostHosted, OnDevice};

fn cases() -> Table {
    let mut all = Vec::new();
    all.extend(rows((Email, Compile, OnDevice), &DEV, &[true; 4]));
    all.extend(rows((Email, Compile, OnDevice), &HELD, &[true; 2]));
    all.extend(rows((Email, Recover, OnDevice), &DEV[..3], &[true; 3]));
    all.extend(rows((Email, Recover, LowCostHosted), &DEV, &[true; 4]));
    all.extend(rows(
        (Email, Write, OnDevice),
        &DEV,
        &[true, false, true, true],
    ));
    all.extend(rows(
        (Email, Write, LowCostHosted),
        &DEV,
        &[false, true, false, true],
    ));
    all.extend(rows((ShopOrder, Compile, OnDevice), &DEV[..2], &[true; 2]));
    all.extend(rows(
        (ShopOrder, Compile, LowCostHosted),
        &DEV,
        &[true, true, true, false],
    ));
    all.extend(
        DEV.iter()
            .map(|attempt| row((ShopOrder, Write, OnDevice), *attempt, true, Level::High)),
    );
    all.extend(rows((ShopOrder, Write, Frontier), &DEV, &[true; 4]));
    all.extend(rows(
        (CalendarCleanup, Compile, OnDevice),
        &DEV,
        &[false, true, true, true],
    ));
    all.extend(rows(
        (CalendarCleanup, Compile, OnDevice),
        &HELD,
        &[true; 2],
    ));
    all.extend(rows((CalendarCleanup, Compile, Frontier), &DEV, &[true; 4]));
    all.extend(rows(
        (CalendarCleanup, Recover, OnDevice),
        &HELD,
        &[true; 2],
    ));
    all.extend(rows((TenantRent, Compile, OnDevice), &DEV, &[true; 4]));
    all.extend(rows((Other, Compile, OnDevice), &DEV, &[true; 4]));
    table(all)
}

#[test]
fn derivation_takes_the_cheapest_tier_that_meets_the_line() {
    let policy = derive(&cases(), &Setup::generated());
    let want = [
        ((Email, Compile), met(OnDevice, Level::Medium, 4, 4)),
        ((Email, Recover), met(LowCostHosted, Level::Medium, 4, 4)),
        ((Email, Write), hand_back(Basis::LineMissed, 3, 4)),
        (
            (Email, Replay),
            PolicyRow {
                pick: PickCell::None,
                level: None,
                basis: Basis::NoModel,
                k: 0,
                n: 0,
            },
        ),
        ((ShopOrder, Compile), hand_back(Basis::Untested, 2, 2)),
        ((ShopOrder, Recover), hand_back(Basis::Untested, 0, 0)),
        ((ShopOrder, Write), met(OnDevice, Level::High, 4, 4)),
        (
            (CalendarCleanup, Compile),
            met(Frontier, Level::Medium, 4, 4),
        ),
        ((CalendarCleanup, Recover), hand_back(Basis::Untested, 0, 0)),
        ((TenantRent, Compile), met(OnDevice, Level::Medium, 4, 4)),
        ((Other, Compile), hand_back(Basis::NotSeed, 0, 0)),
        (
            (UtilityBillPayment, Write),
            hand_back(Basis::Untested, 0, 0),
        ),
    ];
    for ((category, kind), row) in want {
        assert_eq!(*policy.row(category, kind), row, "{category:?} {kind:?}");
    }
}

#[test]
fn derivation_reads_only_allowed_tiers_and_the_filling_model() {
    let table = cases();
    let device = Setup {
        allowed: vec![OnDevice],
        ..Setup::generated()
    };
    let policy = derive(&table, &device);
    assert_eq!(
        *policy.row(Email, Recover),
        hand_back(Basis::Untested, 3, 3)
    );
    assert_eq!(
        *policy.row(CalendarCleanup, Compile),
        hand_back(Basis::LineMissed, 3, 4)
    );
    let hosted = Setup {
        allowed: vec![LowCostHosted, Frontier],
        ..Setup::generated()
    };
    assert_eq!(
        *derive(&table, &hosted).row(Email, Compile),
        hand_back(Basis::Untested, 0, 0)
    );
    assert_eq!(
        *derive(&table, &hosted).row(Email, Recover),
        met(LowCostHosted, Level::Medium, 4, 4)
    );
    let no_driver = Setup {
        driver: None,
        ..Setup::generated()
    };
    let policy = derive(&table, &no_driver);
    assert_eq!(
        *policy.row(Email, Compile),
        hand_back(Basis::Untested, 0, 0)
    );
    assert_eq!(
        *policy.row(ShopOrder, Write),
        met(OnDevice, Level::High, 4, 4)
    );
    let other_writer = Setup {
        writer: Some("m_small".to_owned()),
        ..Setup::generated()
    };
    assert_eq!(
        *derive(&table, &other_writer).row(ShopOrder, Write),
        hand_back(Basis::Untested, 0, 0)
    );
    let recorded = Setup {
        recorded: true,
        ..Setup::generated()
    };
    assert_eq!(
        *derive(&table, &recorded).row(TenantRent, Compile),
        hand_back(Basis::NoRuntime, 0, 0)
    );
    assert_eq!(
        *derive(&table, &recorded).row(Email, Compile),
        met(OnDevice, Level::Medium, 4, 4)
    );
}

#[test]
fn held_out_rows_decide_nothing_and_peek_reads_them() {
    let table = cases();
    let setup = Setup::generated();
    let dev = derive(&table, &setup);
    let held = derive_on(&table, &setup, Split::Held, LINE_HELD_MIN);
    let row = |category, kind| {
        let at = halvo_route::policy::cells()
            .position(|cell| cell == (category, kind))
            .expect("a cell");
        held[at]
    };
    assert_eq!(
        row(CalendarCleanup, Compile),
        met(OnDevice, Level::Medium, 2, 2)
    );
    assert_eq!(
        row(CalendarCleanup, Recover),
        met(OnDevice, Level::Medium, 2, 2)
    );
    assert_eq!(row(Email, Recover), hand_back(Basis::Untested, 0, 0));
    assert_ne!(*dev.rows(), held);
}

#[test]
fn derivation_of_generated_tables_meets_the_traps() {
    let setup = Setup::generated();
    for seed in 1..=100 {
        let policy = derive(&generate(seed), &setup);
        assert_eq!(
            policy.row(UtilityBillPayment, Compile).pick,
            PickCell::Tier(LowCostHosted),
            "seed {seed}"
        );
        assert_eq!(
            policy.row(UtilityBillPayment, Recover).pick,
            PickCell::Tier(Frontier),
            "seed {seed}"
        );
        assert_eq!(
            *policy.row(UtilityBillPayment, Write),
            hand_back(Basis::LineMissed, 0, 4),
            "seed {seed}"
        );
        assert_eq!(Policy::parse_for(&policy.to_text(), &Tier::ALL), Ok(policy));
        for category in CategoryId::ALL {
            assert_eq!(policy.row(category, Replay).pick, PickCell::None);
        }
        for kind in [Compile, Recover, Write] {
            assert_eq!(*policy.row(Other, kind), hand_back(Basis::NotSeed, 0, 0));
            for category in CategoryId::ALL.into_iter().filter(|c| c.is_seed()) {
                let row = policy.row(category, kind);
                assert!(
                    matches!(row.basis, Basis::LineMet | Basis::LineMissed),
                    "seed {seed} {category:?} {kind:?}"
                );
                assert_eq!(row.n, 4);
            }
        }
    }
}

#[test]
fn the_policies_and_tables_of_a_thousand_seeds_parse() {
    let setup = Setup::generated();
    let mut parsed = 0;
    for seed in 1..=1_000 {
        let table = generate(seed);
        assert_eq!(
            Table::parse(&table.to_text()).as_ref(),
            Ok(&table),
            "seed {seed}"
        );
        let text = derive(&table, &setup).to_text();
        assert!(Policy::parse_for(&text, &Tier::ALL).is_ok(), "seed {seed}");
        parsed += 1;
    }
    assert_eq!(parsed, 1_000);
}
