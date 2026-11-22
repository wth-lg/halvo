//! The request maker of `calendar_delete` (A11.2), linked in a build with the feature `loopback` alone, which only the
//! runner's own tests turn on, so that no build that a person can use deletes an event (the coordinator's ruling of
//! 2026-11-21T20:16:11Z, log line 11974, point (2)). Each event that the run's own `calendar_list` kept is one paused
//! request of the port `Paused`, and it leaves by `Runner::release` alone (A5.2, A6.3), which lets it through only
//! from an `approved` row (`Paused::deletes_an_event`).

use std::num::NonZeroU32;
use std::rc::Rc;

use halvo_connect::{Calendar, CallError, Effect, EventToDelete, Graph, Token};
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::names::{FieldName, Source};
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::text::OwnValue;
use halvo_schema::executable::{ActionClass, Bound, BoundField, Cause, ExecutableStep};

use crate::release::{Left, MakerFacts, Paused, Replied};
use crate::run::{Failure, Runner};

/// The delete of one event: the adapter and the token of the delete step, the step's number, the handle of the event
/// and the facts of the run's own list.
struct DeleteEvent {
    graph: Rc<Graph>,
    token: Rc<Token>,
    step: NonZeroU32,
    event: EventToDelete,
    facts: MakerFacts,
}

/// The answer of a delete that left.
struct Answer(Result<(), CallError>);

impl Paused for DeleteEvent {
    fn class(&self) -> ActionClass {
        ActionClass::Delete
    }

    /// `items`, the one field of the class: the provider's id of the event that the handle deletes, as the run's own
    /// `calendar_list` gave it (A9.4). The object holds no slot, so the slots of a later load change nothing.
    fn bound(&mut self, _slots: &[Slot]) -> Result<Bound, Cause> {
        Bound::new(
            ActionClass::Delete,
            vec![BoundField {
                name: FieldName::Items,
                from: Source::Extract,
                origin: None,
                value: OwnValue::new(self.event.event().id()),
            }],
        )
        .map_err(|_| Cause::StepFailed)
    }

    fn facts(&self) -> MakerFacts {
        self.facts.clone()
    }

    /// DELETE of the event, once: `calendar_delete` takes the event's handle.
    fn release(self: Box<Self>) -> Box<dyn Left> {
        Box::new(Answer(self.graph.calendar_delete(&self.token, self.event)))
    }

    fn renew(self: Box<Self>, _slots: &[Slot]) -> Result<Box<dyn Paused>, Cause> {
        Ok(self)
    }

    fn step(&self) -> Option<NonZeroU32> {
        Some(self.step)
    }

    fn deletes_an_event(&self) -> bool {
        true
    }
}

impl Left for Answer {
    /// Made for the delete's success status alone; not made for a case of A11.2 that `effect()` of halvo-connect
    /// gives; no reply when no whole answer came; any other answer is read as neither (CP-159 part 5).
    fn reply(self: Box<Self>) -> Replied {
        match self.0 {
            Ok(()) => Replied::Made,
            Err(error) => match error.effect() {
                Effect::NotMade(case) => Replied::NotMade(case),
                Effect::Unknown if error == CallError::NoAnswer => Replied::NoReply,
                Effect::Unknown => Replied::Answered,
            },
        }
    }
}

impl Runner {
    /// `calendar_delete` of each event that the list step kept, in the order of the list: one paused request each,
    /// handed to `Runner::release` together, so that every delete of the step stands in one confirmation request,
    /// under the adapter for the first origin of the load's bindings. A step whose list kept no event deletes nothing.
    pub(super) fn delete_events(
        &mut self,
        step: &ExecutableStep,
        token: Token,
        loaded: &LoadReply,
    ) -> Result<(), Failure> {
        let events = self
            .lists
            .calendar
            .take()
            .ok_or(Failure::Step(Cause::Adapter))?;
        let origin = loaded
            .bindings
            .as_ref()
            .and_then(|b| b.origins.first().cloned())
            .ok_or(Failure::Step(Cause::Adapter))?;
        let graph = Rc::new(self.graph(loaded)?);
        let token = Rc::new(token);
        let facts = MakerFacts {
            request_origin: Some(origin),
            ids: events.iter().map(|event| event.id().to_owned()).collect(),
            senders: Vec::new(),
        };
        let requests: Vec<Box<dyn Paused>> = events
            .into_deletes()
            .map(|event| -> Box<dyn Paused> {
                Box::new(DeleteEvent {
                    graph: Rc::clone(&graph),
                    token: Rc::clone(&token),
                    step: step.n,
                    event,
                    facts: facts.clone(),
                })
            })
            .collect();
        if requests.is_empty() {
            return Ok(());
        }
        self.release(requests).map_err(Failure::Stop)
    }
}
