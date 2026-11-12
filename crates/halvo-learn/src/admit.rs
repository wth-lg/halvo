use crate::{EDITS_MAX, LOCATORS_MAX, VALUE_MIN};

/// One locator of a step's target (L1.3). Two locators are the same when `by`, `value` and
/// `name` are equal.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Locator {
    /// How the executor finds the control: `role`, `label`, `text`, `test_id` or `css`.
    pub by: String,
    /// The value that `by` looks for.
    pub value: String,
    /// The accessible name, or `None` for null.
    pub name: Option<String>,
}

/// The target of a step that an edit may change (L1.3): its description and its locators.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    /// `target.desc`.
    pub desc: String,
    /// `target.locators`, in the order in which the executor tries them.
    pub locators: Vec<Locator>,
}

tokens! {
    /// The mode of the run that proposes an edit (L3.2); `none` when the run has no
    /// `learn_run` row.
    pub enum RunMode {
        None = "none",
        Compile = "compile",
        Replay = "replay",
    }
}

tokens! {
    /// The causes of [`admit`], in the order of the rows of L3.2.
    pub enum AdmitCause {
        Mode = "mode",
        Base = "base",
        Scope = "scope",
        Class = "class",
        Locator = "locator",
        Value = "value",
        Length = "length",
        Rejected = "rejected",
        Full = "full",
    }
}

/// The eleven facts that [`admit`] reads (L3.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmitFacts {
    /// The mode of the run.
    pub mode: RunMode,
    /// The agent has a stored compile number, the step number is a step of it, and the
    /// compile number that the run loaded is still the agent's newest.
    pub has_base: bool,
    /// The proposed step differs from the served step in `target.desc` and
    /// `target.locators` alone.
    pub same_but_target: bool,
    /// The step's `kind` is `web` and its target is not null.
    pub kind_web: bool,
    /// The class of the proposed step equals the served step's `class`.
    pub class_same: bool,
    /// The playbook check accepts the compiled playbook with the proposed step in place.
    pub playbook_ok: bool,
    /// The number of [`new_locators`] of the proposal against the served list.
    pub new_count: usize,
    /// [`value_hit`] of the proposed target and the run's slot values.
    pub value_hit: bool,
    /// The step that would be served with this edit has a compact JSON of at most 2,048
    /// bytes.
    pub fits: bool,
    /// [`is_rejected`] of the new locators and the barred set of the step.
    pub rejected: bool,
    /// The edits of the agent, in any state, for this `base` and `n`.
    pub edits_for_step: u32,
}

/// Admits a proposed edit, or names the cause of the first row of L3.2 that holds: `mode`
/// when the mode is not `replay`, `base`, `scope`, `class`, `locator` when no locator is
/// new, `value`, `length` when the step does not fit, `rejected`, and `full` when the step
/// has [`EDITS_MAX`] edits or more.
pub fn admit(facts: &AdmitFacts) -> Result<(), AdmitCause> {
    let rows = [
        (facts.mode != RunMode::Replay, AdmitCause::Mode),
        (!facts.has_base, AdmitCause::Base),
        (!facts.same_but_target || !facts.kind_web, AdmitCause::Scope),
        (!facts.class_same || !facts.playbook_ok, AdmitCause::Class),
        (facts.new_count == 0, AdmitCause::Locator),
        (facts.value_hit, AdmitCause::Value),
        (!facts.fits, AdmitCause::Length),
        (facts.rejected, AdmitCause::Rejected),
        (facts.edits_for_step >= EDITS_MAX, AdmitCause::Full),
    ];
    match rows.into_iter().find(|&(holds, _)| holds) {
        Some((_, cause)) => Err(cause),
        None => Ok(()),
    }
}

/// The locators of the proposal that the served list does not hold, in the proposal's
/// order and each once. The served list ends with the compiled locators, so a compiled
/// locator is never new.
pub fn new_locators(proposed: &[Locator], served: &[Locator]) -> Vec<Locator> {
    let mut new: Vec<Locator> = Vec::new();
    for locator in proposed {
        if !served.contains(locator) && !new.contains(locator) {
            new.push(locator.clone());
        }
    }
    new
}

/// The list of a new edit (L3.3): the new locators in their order, then the base edit's
/// list without any locator that is among the new ones, cut from the end to
/// [`LOCATORS_MAX`], with no locator twice. `base` is empty when the edit is built on the
/// compiled step.
pub fn build_list(new: &[Locator], base: &[Locator]) -> Vec<Locator> {
    let mut list: Vec<Locator> = Vec::new();
    for locator in new.iter().chain(base) {
        if !list.contains(locator) {
            list.push(locator.clone());
        }
    }
    list.truncate(LOCATORS_MAX);
    list
}

/// What an edit brought (L3.3): the locators of its list that the list of the edit it names
/// as `on` does not hold, all of them when `on` is null and `on_list` is empty.
pub fn brought(list: &[Locator], on_list: &[Locator]) -> Vec<Locator> {
    list.iter()
        .filter(|locator| !on_list.contains(locator))
        .cloned()
        .collect()
}

/// Whether a `value`, a `name` or the `desc` of the proposed target contains, byte for
/// byte, a slot value of the run's bindings that has at least [`VALUE_MIN`] bytes (L3.2).
pub fn value_hit(proposed: &Target, slot_values: &[String]) -> bool {
    let fields: Vec<&str> = std::iter::once(proposed.desc.as_str())
        .chain(proposed.locators.iter().flat_map(|locator| {
            std::iter::once(locator.value.as_str()).chain(locator.name.as_deref())
        }))
        .collect();
    slot_values
        .iter()
        .filter(|value| value.len() >= VALUE_MIN)
        .any(|value| fields.iter().any(|field| field.contains(value.as_str())))
}

/// Whether one of the new locators is in the barred set: the union of what the `reverted`
/// rows of the same agent, `base` and `n` brought, which the caller builds with
/// [`brought`] (L3.2).
pub fn is_rejected(new: &[Locator], barred: &[Locator]) -> bool {
    new.iter().any(|locator| barred.contains(locator))
}
