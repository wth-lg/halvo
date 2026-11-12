use super::{compose_setup, SetupParam};
use crate::types::{
    AgentId, Body, Category, Cause, Item, ItemId, Limits, Name, Param, Slot, WorkflowId,
};

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
/// owner's acknowledgement; or gone, with its workflow or because its agent is not the
/// workflow's.
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
    /// The owner refused the body with `binding`: the agent is not the workflow's, nothing more
    /// is sent, and a move of the person for it is `unknown_agent` (I3.5, ruling IG-36).
    AgentGone,
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
    /// The choice `none`: the hold stays and the item stays open, with its answers (I3.5,
    /// ruling RD-23 (b)).
    None,
}

/// A move of a setup item: an answer of the person, or what the owner answered to its body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetupMove<'a> {
    /// An answer of the person.
    Input(SetupInput<'a>),
    /// The owner acknowledged the owed body.
    Acknowledged,
    /// The owner refused the owed body by a name after which another value can pass: the item
    /// asks every parameter again (I3.5, ruling IG-34).
    NotTaken,
    /// The owner refused the owed body with `no_workflow`: nothing more is sent.
    Gone,
    /// The owner refused the owed body with `binding`: nothing more is sent (ruling IG-36).
    Binding,
}

/// A setup item that the coordinator holds: its item id, its workflow and agent, its
/// parameters with their candidates, the value given for each so far, whether the owner refused
/// its answers by a name after which another value can pass, and its state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    item: ItemId,
    workflow: WorkflowId,
    agent: AgentId,
    params: Vec<SetupParam>,
    given: Vec<Option<Vec<String>>>,
    not_taken: bool,
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
            not_taken: false,
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

    /// Whether the owner refused the item's answers by a name after which another value can
    /// pass, so that the item asks every parameter again with the line `item.setup.not_taken`.
    pub fn not_taken(&self) -> bool {
        self.not_taken
    }

    /// The item as the page gets it (I3.4, I3.5), for the agent of the name `name` (I2.3):
    /// `compose_setup` of its parameters, with the line `item.setup.not_taken` while it asks
    /// again.
    pub fn page_item(&self, name: &str, limits: &Limits) -> Result<Item, Cause> {
        compose_setup(
            &self.item,
            &self.agent,
            name,
            &self.params,
            self.not_taken,
            limits,
        )
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
/// cause of its refusal, first that fits. An item whose agent the owner refused with `binding`
/// refuses an answer of the person with `unknown_agent` (ruling IG-36) and every other move with
/// `stale`; a closed item and one whose workflow is gone refuse every move with `stale`. While the
/// body is owed, an answer is refused with `state` (ruling IG-10), the acknowledgement closes the
/// item, `not_taken` makes it open again with every answer cleared and the line
/// `item.setup.not_taken` (ruling IG-34), and `gone` ends it. An open item refuses the
/// acknowledgement and `not_taken` with `state`, keeps everything at `none` (I3.5, ruling RD-23
/// (b)), and takes `choose` of a candidate that it shows, or a typed line for a parameter with no
/// candidate (`malformed` otherwise); a later answer for a parameter replaces the earlier one.
/// While a parameter has no answer the item stays open; the answer that gives the last one makes
/// the one body `set_bindings`, with the slots of the given values, which is owed from then on.
pub fn setup_step(setup: &Setup, mv: SetupMove<'_>) -> Result<Setup, Cause> {
    let mut next = setup.clone();
    match (&setup.state, mv) {
        (SetupState::AgentGone, SetupMove::Input(_)) => return Err(Cause::UnknownAgent),
        (SetupState::Closed | SetupState::Gone | SetupState::AgentGone, _) => {
            return Err(Cause::Stale)
        }
        (SetupState::Owed(_), SetupMove::Acknowledged) => next.state = SetupState::Closed,
        (SetupState::Owed(_), SetupMove::NotTaken) => {
            next.given = vec![None; setup.params.len()];
            next.not_taken = true;
            next.state = SetupState::Open;
            return Ok(next);
        }
        (_, SetupMove::Gone) => next.state = SetupState::Gone,
        (_, SetupMove::Binding) => next.state = SetupState::AgentGone,
        (SetupState::Owed(_), _)
        | (SetupState::Open, SetupMove::Acknowledged | SetupMove::NotTaken) => {
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
