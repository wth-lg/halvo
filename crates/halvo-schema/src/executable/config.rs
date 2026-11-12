use std::num::NonZeroU32;

use crate::category::SeedCategory;
use crate::ids::{AgentId, WorkflowId};

use super::json::{self, each_once, from_json, list, object_type, opt};
use super::names::{
    AllowedClass, Autonomy, ModelTier, Period, Permission, Reasoning, Runtime, YesNo,
};
use super::text::V1;
use super::Refusal;

/// The most an account cap may hold, in minor units.
pub const BUDGET_AMOUNT_MAX: u32 = 100_000_000;

object_type! {
    /// Which of the three properties of the session split hold for the agent.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Caps {
        pub untrusted: YesNo = "untrusted" => from_json,
        pub private: YesNo = "private" => from_json,
        pub external: YesNo = "external" => from_json,
    }
    rules(_c) { Ok(()) }
}

object_type! {
    /// The limits of one run: steps, the runner's seconds without the waits for a reply, and model tokens.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Limits {
        pub steps: u32 = "steps" => from_json,
        pub wall_s: u32 = "wall_s" => from_json,
        pub model_tokens: u32 = "model_tokens" => from_json,
    }
    rules(_l) { Ok(()) }
}

object_type! {
    /// A budget in the shape of the account cap: an amount in minor units from 1 to 100,000,000 and a period.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Budget {
        pub amount: u32 = "amount" => from_json,
        pub period: Period = "period" => from_json,
    }
    rules(b) {
        if b.amount == 0 || b.amount > BUDGET_AMOUNT_MAX {
            return Err(Refusal::malformed("Budget", "amount_range").at("amount"));
        }
        Ok(())
    }
}

object_type! {
    /// The record of an agent's configuration, kind `agent_config`. The parser checks the shape and the closed
    /// lists, and refuses a name that stands twice in `allow` or `permissions`; `validate` of halvo-policy checks
    /// the rest. `allow` holds `AllowedClass` values only, so a configuration that lets an agent pay, order, sign
    /// or type a credential unasked cannot be read or written.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct AgentConfig {
        pub v: V1 = "v" => from_json,
        pub workflow: WorkflowId = "workflow" => json::workflow_id,
        pub agent: AgentId = "agent" => json::agent_id,
        pub rev: NonZeroU32 = "rev" => from_json,
        pub category: SeedCategory = "category" => json::seed_category,
        pub tier: ModelTier = "tier" => from_json,
        pub reasoning: Reasoning = "reasoning" => from_json,
        pub autonomy: Autonomy = "autonomy" => from_json,
        pub allow: Vec<AllowedClass> = "allow" => list(from_json),
        pub caps: Caps = "caps" => Caps::from_raw,
        pub limits: Limits = "limits" => Limits::from_raw,
        pub budget: Option<Budget> = "budget" => opt(Budget::from_raw),
        pub permissions: Vec<Permission> = "permissions" => list(from_json),
        pub runtime: Runtime = "runtime" => from_json,
        pub playbook: Option<NonZeroU32> = "playbook" => opt(from_json),
    }
    rules(c) {
        each_once("AgentConfig", &c.allow).map_err(|r| r.at("allow"))?;
        each_once("AgentConfig", &c.permissions).map_err(|r| r.at("permissions"))?;
        Ok(())
    }
}
