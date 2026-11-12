use std::path::{Path, PathBuf};

use halvo_route::line::{meets, Line, LINE_DEV_MIN};
use halvo_route::policy::{cells, PickCell, Policy};
use halvo_route::prices::Prices;
use halvo_route::release;
use halvo_route::tier::{CallKind, CategoryId, Tier};
use halvo_route_gate::derive::derive;
use halvo_route_gate::product::{recorded_setup, Freeze, ProductRouter};
use halvo_route_gate::routers::PolicyRouter;
use halvo_route_gate::score::{score, Score, DEFAULT_DRAWS};
use halvo_route_gate::table::{Split, Table};
use halvo_route_gate::{Router, Setup};

fn case_dir(case: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/r3")
        .join(case)
        .join("expected")
}

fn case_file(case: &str, name: &str) -> String {
    std::fs::read_to_string(case_dir(case).join(name)).expect("a golden file")
}

fn golden(case: &str) -> (Table, Setup) {
    let table = Table::parse(&case_file(case, "recorded.tsv")).expect("a golden table");
    let setup = recorded_setup(Some(&case_file(case, "freeze.txt"))).expect("a golden freeze");
    (table, setup)
}

fn product_score(table: &Table, setup: &Setup) -> (Policy, Score) {
    let text = derive(table, setup).to_text();
    let policy = Policy::parse_for(&text, &setup.allowed).expect("the derived policy parses");
    let router = ProductRouter::new(policy);
    let result = score(table, setup, &router, Prices::release(), DEFAULT_DRAWS);
    (policy, result)
}

fn on_device(policy: &Policy) -> usize {
    policy
        .rows()
        .iter()
        .filter(|row| row.pick == PickCell::Tier(Tier::OnDevice))
        .count()
}

#[test]
fn product_recorded() {
    for (case, routed) in [("full", 5), ("no_writer", 4), ("no_driver", 0)] {
        let (table, setup) = golden(case);
        assert!(!table.is_generated(), "{case}");
        assert_eq!(setup.allowed, release::ruling().allowed(), "{case}");
        assert!(setup.recorded, "{case}");
        let (policy, result) = product_score(&table, &setup);
        assert_eq!(on_device(&policy), routed, "{case}");
        let last = result.lines().last().expect("a verdict").clone();
        assert_eq!(
            last,
            format!(
                "gate table=recorded router=product cells=44 routed={routed} handed_back={} derivation=same held=ok result=pass",
                30 - routed
            ),
            "{case}"
        );
        assert!(result.passed(), "{case}");
    }
}

#[test]
fn recorded_setup_takes_the_freeze_lines_and_the_ruling() {
    let none = recorded_setup(None).expect("no freeze file");
    assert_eq!(
        none,
        Setup {
            allowed: vec![Tier::OnDevice],
            driver: None,
            writer: None,
            recorded: true,
        }
    );
    let full = recorded_setup(Some(&case_file("full", "freeze.txt"))).expect("full");
    assert_eq!(
        (full.driver.as_deref(), full.writer.as_deref()),
        (Some("m_small"), Some("m_small"))
    );
    let no_writer = recorded_setup(Some(&case_file("no_writer", "freeze.txt"))).expect("w");
    assert_eq!(
        (no_writer.driver.as_deref(), no_writer.writer),
        (Some("m_small"), None)
    );
    assert_eq!(
        Freeze::parse(
            "freeze rows=0 models=0 sources=2 date=2024-02-29\non_device driver=none writer=a_1\n"
        ),
        Ok(Freeze {
            rows: 0,
            models: 0,
            date: "2024-02-29".to_owned(),
            driver: None,
            writer: Some("a_1".to_owned()),
        })
    );
    assert_eq!(
        Freeze::parse(
            "freeze rows=0 models=0 sources=2 date=2024-02-29\non_device driver=synthetic_2 writer=none\n"
        )
        .map(|freeze| freeze.driver),
        Ok(Some("synthetic_2".to_owned()))
    );
}

#[test]
fn recorded_setup_refuses_a_text_that_is_not_the_freeze_form() {
    let good = "freeze rows=132 models=6 sources=2 date=2026-11-10\non_device driver=m_small writer=m_small\n";
    assert!(recorded_setup(Some(good)).is_ok());
    let long = "a".repeat(33);
    let cases = [
        String::new(),
        "\n".to_owned(),
        good.strip_suffix('\n').expect("LF").to_owned(),
        format!("{good}\n"),
        format!("{good}on_device driver=none writer=none\n"),
        good.replace('\n', "\r\n"),
        format!("\u{feff}{good}"),
        "freeze rows=132 models=6 sources=2 date=2026-11-10\n".to_owned(),
        good.replace("freeze rows", "Freeze rows"),
        good.replace("rows=132", "rows=0132"),
        good.replace("rows=132", "rows="),
        good.replace("rows=132", "rows=-1"),
        good.replace("rows=132", "rows=18446744073709551616"),
        good.replace("models=6", "models=06"),
        good.replace("models=6 ", ""),
        good.replace("sources=2", "sources=3"),
        good.replace("sources=2", "sources=02"),
        good.replace("date=2026-11-10", "date=2026-13-10"),
        good.replace("date=2026-11-10", "date=2026-02-30"),
        good.replace("date=2026-11-10", "date=2026-11-1"),
        good.replace("date=2026-11-10", "date=20261110"),
        good.replace(" date=2026-11-10", " date=2026-11-10 extra=1"),
        good.replace("rows=132 models=6", "models=6 rows=132"),
        good.replace("freeze rows", "freeze  rows"),
        good.replace("freeze rows", "freeze\trows"),
        good.replace("2026-11-10\n", "2026-11-10 \n"),
        good.replace("on_device driver", "on_device  driver"),
        good.replace("on_device", "low_cost_hosted"),
        good.replace("driver=m_small", "driver=M_small"),
        good.replace("driver=m_small", "driver="),
        good.replace("driver=m_small", &format!("driver={long}")),
        good.replace("writer=m_small", "writer=m-small"),
        good.replace("driver=m_small", "driver=synthetic"),
        good.replace("writer=m_small", "writer=synthetic"),
        good.replace(
            "driver=m_small writer=m_small",
            "writer=m_small driver=m_small",
        ),
        good.replace("writer=m_small\n", "writer=m_small extra\n"),
        good.replace("writer=m_small\n", "writer=m_small \n"),
        format!(
            "on_device driver=m_small writer=m_small\n{}",
            good.lines().next().expect("first").to_owned() + "\n"
        ),
    ];
    for text in &cases {
        let refused = recorded_setup(Some(text));
        assert!(refused.is_err(), "{text:?}");
        assert_eq!(
            refused.unwrap_err(),
            Freeze::parse(text).unwrap_err(),
            "{text:?}"
        );
    }
}

struct Pooled {
    picks: Vec<PickCell>,
}

impl Pooled {
    fn new(table: &Table) -> Pooled {
        let picks = cells()
            .map(|(category, kind)| {
                if kind == CallKind::Replay {
                    return PickCell::None;
                }
                if !category.is_seed() || !category.has_runtime() {
                    return PickCell::HandBack;
                }
                let rows = table.rows().iter().filter(|row| {
                    row.category == category
                        && row.kind == kind
                        && row.tier == Tier::OnDevice
                        && row.split() == Split::Dev
                });
                let (k, n) = rows.fold((0, 0), |(k, n), row| (k + u32::from(row.pass), n + 1));
                if meets(k, n, LINE_DEV_MIN) == Line::Met {
                    PickCell::Tier(Tier::OnDevice)
                } else {
                    PickCell::HandBack
                }
            })
            .collect();
        Pooled { picks }
    }
}

impl Router for Pooled {
    fn name(&self) -> &'static str {
        "pooled"
    }

    fn pick(&self, category: CategoryId, kind: CallKind) -> PickCell {
        let at = cells()
            .position(|cell| cell == (category, kind))
            .expect("a cell");
        self.picks[at]
    }
}

fn flip(table: &Table, model: &str, category: CategoryId, kind: CallKind, split: Split) -> Table {
    let mut rows = table.rows().to_vec();
    let row = rows
        .iter_mut()
        .find(|row| {
            row.model.as_str() == model
                && row.category == category
                && row.kind == kind
                && row.split() == split
                && row.pass
        })
        .expect("a passing row to flip");
    row.pass = false;
    Table::from_rows(&rows).expect("the flipped table")
}

#[test]
fn rows_of_a_second_model_decide_nothing() {
    let (table, setup) = golden("full");
    let models: std::collections::BTreeSet<&str> =
        table.rows().iter().map(|row| row.model.as_str()).collect();
    assert_eq!(
        models.into_iter().collect::<Vec<_>>(),
        ["m_small", "m_tiny"]
    );

    let pooled = score(
        &table,
        &setup,
        &Pooled::new(&table),
        Prices::release(),
        DEFAULT_DRAWS,
    );
    let verdict = pooled.lines().last().expect("a verdict").clone();
    assert!(
        verdict.ends_with(" derivation=differs held=ok result=fail"),
        "{verdict}"
    );
    assert_eq!(
        Pooled::new(&table).pick(CategoryId::ShopOrder, CallKind::Compile),
        PickCell::HandBack
    );
    let (policy, result) = product_score(&table, &setup);
    assert_eq!(
        policy.row(CategoryId::ShopOrder, CallKind::Compile).pick,
        PickCell::Tier(Tier::OnDevice)
    );
    assert!(result.passed());

    let tiny_held = flip(
        &table,
        "m_tiny",
        CategoryId::Email,
        CallKind::Write,
        Split::Held,
    );
    let (_, result) = product_score(&tiny_held, &setup);
    assert!(result.passed());
    let tiny_dev = flip(
        &table,
        "m_tiny",
        CategoryId::UtilityBillPayment,
        CallKind::Recover,
        Split::Dev,
    );
    assert_eq!(derive(&tiny_dev, &setup), derive(&table, &setup));

    let small_held = flip(
        &table,
        "m_small",
        CategoryId::Email,
        CallKind::Write,
        Split::Held,
    );
    let derived = PolicyRouter::derived(&small_held, &setup);
    let result = score(
        &small_held,
        &setup,
        &derived,
        Prices::release(),
        DEFAULT_DRAWS,
    );
    let verdict = result.lines().last().expect("a verdict").clone();
    assert!(
        verdict.ends_with(" derivation=same held=miss result=fail"),
        "{verdict}"
    );
}
