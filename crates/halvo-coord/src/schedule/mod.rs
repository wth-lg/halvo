//! The schedule of the coordinator (contracts I5.1 to I5.5, I5.9, I5.10): the slots of a
//! recurring workflow, what an open slot does, the holds, the start with its ordered steps
//! and its queue, and what one tick raises or starts. The functions `holds`, `start_step` and
//! `tick_step` take values and return values; `drive` is the one function of this module that
//! calls a port.

mod board;
mod drive;
mod hold;
mod queue;
mod slot;
mod start;
mod tick;

pub use board::{Board, Dying, LiveRun, Member, Owed, Owing, Stop};
pub use drive::{drive, stop_run, Answer, Ended, Move, Report, Served};
pub(crate) use drive::{item_closed, owed_ready, read_back, resync, seq_refusal};
pub use hold::{holds, Hold};
pub use queue::{Queue, Waiting};
pub use slot::{current_slot, finished, next_slot, slot_due, slot_open};
pub use start::{
    active_state, needs_model, needs_person, start_step, Around, Clear, Go, StartStep, Verdict,
};
pub use tick::{gives_active, tick_plan, tick_step, wants_active, Ask, Plan};
