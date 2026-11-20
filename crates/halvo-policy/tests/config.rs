use halvo_policy::{preset, validate, ConfigCause, LimitTable, LIMITS};
use halvo_schema::executable::names::{Period, Permission, YesNo};
use halvo_schema::executable::{
    AgentConfig, AllowedClass, Autonomy, Budget, Limits, ModelTier, Reasoning, Runtime,
};
use halvo_schema::{AgentId, Category, SeedCategory, WorkflowId};

fn ids() -> (WorkflowId, AgentId) {
    (
        WorkflowId::from_random([0x30; 16]),
        AgentId::from_random([0x40; 16]),
    )
}

fn preset_of(category: SeedCategory) -> AgentConfig {
    let (workflow, agent) = ids();
    match preset(Category::from(category), workflow, agent) {
        Some(config) => config,
        None => panic!("no preset for {category:?}"),
    }
}

fn limits(steps: u32, wall_s: u32, model_tokens: u32) -> Limits {
    Limits {
        steps,
        wall_s,
        model_tokens,
    }
}

type Row = (
    SeedCategory,
    Runtime,
    Autonomy,
    (YesNo, YesNo, YesNo),
    &'static [Permission],
    (u32, u32, u32),
);

const Y: YesNo = YesNo::Yes;
const N: YesNo = YesNo::No;

const ROWS: [Row; 10] = [
    (
        SeedCategory::UtilityBillPayment,
        Runtime::Web,
        Autonomy::DraftAndConfirm,
        (Y, Y, Y),
        &[],
        (40, 300, 0),
    ),
    (
        SeedCategory::TenantRent,
        Runtime::None,
        Autonomy::Observe,
        (Y, Y, N),
        &[],
        (1, 1, 0),
    ),
    (
        SeedCategory::LeaseRenewalOrChange,
        Runtime::None,
        Autonomy::Observe,
        (Y, Y, N),
        &[],
        (1, 1, 0),
    ),
    (
        SeedCategory::TenantApplicationReview,
        Runtime::None,
        Autonomy::Observe,
        (Y, Y, N),
        &[],
        (1, 1, 0),
    ),
    (
        SeedCategory::Email,
        Runtime::GraphMail,
        Autonomy::DraftAndConfirm,
        (Y, Y, Y),
        &[],
        (20, 120, 0),
    ),
    (
        SeedCategory::SpreadsheetFill,
        Runtime::File,
        Autonomy::DraftAndConfirm,
        (N, Y, N),
        &[Permission::UserFiles],
        (20, 120, 0),
    ),
    (
        SeedCategory::ShopOrder,
        Runtime::Web,
        Autonomy::DraftAndConfirm,
        (Y, Y, Y),
        &[],
        (40, 300, 0),
    ),
    (
        SeedCategory::CalendarCleanup,
        Runtime::GraphCalendar,
        Autonomy::DraftAndConfirm,
        (Y, Y, Y),
        &[],
        (20, 120, 0),
    ),
    (
        SeedCategory::RideOrder,
        Runtime::None,
        Autonomy::Observe,
        (Y, Y, N),
        &[],
        (1, 1, 0),
    ),
    (
        SeedCategory::DailyChartCheck,
        Runtime::None,
        Autonomy::Observe,
        (Y, Y, N),
        &[],
        (1, 1, 0),
    ),
];

#[test]
fn the_preset_of_each_seed_category_has_the_values_of_its_row() {
    let order: Vec<SeedCategory> = ROWS.iter().map(|r| r.0).collect();
    assert_eq!(
        order,
        SeedCategory::ALL,
        "the rows stand in the order of A1.1"
    );
    let (workflow, agent) = ids();
    for (category, runtime, autonomy, caps, permissions, (steps, wall_s, tokens)) in ROWS {
        let c = preset_of(category);
        assert_eq!(c.workflow, workflow);
        assert_eq!(c.agent, agent);
        assert_eq!(c.rev.get(), 1);
        assert_eq!(c.category, category);
        assert_eq!(c.tier, ModelTier::OnDevice);
        assert_eq!(c.reasoning, Reasoning::Medium);
        assert_eq!(c.autonomy, autonomy, "{category:?}");
        assert!(c.allow.is_empty());
        assert_eq!(
            (c.caps.untrusted, c.caps.private, c.caps.external),
            caps,
            "{category:?}"
        );
        assert_eq!(c.limits, limits(steps, wall_s, tokens), "{category:?}");
        assert_eq!(c.limits, LIMITS.of(category));
        assert_eq!(c.budget, None);
        assert_eq!(c.permissions, permissions, "{category:?}");
        assert_eq!(c.runtime, runtime, "{category:?}");
        assert_eq!(c.playbook, None);
        assert_eq!(validate(&c), Ok(()), "{category:?}");
        let json = match c.to_json() {
            Ok(json) => json,
            Err(r) => panic!("{r}"),
        };
        assert_eq!(AgentConfig::parse(json.as_bytes()), Ok(c));
    }
}

#[test]
fn other_has_no_preset() {
    let (workflow, agent) = ids();
    assert_eq!(preset(Category::Other, workflow, agent), None);
}

/// The trial file of the driver (U0.6, A13.6) as the commit holds it.
const DRIVER_TRIAL: &str = include_str!("../../../docs/r3/trials/driver.md");

struct Verdict {
    driver: Option<String>,
    path_a: bool,
    writer: Option<String>,
}

#[derive(Default)]
struct TrialLimits {
    bill: Option<Limits>,
    order: Option<Limits>,
    reply: Option<(u32, u32)>,
}

fn values<'a>(line: &'a str, keys: &[&str]) -> Result<Vec<&'a str>, String> {
    let words: Vec<&str> = line.split(' ').collect();
    if words.len() != keys.len() {
        return Err(format!("{line:?} has not the keys {keys:?}"));
    }
    words
        .iter()
        .zip(keys)
        .map(|(word, key)| match word.split_once('=') {
            Some((k, v)) if k == *key && !v.is_empty() => Ok(v),
            _ => Err(format!("{line:?} has no {key}= in its place")),
        })
        .collect()
}

fn number(line: &str, value: &str) -> Result<u32, String> {
    if !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("{line:?}: {value:?} is no number"));
    }
    value
        .parse()
        .map_err(|_| format!("{line:?}: {value:?} is no number"))
}

fn token(value: &str) -> Option<String> {
    (value != "none").then(|| value.to_owned())
}

fn read_trial(text: &str) -> Result<(Verdict, TrialLimits), String> {
    let mut verdicts = Vec::new();
    let mut found = TrialLimits::default();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("verdict ") {
            let v = values(rest, &["driver", "path", "writer"])?;
            let path_a = match v[1] {
                "A" => true,
                "B" => false,
                other => return Err(format!("{line:?}: the path {other:?} is neither A nor B")),
            };
            verdicts.push(Verdict {
                driver: token(v[0]),
                path_a,
                writer: token(v[2]),
            });
        } else if let Some(rest) = line.strip_prefix("limits ") {
            let (slot, row) = match rest.split(' ').next() {
                Some("task=bill") | Some("task=order") => {
                    let v = values(rest, &["task", "steps", "wall_s", "model_tokens"])?;
                    let row = limits(
                        number(line, v[1])?,
                        number(line, v[2])?,
                        number(line, v[3])?,
                    );
                    let slot = if v[0] == "bill" {
                        &mut found.bill
                    } else {
                        &mut found.order
                    };
                    (slot, row)
                }
                Some("task=reply") => {
                    let v = values(rest, &["task", "wall_s", "model_tokens"])?;
                    if found.reply.is_some() {
                        return Err(format!("{line:?}: a second limits line of reply"));
                    }
                    found.reply = Some((number(line, v[1])?, number(line, v[2])?));
                    continue;
                }
                _ => return Err(format!("{line:?}: no task of a limits line")),
            };
            if slot.is_some() {
                return Err(format!("{line:?}: a second limits line of its task"));
            }
            *slot = Some(row);
        }
    }
    match verdicts.len() {
        1 => Ok((verdicts.remove(0), found)),
        n => Err(format!("{n} verdict lines, not one")),
    }
}

/// Twice `value`, rounded up to a multiple of 10 (A1.3).
fn doubled(value: u32) -> Result<u32, String> {
    value
        .checked_mul(2)
        .and_then(|twice| twice.div_ceil(10).checked_mul(10))
        .ok_or_else(|| format!("{value} doubled is no u32"))
}

/// The table that A1.3 gives for the trial file `text`: `bill` and `order` from their `limits` lines under path A
/// and 40, 300 and 0 under path B; `reply` from its `limits` line when a writer is named and 20, 120 and 0 with
/// `writer=none`; `cleanup` and `fill` 20, 120 and 0; a deferred category 1, 1 and 0; and 2 rebinds.
fn from_trial(text: &str) -> Result<LimitTable, String> {
    let (verdict, found) = read_trial(text)?;
    if verdict.driver.is_some() != verdict.path_a {
        return Err("path=A exactly when a driver is named (A13.6)".to_owned());
    }
    let web = |task: &str, line: Option<Limits>| match (verdict.path_a, line) {
        (true, Some(l)) => Ok(limits(
            doubled(l.steps)?,
            doubled(l.wall_s)?,
            doubled(l.model_tokens)?,
        )),
        (false, None) => Ok(limits(40, 300, 0)),
        (true, None) => Err(format!("a driver is named and {task} has no limits line")),
        (false, Some(_)) => Err(format!("no driver is named and {task} has a limits line")),
    };
    let reply = match (&verdict.writer, found.reply) {
        (Some(_), Some((wall_s, model_tokens))) => {
            limits(20, doubled(wall_s)?, doubled(model_tokens)?)
        }
        (None, None) => limits(20, 120, 0),
        (Some(_), None) => return Err("a writer is named and reply has no limits line".to_owned()),
        (None, Some(_)) => return Err("writer=none and reply has a limits line".to_owned()),
    };
    Ok(LimitTable {
        utility_bill_payment: web("bill", found.bill)?,
        tenant_rent: limits(1, 1, 0),
        lease_renewal_or_change: limits(1, 1, 0),
        tenant_application_review: limits(1, 1, 0),
        email: reply,
        spreadsheet_fill: limits(20, 120, 0),
        shop_order: web("order", found.order)?,
        calendar_cleanup: limits(20, 120, 0),
        ride_order: limits(1, 1, 0),
        daily_chart_check: limits(1, 1, 0),
        rebinds: 2,
    })
}

const SAME_COMMIT: &str = "A pull request that changes the verdict or a limits line of docs/r3/trials/driver.md \
                           changes the table LIMITS in crates/halvo-policy/src/limits.rs in the same commit.";

#[test]
fn limits_from_trial() {
    let table = match from_trial(DRIVER_TRIAL) {
        Ok(table) => table,
        Err(e) => panic!("docs/r3/trials/driver.md: {e}. {SAME_COMMIT}"),
    };
    for &category in SeedCategory::ALL {
        assert_eq!(
            LIMITS.of(category),
            table.of(category),
            "{category:?}: {SAME_COMMIT}"
        );
    }
    assert_eq!(LIMITS, table, "{SAME_COMMIT}");
}

#[test]
fn limits_from_trial_doubles_a_named_drivers_lines_and_keeps_path_b_values() {
    let named = "verdict driver=m path=A writer=m\n\
                 limits task=bill steps=17 wall_s=140 model_tokens=1234\n\
                 limits task=order steps=20 wall_s=151 model_tokens=0\n\
                 limits task=reply wall_s=9 model_tokens=5\n";
    let Ok(t) = from_trial(named) else {
        panic!("a named driver's text is refused");
    };
    assert_eq!(t.utility_bill_payment, limits(40, 280, 2470));
    assert_eq!(t.shop_order, limits(40, 310, 0));
    assert_eq!(t.email, limits(20, 20, 10));

    let none = "```\nverdict driver=none path=B writer=none\n```\n";
    let Ok(t) = from_trial(none) else {
        panic!("the verdict of no driver is refused");
    };
    for row in [&t.utility_bill_payment, &t.shop_order] {
        assert_eq!(*row, limits(40, 300, 0));
    }
    assert_eq!(t.email, limits(20, 120, 0));
    for row in [&t.spreadsheet_fill, &t.calendar_cleanup] {
        assert_eq!(*row, limits(20, 120, 0));
    }
    for row in [
        &t.tenant_rent,
        &t.lease_renewal_or_change,
        &t.tenant_application_review,
        &t.ride_order,
        &t.daily_chart_check,
    ] {
        assert_eq!(*row, limits(1, 1, 0));
    }
    assert_eq!(t.rebinds, 2);

    let writer =
        "verdict driver=none path=B writer=w\r\nlimits task=reply wall_s=17 model_tokens=588\r\n";
    let Ok(t) = from_trial(writer) else {
        panic!("a writer under path B is refused");
    };
    assert_eq!(t.email, limits(20, 40, 1180));
    assert_eq!(t.utility_bill_payment, limits(40, 300, 0));
}

#[test]
fn limits_from_trial_refuses_a_text_for_which_a1_3_gives_no_value() {
    let bill = "limits task=bill steps=1 wall_s=1 model_tokens=0";
    let order = "limits task=order steps=1 wall_s=1 model_tokens=0";
    let reply = "limits task=reply wall_s=1 model_tokens=0";
    let a = "verdict driver=m path=A writer=none";
    let b = "verdict driver=none path=B writer=none";
    let cases: Vec<(&str, String)> = vec![
        ("no verdict", String::new()),
        ("two verdicts", format!("{b}\n{b}")),
        (
            "path A with no driver",
            format!("verdict driver=none path=A writer=none\n{bill}\n{order}"),
        ),
        (
            "an empty driver",
            format!("verdict driver= path=A writer=none\n{bill}\n{order}"),
        ),
        (
            "an empty writer",
            format!("verdict driver=none path=B writer=\n{reply}"),
        ),
        ("path B with a driver", "verdict driver=m path=B writer=none".to_owned()),
        ("a path C", "verdict driver=none path=C writer=none".to_owned()),
        ("a verdict with a key more", format!("{b} x=1")),
        ("path A with no bill line", format!("{a}\n{order}")),
        ("path A with no order line", format!("{a}\n{bill}")),
        ("path B with a bill line", format!("{b}\n{bill}")),
        ("path B with an order line", format!("{b}\n{order}")),
        ("a writer with no reply line", "verdict driver=none path=B writer=w".to_owned()),
        ("writer=none with a reply line", format!("{b}\n{reply}")),
        ("a second bill line", format!("{a}\n{bill}\n{bill}\n{order}")),
        ("a second reply line", format!("verdict driver=none path=B writer=w\n{reply}\n{reply}")),
        ("a task of no row", format!("{b}\nlimits task=fill steps=1 wall_s=1 model_tokens=0")),
        ("a reply line with steps", "verdict driver=none path=B writer=w\nlimits task=reply steps=1 wall_s=1 model_tokens=0".to_owned()),
        ("a bill line with no steps", format!("{a}\nlimits task=bill wall_s=1 model_tokens=0\n{order}")),
        ("a signed number", format!("{a}\nlimits task=bill steps=+1 wall_s=1 model_tokens=0\n{order}")),
        ("a key out of place", format!("{a}\nlimits task=bill wall_s=1 steps=1 model_tokens=0\n{order}")),
        ("an empty value", format!("{a}\nlimits task=bill steps= wall_s=1 model_tokens=0\n{order}")),
        ("a doubling past u32", format!("{a}\nlimits task=bill steps=2147483648 wall_s=1 model_tokens=0\n{order}")),
        ("a rounding past u32", format!("{a}\nlimits task=bill steps=2147483647 wall_s=1 model_tokens=0\n{order}")),
    ];
    for (what, text) in cases {
        assert!(from_trial(&text).is_err(), "{what} is taken");
    }
}

fn unattended_split() -> AgentConfig {
    let mut c = preset_of(SeedCategory::SpreadsheetFill);
    c.autonomy = Autonomy::Unattended;
    c.allow = vec![AllowedClass::Send];
    c
}

#[test]
fn validate_takes_an_unattended_agent_with_an_allowed_class_and_the_split() {
    let c = unattended_split();
    assert_eq!(validate(&c), Ok(()));
    let mut c = preset_of(SeedCategory::Email);
    c.autonomy = Autonomy::Unattended;
    c.allow = vec![AllowedClass::Send, AllowedClass::Delete];
    c.caps.private = N;
    assert_eq!(validate(&c), Ok(()));
}

#[test]
fn validate_answers_each_cause() {
    let mut c = preset_of(SeedCategory::Email);
    c.tier = ModelTier::Frontier;
    assert_eq!(validate(&c), Err(ConfigCause::Tier));
    c.tier = ModelTier::LowCostHosted;
    assert_eq!(validate(&c), Err(ConfigCause::Tier));

    let mut c = preset_of(SeedCategory::Email);
    c.budget = Some(Budget {
        amount: 500,
        period: Period::Month,
    });
    assert_eq!(validate(&c), Err(ConfigCause::Budget));

    let mut c = preset_of(SeedCategory::Email);
    c.allow = vec![AllowedClass::Submit];
    assert_eq!(validate(&c), Err(ConfigCause::Allow), "draft_and_confirm");
    c.autonomy = Autonomy::Observe;
    assert_eq!(validate(&c), Err(ConfigCause::Allow), "observe");
    c.autonomy = Autonomy::Unattended;
    assert_eq!(
        validate(&c),
        Err(ConfigCause::Allow),
        "unattended without the split"
    );
    c.caps.untrusted = N;
    assert_eq!(validate(&c), Ok(()));

    for autonomy in [Autonomy::DraftAndConfirm, Autonomy::Observe] {
        let mut c = preset_of(SeedCategory::SpreadsheetFill);
        c.allow = vec![AllowedClass::Send];
        c.autonomy = autonomy;
        assert_eq!(
            validate(&c),
            Err(ConfigCause::Allow),
            "{autonomy:?} with the split"
        );
    }

    for (category, other) in [
        (SeedCategory::Email, Runtime::Web),
        (SeedCategory::UtilityBillPayment, Runtime::GraphMail),
        (SeedCategory::SpreadsheetFill, Runtime::None),
        (SeedCategory::TenantRent, Runtime::Web),
        (SeedCategory::CalendarCleanup, Runtime::File),
    ] {
        let mut c = preset_of(category);
        c.runtime = other;
        assert_eq!(validate(&c), Err(ConfigCause::Runtime), "{category:?}");
    }
    for autonomy in [Autonomy::DraftAndConfirm, Autonomy::Unattended] {
        let mut c = preset_of(SeedCategory::RideOrder);
        c.autonomy = autonomy;
        assert_eq!(validate(&c), Err(ConfigCause::Runtime), "{autonomy:?}");
    }

    let mut c = preset_of(SeedCategory::Email);
    c.limits.steps = 0;
    assert_eq!(validate(&c), Err(ConfigCause::Limits));
    let mut c = preset_of(SeedCategory::Email);
    c.limits.wall_s = 0;
    assert_eq!(validate(&c), Err(ConfigCause::Limits));
    let mut c = preset_of(SeedCategory::Email);
    c.limits.model_tokens = 0;
    assert_eq!(validate(&c), Ok(()), "model_tokens 0 is no cause");
}

#[test]
fn validate_answers_the_first_cause_of_the_order() {
    let mut c = preset_of(SeedCategory::Email);
    c.tier = ModelTier::Frontier;
    c.budget = Some(Budget {
        amount: 1,
        period: Period::Week,
    });
    c.allow = vec![AllowedClass::Send];
    c.runtime = Runtime::Web;
    c.limits.steps = 0;
    assert_eq!(validate(&c), Err(ConfigCause::Tier));
    c.tier = ModelTier::OnDevice;
    assert_eq!(validate(&c), Err(ConfigCause::Budget));
    c.budget = None;
    assert_eq!(validate(&c), Err(ConfigCause::Allow));
    c.allow = Vec::new();
    assert_eq!(validate(&c), Err(ConfigCause::Runtime));
    c.runtime = Runtime::GraphMail;
    assert_eq!(validate(&c), Err(ConfigCause::Limits));
    c.limits.steps = 1;
    assert_eq!(validate(&c), Ok(()));
}

#[test]
fn the_names_of_the_causes() {
    let names: Vec<&str> = [
        ConfigCause::Tier,
        ConfigCause::Budget,
        ConfigCause::Allow,
        ConfigCause::Runtime,
        ConfigCause::Limits,
    ]
    .iter()
    .map(|c| c.as_str())
    .collect();
    assert_eq!(names, ["tier", "budget", "allow", "runtime", "limits"]);
}
