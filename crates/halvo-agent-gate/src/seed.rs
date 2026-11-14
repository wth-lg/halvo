use std::fmt;
use std::num::NonZeroU32;

use halvo_schema::executable::body::{AddOrigin, CreateAgent, SetBindings};
use halvo_schema::executable::names::Way;
use halvo_schema::executable::record::Slot;
use halvo_schema::executable::{
    CoordinatorCall, CoordinatorReply, Name, OwnValue, RunId, WebOrigin,
};
use halvo_schema::{AgentId, ErrorName, SeedCategory, WorkflowId};
use serde_json::{Map, Value};

use crate::host::{minted, GateHost};

const START: &str = "start";
const REPLY_SENDERS: &str = "reply_senders";
const SENDER_SEPARATOR: &str = ", ";

/// The four tasks whose seed file holds a binding (docs/r3/fixtures.md): `fill` has none, because the gate command
/// binds its slots to the files of its own temporary directory (A1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedTask {
    Bill,
    Order,
    Reply,
    Cleanup,
}

impl SeedTask {
    /// The four, in the order of the key `bindings` of a seed file.
    pub const ALL: &'static [SeedTask] = &[
        SeedTask::Bill,
        SeedTask::Order,
        SeedTask::Reply,
        SeedTask::Cleanup,
    ];

    /// The task's name, the key of its binding in a seed file.
    pub fn as_str(self) -> &'static str {
        match self {
            SeedTask::Bill => "bill",
            SeedTask::Order => "order",
            SeedTask::Reply => "reply",
            SeedTask::Cleanup => "cleanup",
        }
    }

    /// The seed category of the task (A1.2).
    pub fn category(self) -> SeedCategory {
        match self {
            SeedTask::Bill => SeedCategory::UtilityBillPayment,
            SeedTask::Order => SeedCategory::ShopOrder,
            SeedTask::Reply => SeedCategory::Email,
            SeedTask::Cleanup => SeedCategory::CalendarCleanup,
        }
    }
}

/// What the gate host binds for one agent: the origins, each sent with `add_origin`, and the slots, sent together
/// with `set_bindings`, each in its order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub origins: Vec<WebOrigin>,
    pub slots: Vec<Slot>,
}

/// Why `join_senders` refuses a list of addresses for the slot `reply_senders` (the coordinator's ruling CP-112).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendersRefused {
    /// The list holds no address.
    Empty,
    /// An address is empty or ASCII spaces alone.
    EmptyAddress,
    /// An address holds a comma, which separates the addresses of the slot's one text.
    Comma,
    /// One address stands twice, without ASCII case and the ASCII spaces around it.
    Twice,
}

/// Why `seed_binding` refuses a seed file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeedRefused {
    /// The file is no JSON object with `bindings.<task>` of the shape `{"origins":[<string>...],"slots":[{"name":
    /// <string>,"value":[<string>...]}...]}`.
    Json,
    /// An origin is not of the seed's form, `http://<lower-case letters>.agent.test` (docs/r3/fixtures.md, the
    /// form that the fixture's reader takes), or its origin with the port is not the form of `WebOrigin`.
    Origin,
    /// The slot `start` is not one text `http://<host>` with no port whose origin with the port is the form of
    /// `WebOrigin`.
    Start,
    /// The slot `reply_senders` is a list that `join_senders` refuses.
    Senders(SendersRefused),
    /// A slot other than `reply_senders` is not one non-empty text.
    Value,
    /// A slot name is not a name of A0.7.
    Name,
}

/// Why a gate run stopped: the body that the handler refused, by its `op`, and its answer. The gate run sends no
/// body after it and starts no runner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stopped {
    pub op: &'static str,
    pub cause: StopCause,
}

/// The answer that stopped a gate run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopCause {
    /// The handler refused the body with this error.
    Refused(ErrorName),
    /// The handler answered with a reply that is not the body's reply form.
    Reply,
}

impl fmt::Display for Stopped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.cause {
            StopCause::Refused(error) => write!(
                f,
                "the seed's body {} was refused with {}",
                self.op,
                error.as_str()
            ),
            StopCause::Reply => write!(f, "the seed's body {} got another reply", self.op),
        }
    }
}

/// A gate run's workflow and agent after its seed is bound, with the newest revision of the bindings that a body
/// answered, none where the binding is empty.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub workflow: WorkflowId,
    pub agent: AgentId,
    pub rev: Option<NonZeroU32>,
}

/// The one text of the slot `reply_senders` from a list of addresses: the addresses as given, joined by a comma and
/// a space (CP-112). A reader splits it at commas and trims ASCII spaces, so the join refuses what that reader
/// would refuse and the gate can meet: an empty list, an empty address, an address with a comma, and one address
/// twice.
pub fn join_senders(addresses: &[String]) -> Result<String, SendersRefused> {
    if addresses.is_empty() {
        return Err(SendersRefused::Empty);
    }
    let mut seen: Vec<String> = Vec::with_capacity(addresses.len());
    for address in addresses {
        if address.contains(',') {
            return Err(SendersRefused::Comma);
        }
        let trimmed = address.trim_matches(' ');
        if trimmed.is_empty() {
            return Err(SendersRefused::EmptyAddress);
        }
        let folded = trimmed.to_ascii_lowercase();
        if seen.contains(&folded) {
            return Err(SendersRefused::Twice);
        }
        seen.push(folded);
    }
    Ok(addresses.join(SENDER_SEPARATOR))
}

/// Reads the binding of `task` from a seed file (docs/r3/fixtures.md) for a fixture that listens on `port`: each
/// origin, which must be of the seed's form, and the slot `start`, which must be `http://<host>`, with `:<port>` added, the slot `reply_senders` as the one text of `join_senders`,
/// and every other slot as its one text, each in the seed's order.
pub fn seed_binding(seed: &[u8], task: SeedTask, port: u16) -> Result<Binding, SeedRefused> {
    let seed: Value = serde_json::from_slice(seed).map_err(|_| SeedRefused::Json)?;
    let binding = seed
        .get("bindings")
        .and_then(|b| b.get(task.as_str()))
        .and_then(Value::as_object)
        .ok_or(SeedRefused::Json)?;
    only_keys(binding, &["origins", "slots"])?;
    let origins = strings(binding.get("origins"))?
        .iter()
        .map(|o| {
            seed_origin(o)
                .then(|| with_port(o, port))
                .flatten()
                .ok_or(SeedRefused::Origin)
        })
        .collect::<Result<Vec<WebOrigin>, SeedRefused>>()?;
    let slots = binding
        .get("slots")
        .and_then(Value::as_array)
        .ok_or(SeedRefused::Json)?
        .iter()
        .map(|slot| seed_slot(slot, port))
        .collect::<Result<Vec<Slot>, SeedRefused>>()?;
    Ok(Binding { origins, slots })
}

fn only_keys(object: &Map<String, Value>, keys: &[&str]) -> Result<(), SeedRefused> {
    if object.len() == keys.len() && keys.iter().all(|k| object.contains_key(*k)) {
        Ok(())
    } else {
        Err(SeedRefused::Json)
    }
}

fn strings(value: Option<&Value>) -> Result<Vec<String>, SeedRefused> {
    value
        .and_then(Value::as_array)
        .ok_or(SeedRefused::Json)?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or(SeedRefused::Json))
        .collect()
}

fn seed_origin(origin: &str) -> bool {
    origin
        .strip_prefix("http://")
        .and_then(|host| host.strip_suffix(".agent.test"))
        .is_some_and(|label| !label.is_empty() && label.bytes().all(|b| b.is_ascii_lowercase()))
}

fn with_port(origin: &str, port: u16) -> Option<WebOrigin> {
    WebOrigin::parse(&format!("{origin}:{port}")).ok()
}

fn seed_slot(slot: &Value, port: u16) -> Result<Slot, SeedRefused> {
    let object = slot.as_object().ok_or(SeedRefused::Json)?;
    only_keys(object, &["name", "value"])?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .ok_or(SeedRefused::Json)?;
    let values = strings(object.get("value"))?;
    let name = Name::parse(name).map_err(|_| SeedRefused::Name)?;
    let text = match name.as_str() {
        REPLY_SENDERS => join_senders(&values).map_err(SeedRefused::Senders)?,
        START => match values.as_slice() {
            [one] if one.starts_with("http://") => {
                with_port(one, port).ok_or(SeedRefused::Start)?.to_string()
            }
            _ => return Err(SeedRefused::Start),
        },
        _ => match values.as_slice() {
            [one] if !one.is_empty() => one.clone(),
            _ => return Err(SeedRefused::Value),
        },
    };
    Ok(Slot {
        name,
        value: OwnValue::new(&text),
    })
}

/// Binds `binding` for the agent as the product's parties send it: the slots with one `set_bindings`, as the
/// interface run's coordinator sends it, where there is a slot, then each origin with its own `add_origin` with the
/// way `sign_in` and the run `run`, as the supervisor library sends it after a sign-in (A14.5, CP-42, CP-49 (4) and
/// (5)). It stops at the first body that the handler refuses and sends nothing after it. It answers the revision
/// of the last body, none where the binding is empty.
pub fn bind(
    host: &mut GateHost,
    workflow: &WorkflowId,
    agent: &AgentId,
    run: &RunId,
    binding: &Binding,
) -> Result<Option<NonZeroU32>, Stopped> {
    let mut rev = None;
    if !binding.slots.is_empty() {
        rev = Some(revision(
            host.send(&CoordinatorCall::SetBindings(SetBindings {
                workflow: workflow.clone(),
                agent: agent.clone(),
                slots: binding.slots.clone(),
            })),
            "set_bindings",
        )?);
    }
    for origin in &binding.origins {
        rev = Some(revision(
            host.send(&CoordinatorCall::AddOrigin(AddOrigin {
                workflow: workflow.clone(),
                agent: agent.clone(),
                run: run.clone(),
                origin: origin.clone(),
                way: Way::SignIn,
            })),
            "add_origin",
        )?);
    }
    Ok(rev)
}

fn revision(
    answer: Result<CoordinatorReply, ErrorName>,
    op: &'static str,
) -> Result<NonZeroU32, Stopped> {
    match answer {
        Ok(CoordinatorReply::Rev(reply)) => Ok(reply.rev),
        Ok(_) => Err(Stopped {
            op,
            cause: StopCause::Reply,
        }),
        Err(error) => Err(Stopped {
            op,
            cause: StopCause::Refused(error),
        }),
    }
}

/// The steps of a gate run before its runner starts: one approved workflow of the task's category, its agent by
/// `create_agent` with a fresh id, and `bind` of the seed's binding with the run `run`, the run that the host
/// starts next. It stops at the first refused body.
pub fn prepare(
    host: &mut GateHost,
    task: SeedTask,
    binding: &Binding,
    run: &RunId,
) -> Result<Prepared, Stopped> {
    let workflow = host.put_seed_workflow(task.category());
    let agent = AgentId::from_random(minted());
    revision(
        host.send(&CoordinatorCall::CreateAgent(CreateAgent {
            workflow: workflow.clone(),
            agent: agent.clone(),
        })),
        "create_agent",
    )?;
    let rev = bind(host, &workflow, &agent, run, binding)?;
    Ok(Prepared {
        workflow,
        agent,
        rev,
    })
}
