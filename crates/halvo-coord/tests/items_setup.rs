#[path = "items_support.rs"]
mod support;

use halvo_coord::items::{params, raise_step, Raise, Source};
use halvo_coord::mem::Config;
use halvo_coord::schedule::{holds, Hold};
use halvo_coord::types::fixtures::{self, State};
use halvo_coord::types::{Category, Doing, Facts, Limits, OwnItemKind, Param};
use support::name;

const LIMITS: Limits = Limits::PRODUCT;

/// An idle agent with no item and no run of the category, whose newest bindings hold the slots
/// `bound`.
fn idle(category: Category, bound: &[Param]) -> Facts<Config> {
    let mut f = fixtures::facts(State::Ten)
        .into_iter()
        .next()
        .expect("an agent");
    f.workflow.category = category;
    f.slots = bound.iter().map(|p| name(p.name())).collect();
    f.doing = Doing::Idle(None);
    f
}

/// Every set of the nine parameters of I3.5, as a list.
fn every_set() -> Vec<Vec<Param>> {
    (0u32..1 << Param::ALL.len())
        .map(|bits| {
            Param::ALL
                .iter()
                .enumerate()
                .filter(|(i, _)| bits & (1 << i) != 0)
                .map(|(_, p)| *p)
                .collect()
        })
        .collect()
}

#[test]
fn the_hold_setup_and_the_item_setup_read_one_table_of_parameters() {
    let sets = every_set();
    assert_eq!(sets.len(), 512);
    for c in Category::ALL {
        for bound in &sets {
            let f = idle(*c, bound);
            let hold = holds(&f).contains(&Hold::Setup);
            let item = raise_step(&f, Source::Setup, &LIMITS) == Raise::Own(OwnItemKind::Setup);
            assert_eq!(hold, item, "{c} with the slots {bound:?}");
            let unbound = params(*c).iter().any(|p| !bound.contains(p));
            assert_eq!(hold, unbound, "{c} with the slots {bound:?}");
        }
    }
}
