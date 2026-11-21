//! The two mark requests (contracts.md C24.12 of the workflows run): a sender tells the daemon that an agent run
//! starts or ends, and the daemon stores one `run_mark` record for it.
//!
//! ```text
//! run_start{}   answers   run_marked{edge: "start", stored: true|false}
//! run_end{}     answers   run_marked{edge: "end", stored: true|false}
//! ```
//!
//! Both are requests of protocol v1 with no field, `{"v":1,"body":{"op":"run_start"}}` and
//! `{"v":1,"body":{"op":"run_end"}}`: the two names of [`crate::proto::RESERVED_OPS`]. The reply is
//! `{"v":1,"body":{"kind":"run_marked","edge":"start","stored":true}}`. `stored` is false when the store owner did
//! not take the record, which is no error. A refusal is an error reply of v1: `invalid_argument` for a body with a
//! key beside `op`.
//!
//! They are no bodies of protocol v2, so `dispatch_unknown` does not serve them. On a request that the v1 table does
//! not hold, the server calls [`mark_request`] first, answers what it returns, and goes on to `dispatch_unknown` for
//! `None` alone. A daemon built without the feature answers both names `unsupported_op`.

use serde::{Deserialize, Serialize};

use crate::model::one_spelling;
use crate::proto::{ErrorCode, ErrorReply, ReservedOp, UnknownRequest, PROTOCOL_VERSION};

/// The edge of an agent run that a mark request names: `run_start` opens the run and `run_end` closes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEdge {
    Start,
    End,
}

one_spelling!(RunEdge, "run edge", [Start => "start", End => "end"]);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<B> {
    v: u64,
    body: B,
}

#[derive(Serialize)]
struct RequestBody {
    op: &'static str,
}

impl RunEdge {
    /// The op of the request that names the edge.
    pub const fn op(self) -> &'static str {
        match self {
            RunEdge::Start => ReservedOp::RunStart.as_str(),
            RunEdge::End => ReservedOp::RunEnd.as_str(),
        }
    }

    /// The bytes of the request: `{"v":1,"body":{"op":"run_start"}}` or `{"v":1,"body":{"op":"run_end"}}`.
    pub fn request(self) -> Vec<u8> {
        serde_json::to_vec(&Envelope {
            v: PROTOCOL_VERSION,
            body: RequestBody { op: self.op() },
        })
        .expect("a value of these types always serializes")
    }
}

/// The reply `run_marked`: the edge of the request, and whether the store owner took its record. `stored` true means
/// appended to the batch that the owner is filling, not yet persisted; false is no error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunMarked {
    pub edge: RunEdge,
    pub stored: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MarkedKind {
    RunMarked,
}

#[derive(Serialize, Deserialize)]
struct MarkedBody {
    kind: MarkedKind,
    edge: RunEdge,
    stored: bool,
}

impl RunMarked {
    /// The bytes of the reply, in its one spelling.
    pub fn encode(self) -> Vec<u8> {
        serde_json::to_vec(&Envelope {
            v: PROTOCOL_VERSION,
            body: MarkedBody {
                kind: MarkedKind::RunMarked,
                edge: self.edge,
                stored: self.stored,
            },
        })
        .expect("a value of these types always serializes")
    }

    /// Reads a reply frame. `None` for a frame that is no `run_marked` of protocol v1, an error reply among them,
    /// which the caller reads with [`crate::proto::decode_reply`]. A key of the body that this build does not know
    /// is skipped, as in every reply of v1.
    pub fn decode(frame: &[u8]) -> Option<Self> {
        let Envelope { v, body } = serde_json::from_slice::<Envelope<MarkedBody>>(frame).ok()?;
        (v == PROTOCOL_VERSION).then_some(RunMarked {
            edge: body.edge,
            stored: body.stored,
        })
    }
}

/// The edge that `request` names, for the server's dispatch of a request that the v1 table does not hold. `None` for
/// a request that is no mark request: a version other than 1, or another op. A mark request whose body holds a key
/// beside `op` is refused with `invalid_argument`.
pub fn mark_request(request: &UnknownRequest) -> Option<Result<RunEdge, ErrorReply>> {
    if request.version() != PROTOCOL_VERSION {
        return None;
    }
    let edge = RunEdge::ALL
        .iter()
        .copied()
        .find(|edge| request.op() == Some(edge.op()))?;
    Some(match request.body().len() {
        1 => Ok(edge),
        _ => Err(ErrorReply::new(
            ErrorCode::InvalidArgument,
            format!("the request {} has no field", edge.op()),
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::{mark_request, RunEdge, RunMarked};
    use crate::proto::{decode_request, Decoded};

    #[test]
    fn a_mark_reads_as_its_edge() {
        let frame = br#"{"v":1,"body":{"op":"run_start"}}"#;
        assert_eq!(RunEdge::Start.request(), frame);
        let Ok(Decoded::Unknown(request)) = decode_request(frame) else {
            panic!("a mark request is in the request table of protocol v1");
        };
        assert_eq!(mark_request(&request), Some(Ok(RunEdge::Start)));
        let marked = RunMarked {
            edge: RunEdge::Start,
            stored: true,
        };
        let reply = br#"{"v":1,"body":{"kind":"run_marked","edge":"start","stored":true}}"#;
        assert_eq!(marked.encode(), reply);
        assert_eq!(RunMarked::decode(reply), Some(marked));
    }
}
