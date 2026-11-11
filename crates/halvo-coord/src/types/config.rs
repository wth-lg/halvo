use super::{Autonomy, Period, Permission, Reasoning, Runtime, Tier};

/// A budget of a configuration: an amount in minor units and a period (A-A2.1, A-A2.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Minor units, 1 to 100,000,000.
    pub amount: u64,
    /// `week` or `month`.
    pub period: Period,
}

/// The keys of an agent's configuration (A-A2.1) that the coordinator reads (I2.1).
/// `halvo_coord_app` implements it for the schema crate's `AgentConfig`, and tests for
/// `crate::mem::Config`, which reads the record of A-A2.1.
pub trait ConfigView {
    /// `tier`.
    fn tier(&self) -> Tier;
    /// `reasoning`.
    fn reasoning(&self) -> Reasoning;
    /// `autonomy`.
    fn autonomy(&self) -> Autonomy;
    /// Whether `allow` is empty.
    fn allow_is_empty(&self) -> bool;
    /// Whether `caps.external` is `yes`.
    fn caps_external(&self) -> bool;
    /// `limits.wall_s`.
    fn wall_s(&self) -> u64;
    /// `limits.model_tokens`.
    fn model_tokens(&self) -> u64;
    /// `budget`, or none for null.
    fn budget(&self) -> Option<Budget>;
    /// `permissions`.
    fn permissions(&self) -> Vec<Permission>;
    /// `runtime`.
    fn runtime(&self) -> Runtime;
    /// `playbook`, or none for null.
    fn playbook(&self) -> Option<u32>;
    /// `rev`.
    fn rev(&self) -> u32;
}
