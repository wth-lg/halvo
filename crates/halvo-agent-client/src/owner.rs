use halvo_schema::executable::body::{
    CheckpointsReply, CoordinatorCall, CoordinatorReply, ListCheckpoints,
};
use halvo_schema::executable::text::{Null, RunId};
use halvo_schema::request::{decode_reply, CoordinatorWrite, ReplyResult};
use halvo_schema::RequestId;

use crate::api::ListFailed;
use crate::supervisor::Inner;
use crate::sys;

/// A request id from 16 bytes of the system's generator.
pub(crate) fn request_id() -> Option<RequestId> {
    sys::random16().ok().map(RequestId::from_random)
}

/// What one send of a coordinator body met.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Ack {
    /// The owner's reply carries the body's id and the reply that the body's call answers with.
    Acknowledged,
    /// A reply arrived and `decode_reply` refused it.
    Refused,
    /// No reply that acknowledges the body: a link error, a reply with another id, an error reply, or a reply of
    /// another form.
    Unanswered,
}

/// Why one send of a coordinator body brought no reply that answers it.
enum Missed {
    /// A reply arrived and `decode_reply` refused it.
    Refused,
    /// A link error, a reply with another id, an error reply, or a reply of another form.
    Unanswered,
}

/// Sends one coordinator body over the owner link under the id `id` and says what it met.
pub(crate) fn acknowledged(inner: &Inner, id: &RequestId, call: CoordinatorCall) -> Ack {
    match exchange(inner, id, call) {
        Ok(_) => Ack::Acknowledged,
        Err(Missed::Refused) => Ack::Refused,
        Err(Missed::Unanswered) => Ack::Unanswered,
    }
}

/// Reads one page of the list of checkpoints: `list_checkpoints` with `workflow` and `agent` null and `after`, under
/// a new request id; the page as the owner gave it, or `ListFailed` when the reply is not the list answer.
pub(crate) fn checkpoints(
    inner: &Inner,
    after: Option<RunId>,
) -> Result<CheckpointsReply, ListFailed> {
    let id = request_id().ok_or(ListFailed)?;
    let call = CoordinatorCall::ListCheckpoints(ListCheckpoints {
        workflow: Null,
        agent: Null,
        after,
    });
    match exchange(inner, &id, call) {
        Ok(CoordinatorReply::Checkpoints(page)) => Ok(page),
        _ => Err(ListFailed),
    }
}

/// The one exchange of a coordinator body over the owner link, under the id `id`: it sends the body, reads the
/// reply with `decode_reply`, which refuses a reply of more than `REPLY_MAX` bytes as given before it reads
/// anything, compares the id, and gives the value only when `CoordinatorReply::parse` reads it and it has the form
/// that the body's call answers with.
fn exchange(
    inner: &Inner,
    id: &RequestId,
    call: CoordinatorCall,
) -> Result<CoordinatorReply, Missed> {
    let write = CoordinatorWrite::new(&inner.role, id.clone(), call);
    let bytes = write.to_bytes().map_err(|_| Missed::Unanswered)?;
    let reply = inner
        .parts
        .link
        .request(&bytes)
        .map_err(|_| Missed::Unanswered)?;
    let reply = decode_reply(&reply).map_err(|_| Missed::Refused)?;
    if reply.id.as_ref() != Some(id) {
        return Err(Missed::Unanswered);
    }
    let Ok(ReplyResult::Raw(raw)) = reply.outcome else {
        return Err(Missed::Unanswered);
    };
    match CoordinatorReply::parse(raw.get().as_bytes()) {
        Ok(value) if value.answers(write.body()) => Ok(value),
        _ => Err(Missed::Unanswered),
    }
}
