//! The router `product` of contract T6.6, the product's own pick on a policy, and the guard under
//! which the tool asks a router for its picks.

use std::num::NonZeroU32;
use std::panic::{catch_unwind, AssertUnwindSafe};

use halvo_route::pick::pick;
use halvo_route::policy::{cells, PickCell, Policy, CELL_COUNT};
use halvo_route::prices::Prices;
use halvo_route::tier::{CallKind, CategoryId, Tier};

use crate::score::{score, Score};
use crate::table::Table;
use crate::{Router, Setup};

/// The router `product`: `halvo_route::pick::pick` on its policy with the ceiling `frontier`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ProductRouter {
    policy: Policy,
}

impl ProductRouter {
    pub const fn new(policy: Policy) -> ProductRouter {
        ProductRouter { policy }
    }
}

impl Router for ProductRouter {
    fn name(&self) -> &'static str {
        "product"
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        pick(&self.policy, category, kind, Tier::Frontier).cell()
    }
}

/// The first cell, in the order of `cells()`, at which a router panicked when it was asked.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Panicked {
    pub category: CategoryId,
    pub kind: CallKind,
}

impl Panicked {
    /// The one line that `route-gate score` prints for it, with the exit status 1:
    /// `gate fault=router category=<id> kind=<kind>`.
    pub fn line(&self) -> String {
        format!(
            "gate fault=router category={} kind={}",
            self.category.as_str(),
            self.kind.as_str()
        )
    }
}

/// A router's picks, each cell asked once in the order of `cells()`, under the router's name.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Answers {
    name: &'static str,
    picks: [PickCell; CELL_COUNT],
}

impl Answers {
    /// Asks `router` once for each cell, in the order of `cells()`, and keeps the answers. A panic
    /// of the router is caught, and the first cell at which it panicked is returned.
    pub fn ask(router: &dyn Router) -> Result<Answers, Panicked> {
        let mut picks = [PickCell::None; CELL_COUNT];
        for (slot, (category, kind)) in picks.iter_mut().zip(cells()) {
            *slot = catch_unwind(AssertUnwindSafe(|| router.pick(category, kind)))
                .map_err(|_| Panicked { category, kind })?;
        }
        Ok(Answers {
            name: router.name(),
            picks,
        })
    }

    /// What stands for a router that panicked: `none` for every cell. The derivation writes
    /// `none` for no model cell, so these answers fail R1 on every table.
    pub const fn panicked(name: &'static str) -> Answers {
        Answers {
            name,
            picks: [PickCell::None; CELL_COUNT],
        }
    }
}

impl Router for Answers {
    fn name(&self) -> &'static str {
        self.name
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        let at = cells()
            .position(|cell| cell == (category, kind))
            .expect("every cell is in the list");
        self.picks[at]
    }
}

/// Scores `router` on `table` as `score` does, with each cell asked once by `Answers::ask`: the
/// score, or the first cell at which the router panicked.
pub fn score_once(
    table: &Table,
    setup: &Setup,
    router: &dyn Router,
    prices: &Prices,
    draws: NonZeroU32,
) -> Result<Score, Panicked> {
    let answers = Answers::ask(router)?;
    Ok(score(table, setup, &answers, prices, draws))
}

/// `router` asked once for each cell: its answers, or `Answers::panicked` when it panicked.
pub fn guarded(router: &dyn Router) -> Box<dyn Router> {
    match Answers::ask(router) {
        Ok(answers) => Box::new(answers),
        Err(_) => Box::new(Answers::panicked(router.name())),
    }
}

/// What `route-gate self-test` builds the router `product` with: `ProductRouter` on the policy,
/// asked under `guarded`.
pub fn build_product(policy: Policy) -> Box<dyn Router> {
    guarded(&ProductRouter::new(policy))
}
