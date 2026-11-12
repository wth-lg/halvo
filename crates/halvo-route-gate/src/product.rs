//! The router `product` of contract T6.6, the product's own pick on a policy, the guard under
//! which the tool asks a router for its picks, and the setup of a recorded table.

use std::num::NonZeroU32;
use std::panic::{catch_unwind, AssertUnwindSafe};

use halvo_route::pick::pick;
use halvo_route::policy::{cells, PickCell, Policy, CELL_COUNT};
use halvo_route::prices::Prices;
use halvo_route::release;
use halvo_route::tier::{CallKind, CategoryId, Tier};

use crate::import::Date;
use crate::score::{score, Score};
use crate::table::{Model, Table};
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

/// The setup of a recorded table (contract T6.6): the tiers that `release::ruling()` allows, and
/// the `driver` and `writer` of the freeze file whose text is given, or none of either when no
/// text is given. A text that is not the two lines of T5.2 is refused; the string is the detail
/// of `REFUSED file`.
pub fn recorded_setup(freeze: Option<&str>) -> Result<Setup, String> {
    let (driver, writer) = match freeze {
        Some(text) => {
            let freeze = Freeze::parse(text)?;
            (freeze.driver, freeze.writer)
        }
        None => (None, None),
    };
    Ok(Setup {
        allowed: release::ruling().allowed().to_vec(),
        driver,
        writer,
        recorded: true,
    })
}

/// The freeze file of contract T5.2: `freeze rows=<n> models=<n> sources=2 date=<YYYY-MM-DD>`
/// and `on_device driver=<token|none> writer=<token|none>`, each line ended by one LF, where a
/// token is a model token other than `synthetic`.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Freeze {
    pub rows: u64,
    pub models: u64,
    pub date: String,
    pub driver: Option<String>,
    pub writer: Option<String>,
}

impl Freeze {
    /// Reads the two lines and refuses any other text, with a detail that names the line.
    pub fn parse(text: &str) -> Result<Freeze, String> {
        let refused = |line: usize| format!("--freeze line {line} is not of the freeze form");
        let Some(body) = text.strip_suffix('\n') else {
            return Err("--freeze does not end with LF".to_owned());
        };
        let mut lines = body.split('\n');
        let (Some(first), Some(second), None) = (lines.next(), lines.next(), lines.next()) else {
            return Err("--freeze does not hold two lines".to_owned());
        };
        let mut fields = first.split(' ');
        let (Some("freeze"), Some(rows), Some(models), Some("sources=2"), Some(date), None) = (
            fields.next(),
            fields.next().and_then(|f| f.strip_prefix("rows=")),
            fields.next().and_then(|f| f.strip_prefix("models=")),
            fields.next(),
            fields.next().and_then(|f| f.strip_prefix("date=")),
            fields.next(),
        ) else {
            return Err(refused(1));
        };
        let (Some(rows), Some(models), Some(date)) =
            (decimal(rows), decimal(models), Date::parse(date))
        else {
            return Err(refused(1));
        };
        let mut fields = second.split(' ');
        let (Some("on_device"), Some(driver), Some(writer), None) = (
            fields.next(),
            fields.next().and_then(|f| f.strip_prefix("driver=")),
            fields.next().and_then(|f| f.strip_prefix("writer=")),
            fields.next(),
        ) else {
            return Err(refused(2));
        };
        let (Some(driver), Some(writer)) = (model(driver), model(writer)) else {
            return Err(refused(2));
        };
        Ok(Freeze {
            rows,
            models,
            date: date.as_str().to_owned(),
            driver,
            writer,
        })
    }
}

fn decimal(text: &str) -> Option<u64> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    canonical.then(|| text.parse().ok()).flatten()
}

fn model(text: &str) -> Option<Option<String>> {
    if text == "none" {
        return Some(None);
    }
    if text == Model::synthetic().as_str() {
        return None;
    }
    Model::parse(text)
        .ok()
        .map(|model| Some(model.as_str().to_owned()))
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
