//! The generator of contract T5.6 and its stream.
//!
//! The stream is SplitMix64, written here: `SplitMix64::new(seed)` starts it from a `u64` seed,
//! `SplitMix64::next_u64` draws its next `u64`, and `SplitMix64::below(bound)` draws evenly from 0
//! to `bound - 1`, rejecting each draw under `2^64 mod bound`. Other code that needs a seeded stream,
//! the simulation of contract T7.5 among it, uses these three items as they are.
//!
//! `generate(seed)` makes the 540 rows of a generated table from `SplitMix64::new(seed)`. Its
//! draws, in this order: for each seed category in the order of `CategoryId::ALL`, each of the
//! kinds `compile`, `recover` and `write`, and each tier in the order of `Tier::ALL`, first the
//! behaviour, `below(3)` for every attempt passes, one development attempt fails, every attempt
//! fails (no draw for the three cells of `utility_bill_payment`, whose behaviours are the traps of
//! T5.6); then, for the behaviour in which one development attempt fails, which one, `below(4)`
//! over (seed 1, `gen_a`), (seed 1, `gen_b`), (seed 2, `gen_a`), (seed 2, `gen_b`); then, for each
//! of the six attempts (the seeds 1 to 3, and for each the machines `gen_a` and `gen_b`), `steps`,
//! `wall_s`, `tokens_in` and `tokens_out`, each `1 + below(1000)`. The rows are then sorted by the
//! byte order of their text.

use std::num::{NonZeroU32, NonZeroU64};

use halvo_route::tier::{CallKind, CategoryId, Level, Tier};

use crate::table::{Machine, Model, Row, Seed, Source, Table, Task};

/// The number of rows of a generated table: 10 categories, 3 kinds, 3 tiers, 3 seeds, 2 machines.
pub const ROWS: usize = 540;

/// The SplitMix64 stream.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub const fn new(seed: u64) -> SplitMix64 {
        SplitMix64 { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, bound: NonZeroU64) -> u64 {
        let bound = bound.get();
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let draw = self.next_u64();
            if draw >= threshold {
                return draw % bound;
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Behaviour {
    AllPass,
    OneDevFails,
    AllFail,
}

const BEHAVIOURS: [Behaviour; 3] = [
    Behaviour::AllPass,
    Behaviour::OneDevFails,
    Behaviour::AllFail,
];
const KINDS: [CallKind; 3] = [CallKind::Compile, CallKind::Recover, CallKind::Write];
const ATTEMPTS: [(Seed, Machine); 6] = [
    (Seed::One, Machine::GenA),
    (Seed::One, Machine::GenB),
    (Seed::Two, Machine::GenA),
    (Seed::Two, Machine::GenB),
    (Seed::Three, Machine::GenA),
    (Seed::Three, Machine::GenB),
];
const DEV_ATTEMPTS: NonZeroU64 = NonZeroU64::new(4).expect("four");
const BEHAVIOUR_COUNT: NonZeroU64 = NonZeroU64::new(3).expect("three");
const FIGURE_TOP: NonZeroU64 = NonZeroU64::new(1_000).expect("a thousand");
const FIRST: NonZeroU32 = NonZeroU32::new(1).expect("one");

const fn trap(category: CategoryId, kind: CallKind, tier: Tier) -> Option<Behaviour> {
    if !matches!(category, CategoryId::UtilityBillPayment) {
        return None;
    }
    match (kind, tier) {
        (CallKind::Compile, Tier::OnDevice) | (CallKind::Recover, Tier::OnDevice) => {
            Some(Behaviour::OneDevFails)
        }
        (CallKind::Compile, _) | (CallKind::Recover, Tier::Frontier) => Some(Behaviour::AllPass),
        (CallKind::Recover, Tier::LowCostHosted) | (CallKind::Write, _) => Some(Behaviour::AllFail),
        (CallKind::Replay, _) => None,
    }
}

/// The generated table of a seed.
pub fn generate(seed: u64) -> Table {
    let mut stream = SplitMix64::new(seed);
    let mut rows = Vec::with_capacity(ROWS);
    for category in CategoryId::ALL
        .into_iter()
        .filter(|category| category.is_seed())
    {
        for kind in KINDS {
            for tier in Tier::ALL {
                let behaviour = match trap(category, kind, tier) {
                    Some(fixed) => fixed,
                    None => BEHAVIOURS[stream.below(BEHAVIOUR_COUNT) as usize],
                };
                let failing = (behaviour == Behaviour::OneDevFails)
                    .then(|| stream.below(DEV_ATTEMPTS) as usize);
                for (i, (seed, machine)) in ATTEMPTS.into_iter().enumerate() {
                    let pass = match behaviour {
                        Behaviour::AllPass => true,
                        Behaviour::OneDevFails => failing != Some(i),
                        Behaviour::AllFail => false,
                    };
                    let mut figure = || 1 + stream.below(FIGURE_TOP);
                    let (steps, wall_s, tokens_in, tokens_out) =
                        (figure(), figure(), figure(), figure());
                    rows.push(Row {
                        task: Task::Gen,
                        category,
                        kind,
                        tier,
                        model: Model::synthetic(),
                        machine,
                        seed,
                        attempt: FIRST,
                        pass,
                        steps,
                        wall_s,
                        tokens_in,
                        tokens_out,
                        level: Level::Medium,
                        source: Source::Generated,
                    });
                }
            }
        }
    }
    rows.sort_by_cached_key(Row::to_line);
    Table::from_rows(&rows).expect("the generator makes rows of the table form")
}
