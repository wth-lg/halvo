use super::SetupParam;
use crate::types::{AgentId, Body, Category, Cause, ItemId, Name, Param, Slot, WorkflowId};

const TYPED_MAX: usize = 1024;

/// The body of the setup flow (I3.5): the setup's `set_bindings` with one slot per parameter of
/// the item, each with the value that the person chose or typed, in the order of the item's
/// parameters. It has no place for an origin, because the coordinator never sends one (the
/// agent-setup run's CP-36, ruling IG-6b).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupBody {
    workflow: WorkflowId,
    agent: AgentId,
    slots: Vec<Slot>,
}

impl SetupBody {
    /// The slots, in the order of the item's parameters.
    pub fn slots(&self) -> &[Slot] {
        &self.slots
    }

    /// The body `set_bindings` that `Owner::send` carries.
    pub fn body(&self) -> Body {
        Body::SetBindings {
            workflow: self.workflow.clone(),
            agent: self.agent.clone(),
            slots: self.slots.clone(),
        }
    }
}

/// The state of a setup item (I3.5): open for answers; its body owed, sent and not yet
/// acknowledged, while the item stays open and takes no answer (ruling IG-10); closed at the
/// owner's acknowledgement; or gone with its workflow, which the owner no longer lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupState {
    /// Takes answers.
    Open,
    /// The body is owed until the owner acknowledges it.
    Owed(SetupBody),
    /// The owner acknowledged the body.
    Closed,
    /// The workflow is gone: nothing more is sent (I5.7).
    Gone,
}

/// An answer of the person to a setup item (I3.1, I8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupInput<'a> {
    /// The choice `choose` of `agents_answer`: the candidate `n`, from 1, of `param`.
    Choose {
        /// The parameter.
        param: Param,
        /// The candidate's number.
        n: u32,
    },
    /// `agents_setup_typed`: the text of a parameter that has no candidate.
    Typed {
        /// The parameter.
        param: Param,
        /// The text.
        text: &'a str,
    },
    /// The choice `none`: the hold stays and the item stays open (I3.5).
    None,
}

/// A move of a setup item: an answer of the person, or what the owner answered to its body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupMove<'a> {
    /// An answer of the person.
    Input(SetupInput<'a>),
    /// The owner acknowledged the owed body.
    Acknowledged,
    /// The owner refused the owed body with `no_workflow`: the workflow is gone.
    Gone,
}

/// A setup item that the coordinator holds: its item id, its workflow and agent, its
/// parameters with their candidates, the value given for each so far and its state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    item: ItemId,
    workflow: WorkflowId,
    agent: AgentId,
    params: Vec<SetupParam>,
    given: Vec<Option<Vec<String>>>,
    state: SetupState,
}

impl Setup {
    /// An open setup item that asks `params` of an agent of `category`, each with its candidates
    /// as the item shows them; `malformed` for no parameter, a parameter that the one table
    /// (`params`) does not give the category, or a parameter asked twice.
    pub fn new(
        item: ItemId,
        workflow: WorkflowId,
        agent: AgentId,
        category: Category,
        params: Vec<SetupParam>,
    ) -> Result<Setup, Cause> {
        let table = super::params(category);
        let fits = !params.is_empty()
            && params.iter().enumerate().all(|(i, p)| {
                table.contains(&p.param) && !params[..i].iter().any(|q| q.param == p.param)
            });
        if !fits {
            return Err(Cause::Malformed);
        }
        Ok(Setup {
            item,
            workflow,
            agent,
            given: vec![None; params.len()],
            params,
            state: SetupState::Open,
        })
    }

    /// The item id.
    pub fn item(&self) -> &ItemId {
        &self.item
    }

    /// The workflow.
    pub fn workflow(&self) -> &WorkflowId {
        &self.workflow
    }

    /// The agent.
    pub fn agent(&self) -> &AgentId {
        &self.agent
    }

    /// The parameters with their candidates.
    pub fn params(&self) -> &[SetupParam] {
        &self.params
    }

    /// The state.
    pub fn state(&self) -> &SetupState {
        &self.state
    }

    fn at(&self, param: Param) -> Result<usize, Cause> {
        self.params
            .iter()
            .position(|p| p.param == param)
            .ok_or(Cause::Malformed)
    }

    fn body(&self) -> Option<SetupBody> {
        let mut slots = Vec::new();
        for (p, given) in self.params.iter().zip(&self.given) {
            slots.push(Slot {
                name: Name::parse(p.param.name()).ok()?,
                value: given.clone()?,
            });
        }
        Some(SetupBody {
            workflow: self.workflow.clone(),
            agent: self.agent.clone(),
            slots,
        })
    }
}

/// A typed answer: one line of at most 1,024 bytes that holds something other than white space
/// (I3.5); else `malformed`.
fn typed(text: &str) -> Result<Vec<String>, Cause> {
    let one_line = !text.contains(['\n', '\r']);
    if one_line && text.len() <= TYPED_MAX && !text.trim().is_empty() {
        Ok(vec![text.to_owned()])
    } else {
        Err(Cause::Malformed)
    }
}

/// The decision of the setup flow (I3.5, I5.10, ruling IG-6c): the setup after `mv`, or the
/// cause of its refusal, first that fits. A closed item or one whose workflow is gone refuses
/// every move with `stale`. While the body is owed, an answer is refused with `state` (ruling
/// IG-10), the acknowledgement closes the item, and `gone` ends it. An open item refuses the
/// acknowledgement with `state`, keeps everything at `none`, and takes `choose` of a candidate
/// that it shows, or a typed line for a parameter with no candidate (`malformed` otherwise); a
/// later answer for a parameter replaces the earlier one. While a parameter has no answer the
/// item stays open; the answer that gives the last one makes the one body `set_bindings`, with
/// the slots of the given values, which is owed from then on.
pub fn setup_step(setup: &Setup, mv: SetupMove<'_>) -> Result<Setup, Cause> {
    let mut next = setup.clone();
    match (&setup.state, mv) {
        (SetupState::Closed | SetupState::Gone, _) => return Err(Cause::Stale),
        (SetupState::Owed(_), SetupMove::Acknowledged) => next.state = SetupState::Closed,
        (_, SetupMove::Gone) => next.state = SetupState::Gone,
        (SetupState::Owed(_), _) | (SetupState::Open, SetupMove::Acknowledged) => {
            return Err(Cause::State)
        }
        (SetupState::Open, SetupMove::Input(SetupInput::None)) => {}
        (SetupState::Open, SetupMove::Input(SetupInput::Choose { param, n })) => {
            let i = setup.at(param)?;
            let candidates = &setup.params[i].candidates;
            let k = usize::try_from(n).map_err(|_| Cause::Malformed)?;
            let (value, _) = k
                .checked_sub(1)
                .and_then(|k| candidates.get(k))
                .ok_or(Cause::Malformed)?;
            next.given[i] = Some(value.clone());
        }
        (SetupState::Open, SetupMove::Input(SetupInput::Typed { param, text })) => {
            let i = setup.at(param)?;
            if !setup.params[i].candidates.is_empty() {
                return Err(Cause::Malformed);
            }
            next.given[i] = Some(typed(text)?);
        }
    }
    if next.state == SetupState::Open {
        if let Some(body) = next.body() {
            next.state = SetupState::Owed(body);
        }
    }
    Ok(next)
}
