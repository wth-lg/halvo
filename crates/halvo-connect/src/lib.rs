#![forbid(unsafe_code)]
//! The connect adapters of the agent-setup run (A11.2): the trait `Mail`, the trait `Calendar`, and `Graph`, one
//! Graph-shaped adapter for both. A token is passed to each call and kept by no value of this crate. An operation
//! that reads a kept list runs once per item of it, and the adapter offers no request beyond its operations.

mod address;
mod answer;
mod calendar;
mod error;
mod graph;
mod mail;
mod token;

pub use address::{reply_senders, same_address, SendersError};
pub use calendar::{Calendar, Event, EventToDelete, Events, RunDay};
pub use error::{AnswerFault, BuildError, CallError, Effect};
pub use graph::{Graph, Route};
pub use mail::{Draft, DraftToSend, Drafts, Mail, Message, Messages, ReplyRule};
pub use token::{Token, TokenError};
