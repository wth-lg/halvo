//! The self-test of contract T6.4: generated tables scored with the five routers, where the
//! routers `derived` and `product` must pass and `one_tier`, `random` and `peek` must fail.

use std::num::NonZeroU32;

use halvo_route::policy::Policy;
use halvo_route::prices::Prices;
use halvo_route::tier::Tier;

use crate::derive::derive;
use crate::generate::generate;
use crate::routers::{OneTier, PolicyRouter, Random};
use crate::score::{score, DEFAULT_DRAWS};
use crate::{Router, Setup};

/// What builds the router `product` from a table's derived policy.
pub type ProductRouter<'a> = &'a dyn Fn(Policy) -> Box<dyn Router>;

/// The counts of a self-test, with one miss for each table on which `derived` or `product` failed
/// or a bad router passed.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct SelfTest {
    pub tables: u32,
    pub misses: Vec<(&'static str, u64)>,
    pub derived_pass: u32,
    pub product_pass: Option<u32>,
    pub one_tier_fail: u32,
    pub random_fail: u32,
    pub peek_fail: u32,
}

impl SelfTest {
    /// Each count equals the number of tables; `product_pass` counts only when a product router
    /// was given.
    pub fn passed(&self) -> bool {
        let counts = [
            self.derived_pass,
            self.product_pass.unwrap_or(self.tables),
            self.one_tier_fail,
            self.random_fail,
            self.peek_fail,
        ];
        counts.iter().all(|count| *count == self.tables)
    }

    /// The miss lines, then the count line.
    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .misses
            .iter()
            .map(|(router, seed)| format!("selftest miss router={router} seed={seed}"))
            .collect();
        let product = self
            .product_pass
            .map_or("-".to_owned(), |count| count.to_string());
        lines.push(format!(
            "selftest tables={} derived_pass={} product_pass={} one_tier_fail={} random_fail={} peek_fail={}",
            self.tables, self.derived_pass, product, self.one_tier_fail, self.random_fail, self.peek_fail
        ));
        lines
    }
}

/// The seeds from `seed` to `seed + tables - 1` do not all fit a `u64`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SeedRange;

/// Generates a table for each seed from `seed` to `seed + tables - 1` and scores it with the
/// routers `derived`, `product` (when `product` is given: built from the policy that `derived`
/// derives, written and read again by `Policy::parse_for` with every tier), `one_tier`, `random`
/// and `peek`, costed with `Prices::release()`.
pub fn self_test(
    tables: NonZeroU32,
    seed: u64,
    product: Option<ProductRouter<'_>>,
) -> Result<SelfTest, SeedRange> {
    let count = tables.get();
    seed.checked_add(u64::from(count) - 1).ok_or(SeedRange)?;
    let setup = Setup::generated();
    let prices = Prices::release();
    let mut result = SelfTest {
        tables: count,
        misses: Vec::new(),
        derived_pass: 0,
        product_pass: product.map(|_| 0),
        one_tier_fail: 0,
        random_fail: 0,
        peek_fail: 0,
    };
    for table_seed in (0..u64::from(count)).map(|i| seed + i) {
        let table = generate(table_seed);
        let passes =
            |router: &dyn Router| score(&table, &setup, router, prices, DEFAULT_DRAWS).passed();
        let derived = PolicyRouter::derived(&table, &setup);
        if passes(&derived) {
            result.derived_pass += 1;
        } else {
            result.misses.push(("derived", table_seed));
        }
        if let (Some(build), Some(product_pass)) = (product, result.product_pass.as_mut()) {
            let text = derive(&table, &setup).to_text();
            let product_passes = match Policy::parse_for(&text, &Tier::ALL) {
                Ok(policy) => passes(build(policy).as_ref()),
                Err(_) => false,
            };
            if product_passes {
                *product_pass += 1;
            } else {
                result.misses.push(("product", table_seed));
            }
        }
        let bad: [(&dyn Router, &mut u32); 3] = [
            (&OneTier { recorded: false }, &mut result.one_tier_fail),
            (&Random::new(&table, &setup), &mut result.random_fail),
            (&PolicyRouter::peek(&table, &setup), &mut result.peek_fail),
        ];
        for (router, fails) in bad {
            if passes(router) {
                result.misses.push((router.name(), table_seed));
            } else {
                *fails += 1;
            }
        }
    }
    Ok(result)
}
