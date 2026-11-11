//! The routers under test of contract T6.2 that this unit builds: `derived`, `one_tier`, `random`
//! and `peek`. Each returns `none` for a `replay` cell and `hand_back` for a cell of `other` and,
//! on a recorded table, for a category with no runtime; the rest is its own rule.

use std::num::NonZeroU64;

use halvo_route::line::LINE_HELD_MIN;
use halvo_route::policy::{cells, PickCell, Policy, CELL_COUNT};
use halvo_route::tier::{CallKind, CategoryId, Tier};

use crate::derive::{derive, derive_on, tier_rows};
use crate::generate::SplitMix64;
use crate::table::{Split, Table};
use crate::{Router, Setup};

/// The seed of the draws of the router `random` and of the comparison `random`.
pub const RANDOM_SEED: u64 = 1;

/// The pick that every router under test returns for a cell that is no model cell it may route,
/// or none for a cell that it decides.
pub const fn fixed(category: CategoryId, kind: CallKind, recorded: bool) -> Option<PickCell> {
    if matches!(kind, CallKind::Replay) {
        Some(PickCell::None)
    } else if !category.is_seed() || (recorded && !category.has_runtime()) {
        Some(PickCell::HandBack)
    } else {
        None
    }
}

/// A router that picks what a policy says.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PolicyRouter {
    name: &'static str,
    policy: Policy,
}

impl PolicyRouter {
    /// `derived`: the derivation of contract T2.2 on the table's development rows.
    pub fn derived(table: &Table, setup: &Setup) -> PolicyRouter {
        PolicyRouter {
            name: "derived",
            policy: derive(table, setup),
        }
    }

    /// `peek`: the derivation computed on the held-out rows, with `LINE_HELD_MIN`.
    pub fn peek(table: &Table, setup: &Setup) -> PolicyRouter {
        PolicyRouter {
            name: "peek",
            policy: derive_on(table, setup, Split::Held, LINE_HELD_MIN),
        }
    }

    pub const fn policy(&self) -> &Policy {
        &self.policy
    }
}

impl Router for PolicyRouter {
    fn name(&self) -> &'static str {
        self.name
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        self.policy.row(category, kind).pick
    }
}

/// `one_tier`: every model cell on `on_device`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OneTier {
    pub recorded: bool,
}

impl Router for OneTier {
    fn name(&self) -> &'static str {
        "one_tier"
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        fixed(category, kind, self.recorded).unwrap_or(PickCell::Tier(Tier::OnDevice))
    }
}

/// `random`: for each model cell, in the order of the cells, one draw from `SplitMix64::new(1)`
/// that picks evenly among `hand_back` and the tiers that have rows of that cell, in that order.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Random {
    picks: [PickCell; CELL_COUNT],
}

impl Random {
    pub fn new(table: &Table, setup: &Setup) -> Random {
        let mut stream = SplitMix64::new(RANDOM_SEED);
        let mut picks = [PickCell::None; CELL_COUNT];
        for (pick, (category, kind)) in picks.iter_mut().zip(cells()) {
            *pick = fixed(category, kind, setup.recorded).unwrap_or_else(|| {
                let mut options = vec![PickCell::HandBack];
                for tier in Tier::ALL {
                    if tier_rows(table, setup, category, kind, tier)
                        .next()
                        .is_some()
                    {
                        options.push(PickCell::Tier(tier));
                    }
                }
                let count =
                    NonZeroU64::new(options.len() as u64).expect("hand_back is always an option");
                options[stream.below(count) as usize]
            });
        }
        Random { picks }
    }
}

impl Router for Random {
    fn name(&self) -> &'static str {
        "random"
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        let at = cells()
            .position(|cell| cell == (category, kind))
            .expect("every cell is in the list");
        self.picks[at]
    }
}
