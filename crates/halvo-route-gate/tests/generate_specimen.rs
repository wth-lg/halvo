use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;

use halvo_route::tier::{CallKind, CategoryId, Level, Tier};
use halvo_route_gate::generate::{generate, SplitMix64, ROWS};
use halvo_route_gate::table::{Machine, Source, Split, Table, Task, SYNTHETIC};

const SPECIMEN: &str = include_str!("data/specimen.tsv");

#[test]
fn stream_gives_the_reference_values() {
    let mut stream = SplitMix64::new(1_234_567);
    let first: Vec<u64> = (0..5).map(|_| stream.next_u64()).collect();
    assert_eq!(
        first,
        [
            6_457_827_717_110_365_317,
            3_203_168_211_198_807_973,
            9_817_491_932_198_370_423,
            4_593_380_528_125_082_431,
            16_408_922_859_458_223_821
        ]
    );
    let mut zero = SplitMix64::new(0);
    assert_eq!(zero.next_u64(), 16_294_208_416_658_607_535);
    let mut one = SplitMix64::new(1);
    assert_eq!(one.next_u64(), 10_451_216_379_200_822_465);
}

#[test]
fn below_draws_evenly_under_its_bound() {
    let bound = |n: u64| NonZeroU64::new(n).expect("not zero");
    let mut stream = SplitMix64::new(7);
    assert!((0..100).all(|_| stream.below(bound(1)) == 0));
    let mut seen = [0u32; 3];
    for _ in 0..30_000 {
        seen[usize::try_from(stream.below(bound(3))).expect("small")] += 1;
    }
    assert!(
        seen.iter().all(|n| (9_400..=10_600).contains(n)),
        "{seen:?}"
    );
    let top = bound(u64::MAX);
    assert!((0..1_000).all(|_| stream.below(top) < u64::MAX));
    let mut a = SplitMix64::new(3);
    let mut b = SplitMix64::new(3);
    assert_eq!(a.below(bound(1_000)), b.next_u64() % 1_000);
}

#[test]
fn specimen_is_the_output_for_seed_1() {
    assert_eq!(generate(1).to_text(), SPECIMEN);
    assert_eq!(SPECIMEN.lines().count(), ROWS + 1);
    assert_eq!(
        Table::parse(SPECIMEN)
            .expect("the specimen parses")
            .to_text(),
        SPECIMEN
    );
}

#[test]
fn generated_tables_have_their_form() {
    let mut texts = BTreeSet::new();
    for seed in 1..=30 {
        let table = generate(seed);
        assert_eq!(table.rows().len(), ROWS);
        assert!(table.is_generated());
        assert_eq!(Table::parse(&table.to_text()).as_ref(), Ok(&table));
        let mut places = BTreeSet::new();
        for row in table.rows() {
            assert_eq!(
                (
                    row.task,
                    row.model.as_str(),
                    row.level,
                    row.source,
                    row.attempt.get()
                ),
                (Task::Gen, SYNTHETIC, Level::Medium, Source::Generated, 1)
            );
            assert!(row.category.is_seed() && row.kind != CallKind::Replay);
            assert!(matches!(row.machine, Machine::GenA | Machine::GenB));
            for figure in [row.steps, row.wall_s, row.tokens_in, row.tokens_out] {
                assert!((1..=1_000).contains(&figure));
            }
            assert!(places.insert((row.category, row.kind, row.tier, row.seed, row.machine)));
        }
        assert_eq!(places.len(), 10 * 3 * 3 * 3 * 2);
        assert!(texts.insert(table.to_text()), "seed {seed} repeats a table");
    }
}

type Passes = BTreeMap<(CategoryId, CallKind, Tier), (u32, u32, u32, u32)>;

fn passes(table: &Table) -> Passes {
    let mut passes = Passes::new();
    for row in table.rows() {
        let entry = passes
            .entry((row.category, row.kind, row.tier))
            .or_default();
        match row.split() {
            Split::Dev => {
                entry.0 += u32::from(row.pass);
                entry.1 += 1;
            }
            Split::Held => {
                entry.2 += u32::from(row.pass);
                entry.3 += 1;
            }
        }
    }
    passes
}

#[test]
fn traps_stand_in_every_table() {
    let bill = CategoryId::UtilityBillPayment;
    for seed in 1..=200 {
        let passes = passes(&generate(seed));
        let want = [
            (CallKind::Compile, Tier::OnDevice, (3, 4, 2, 2)),
            (CallKind::Compile, Tier::LowCostHosted, (4, 4, 2, 2)),
            (CallKind::Compile, Tier::Frontier, (4, 4, 2, 2)),
            (CallKind::Recover, Tier::OnDevice, (3, 4, 2, 2)),
            (CallKind::Recover, Tier::LowCostHosted, (0, 4, 0, 2)),
            (CallKind::Recover, Tier::Frontier, (4, 4, 2, 2)),
            (CallKind::Write, Tier::OnDevice, (0, 4, 0, 2)),
            (CallKind::Write, Tier::LowCostHosted, (0, 4, 0, 2)),
            (CallKind::Write, Tier::Frontier, (0, 4, 0, 2)),
        ];
        for (kind, tier, counts) in want {
            assert_eq!(
                passes[&(bill, kind, tier)],
                counts,
                "seed {seed} {kind:?} {tier:?}"
            );
        }
    }
}

#[test]
fn every_tier_passes_all_fails_one_development_attempt_or_fails_all() {
    let mut behaviours = BTreeMap::new();
    for seed in 1..=200 {
        for (place, counts) in passes(&generate(seed)) {
            assert!(
                matches!(counts, (4, 4, 2, 2) | (3, 4, 2, 2) | (0, 4, 0, 2)),
                "seed {seed} {place:?} {counts:?}"
            );
            if place.0 != CategoryId::UtilityBillPayment {
                *behaviours.entry(counts).or_insert(0u32) += 1;
            }
        }
    }
    assert_eq!(behaviours.len(), 3);
    assert!(
        behaviours.values().all(|n| (5_100..=5_700).contains(n)),
        "{behaviours:?}"
    );
}
