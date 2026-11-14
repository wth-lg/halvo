use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;

use crate::ids::{AgentId, WorkflowId};
use crate::request::CoordinatorBody;

use super::json::{self, list, Refusal};
use super::names::RefinementKind;
use super::record::{agent_id, workflow_id, Day};

/// The coordinator body of the learning run, `{"op":"list_refinements","workflow":..,"agent":..}`: the read of
/// what the run learned for one agent of one workflow (contracts.md L2.2). Its reply is `Refinements`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LearnedRead {
    pub workflow: WorkflowId,
    pub agent: AgentId,
}

const BODY_KEYS: [&str; 3] = ["op", "workflow", "agent"];

impl LearnedRead {
    /// The name under the key `op`.
    pub const OP: &'static str = "list_refinements";

    fn from_raw(raw: &RawValue) -> Result<LearnedRead, Refusal> {
        let obj = json::object("LearnedRead", raw)?;
        obj.exactly(&BODY_KEYS)?;
        let op = obj.read("op", |raw| json::string("LearnedRead", raw))?;
        if op != LearnedRead::OP {
            return Err(Refusal::new("LearnedRead", "unknown_op").at("op"));
        }
        Ok(LearnedRead {
            workflow: obj.read("workflow", workflow_id)?,
            agent: obj.read("agent", agent_id)?,
        })
    }

    /// Reads one body and refuses every other input.
    pub fn parse(bytes: &[u8]) -> Result<LearnedRead, Refusal> {
        LearnedRead::from_raw(json::whole("LearnedRead", bytes)?)
    }

    /// The compact JSON of the body, `op` first.
    pub fn to_json(&self) -> Result<String, Refusal> {
        json::write("LearnedRead", self)
    }
}

impl CoordinatorBody for LearnedRead {
    type Reply = Refinements;
}

impl Serialize for LearnedRead {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(BODY_KEYS.len()))?;
        map.serialize_entry("op", LearnedRead::OP)?;
        map.serialize_entry("workflow", &self.workflow)?;
        map.serialize_entry("agent", &self.agent)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for LearnedRead {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        json::deserialize("LearnedRead", deserializer, LearnedRead::from_raw)
    }
}

/// One entry of the reply of `list_refinements`, `{"day":..,"what":..}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Refinement {
    pub day: Day,
    pub what: RefinementKind,
}

const ENTRY_KEYS: [&str; 2] = ["day", "what"];

impl Refinement {
    fn from_raw(raw: &RawValue) -> Result<Refinement, Refusal> {
        let obj = json::object("Refinement", raw)?;
        obj.exactly(&ENTRY_KEYS)?;
        Ok(Refinement {
            day: obj.read("day", Day::from_raw)?,
            what: obj.read("what", RefinementKind::from_raw)?,
        })
    }
}

impl Serialize for Refinement {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(ENTRY_KEYS.len()))?;
        map.serialize_entry("day", &self.day)?;
        map.serialize_entry("what", &self.what)?;
        map.end()
    }
}

/// The reply of `list_refinements`, `{"refinements":[..]}` (contracts.md L2.2): the entries in ascending order
/// of `day`, then of `what` in byte order, with `routine_stale` at most once. It holds no label, no step number
/// and no other string from a page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refinements {
    pub refinements: Vec<Refinement>,
}

const REPLY_KEYS: [&str; 1] = ["refinements"];

impl Refinements {
    /// Checks the order of the entries and that `routine_stale` stands at most once.
    pub fn rules(&self) -> Result<(), Refusal> {
        let refuse = |rule: &'static str| Refusal::new("Refinements", rule).at("refinements");
        let out_of_order = self.refinements.windows(2).any(|pair| {
            (pair[0].day, pair[0].what.as_str().as_bytes())
                > (pair[1].day, pair[1].what.as_str().as_bytes())
        });
        if out_of_order {
            return Err(refuse("order"));
        }
        let stale = self
            .refinements
            .iter()
            .filter(|entry| entry.what == RefinementKind::RoutineStale)
            .count();
        if stale > 1 {
            return Err(refuse("repeated_stale"));
        }
        Ok(())
    }

    fn from_raw(raw: &RawValue) -> Result<Refinements, Refusal> {
        let obj = json::object("Refinements", raw)?;
        obj.exactly(&REPLY_KEYS)?;
        let reply = Refinements {
            refinements: obj.read("refinements", list(Refinement::from_raw))?,
        };
        reply.rules()?;
        Ok(reply)
    }

    /// Reads one reply and refuses every other input.
    pub fn parse(bytes: &[u8]) -> Result<Refinements, Refusal> {
        Refinements::from_raw(json::whole("Refinements", bytes)?)
    }

    /// The compact JSON of the reply, or the refusal of a rule it breaks.
    pub fn to_json(&self) -> Result<String, Refusal> {
        self.rules()?;
        json::write("Refinements", self)
    }
}

impl Serialize for Refinements {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.rules()
            .map_err(<S::Error as serde::ser::Error>::custom)?;
        let mut map = serializer.serialize_map(Some(REPLY_KEYS.len()))?;
        map.serialize_entry("refinements", &self.refinements)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for Refinements {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        json::deserialize("Refinements", deserializer, Refinements::from_raw)
    }
}
