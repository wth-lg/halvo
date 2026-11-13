use halvo_schema::executable::body::{CoordinatorCall, CoordinatorReply};
use halvo_schema::request::{decode_reply, CoordinatorWrite, ReplyResult};
use halvo_schema::RequestId;

use crate::supervisor::Inner;
use crate::sys;

/// A request id from 16 bytes of the system's generator.
pub(crate) fn request_id() -> Option<RequestId> {
    sys::random16().ok().map(RequestId::from_random)
}

/// Sends one coordinator body over the owner link under the id `id`; true only when the owner's reply carries
/// that id and the reply that the body's call answers with.
pub(crate) fn acknowledged(inner: &Inner, id: &RequestId, call: CoordinatorCall) -> bool {
    let write = CoordinatorWrite::new(&inner.role, id.clone(), call);
    let Ok(bytes) = write.to_bytes() else {
        return false;
    };
    let Ok(reply) = inner.parts.link.request(&bytes) else {
        return false;
    };
    let Ok(reply) = decode_reply(&reply) else {
        return false;
    };
    if reply.id.as_ref() != Some(id) {
        return false;
    }
    match reply.outcome {
        Ok(ReplyResult::Raw(raw)) => {
            CoordinatorReply::parse(raw.get().as_bytes()).is_ok_and(|r| r.answers(write.body()))
        }
        _ => false,
    }
}
