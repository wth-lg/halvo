use halvo_route::pick::{explain, pick, Cause, Pick};
use halvo_route::policy::{Basis, PickCell, Policy, Row};
use halvo_route::release;
use halvo_route::tier::{CallKind, CategoryId, Level, Tier};

const SECOND: &str = "category\tkind\tpick\tlevel\tbasis\tk\tn\n\
utility_bill_payment\tcompile\ton_device\tlow\tline_met\t4\t4\n\
utility_bill_payment\trecover\tlow_cost_hosted\tmedium\tline_met\t5\t5\n\
utility_bill_payment\twrite\tfrontier\thigh\tline_met\t6\t6\n\
utility_bill_payment\treplay\tnone\t-\tno_model\t0\t0\n\
tenant_rent\tcompile\thand_back\t-\tno_runtime\t0\t0\n\
tenant_rent\trecover\tfrontier\tlow\tline_met\t4\t4\n\
tenant_rent\twrite\thand_back\t-\tuntested\t1\t3\n\
tenant_rent\treplay\tnone\t-\tno_model\t0\t0\n\
lease_renewal_or_change\tcompile\tlow_cost_hosted\tlow\tline_met\t4\t4\n\
lease_renewal_or_change\trecover\thand_back\t-\tline_missed\t3\t4\n\
lease_renewal_or_change\twrite\ton_device\thigh\tline_met\t7\t7\n\
lease_renewal_or_change\treplay\tnone\t-\tno_model\t0\t0\n\
tenant_application_review\tcompile\thand_back\t-\tuntested\t0\t0\n\
tenant_application_review\trecover\ton_device\tmedium\tline_met\t4\t4\n\
tenant_application_review\twrite\tfrontier\tmedium\tline_met\t9\t9\n\
tenant_application_review\treplay\tnone\t-\tno_model\t0\t0\n\
email\tcompile\tfrontier\tlow\tline_met\t4\t4\n\
email\trecover\tfrontier\thigh\tline_met\t4\t4\n\
email\twrite\tlow_cost_hosted\tlow\tline_met\t8\t8\n\
email\treplay\tnone\t-\tno_model\t0\t0\n\
spreadsheet_fill\tcompile\thand_back\t-\tline_missed\t0\t4\n\
spreadsheet_fill\trecover\tlow_cost_hosted\thigh\tline_met\t4\t4\n\
spreadsheet_fill\twrite\ton_device\tmedium\tline_met\t4\t4\n\
spreadsheet_fill\treplay\tnone\t-\tno_model\t0\t0\n\
shop_order\tcompile\ton_device\thigh\tline_met\t12\t12\n\
shop_order\trecover\thand_back\t-\tuntested\t2\t2\n\
shop_order\twrite\thand_back\t-\tline_missed\t5\t6\n\
shop_order\treplay\tnone\t-\tno_model\t0\t0\n\
calendar_cleanup\tcompile\tlow_cost_hosted\tmedium\tline_met\t4\t4\n\
calendar_cleanup\trecover\ton_device\tlow\tline_met\t4\t4\n\
calendar_cleanup\twrite\tfrontier\tlow\tline_met\t4\t4\n\
calendar_cleanup\treplay\tnone\t-\tno_model\t0\t0\n\
ride_order\tcompile\thand_back\t-\tno_runtime\t0\t0\n\
ride_order\trecover\thand_back\t-\tno_runtime\t0\t0\n\
ride_order\twrite\ton_device\tlow\tline_met\t4\t4\n\
ride_order\treplay\tnone\t-\tno_model\t0\t0\n\
daily_chart_check\tcompile\tfrontier\thigh\tline_met\t4\t4\n\
daily_chart_check\trecover\thand_back\t-\tuntested\t3\t3\n\
daily_chart_check\twrite\thand_back\t-\tno_runtime\t0\t0\n\
daily_chart_check\treplay\tnone\t-\tno_model\t0\t0\n\
other\tcompile\thand_back\t-\tnot_seed\t0\t0\n\
other\trecover\thand_back\t-\tnot_seed\t0\t0\n\
other\twrite\thand_back\t-\tnot_seed\t0\t0\n\
other\treplay\tnone\t-\tno_model\t0\t0\n";

const SECOND_PICKS: [[&str; 3]; 44] = [
    ["on_device:low", "on_device:low", "on_device:low"],
    [
        "hand_back:ceiling",
        "low_cost_hosted:medium",
        "low_cost_hosted:medium",
    ],
    ["hand_back:ceiling", "hand_back:ceiling", "frontier:high"],
    ["none", "none", "none"],
    [
        "hand_back:no_runtime",
        "hand_back:no_runtime",
        "hand_back:no_runtime",
    ],
    ["hand_back:ceiling", "hand_back:ceiling", "frontier:low"],
    [
        "hand_back:untested",
        "hand_back:untested",
        "hand_back:untested",
    ],
    ["none", "none", "none"],
    [
        "hand_back:ceiling",
        "low_cost_hosted:low",
        "low_cost_hosted:low",
    ],
    [
        "hand_back:line_missed",
        "hand_back:line_missed",
        "hand_back:line_missed",
    ],
    ["on_device:high", "on_device:high", "on_device:high"],
    ["none", "none", "none"],
    [
        "hand_back:untested",
        "hand_back:untested",
        "hand_back:untested",
    ],
    ["on_device:medium", "on_device:medium", "on_device:medium"],
    ["hand_back:ceiling", "hand_back:ceiling", "frontier:medium"],
    ["none", "none", "none"],
    ["hand_back:ceiling", "hand_back:ceiling", "frontier:low"],
    ["hand_back:ceiling", "hand_back:ceiling", "frontier:high"],
    [
        "hand_back:ceiling",
        "low_cost_hosted:low",
        "low_cost_hosted:low",
    ],
    ["none", "none", "none"],
    [
        "hand_back:line_missed",
        "hand_back:line_missed",
        "hand_back:line_missed",
    ],
    [
        "hand_back:ceiling",
        "low_cost_hosted:high",
        "low_cost_hosted:high",
    ],
    ["on_device:medium", "on_device:medium", "on_device:medium"],
    ["none", "none", "none"],
    ["on_device:high", "on_device:high", "on_device:high"],
    [
        "hand_back:untested",
        "hand_back:untested",
        "hand_back:untested",
    ],
    [
        "hand_back:line_missed",
        "hand_back:line_missed",
        "hand_back:line_missed",
    ],
    ["none", "none", "none"],
    [
        "hand_back:ceiling",
        "low_cost_hosted:medium",
        "low_cost_hosted:medium",
    ],
    ["on_device:low", "on_device:low", "on_device:low"],
    ["hand_back:ceiling", "hand_back:ceiling", "frontier:low"],
    ["none", "none", "none"],
    [
        "hand_back:no_runtime",
        "hand_back:no_runtime",
        "hand_back:no_runtime",
    ],
    [
        "hand_back:no_runtime",
        "hand_back:no_runtime",
        "hand_back:no_runtime",
    ],
    ["on_device:low", "on_device:low", "on_device:low"],
    ["none", "none", "none"],
    ["hand_back:ceiling", "hand_back:ceiling", "frontier:high"],
    [
        "hand_back:untested",
        "hand_back:untested",
        "hand_back:untested",
    ],
    [
        "hand_back:no_runtime",
        "hand_back:no_runtime",
        "hand_back:no_runtime",
    ],
    ["none", "none", "none"],
    [
        "hand_back:not_seed",
        "hand_back:not_seed",
        "hand_back:not_seed",
    ],
    [
        "hand_back:not_seed",
        "hand_back:not_seed",
        "hand_back:not_seed",
    ],
    [
        "hand_back:not_seed",
        "hand_back:not_seed",
        "hand_back:not_seed",
    ],
    ["none", "none", "none"],
];

fn second() -> Policy {
    Policy::parse_for(SECOND, &Tier::ALL).expect("the second table parses with every tier")
}

fn cells() -> impl Iterator<Item = (CategoryId, CallKind)> {
    CategoryId::ALL
        .into_iter()
        .flat_map(|category| CallKind::ALL.into_iter().map(move |kind| (category, kind)))
}

fn word(result: Pick) -> String {
    match result {
        Pick::NoModel => result.cell().as_str().to_owned(),
        Pick::Tier { level, .. } => format!("{}:{}", result.cell().as_str(), level.as_str()),
        Pick::HandBack(cause) => format!("{}:{}", result.cell().as_str(), cause.as_str()),
    }
}

fn first_rule(row: &Row, kind: CallKind, ceiling: Tier) -> String {
    if kind == CallKind::Replay {
        return "none".to_owned();
    }
    match (row.pick, row.level) {
        (PickCell::HandBack, None) => format!("hand_back:{}", row.basis.as_str()),
        (PickCell::Tier(tier), Some(_)) if tier > ceiling => "hand_back:ceiling".to_owned(),
        (PickCell::Tier(tier), Some(level)) => format!("{}:{}", tier.as_str(), level.as_str()),
        _ => panic!("a row that parse_for refuses: {row:?}"),
    }
}

#[test]
fn pick_total() {
    let second = second();
    let cells_of_second: Vec<PickCell> = second.rows().iter().map(|row| row.pick).collect();
    for cell in PickCell::ALL {
        assert!(cells_of_second.contains(&cell), "{}", cell.as_str());
    }
    let mut seen = std::collections::BTreeSet::new();
    for (table, policy) in [("release", release::policy()), ("second", &second)] {
        for (i, (category, kind)) in cells().enumerate() {
            assert_eq!(explain(policy, category, kind), policy.row(category, kind));
            for (j, ceiling) in Tier::ALL.into_iter().enumerate() {
                let result = pick(policy, category, kind, ceiling);
                let got = word(result);
                let row = explain(policy, category, kind);
                assert_eq!(
                    got,
                    first_rule(row, kind, ceiling),
                    "{table} {} {} {}",
                    category.as_str(),
                    kind.as_str(),
                    ceiling.as_str()
                );
                if table == "second" {
                    assert_eq!(
                        got,
                        SECOND_PICKS[i][j],
                        "{} {} {}",
                        category.as_str(),
                        kind.as_str(),
                        ceiling.as_str()
                    );
                    seen.insert(got);
                }
            }
        }
    }
    let mut want: Vec<String> = vec!["none".to_owned()];
    for cause in Cause::ALL {
        want.push(format!("hand_back:{}", cause.as_str()));
    }
    for tier in Tier::ALL {
        assert!(
            seen.iter()
                .any(|got| got.starts_with(&format!("{}:", tier.as_str()))),
            "{}",
            tier.as_str()
        );
    }
    for got in &want {
        assert!(seen.contains(got), "{got}");
    }
    assert_eq!(
        Cause::ALL.map(Cause::as_str),
        [
            "line_missed",
            "untested",
            "no_runtime",
            "not_seed",
            "ceiling"
        ]
    );
    assert_eq!(
        [
            Basis::LineMissed,
            Basis::Untested,
            Basis::NoRuntime,
            Basis::NotSeed
        ]
        .map(Basis::as_str),
        ["line_missed", "untested", "no_runtime", "not_seed"]
    );
}

#[test]
fn pick_ceiling() {
    let second = second();
    for ceiling in Tier::ALL {
        let mut reached = false;
        let mut cut = false;
        for (category, kind) in cells() {
            match pick(&second, category, kind, ceiling) {
                Pick::Tier { tier, .. } => {
                    assert!(
                        tier <= ceiling,
                        "{} {} {} gave {}",
                        category.as_str(),
                        kind.as_str(),
                        ceiling.as_str(),
                        tier.as_str()
                    );
                    reached |= tier == ceiling;
                }
                Pick::HandBack(Cause::Ceiling) => cut = true,
                Pick::NoModel | Pick::HandBack(_) => {}
            }
        }
        assert!(reached, "no pick at the ceiling {}", ceiling.as_str());
        assert_eq!(cut, ceiling != Tier::Frontier, "{}", ceiling.as_str());
    }
}

#[test]
fn pick_allowed() {
    let ruling = release::ruling();
    let allowed = ruling.allowed();
    for (category, kind) in cells() {
        for ceiling in Tier::ALL {
            let result = pick(release::policy(), category, kind, ceiling);
            if let Pick::Tier { tier, .. } = result {
                assert!(
                    allowed.contains(&tier),
                    "{} {} {} gave {}",
                    category.as_str(),
                    kind.as_str(),
                    ceiling.as_str(),
                    tier.as_str()
                );
            }
            if let PickCell::Tier(tier) = explain(release::policy(), category, kind).pick {
                assert!(allowed.contains(&tier), "{}", tier.as_str());
            }
        }
    }
}

#[test]
fn pick_replay() {
    let second = second();
    for policy in [release::policy(), &second] {
        for category in CategoryId::ALL {
            for kind in CallKind::ALL {
                for ceiling in Tier::ALL {
                    let result = pick(policy, category, kind, ceiling);
                    assert_eq!(
                        result == Pick::NoModel,
                        kind == CallKind::Replay,
                        "{} {} {}",
                        category.as_str(),
                        kind.as_str(),
                        ceiling.as_str()
                    );
                }
            }
        }
    }
    assert_eq!(Pick::NoModel.cell(), PickCell::None);
    assert_eq!(
        Pick::Tier {
            tier: Tier::Frontier,
            level: Level::High
        }
        .cell(),
        PickCell::Tier(Tier::Frontier)
    );
    assert_eq!(Pick::HandBack(Cause::Ceiling).cell(), PickCell::HandBack);
}
