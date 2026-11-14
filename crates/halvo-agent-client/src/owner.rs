use halvo_schema::executable::body::{CoordinatorCall, CoordinatorReply};
use halvo_schema::request::{decode_reply, CoordinatorWrite, ReplyResult};
use halvo_schema::RequestId;

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

/// Sends one coordinator body over the owner link under the id `id` and says what it met.
pub(crate) fn acknowledged(inner: &Inner, id: &RequestId, call: CoordinatorCall) -> Ack {
    let write = CoordinatorWrite::new(&inner.role, id.clone(), call);
    let Ok(bytes) = write.to_bytes() else {
        return Ack::Unanswered;
    };
    let Ok(reply) = inner.parts.link.request(&bytes) else {
        return Ack::Unanswered;
    };
    let Ok(reply) = decode_reply(&reply) else {
        return Ack::Refused;
    };
    if reply.id.as_ref() != Some(id) {
        return Ack::Unanswered;
    }
    match reply.outcome {
        Ok(ReplyResult::Raw(raw))
            if CoordinatorReply::parse(raw.get().as_bytes())
                .is_ok_and(|r| r.answers(write.body())) =>
        {
            Ack::Acknowledged
        }
        _ => Ack::Unanswered,
    }
}
