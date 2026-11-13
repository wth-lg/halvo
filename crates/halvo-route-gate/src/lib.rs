//! The library of the gate tool `route-gate`: the outcome table (`table`), its generator
//! (`generate`), the derivation of the policy (`derive`), the `--out` file (`out`), the routers
//! under test (`routers`), the scorer (`score`) and the self-test (`selftest`). `Router` and
//! `Setup` are the seam for the router `product`.

use halvo_route::policy::PickCell;
use halvo_route::tier::{CallKind, CategoryId, Tier};

pub mod derive;
pub mod generate;
pub mod import;
pub mod out;
pub mod probe;
pub mod product;
pub mod routers;
pub mod score;
pub mod selftest;
pub mod spendsim;
pub mod table;

/// A router under test: its pick for each cell. It returns `none` for a `replay` cell, and
/// `hand_back` for a cell of `other` and, on a recorded table, for a category with no runtime.
pub trait Router {
    fn name(&self) -> &'static str;
    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell;
}

/// What the derivation and the scorer read a table with: the tiers that may be picked, the models
/// that fill the tier `on_device` (`driver` for `compile` and `recover`, `writer` for `write`), and
/// whether the table is recorded.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Setup {
    pub allowed: Vec<Tier>,
    pub driver: Option<String>,
    pub writer: Option<String>,
    pub recorded: bool,
}

impl Setup {
    /// The setup of a generated table: every tier, the model `synthetic` for both roles.
    pub fn generated() -> Setup {
        Setup {
            allowed: Tier::ALL.to_vec(),
            driver: Some(table::SYNTHETIC.to_owned()),
            writer: Some(table::SYNTHETIC.to_owned()),
            recorded: false,
        }
    }

    /// The model whose rows count for a kind: `driver` for `compile` and `recover`, `writer` for
    /// `write`, and none for `replay`.
    pub fn fill(&self, kind: CallKind) -> Option<&str> {
        match kind {
            CallKind::Compile | CallKind::Recover => self.driver.as_deref(),
            CallKind::Write => self.writer.as_deref(),
            CallKind::Replay => None,
        }
    }
}
