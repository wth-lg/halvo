use std::cell::RefCell;
use std::collections::BTreeMap;
use std::num::NonZeroU32;

use halvo_route::policy::{cells, PickCell, Policy};
use halvo_route::prices::Prices;
use halvo_route::tier::{CallKind, CategoryId, Tier};
use halvo_route_gate::derive::derive;
use halvo_route_gate::generate::generate;
use halvo_route_gate::product::{
    build_product, guarded, score_once, Answers, Panicked, ProductRouter,
};
use halvo_route_gate::score::{score, DEFAULT_DRAWS};
use halvo_route_gate::selftest::self_test;
use halvo_route_gate::{Router, Setup};

const ROWS: [(&str, &str, &str); 5] = [
    (
        "utility_bill_payment",
        "compile",
        "on_device\tlow\tline_met\t4\t4",
    ),
    (
        "utility_bill_payment",
        "recover",
        "low_cost_hosted\tmedium\tline_met\t4\t4",
    ),
    (
        "utility_bill_payment",
        "write",
        "frontier\thigh\tline_met\t5\t5",
    ),
    ("email", "compile", "hand_back\t-\tline_missed\t3\t4"),
    ("email", "write", "hand_back\t-\tuntested\t2\t2"),
];

fn every_pick() -> Policy {
    let mut text = Policy::hand_back_all().to_text();
    for (category, kind, rest) in ROWS {
        let old = format!("{category}\t{kind}\thand_back\t-\tuntested\t0\t0\n");
        let new = format!("{category}\t{kind}\t{rest}\n");
        let changed = text.replacen(&old, &new, 1);
        assert_ne!(changed, text, "{category} {kind}");
        text = changed;
    }
    Policy::parse_for(&text, &Tier::ALL).expect("a policy with every pick")
}

#[test]
fn product_picks_what_its_policy_says() {
    let policy = every_pick();
    let router = ProductRouter::new(policy);
    assert_eq!(router.name(), "product");
    let mut seen = BTreeMap::new();
    for (category, kind) in cells() {
        let pick = router.pick(category, kind);
        assert_eq!(
            pick,
            policy.row(category, kind).pick,
            "{category:?} {kind:?}"
        );
        *seen.entry(pick.as_str()).or_insert(0) += 1;
    }
    assert_eq!(
        seen,
        BTreeMap::from([
            ("frontier", 1),
            ("hand_back", 30),
            ("low_cost_hosted", 1),
            ("none", 11),
            ("on_device", 1),
        ])
    );
}

#[test]
fn product_passes_every_generated_table_and_answers_as_derived() {
    let setup = Setup::generated();
    for seed in 1..=20 {
        let table = generate(seed);
        let policy = Policy::parse_for(&derive(&table, &setup).to_text(), &Tier::ALL)
            .expect("a derived policy parses");
        let router = ProductRouter::new(policy);
        for (category, kind) in cells() {
            assert_eq!(
                router.pick(category, kind),
                policy.row(category, kind).pick,
                "seed {seed}"
            );
        }
        let result = score(&table, &setup, &router, Prices::release(), DEFAULT_DRAWS);
        assert!(result.passed(), "seed {seed}");
        assert_eq!(result.verdict.router, "product");
    }
    let tables = NonZeroU32::new(20).expect("20");
    let result = self_test(tables, 1, Some(&build_product)).expect("seeds fit");
    assert_eq!(result.product_pass, Some(20));
    assert!(result.passed(), "{:?}", result.lines());
}

struct Panics {
    at: (CategoryId, CallKind),
}

impl Router for Panics {
    fn name(&self) -> &'static str {
        "panics"
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        assert!((category, kind) != self.at, "a router that panics");
        PickCell::HandBack
    }
}

#[test]
fn a_router_that_panics_is_a_fault_and_no_crash() {
    let at = (CategoryId::Email, CallKind::Write);
    let fault = Answers::ask(&Panics { at }).expect_err("it panics");
    assert_eq!(
        fault,
        Panicked {
            category: CategoryId::Email,
            kind: CallKind::Write
        }
    );
    assert_eq!(fault.line(), "gate fault=router category=email kind=write");

    let table = generate(1);
    let setup = Setup::generated();
    let scored = score_once(
        &table,
        &setup,
        &Panics { at },
        Prices::release(),
        DEFAULT_DRAWS,
    );
    assert_eq!(scored.map(|_| ()), Err(fault));
    let derived = halvo_route_gate::routers::PolicyRouter::derived(&table, &setup);
    let once =
        score_once(&table, &setup, &derived, Prices::release(), DEFAULT_DRAWS).expect("no panic");
    assert_eq!(
        once,
        score(&table, &setup, &derived, Prices::release(), DEFAULT_DRAWS)
    );

    let stand_in = guarded(&Panics { at });
    assert_eq!(stand_in.name(), "panics");
    for (category, kind) in cells() {
        assert_eq!(stand_in.pick(category, kind), PickCell::None);
    }
    let result = score(
        &table,
        &setup,
        stand_in.as_ref(),
        Prices::release(),
        DEFAULT_DRAWS,
    );
    assert!(!result.verdict.derivation_same && !result.passed());

    let tables = NonZeroU32::new(3).expect("3");
    let build = |_policy: Policy| guarded(&Panics { at });
    let result = self_test(tables, 5, Some(&build)).expect("seeds fit");
    assert_eq!(result.product_pass, Some(0));
    assert_eq!(
        result.misses,
        [("product", 5), ("product", 6), ("product", 7)]
    );
    assert!(!result.passed());
}

struct Counting {
    asked: RefCell<BTreeMap<(CategoryId, CallKind), u32>>,
}

impl Router for Counting {
    fn name(&self) -> &'static str {
        "counting"
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        let mut asked = self.asked.borrow_mut();
        let count = asked.entry((category, kind)).or_insert(0);
        *count += 1;
        if *count == 1 && kind != CallKind::Replay {
            PickCell::HandBack
        } else if kind == CallKind::Replay {
            PickCell::None
        } else {
            PickCell::Tier(Tier::Frontier)
        }
    }
}

#[test]
fn answers_ask_each_cell_once_and_keep_the_first_answer() {
    let router = Counting {
        asked: RefCell::new(BTreeMap::new()),
    };
    let answers = Answers::ask(&router).expect("no panic");
    assert_eq!(answers.name(), "counting");
    let asked = router.asked.borrow().clone();
    assert_eq!(asked.len(), 44);
    assert!(asked.values().all(|count| *count == 1));
    for (category, kind) in cells() {
        let want = if kind == CallKind::Replay {
            PickCell::None
        } else {
            PickCell::HandBack
        };
        assert_eq!(answers.pick(category, kind), want);
        assert_eq!(answers.pick(category, kind), want);
    }
    assert_eq!(router.asked.borrow().values().sum::<u32>(), 44);

    let table = generate(1);
    let setup = Setup::generated();
    let fresh = Counting {
        asked: RefCell::new(BTreeMap::new()),
    };
    let result =
        score_once(&table, &setup, &fresh, Prices::release(), DEFAULT_DRAWS).expect("no panic");
    assert_eq!(fresh.asked.borrow().values().sum::<u32>(), 44);
    assert!(fresh.asked.borrow().values().all(|count| *count == 1));
    assert_eq!(
        result,
        score(&table, &setup, &answers, Prices::release(), DEFAULT_DRAWS)
    );
    assert_eq!(result.verdict.handed_back, 30);
    assert_eq!(result.verdict.routed, 0);
}
