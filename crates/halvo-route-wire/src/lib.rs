//! The gateway request of contract T11 of the routing run: the body that a device sends in
//! `POST /v1/chat`, its limits, and the two answers that it reads back.
//!
//! The body holds the tier as a ceiling, an effort level, an output limit and messages of text, and
//! nothing else. `Body::parse` and `Body::to_bytes` read and write it, and `Answer::parse` reads a
//! success or a refusal. Every public type of this crate stands at its root.

mod answer;
mod body;
mod json;

pub use answer::{Answer, AnswerError, Reason};
pub use body::{Body, BodyError, Effort, Message, Role, WireTier};

/// The largest body in bytes, as read or written; a larger body is `BodyError::Size`.
pub const MAX_BODY_BYTES: usize = 131_072;

/// The largest output limit of a body; `max_output_tokens` runs from 1 to this, else `BodyError::Limit`.
pub const MAX_OUTPUT_TOKENS: u32 = 2_048;

/// The most messages of a body; a body holds 1 to this many, else `BodyError::Limit`.
pub const MAX_MESSAGES: usize = 64;
