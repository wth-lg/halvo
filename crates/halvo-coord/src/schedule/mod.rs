//! The schedule of the coordinator (contracts I5.1 to I5.5, I5.9, I5.10): the slots of a
//! recurring workflow, what an open slot does, the holds, the start with its ordered steps
//! and its queue, and what one tick raises or starts. The functions `holds`, `start_step` and
//! `tick_step` take values and return values.

mod board;
mod drive;
mod hold;
mod queue;
mod slot;
mod start;
mod tick;

pub use board::{Board, Dying, LiveRun, Member, Owed, Owing, Stop};
pub use drive::{drive, drive_with, stop_run, Answer, Ended, Halt, Move, Report, Served};
pub use drive::{drop_sessions, pause_owed, refused_active, set_model};
pub(crate) use drive::{hold_refusals_at, item_closed, owed_of_kind, owed_ready};
pub(crate) use drive::{read_back, resync, seq_refusal};
pub use hold::{held_by_events, kept_off, refusal_unstored, refused_runs};
pub use hold::{holds, Hold};
pub use queue::{Queue, Waiting};
pub use slot::{clean, current_slot, finished, next_slot, slot_due, slot_open};
pub use start::{
    active_state, compiles, needs_model, needs_person, start_step, Around, Clear, Go, StartStep,
    Verdict,
};
pub use tick::{gives_active, tick_plan, tick_step, wants_active, Ask, Plan};
pub(crate) use tick::{never_allowed, rests};
pub use tick::{switch_on_state, wants_paused};
