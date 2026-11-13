use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::ids::{AgentId, WorkflowId};
use crate::request::CoordinatorBody;

use super::json::{self, list, opt, Refusal};
use super::record::{agent_id, workflow_id, CoordEvent};
use super::{HISTORY_RECORDS, REQUEST_BYTES, REQUEST_RECORDS};

fn op(type_name: &'static str, want: &'static str) -> impl Fn(&RawValue) -> Result<(), Refusal> {
    move |raw| {
        if json::string(type_name, raw)? == want {
            Ok(())
        } else {
            Err(Refusal::new(type_name, "unknown_op"))
        }
    }
}

fn one_agent<'a>(
    type_name: &'static str,
    mut records: impl Iterator<Item = &'a CoordEvent>,
    workflow: &WorkflowId,
    agent: &AgentId,
) -> Result<(), Refusal> {
    if records.any(|r| &r.workflow != workflow || &r.agent != agent) {
        return Err(Refusal::new(type_name, "other_agent").at("records"));
    }
    Ok(())
}

/// The body `{"op":"coord_log","workflow":..,"agent":..,"records":[..]}` (contracts.md I7.2): 1 to
/// `REQUEST_RECORDS` records of the body's workflow and agent, and at most `REQUEST_BYTES` bytes: `parse` counts
/// the bytes of the input as it arrives, white space around the value among them, before it reads any of them, and
/// `to_json` refuses a body whose compact text has more. Its reply is `Stored`. That each `seq` is the stored last
/// plus 1 is the handler's rule, which answers `seq`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoordLog {
    pub workflow: WorkflowId,
    pub agent: AgentId,
    pub records: Vec<CoordEvent>,
}

const LOG_KEYS: [&str; 4] = ["op", "workflow", "agent", "records"];

struct LogFields<'a>(&'a CoordLog);

impl Serialize for LogFields<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(LOG_KEYS.len()))?;
        map.serialize_entry("op", CoordLog::OP)?;
        map.serialize_entry("workflow", &self.0.workflow)?;
        map.serialize_entry("agent", &self.0.agent)?;
        map.serialize_entry("records", &self.0.records)?;
        map.end()
    }
}

impl CoordLog {
    /// The name under the key `op`.
    pub const OP: &'static str = "coord_log";

    /// Checks that the body holds 1 to `REQUEST_RECORDS` records, each of its workflow and agent.
    pub fn rules(&self) -> Result<(), Refusal> {
        if self.records.is_empty() {
            return Err(Refusal::new("CoordLog", "empty").at("records"));
        }
        if self.records.len() > REQUEST_RECORDS {
            return Err(Refusal::new("CoordLog", "records").at("records"));
        }
        one_agent("CoordLog", self.records.iter(), &self.workflow, &self.agent)
    }

    fn from_raw(raw: &RawValue) -> Result<CoordLog, Refusal> {
        if raw.get().len() > REQUEST_BYTES {
            return Err(Refusal::new("CoordLog", "bytes"));
        }
        let obj = json::object("CoordLog", raw)?;
        obj.exactly(&LOG_KEYS)?;
        obj.read("op", op("CoordLog", CoordLog::OP))?;
        let body = CoordLog {
            workflow: obj.read("workflow", workflow_id)?,
            agent: obj.read("agent", agent_id)?,
            records: obj.read("records", list(CoordEvent::from_raw))?,
        };
        body.rules()?;
        Ok(body)
    }

    /// Reads one body and refuses every other input; an input of more than `REQUEST_BYTES` bytes is refused with
    /// the rule `bytes` before any of it is read.
    pub fn parse(bytes: &[u8]) -> Result<CoordLog, Refusal> {
        if bytes.len() > REQUEST_BYTES {
            return Err(Refusal::new("CoordLog", "bytes"));
        }
        CoordLog::from_raw(json::whole("CoordLog", bytes)?)
    }

    /// The compact JSON of the body, `op` first, or the refusal of a rule or of the bound in bytes that it breaks.
    pub fn to_json(&self) -> Result<String, Refusal> {
        self.rules()?;
        let text = json::write("CoordLog", &LogFields(self))?;
        if text.len() > REQUEST_BYTES {
            return Err(Refusal::new("CoordLog", "bytes"));
        }
        Ok(text)
    }
}

impl CoordinatorBody for CoordLog {
    type Reply = Stored;
}

impl Serialize for CoordLog {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_json()
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        LogFields(self).serialize(serializer)
    }
}

/// Reads a body from a value that the caller already holds. It bounds that value's own text by `REQUEST_BYTES`
/// after the caller holds it, so it cannot bound what arrived: a caller that holds the bytes of a request calls
/// `CoordLog::parse`, and a caller that reads from a stream bounds the read itself.
impl<'de> Deserialize<'de> for CoordLog {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        json::deserialize("CoordLog", deserializer, CoordLog::from_raw)
    }
}

/// The reply of `coord_log`, `{"stored":<count>}` (contracts.md I7.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stored {
    pub stored: u32,
}

const STORED_KEYS: [&str; 1] = ["stored"];

impl Stored {
    fn from_raw(raw: &RawValue) -> Result<Stored, Refusal> {
        let obj = json::object("Stored", raw)?;
        obj.exactly(&STORED_KEYS)?;
        Ok(Stored {
            stored: obj.read("stored", json::count)?,
        })
    }

    /// Reads one reply and refuses every other input.
    pub fn parse(bytes: &[u8]) -> Result<Stored, Refusal> {
        Stored::from_raw(json::whole("Stored", bytes)?)
    }

    /// The compact JSON of the reply.
    pub fn to_json(&self) -> Result<String, Refusal> {
        json::write("Stored", self)
    }
}

impl Serialize for Stored {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(STORED_KEYS.len()))?;
        map.serialize_entry("stored", &self.stored)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for Stored {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        json::deserialize("Stored", deserializer, Stored::from_raw)
    }
}

/// The body `{"op":"coord_history","workflow":..,"agent":..,"after":<count>|null}` (contracts.md I7.2): the
/// records of the agent with a `seq` above `after`, or from the first where it is null. Its reply is `History`.
/// `parse` refuses an input of more than `REQUEST_BYTES` bytes as it arrives before it reads any of them. The
/// compact text of this body has at most 130 bytes, far under the bound, so `to_json` has no rule for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoordHistory {
    pub workflow: WorkflowId,
    pub agent: AgentId,
    pub after: Option<u32>,
}

const HISTORY_BODY_KEYS: [&str; 4] = ["op", "workflow", "agent", "after"];

impl CoordHistory {
    /// The name under the key `op`.
    pub const OP: &'static str = "coord_history";

    fn from_raw(raw: &RawValue) -> Result<CoordHistory, Refusal> {
        let obj = json::object("CoordHistory", raw)?;
        obj.exactly(&HISTORY_BODY_KEYS)?;
        obj.read("op", op("CoordHistory", CoordHistory::OP))?;
        Ok(CoordHistory {
            workflow: obj.read("workflow", workflow_id)?,
            agent: obj.read("agent", agent_id)?,
            after: obj.read("after", opt(json::count))?,
        })
    }

    /// Reads one body and refuses every other input; an input of more than `REQUEST_BYTES` bytes is refused with
    /// the rule `bytes` before any of it is read.
    pub fn parse(bytes: &[u8]) -> Result<CoordHistory, Refusal> {
        if bytes.len() > REQUEST_BYTES {
            return Err(Refusal::new("CoordHistory", "bytes"));
        }
        CoordHistory::from_raw(json::whole("CoordHistory", bytes)?)
    }

    /// The compact JSON of the body, `op` first.
    pub fn to_json(&self) -> Result<String, Refusal> {
        json::write("CoordHistory", self)
    }
}

impl CoordinatorBody for CoordHistory {
    type Reply = History;
}

impl Serialize for CoordHistory {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(HISTORY_BODY_KEYS.len()))?;
        map.serialize_entry("op", CoordHistory::OP)?;
        map.serialize_entry("workflow", &self.workflow)?;
        map.serialize_entry("agent", &self.agent)?;
        map.serialize_entry("after", &self.after)?;
        map.end()
    }
}

/// Reads a body from a value that the caller already holds, with no bound in bytes: a caller that holds the bytes
/// of a request calls `CoordHistory::parse`, and a caller that reads from a stream bounds the read itself.
impl<'de> Deserialize<'de> for CoordHistory {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        json::deserialize("CoordHistory", deserializer, CoordHistory::from_raw)
    }
}

/// The reply of `coord_history`, `{"records":[..],"last":<count>}` (contracts.md I7.2): at most `HISTORY_RECORDS`
/// records of one workflow and agent in ascending `seq`, each at most `last`, the agent's stored last `seq`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct History {
    pub records: Vec<CoordEvent>,
    pub last: u32,
}

const HISTORY_KEYS: [&str; 2] = ["records", "last"];

impl History {
    /// Checks the bound, the order, the one agent and `last`.
    pub fn rules(&self) -> Result<(), Refusal> {
        let refuse = |rule: &'static str| Refusal::new("History", rule).at("records");
        if self.records.len() > HISTORY_RECORDS {
            return Err(refuse("records"));
        }
        if self
            .records
            .windows(2)
            .any(|pair| pair[0].seq >= pair[1].seq)
        {
            return Err(refuse("order"));
        }
        if self.records.iter().any(|r| r.seq.get() > self.last) {
            return Err(refuse("last"));
        }
        if let Some(first) = self.records.first() {
            one_agent(
                "History",
                self.records.iter(),
                &first.workflow,
                &first.agent,
            )?;
        }
        Ok(())
    }

    fn from_raw(raw: &RawValue) -> Result<History, Refusal> {
        let obj = json::object("History", raw)?;
        obj.exactly(&HISTORY_KEYS)?;
        let reply = History {
            records: obj.read("records", list(CoordEvent::from_raw))?,
            last: obj.read("last", json::count)?,
        };
        reply.rules()?;
        Ok(reply)
    }

    /// Reads one reply and refuses every other input.
    pub fn parse(bytes: &[u8]) -> Result<History, Refusal> {
        History::from_raw(json::whole("History", bytes)?)
    }

    /// The compact JSON of the reply, or the refusal of a rule that it breaks.
    pub fn to_json(&self) -> Result<String, Refusal> {
        self.rules()?;
        json::write("History", self)
    }
}

impl Serialize for History {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.rules()
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        let mut map = serializer.serialize_map(Some(HISTORY_KEYS.len()))?;
        map.serialize_entry("records", &self.records)?;
        map.serialize_entry("last", &self.last)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for History {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        json::deserialize("History", deserializer, History::from_raw)
    }
}
