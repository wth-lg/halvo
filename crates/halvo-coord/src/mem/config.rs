use crate::types::{
    AgentId, Autonomy, Budget, Category, Cause, Class, ConfigView, Period, Permission, Reasoning,
    Runtime, Tier, WorkflowId,
};
use serde_json::{Map, Value};

/// A configuration read from the record `agent_config` of A-A2.1, for tests (I2.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// `workflow`.
    pub workflow: WorkflowId,
    /// `agent`.
    pub agent: AgentId,
    /// `category`.
    pub category: Category,
    rev: u32,
    tier: Tier,
    reasoning: Reasoning,
    autonomy: Autonomy,
    allow: Vec<Class>,
    caps_external: bool,
    wall_s: u64,
    model_tokens: u64,
    budget: Option<Budget>,
    permissions: Vec<Permission>,
    runtime: Runtime,
    playbook: Option<u32>,
}

const KEYS: [&str; 15] = [
    "v",
    "workflow",
    "agent",
    "rev",
    "category",
    "tier",
    "reasoning",
    "autonomy",
    "allow",
    "caps",
    "limits",
    "budget",
    "permissions",
    "runtime",
    "playbook",
];

fn exact<'a>(value: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, Cause> {
    let object = value.as_object().ok_or(Cause::Malformed)?;
    if object.len() == keys.len() && keys.iter().all(|k| object.contains_key(*k)) {
        Ok(object)
    } else {
        Err(Cause::Malformed)
    }
}

fn text(value: &Value) -> Result<&str, Cause> {
    value.as_str().ok_or(Cause::Malformed)
}

fn count(value: &Value) -> Result<u64, Cause> {
    value.as_u64().ok_or(Cause::Malformed)
}

fn count32(value: &Value) -> Result<u32, Cause> {
    u32::try_from(count(value)?).map_err(|_| Cause::Malformed)
}

fn list<T>(value: &Value, read: fn(&str) -> Result<T, Cause>) -> Result<Vec<T>, Cause> {
    value
        .as_array()
        .ok_or(Cause::Malformed)?
        .iter()
        .map(|v| read(text(v)?))
        .collect()
}

fn yes(value: &Value) -> Result<bool, Cause> {
    match text(value)? {
        "yes" => Ok(true),
        "no" => Ok(false),
        _ => Err(Cause::Malformed),
    }
}

impl Config {
    /// Reads one record of A-A2.1: exactly its keys, `v` 1, every name of its lists and every
    /// count that fits; anything else is `malformed`.
    pub fn parse(json: &str) -> Result<Config, Cause> {
        let value: Value = serde_json::from_str(json).map_err(|_| Cause::Malformed)?;
        let o = exact(&value, &KEYS)?;
        if o["v"].as_u64() != Some(1) {
            return Err(Cause::Malformed);
        }
        let allow = list(&o["allow"], Class::parse)?;
        if allow
            .iter()
            .any(|c| !matches!(c, Class::Send | Class::Delete | Class::Submit))
        {
            return Err(Cause::Malformed);
        }
        let caps = exact(&o["caps"], &["untrusted", "private", "external"])?;
        yes(&caps["untrusted"])?;
        yes(&caps["private"])?;
        let limits = exact(&o["limits"], &["steps", "wall_s", "model_tokens"])?;
        count32(&limits["steps"])?;
        let budget = match &o["budget"] {
            Value::Null => None,
            b => {
                let b = exact(b, &["amount", "period"])?;
                Some(Budget {
                    amount: count(&b["amount"])?,
                    period: Period::parse(text(&b["period"])?)?,
                })
            }
        };
        let playbook = match &o["playbook"] {
            Value::Null => None,
            p => Some(count32(p)?),
        };
        Ok(Config {
            workflow: WorkflowId::parse(text(&o["workflow"])?)?,
            agent: AgentId::parse(text(&o["agent"])?)?,
            category: Category::parse(text(&o["category"])?)?,
            rev: count32(&o["rev"])?,
            tier: Tier::parse(text(&o["tier"])?)?,
            reasoning: Reasoning::parse(text(&o["reasoning"])?)?,
            autonomy: Autonomy::parse(text(&o["autonomy"])?)?,
            allow,
            caps_external: yes(&caps["external"])?,
            wall_s: count(&limits["wall_s"])?,
            model_tokens: count(&limits["model_tokens"])?,
            budget,
            permissions: list(&o["permissions"], Permission::parse)?,
            runtime: Runtime::parse(text(&o["runtime"])?)?,
            playbook,
        })
    }
}

impl ConfigView for Config {
    fn tier(&self) -> Tier {
        self.tier
    }
    fn reasoning(&self) -> Reasoning {
        self.reasoning
    }
    fn autonomy(&self) -> Autonomy {
        self.autonomy
    }
    fn allow_is_empty(&self) -> bool {
        self.allow.is_empty()
    }
    fn caps_external(&self) -> bool {
        self.caps_external
    }
    fn wall_s(&self) -> u64 {
        self.wall_s
    }
    fn model_tokens(&self) -> u64 {
        self.model_tokens
    }
    fn budget(&self) -> Option<Budget> {
        self.budget
    }
    fn permissions(&self) -> Vec<Permission> {
        self.permissions.clone()
    }
    fn runtime(&self) -> Runtime {
        self.runtime
    }
    fn playbook(&self) -> Option<u32> {
        self.playbook
    }
    fn rev(&self) -> u32 {
        self.rev
    }
}
