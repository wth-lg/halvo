use halvo_policy::{preset, validate, ConfigCause, LIMITS};
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
        (20, 120, 0),
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
        (20, 120, 0),
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

#[test]
fn the_limits_table_has_twenty_one_hundred_twenty_and_zero_for_a_task_and_two_rebinds() {
    let task = [
        SeedCategory::UtilityBillPayment,
        SeedCategory::Email,
        SeedCategory::SpreadsheetFill,
        SeedCategory::ShopOrder,
        SeedCategory::CalendarCleanup,
    ];
    for &category in SeedCategory::ALL {
        let want = if task.contains(&category) {
            limits(20, 120, 0)
        } else {
            limits(1, 1, 0)
        };
        assert_eq!(LIMITS.of(category), want, "{category:?}");
    }
    assert_eq!(LIMITS.rebinds, 2);
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
