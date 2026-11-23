#![forbid(unsafe_code)]
//! The connect adapters of the agent-setup run (A11.2): the trait `Mail`, the trait `Calendar`, and `Graph`, one
//! Graph-shaped adapter for both. A token is passed to each call and kept by no value of this crate. An operation
//! that reads a kept list runs once per item of it, and the adapter offers no request beyond its operations.

mod address;
mod answer;
mod calendar;
mod error;
mod graph;
mod list;
mod mail;
pub mod model;
mod token;

pub use address::{reply_senders, same_address, Senders, SendersError};
pub use calendar::{Calendar, Event, EventCounts, EventToDelete, Events, RunDay};
pub use error::{AnswerFault, BuildError, CallError, Effect, NotMade};
#[cfg(feature = "loopback")]
pub use graph::LOOPBACK_MARKER;
pub use graph::{Graph, Route};
pub use mail::{
    Answered, Draft, DraftFailure, DraftStop, DraftToSend, Drafted, Drafting, Drafts, ListStop,
    Mail, MailCounts, Marked, Message, Messages, ReplyRule, Sent, Unfinished, Unsent, UnsentDraft,
};
pub use token::{Token, TokenError};
