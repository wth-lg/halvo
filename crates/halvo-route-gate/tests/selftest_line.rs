use std::num::NonZeroU32;

use halvo_route::policy::{PickCell, Policy};
use halvo_route::tier::{CallKind, CategoryId, Tier};
use halvo_route_gate::selftest::{self_test, SeedRange};
use halvo_route_gate::Router;

fn tables(count: u32) -> NonZeroU32 {
    NonZeroU32::new(count).expect("not zero")
}

#[test]
fn self_test_line_of_the_contract() {
    let result = self_test(tables(20), 1, None).expect("the seeds fit");
    assert_eq!(result.lines(), ["selftest tables=20 derived_pass=20 product_pass=- one_tier_fail=20 random_fail=20 peek_fail=20"]);
    assert!(result.passed());
    let later = self_test(tables(20), 21, None).expect("the seeds fit");
    assert_eq!(later.lines(), ["selftest tables=20 derived_pass=20 product_pass=- one_tier_fail=20 random_fail=20 peek_fail=20"]);
}

struct ByPolicy(Policy);

impl Router for ByPolicy {
    fn name(&self) -> &'static str {
        "product"
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        self.0.row(category, kind).pick
    }
}

struct Cheapest;

impl Router for Cheapest {
    fn name(&self) -> &'static str {
        "product"
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        match (category.is_seed(), kind) {
            (_, CallKind::Replay) => PickCell::None,
            (false, _) => PickCell::HandBack,
            _ => PickCell::Tier(Tier::OnDevice),
        }
    }
}

#[test]
fn product_count_counts_a_given_router() {
    let right = |policy: Policy| -> Box<dyn Router> { Box::new(ByPolicy(policy)) };
    let result = self_test(tables(5), 3, Some(&right)).expect("the seeds fit");
    assert_eq!(result.lines(), ["selftest tables=5 derived_pass=5 product_pass=5 one_tier_fail=5 random_fail=5 peek_fail=5"]);
    assert!(result.passed());
    let wrong = |_: Policy| -> Box<dyn Router> { Box::new(Cheapest) };
    let result = self_test(tables(3), 3, Some(&wrong)).expect("the seeds fit");
    assert_eq!(
        result.lines(),
        [
            "selftest miss router=product seed=3",
            "selftest miss router=product seed=4",
            "selftest miss router=product seed=5",
            "selftest tables=3 derived_pass=3 product_pass=0 one_tier_fail=3 random_fail=3 peek_fail=3",
        ]
    );
    assert!(!result.passed());
}

#[test]
fn seeds_must_fit_a_u64() {
    assert_eq!(self_test(tables(2), u64::MAX, None), Err(SeedRange));
    assert_eq!(
        self_test(tables(1), u64::MAX, None).map(|result| result.tables),
        Ok(1)
    );
}
