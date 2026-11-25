//! The router's pick before a run's model request (the routing plan's requirement A5), with the bridge between the
//! schema's ids and the router's (the routing run's U5.1), as the coordinator ruled on 2026-11-25 (log line 22454):
//! the runner calls the pick itself, on the release policy, with the agent's category, the call's kind and the
//! agent's own tier as the ceiling. A pick that hands the call back, or that picks a tier other than the run's own,
//! lets no request leave. The one file of the runner that names the router's crate, and it names the pick, the policy
//! and the id types alone: crates/halvo-model/tests/door.rs holds the closed list of what the runner may name of
//! that crate.

use halvo_route::pick::pick;
use halvo_route::pick::Pick;
#[cfg(test)]
use halvo_route::policy::Policy;
use halvo_route::release::policy;
use halvo_route::tier::CallKind;
use halvo_route::tier::CategoryId;
use halvo_route::tier::Level;
use halvo_route::tier::Tier;
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::names::ModelTier;
use halvo_schema::SeedCategory;

use crate::model::ask;

/// The kind of a run's model request: the next action of a compile, or the body of a reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Call {
    Compile,
    Write,
}

/// What the router's pick chose for one model request of a run: the kind of the call, the run's own tier and the level
/// of the request. `picked` alone makes it, since its fields are private to this module, and `ask` takes it
/// (crate::model::ask), so that a caller that did not pick does not compile; `ask` reads the request's level from it
/// and from nowhere else, and holds the request's grammar against its kind of call (the coordinator's ruling on the
/// pick as a value).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Picked {
    call: Call,
    tier: ModelTier,
    level: ask::Level,
}

impl Picked {
    /// The kind of the call that the pick was made for, which `ask` holds against the request's grammar.
    pub(crate) fn call(&self) -> Call {
        self.call
    }

    /// The tier that the pick chose, which is always the run's own tier.
    pub(crate) fn tier(&self) -> ModelTier {
        self.tier
    }

    /// The level that the pick chose for the request.
    pub(crate) fn level(&self) -> ask::Level {
        self.level
    }
}

/// The pick of a unit test's request, made by hand: in a test build alone, for the tests that ask with a kind of call,
/// a tier and a level of their own through a run whose release policy hands every call back (crate::model's tests).
#[cfg(test)]
pub(crate) fn picked_for_a_test(call: Call, tier: ModelTier, level: ask::Level) -> Picked {
    Picked { call, tier, level }
}

/// The router handed the call back, or picked a tier other than the run's own: the run sends no model request and
/// ends `handed_back` with the cause `shortfall`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Shortfall;

#[cfg(test)]
thread_local! {
    /// The policy that a unit test gives in the place of the release policy, for the calls of its own thread.
    pub(crate) static TEST_POLICY: std::cell::Cell<Option<&'static Policy>> = const { std::cell::Cell::new(None) };
}

/// The pick for the request of the kind `call` of a run whose load is `loaded`, the one maker of `Picked`: the router's
/// pick on the release policy, for the agent's category and `call`, with the agent's own tier as the ceiling. A
/// `Picked` of `call`, that tier and the pick's level where the pick's tier is the agent's own tier, the tier that the
/// person allowed and the door is started with; else `Shortfall`, for a pick that hands the call back and for a pick
/// of a lower tier.
pub(crate) fn picked(loaded: &LoadReply, call: Call) -> Result<Picked, Shortfall> {
    #[cfg(test)]
    let table = TEST_POLICY
        .with(std::cell::Cell::get)
        .unwrap_or_else(policy);
    #[cfg(not(test))]
    let table = policy();
    let own = tier(loaded.config.tier);
    let kind = match call {
        Call::Compile => CallKind::Compile,
        Call::Write => CallKind::Write,
    };
    match pick(table, category(loaded.config.category), kind, own) {
        Pick::Tier { tier, level } if tier == own => Ok(Picked {
            call,
            tier: loaded.config.tier,
            level: match level {
                Level::Low => ask::Level::Low,
                Level::Medium => ask::Level::Medium,
                Level::High => ask::Level::High,
            },
        }),
        Pick::Tier { .. } | Pick::HandBack(_) | Pick::NoModel => Err(Shortfall),
    }
}

/// The router's id of a seed category of the schema, by its equal name (`ids_equal_schema`).
fn category(seed: SeedCategory) -> CategoryId {
    match seed {
        SeedCategory::UtilityBillPayment => CategoryId::UtilityBillPayment,
        SeedCategory::TenantRent => CategoryId::TenantRent,
        SeedCategory::LeaseRenewalOrChange => CategoryId::LeaseRenewalOrChange,
        SeedCategory::TenantApplicationReview => CategoryId::TenantApplicationReview,
        SeedCategory::Email => CategoryId::Email,
        SeedCategory::SpreadsheetFill => CategoryId::SpreadsheetFill,
        SeedCategory::ShopOrder => CategoryId::ShopOrder,
        SeedCategory::CalendarCleanup => CategoryId::CalendarCleanup,
        SeedCategory::RideOrder => CategoryId::RideOrder,
        SeedCategory::DailyChartCheck => CategoryId::DailyChartCheck,
    }
}

/// The router's tier of a model tier of the schema, by its equal name (`ids_equal_schema`).
fn tier(tier: ModelTier) -> Tier {
    match tier {
        ModelTier::OnDevice => Tier::OnDevice,
        ModelTier::LowCostHosted => Tier::LowCostHosted,
        ModelTier::Frontier => Tier::Frontier,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use halvo_schema::{AgentId, Category, WorkflowId};

    use super::*;

    /// The load of an agent of `category` at `tier`, as the store answers it for a run with no stored playbook.
    fn loaded(category: Category, tier: ModelTier) -> LoadReply {
        let mut config = halvo_policy::preset(
            category,
            WorkflowId::from_random([0x91; 16]),
            AgentId::from_random([0x92; 16]),
        )
        .expect("a preset");
        config.tier = tier;
        LoadReply {
            config,
            playbook: None,
            next_playbook: std::num::NonZeroU32::MIN,
            bindings: None,
            sessions: Vec::new(),
            start: None,
        }
    }

    /// The release policy's text with `cells` in the place of the rows of their category and kind.
    pub(crate) fn policy_with(cells: &[&str]) -> &'static Policy {
        let mut text = policy().to_text();
        for cell in cells {
            let head: Vec<&str> = cell.split('\t').take(2).collect();
            let prefix = format!("{}\t{}\t", head[0], head[1]);
            let line = text
                .lines()
                .find(|line| line.starts_with(&prefix))
                .expect("a row of the cell")
                .to_owned();
            text = text.replacen(&format!("{line}\n"), &format!("{cell}\n"), 1);
        }
        let table = Policy::parse_for(&text, &[Tier::LowCostHosted, Tier::Frontier])
            .expect("the test's policy parses");
        Box::leak(Box::new(table))
    }

    /// The level of the pick that `picked` makes for a run of `category` at `tier` that asks for `call` on `table`.
    fn level_on(
        table: &'static Policy,
        category: Category,
        tier: ModelTier,
        call: Call,
    ) -> Result<ask::Level, Shortfall> {
        TEST_POLICY.with(|cell| cell.set(Some(table)));
        let answer = picked(&loaded(category, tier), call).map(|pick| pick.level());
        TEST_POLICY.with(|cell| cell.set(None));
        answer
    }

    /// The function on a policy of its own: a cell at the run's tier gives its level; a cell above the run's tier,
    /// a cell below it, and a cell that hands back give `Shortfall`.
    #[test]
    fn u72b_the_pick_gives_the_level_of_a_cell_at_the_runs_own_tier_and_else_a_shortfall() {
        let table = policy_with(&[
            "utility_bill_payment\tcompile\tfrontier\thigh\tline_met\t4\t4",
            "shop_order\tcompile\tlow_cost_hosted\tmedium\tline_met\t4\t4",
        ]);
        let rows = [
            (
                "a cell at the run's tier",
                level_on(
                    table,
                    Category::UtilityBillPayment,
                    ModelTier::Frontier,
                    Call::Compile,
                ),
                Ok(ask::Level::High),
            ),
            (
                "a cell above the run's tier",
                level_on(
                    table,
                    Category::UtilityBillPayment,
                    ModelTier::LowCostHosted,
                    Call::Compile,
                ),
                Err(Shortfall),
            ),
            (
                "a cell below the run's tier",
                level_on(
                    table,
                    Category::ShopOrder,
                    ModelTier::Frontier,
                    Call::Compile,
                ),
                Err(Shortfall),
            ),
            (
                "a cell that hands back",
                level_on(table, Category::Email, ModelTier::Frontier, Call::Write),
                Err(Shortfall),
            ),
        ];
        for (row, got, want) in rows {
            println!("row: {row}: {got:?}");
            assert_eq!(got, want, "{row}");
        }
    }

    /// The bridge of the routing run's U5.1: each seed category of the schema maps to the router's category of the
    /// same name, and each of the router's categories but `other`, which no agent of the schema has, is the map of
    /// one of them; each model tier of the schema maps to the router's tier of the same name, and the router has no
    /// tier more.
    #[test]
    fn ids_equal_schema() {
        let seeds: Vec<(&str, &str)> = SeedCategory::ALL
            .iter()
            .map(|seed| (seed.as_str(), category(*seed).as_str()))
            .collect();
        let mut routed: Vec<&str> = seeds.iter().map(|(_, routed)| *routed).collect();
        routed.sort_unstable();
        let mut others: Vec<&str> = CategoryId::ALL
            .iter()
            .map(|id| id.as_str())
            .filter(|name| *name != "other")
            .collect();
        others.sort_unstable();
        let tiers: Vec<(&str, &str)> = ModelTier::ALL
            .iter()
            .map(|model| (model.as_str(), tier(*model).as_str()))
            .collect();
        println!(
            "categories: {} of the schema, {} of the router; tiers: {} of the schema, {} of the router",
            seeds.len(),
            CategoryId::ALL.len(),
            tiers.len(),
            Tier::ALL.len()
        );
        assert!(
            seeds.iter().all(|(seed, routed)| seed == routed),
            "{seeds:?}"
        );
        assert_eq!(routed, others);
        assert!(
            tiers.iter().all(|(model, routed)| model == routed),
            "{tiers:?}"
        );
        assert_eq!(tiers.len(), Tier::ALL.len());
    }

    /// The categories and kinds of call that read the limit `tokens` (A6.5: a compile and a step's recovery) for
    /// which `table` picks a tier, among the categories whose row of halvo-policy's limits holds `model_tokens` 0: a
    /// compile of such a category would end `limit` with `tokens` before its first request.
    fn picked_with_no_tokens(table: &Policy) -> Vec<(&'static str, &'static str)> {
        let mut picked = Vec::new();
        for seed in SeedCategory::ALL {
            if halvo_policy::LIMITS.of(*seed).model_tokens != 0 {
                continue;
            }
            for kind in [CallKind::Compile, CallKind::Recover] {
                if let Pick::Tier { .. } = pick(table, category(*seed), kind, Tier::Frontier) {
                    picked.push((seed.as_str(), kind.as_str()));
                }
            }
        }
        picked
    }

    /// THE FACT of `model_tokens` 0 (the coordinator's log line 22546): the release policy picks no tier for a call
    /// that reads `tokens` of a category whose limits give it no tokens, so a data unit that opens such a cell beside
    /// a 0 fails here, and no person gets a compile that ends at once.
    #[test]
    fn u72b_no_call_that_reads_tokens_is_picked_for_a_category_with_no_tokens() {
        let zero = SeedCategory::ALL
            .iter()
            .filter(|seed| halvo_policy::LIMITS.of(**seed).model_tokens == 0)
            .count();
        let picked = picked_with_no_tokens(policy());
        println!(
            "tokens: categories with model_tokens 0: {zero} of {}; picked calls that read tokens: {picked:?}",
            SeedCategory::ALL.len()
        );
        assert_eq!(picked, Vec::<(&str, &str)>::new());
    }

    /// A test's probe of whether `T` has a default, or converts from `U`: the inherent const where the bound holds,
    /// the trait's otherwise (the form of crate::model::snapshot's test of its type).
    struct Probe<T, U>(std::marker::PhantomData<(T, U)>);

    trait NoBuild {
        const BUILDS: bool = false;
    }

    impl<T, U> NoBuild for Probe<T, U> {}

    impl<T: From<U>, U> Probe<T, U> {
        const BUILDS: bool = true;
    }

    struct Default_<T>(std::marker::PhantomData<T>);
    impl<T> NoBuild for Default_<T> {}
    impl<T: Default> Default_<T> {
        const BUILDS: bool = true;
    }

    /// The pick is a value that `picked` alone makes: a value made by hand does not build as it. Its fields are
    /// private to this module, so no struct literal of another module compiles (the compiler says it); and it has no
    /// default and no conversion from what it carries, which this test reads.
    #[test]
    fn u72b_a_value_made_by_hand_does_not_build_as_the_pick() {
        let builds = [
            ("Default", Default_::<Picked>::BUILDS),
            ("From<Call>", Probe::<Picked, Call>::BUILDS),
            ("From<ModelTier>", Probe::<Picked, ModelTier>::BUILDS),
            ("From<Level>", Probe::<Picked, ask::Level>::BUILDS),
            (
                "From<(Call, ModelTier, Level)>",
                Probe::<Picked, (Call, ModelTier, ask::Level)>::BUILDS,
            ),
        ];
        let control = [
            ("String: Default", Default_::<String>::BUILDS),
            ("String: From<&str>", Probe::<String, &str>::BUILDS),
        ];
        println!("the pick: {builds:?}; control: {control:?}");
        assert!(builds.iter().all(|(_, b)| !b), "{builds:?}");
        assert!(control.iter().all(|(_, b)| *b), "{control:?}");
    }
}
