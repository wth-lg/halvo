use super::chars::composed;
use super::compose::setup_with_line;
use super::{bound_field, value_bound, Marking, SetupParam};
use crate::ports::{Refused, Shared};
use crate::types::{
    AgentId, Body, BoundFrom, Category, Cause, Class, Item, ItemId, Limits, Name, Overlapping,
    Param, Slot, WorkflowId,
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
    /// The owner refused the owed body with `overlap` once more: the item asks every parameter
    /// again, as after `NotTaken`, and keeps the refused body in memory, never stored (I3.5 (a);
    /// ruling RD-67 3.2 (a)).
    Overlap,
    /// The kept body goes once more: the open item that keeps a refused body owes it again
    /// (I3.5 (d), (e); ruling RD-67 3.2 (d), (e)).
    Again,
}

/// A setup item that the coordinator holds: its item id, its workflow and agent, its
/// parameters with their candidates, the value given for each so far, the refusal by entry that
/// the reader of senders gave the newest answer for each, whether the owner refused its answers
/// by a name after which another value can pass, and its state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    item: ItemId,
    workflow: WorkflowId,
    agent: AgentId,
    category: Category,
    params: Vec<SetupParam>,
    given: Vec<Option<Vec<String>>>,
    refused: Vec<Option<Refused>>,
    not_taken: bool,
    step: u32,
    state: SetupState,
    kept: Option<SetupBody>,
    shared: Option<Shared>,
    waits: Option<AgentId>,
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
            category,
            given: vec![None; params.len()],
            refused: vec![None; params.len()],
            params,
            not_taken: false,
            step: 0,
            state: SetupState::Open,
            kept: None,
            shared: None,
            waits: None,
        })
    }

    /// The body that the owner refused with `overlap`, kept in memory and never stored, which
    /// the flow sends once more after a read with no entry or a switch-off of an agent that the
    /// sentence names; none after any other move that ends it (I3.5 (a); ruling RD-67 3.2 (a)).
    pub fn kept(&self) -> Option<&SetupBody> {
        self.kept.as_ref()
    }

    /// What the item holds to show of the newest refusal `overlap` of its body: the entries of
    /// the read `Owner::overlaps` that followed it, or that the read did not answer; none where
    /// no such refusal stands (I3.5 (c), (d); ruling RD-67 3.2). The sentence's words are U2.4d's.
    pub fn shared(&self) -> Option<&Shared> {
        self.shared.as_ref()
    }

    /// Takes the answer of the read that follows the newest refusal `overlap` in place of an
    /// earlier one, for an item that keeps a refused body; nothing otherwise.
    pub(crate) fn take_shared(&mut self, shared: Option<Shared>) {
        if self.kept.is_some() {
            self.shared = shared;
        }
    }

    /// The agent that the person switched off whose `paused` the owner has not acknowledged yet,
    /// for an open item whose kept body that switch-off freed: the body waits and goes at the
    /// first tick at which that agent's workflow is listed `paused` or has left the list (I3.5
    /// (e); ruling RD-160 (2)). Every move of the item but the answer `none` ends the wait.
    pub(crate) fn waits(&self) -> Option<&AgentId> {
        self.waits.as_ref()
    }

    /// Holds the kept body of an open item until the `paused` of `off` is acknowledged; nothing
    /// for an item that is not open or keeps no body.
    pub(crate) fn wait_for(&mut self, off: &AgentId) {
        if self.state == SetupState::Open && self.kept.is_some() {
            self.waits = Some(off.clone());
        }
    }

    /// The step mark (I3.4, ruling RD-87 1): 0 at the first ask, and one more each time the owner
    /// refuses the body by a name after which the item asks every parameter again; no other move
    /// changes it.
    pub fn step(&self) -> u32 {
        self.step
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

    /// The value that the item holds for `param`, as the person gave it; none while it holds
    /// none, and for a parameter that it does not ask.
    pub fn given(&self, param: Param) -> Option<&[String]> {
        let i = self.at(param).ok()?;
        self.given[i].as_deref()
    }

    /// Takes the refusal by entry that the reader of senders gave the answer for `param`: the
    /// parameter holds no value and carries the refusal until its next answer, and nothing else
    /// of the item changes (ruling RD-81 9.2); nothing for a parameter that the item does not
    /// ask.
    pub(crate) fn refuse(&mut self, param: Param, refused: Refused) {
        if let Ok(i) = self.at(param) {
            self.given[i] = None;
            self.refused[i] = Some(refused);
        }
    }

    /// The item as the page gets it (I3.4, I3.5), for the agent of the name `name` (I2.3):
    /// `compose_setup` of its parameters, with the line `item.setup.not_taken` while it asks
    /// again and the key `refused` of each parameter whose newest answer the reader of senders
    /// refused (ruling RD-81 9.3). Where the item holds what the read after the newest refusal
    /// `overlap` gave, the overlap sentence stands in the place of `item.setup.not_taken`, with
    /// the name of the other agent's row from `name_of` (I3.5 (c)).
    pub fn page_item(
        &self,
        name: &str,
        name_of: impl Fn(&AgentId) -> Option<String>,
        limits: &Limits,
    ) -> Result<Item, Cause> {
        let line = match &self.shared {
            Some(shared) => Some(Overlapping::of(shared, name_of).setup_line()?),
            None if self.not_taken => Some(crate::words::text("item.setup.not_taken", &[])?),
            None => None,
        };
        let item = setup_with_line(&self.item, &self.agent, name, &self.params, line, limits)?;
        let refused = self.params.iter().zip(&self.refused);
        Ok(refused
            .filter_map(|(p, r)| r.map(|r| (p.param, r)))
            .fold(item.at_step(self.step), |item, (param, r)| {
                item.refused_at(param, r)
            }))
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

/// The deciding field of I4.3 that the slot `param` of an agent of `category` binds, with its
/// class (ruling RD-90 items 3 and 6): a field of the class of the category's effects that
/// A-A9.3 binds from a slot and the table marks as deciding, so the payee and the instrument of a
/// payment and the seller and the instrument of a purchase; none for every other parameter.
fn deciding(category: Category, param: Param) -> Option<(Class, &'static str)> {
    let class = match category {
        Category::UtilityBillPayment => Class::Payment,
        Category::ShopOrder => Class::Purchase,
        _ => return None,
    };
    let field = param.name();
    match bound_field(class, field) {
        Some((sources, Marking::Deciding)) if sources.contains(&BoundFrom::Slot) => {
            Some((class, field))
        }
        _ => None,
    }
}

/// Whether `text`, given for `param` of an agent of `category`, holds to the predicate of a
/// deciding field (ruling RD-90 items 2 and 6): it equals its own composed form at the bound of
/// its field (`chars::composed` at `value_bound`), as a raised value of that field must, so that
/// a person's own value never trips the refusal at the raise later. Every text holds for a
/// parameter that binds no deciding field.
pub(super) fn fits(category: Category, param: Param, text: &str) -> bool {
    match deciding(category, param) {
        Some((class, field)) => {
            value_bound(class, field, BoundFrom::Slot).is_ok_and(|most| composed(text, most))
        }
        None => true,
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
/// item, `not_taken` makes it open again with every answer cleared, its step mark one more, and
/// the line `item.setup.not_taken` (ruling IG-34), and `gone` ends it. An open item refuses the
/// acknowledgement and `not_taken` with `state`, keeps everything at `none` (I3.5, ruling RD-23
/// (b)), and takes `choose` of a candidate that it shows, or a typed line for a parameter with no
/// candidate that holds, for a parameter that binds a deciding field, to that field's predicate
/// (`fits`; ruling RD-90 item 6) (`malformed` otherwise); a later answer for a parameter replaces
/// the earlier one.
/// While a parameter has no answer the item stays open; the answer that gives the last one makes
/// the one body `set_bindings`, with the slots of the given values, which is owed from then on.
/// `overlap` (I3.5 (a); ruling RD-67 3.2) makes an owed item open as `not_taken` does and keeps
/// its body in memory, never stored, and `again` makes an open item that keeps a body owe it once
/// more, `state` for an item that keeps none; the answer that makes a new body, `not_taken` and
/// the close end the kept body and what the item holds of its read, and an answer of the person
/// stops the read that a tick asks again; every move but the answer `none` ends the wait of a
/// freed body (`Setup::waits`). An answer that it takes for a parameter ends the refusal by
/// entry that the parameter carried, and a move after which the item asks every parameter again
/// ends every such refusal (ruling RD-81 9.2).
pub fn setup_step(setup: &Setup, mv: SetupMove<'_>) -> Result<Setup, Cause> {
    let mut next = setup.clone();
    if !matches!(mv, SetupMove::Input(SetupInput::None)) {
        next.waits = None;
    }
    match (&setup.state, mv) {
        (SetupState::AgentGone, SetupMove::Input(_)) => return Err(Cause::UnknownAgent),
        (SetupState::Closed | SetupState::Gone | SetupState::AgentGone, _) => {
            return Err(Cause::Stale)
        }
        (SetupState::Owed(_), SetupMove::Acknowledged) => next.state = SetupState::Closed,
        (SetupState::Owed(_), SetupMove::NotTaken) => {
            next.given = vec![None; setup.params.len()];
            next.refused = vec![None; setup.params.len()];
            next.not_taken = true;
            next.step = setup.step.saturating_add(1);
            next.state = SetupState::Open;
            next.kept = None;
            next.shared = None;
            return Ok(next);
        }
        (SetupState::Owed(body), SetupMove::Overlap) => {
            next.given = vec![None; setup.params.len()];
            next.refused = vec![None; setup.params.len()];
            next.not_taken = true;
            next.step = setup.step.saturating_add(1);
            next.state = SetupState::Open;
            next.kept = Some(body.clone());
            next.shared = None;
            return Ok(next);
        }
        (SetupState::Open, SetupMove::Again) => {
            let Some(kept) = setup.kept.clone() else {
                return Err(Cause::State);
            };
            next.state = SetupState::Owed(kept);
            return Ok(next);
        }
        (SetupState::Open, SetupMove::Overlap) => return Err(Cause::State),
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
            next.refused[i] = None;
        }
        (SetupState::Open, SetupMove::Input(SetupInput::Typed { param, text })) => {
            let i = setup.at(param)?;
            if !setup.params[i].candidates.is_empty() {
                return Err(Cause::Malformed);
            }
            let value = typed(text)?;
            if !fits(setup.category, param, text) {
                return Err(Cause::Malformed);
            }
            next.given[i] = Some(value);
            next.refused[i] = None;
        }
    }
    if matches!(
        mv,
        SetupMove::Input(SetupInput::Choose { .. } | SetupInput::Typed { .. })
    ) {
        if let Some(Shared::Unread { again }) = next.shared.as_mut() {
            *again = false;
        }
    }
    if next.state == SetupState::Open {
        if let Some(body) = next.body() {
            next.state = SetupState::Owed(body);
            next.kept = None;
            next.shared = None;
        }
    }
    Ok(next)
}
