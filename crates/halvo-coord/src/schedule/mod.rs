//! The schedule of the coordinator (contracts I5.1 to I5.5, I5.9, I5.10): the slots of a
//! recurring workflow, what an open slot does, the holds, the start with its ordered steps
//! and its queue, and what one tick raises or starts. The functions `holds`, `start_step` and
//! `tick_step` take values and return values; `drive` is the one function of this module that
//! calls a port.

mod hold;
mod slot;

pub use hold::{holds, params, Hold};
pub use slot::{current_slot, finished, next_slot, slot_due, slot_open};
