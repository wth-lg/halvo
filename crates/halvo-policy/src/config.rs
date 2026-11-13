use std::num::NonZeroU32;

use halvo_schema::executable::names::{Permission, YesNo};
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{AgentConfig, Autonomy, Caps, ModelTier, Reasoning, Runtime};
use halvo_schema::{AgentId, Category, SeedCategory, WorkflowId};

use crate::limits::LIMITS;

/// The first cause of A2.4 that a parsed configuration breaks. `malformed`, the first cause of that list, is the
/// answer of the parser (`AgentConfig::parse`), which comes before `validate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigCause {
    /// `tier` is not `on_device`.
    Tier,
    /// `budget` is not null.
    Budget,
    /// `allow` is not empty while `autonomy` is not `unattended`, or while `caps.untrusted` and `caps.private` are
    /// both `yes`.
    Allow,
    /// `runtime` is not the runtime of the category's preset, or `autonomy` is not `observe` while `runtime` is
    /// `none`.
    Runtime,
    /// `steps` or `wall_s` is 0.
    Limits,
}

impl ConfigCause {
    /// The name of the cause.
    pub fn as_str(self) -> &'static str {
        match self {
            ConfigCause::Tier => "tier",
            ConfigCause::Budget => "budget",
            ConfigCause::Allow => "allow",
            ConfigCause::Runtime => "runtime",
            ConfigCause::Limits => "limits",
        }
    }
}

struct Row {
    runtime: Runtime,
    autonomy: Autonomy,
    caps: (YesNo, YesNo, YesNo),
    permissions: &'static [Permission],
}

fn row(category: SeedCategory) -> Row {
    const ALL_THREE: (YesNo, YesNo, YesNo) = (YesNo::Yes, YesNo::Yes, YesNo::Yes);
    let task = |runtime| Row {
        runtime,
        autonomy: Autonomy::DraftAndConfirm,
        caps: ALL_THREE,
        permissions: &[],
    };
    let deferred = Row {
        runtime: Runtime::None,
        autonomy: Autonomy::Observe,
        caps: (YesNo::Yes, YesNo::Yes, YesNo::No),
        permissions: &[],
    };
    match category {
        SeedCategory::UtilityBillPayment | SeedCategory::ShopOrder => task(Runtime::Web),
        SeedCategory::Email => task(Runtime::GraphMail),
        SeedCategory::CalendarCleanup => task(Runtime::GraphCalendar),
        SeedCategory::SpreadsheetFill => Row {
            runtime: Runtime::File,
            autonomy: Autonomy::DraftAndConfirm,
            caps: (YesNo::No, YesNo::Yes, YesNo::No),
            permissions: &[Permission::UserFiles],
        },
        SeedCategory::TenantRent
        | SeedCategory::LeaseRenewalOrChange
        | SeedCategory::TenantApplicationReview
        | SeedCategory::RideOrder
        | SeedCategory::DailyChartCheck => deferred,
    }
}

/// The configuration of a new agent of `category` (A1.3): the values of its row, `rev` 1, `tier` `on_device`,
/// `budget` null, `allow` empty, `playbook` null and the limits of its category in `LIMITS`; none for `other`.
pub fn preset(category: Category, workflow: WorkflowId, agent: AgentId) -> Option<AgentConfig> {
    let category = category.seed()?;
    let row = row(category);
    Some(AgentConfig {
        v: V1,
        workflow,
        agent,
        rev: NonZeroU32::MIN,
        category,
        tier: ModelTier::OnDevice,
        reasoning: Reasoning::Medium,
        autonomy: row.autonomy,
        allow: Vec::new(),
        caps: Caps {
            untrusted: row.caps.0,
            private: row.caps.1,
            external: row.caps.2,
        },
        limits: LIMITS.of(category),
        budget: None,
        permissions: row.permissions.to_vec(),
        runtime: row.runtime,
        playbook: None,
    })
}

/// `Ok` for a valid configuration, else the first cause of A2.4 after `malformed`, in its order: `tier`, `budget`,
/// `allow`, `runtime`, `limits`.
pub fn validate(config: &AgentConfig) -> Result<(), ConfigCause> {
    if config.tier != ModelTier::OnDevice {
        return Err(ConfigCause::Tier);
    }
    if config.budget.is_some() {
        return Err(ConfigCause::Budget);
    }
    let joined = config.caps.untrusted == YesNo::Yes && config.caps.private == YesNo::Yes;
    if !config.allow.is_empty() && (config.autonomy != Autonomy::Unattended || joined) {
        return Err(ConfigCause::Allow);
    }
    if config.runtime != row(config.category).runtime
        || (config.autonomy != Autonomy::Observe && config.runtime == Runtime::None)
    {
        return Err(ConfigCause::Runtime);
    }
    if config.limits.steps == 0 || config.limits.wall_s == 0 {
        return Err(ConfigCause::Limits);
    }
    Ok(())
}
