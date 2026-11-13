use crate::args::Args;
use crate::bell::Bell;
use crate::wired::Wired;
use halvo_coord::guard::Policy;
use halvo_coord::ports::{Analysis, Clock, Owner, Rng, Runs};
use halvo_coord::sync::Next;
use halvo_coord::types::commands::LIST;
use halvo_coord::types::{
    Action, AgentId, Card, Cause, Choice, ClosedReply, ConfigView, ErrorReply, ItemId, Limits,
    OkReply, Param, Snapshot, YesNo,
};
use halvo_coord::Coordinator;
use serde::{Serialize, Serializer};
use serde_json::Value;
use std::sync::Arc;

/// A reply of a command (I8.1), written as the page's fixtures write it (I8.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reply {
    /// The state: `agents_snapshot`, and `agents_next` at a change or after its wait.
    Snapshot(Snapshot),
    /// `agents_card`.
    Card(Card),
    /// `{"ok":"yes"}`.
    Ok,
    /// `{"closed":"yes"}` of `agents_next`.
    Closed,
    /// `{"error":"<cause>"}`.
    Error(Cause),
}

impl Serialize for Reply {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Reply::Snapshot(state) => state.serialize(s),
            Reply::Card(card) => card.serialize(s),
            Reply::Ok => OkReply.serialize(s),
            Reply::Closed => ClosedReply { closed: YesNo::Yes }.serialize(s),
            Reply::Error(cause) => ErrorReply { error: *cause }.serialize(s),
        }
    }
}

impl From<Result<(), Cause>> for Reply {
    fn from(out: Result<(), Cause>) -> Reply {
        match out {
            Ok(()) => Reply::Ok,
            Err(cause) => Reply::Error(cause),
        }
    }
}

/// The hub's side that the app, the shell and the Tauri handler call, with no type of the
/// ports in it.
pub trait Core: Send + Sync {
    /// Runs the command of `LIST` named `command` with the arguments of its call, which are
    /// none when the call's body is no JSON; the reply of every refusal is
    /// `{"error":"<cause>"}`, so the call never fails.
    fn call(&self, command: &str, args: Option<&Value>) -> Reply;
}

/// The value that the app manages for the handler: the hub behind `Core`.
#[derive(Clone)]
pub struct Agents(pub Arc<dyn Core>);

type Cfg<P> = <P as Owner>::Config;

/// The coordinator of the app over its ports `P`, the bell of its page and tray, and the waits
/// of the waiting call.
pub struct Hub<P: Owner> {
    coord: Coordinator<Wired<P>>,
}

impl<P> Hub<P>
where
    P: Owner + Runs + Analysis + Clock + Rng + Policy<Config = Cfg<P>> + Send + Sync + 'static,
    Cfg<P>: ConfigView + Clone + Send + 'static,
{
    /// A hub of a new life over `ports` and `limits`, whose count of agents under `needs_you`
    /// goes to `tray`.
    pub fn new(ports: P, limits: Limits, tray: Box<dyn Fn(u32) + Send + Sync>) -> Arc<Hub<P>> {
        let bell = Arc::new(Bell::new(tray));
        Arc::new(Hub {
            coord: Coordinator::new(Wired::new(ports, bell), limits),
        })
    }

    /// The coordinator.
    pub fn coordinator(&self) -> &Coordinator<Wired<P>> {
        &self.coord
    }

    fn run(&self, command: &str, args: &Args<'_>) -> Option<Reply> {
        let item = || ItemId::parse(args.text("item"));
        let agent = || AgentId::parse(args.text("agent"));
        Some(match command {
            "agents_snapshot" => Reply::Snapshot(self.coord.snapshot()),
            "agents_next" => self.next(args.count("after")?),
            "agents_card" => match agent().and_then(|a| self.coord.card(&a)) {
                Ok(card) => Reply::Card(card),
                Err(cause) => Reply::Error(cause),
            },
            "agents_answer" => self.answer(item, args).into(),
            "agents_setup_typed" => item()
                .and_then(|i| {
                    let param = Param::parse(args.text("param"))?;
                    self.coord.setup_typed(&i, param, args.text("text"))
                })
                .into(),
            "agents_present" => item().and_then(|i| self.coord.present(&i)).into(),
            "agents_control" => agent()
                .and_then(|a| {
                    let action = Action::parse(args.text("action"))?;
                    self.coord.control(&a, action)
                })
                .into(),
            _ => return None,
        })
    }

    fn answer(
        &self,
        item: impl Fn() -> Result<ItemId, Cause>,
        args: &Args<'_>,
    ) -> Result<(), Cause> {
        let item = item()?;
        let choice = Choice::parse(args.text("choice"))?;
        let pick = match choice {
            Choice::Choose => {
                let n = args.count("n").ok_or(Cause::Malformed)?;
                Some((Param::parse(args.text("param"))?, n))
            }
            _ => None,
        };
        self.coord.answer(&item, choice, pick)
    }

    fn next(&self, after: u32) -> Reply {
        match self.coord.next(after) {
            Next::Changed(state) => Reply::Snapshot(state),
            Next::Same => Reply::Snapshot(self.coord.same()),
        }
    }
}

impl<P> Core for Hub<P>
where
    P: Owner + Runs + Analysis + Clock + Rng + Policy<Config = Cfg<P>> + Send + Sync + 'static,
    Cfg<P>: ConfigView + Clone + Send + 'static,
{
    fn call(&self, command: &str, args: Option<&Value>) -> Reply {
        let Some(found) = LIST.iter().find(|c| c.name == command) else {
            return Reply::Error(Cause::Malformed);
        };
        match Args::read(found, args) {
            Ok(args) => self
                .run(found.name, &args)
                .unwrap_or(Reply::Error(Cause::Malformed)),
            Err(cause) => Reply::Error(cause),
        }
    }
}
