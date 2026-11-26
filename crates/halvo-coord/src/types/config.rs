use super::{Autonomy, Period, Permission, Reasoning, Runtime, Tier};

/// A budget of a configuration: an amount in minor units and a period (A-A2.1, A-A2.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget {
    /// Minor units, 1 to 100,000,000.
    pub amount: u64,
    /// `week` or `month`.
    pub period: Period,
}

/// What the app hands the core beside an agent's configuration of its use of an AI model at this
/// build's host (r4 U3.17b): the host, the model words that a yes covers, whether the agent's
/// stored yes stands for its stored tier, and what the app read of the person's key and spending
/// limit at the same list of the agents. The app hands none where this build's table of hosts
/// holds no row or its rows name two origins, so that no row offers either control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelUse {
    /// The host's name, as the app's one function `model_key::host` gives it.
    pub host: String,
    /// The model word of the table's row, or the words of its two rows joined by ` and `, in the
    /// order of the ruling's allowed tiers.
    pub models: String,
    /// Whether the agent's stored yes stands for its stored tier (the app's `agreement::stands`).
    pub stands: bool,
    /// Whether a key is saved: yes or no, or none where its state could not be read.
    pub key: Option<bool>,
    /// The person's spending limit as the reply of the limit's view gives it.
    pub limit: SpendLimit,
    /// Whether every compile, recover and write row of the router's table hands back, as the app's
    /// one function `model_key::hands_back` reads it (r4 U3.34): while it does, no agent of the
    /// build asks a model, and a row offers no Use an AI model.
    pub hands_back: bool,
}

/// The person's spending limit as the reply of the limit's view gives it (r7 8.1L).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpendLimit {
    /// A limit stands (the states `ok`, `near`, `reached` and `none`): its amount and the word of
    /// its period, as the reply writes them, and whether the room left is at most a fifth of the
    /// limit (`near`, for the states `near` and `reached`; r3 U7.8).
    Stands {
        /// The amount, as the reply's `cap` writes it.
        amount: String,
        /// The word of the period, as the reply's `period` writes it.
        period: String,
        /// Whether the reply's state is `near` or `reached`.
        near: bool,
    },
    /// No limit is set (the state `no_limit`).
    NotSet,
    /// The count's state could not be read (`unread`, `not_known`, or a reply with no amount).
    Unread,
}

/// The keys of an agent's configuration (A-A2.1) that the coordinator reads (I2.1).
/// `halvo_coord_app` implements it for the schema crate's `AgentConfig`, and tests for
/// `crate::mem::Config`, which reads the record of A-A2.1.
pub trait ConfigView {
    /// What the app hands beside the record of the agent's use of an AI model (r4 U3.17b), none
    /// where this build offers none: the one method that reads no key of the record. None unless
    /// the implementation hands one.
    fn model(&self) -> Option<ModelUse> {
        None
    }
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
    /// `category`.
    fn category(&self) -> super::Category;
}
