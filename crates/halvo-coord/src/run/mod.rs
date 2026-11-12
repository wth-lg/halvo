//! The run of the coordinator (contracts I5.6 to I5.8, I5.10): the moves of a run with
//! `run_step`, the stop and its kill, and `drive`, which carries out the moves of a run on the
//! ports, the board of the schedule and the guard's table.

mod check;
mod delete;
mod drive;
mod recover;
mod step;

pub use check::check_due;
pub use delete::{delete_step, forgotten, Gone};
pub use drive::{drive, Act, Done, StopBy};
pub use recover::{recover, recover_step, Earlier, Life, Recovery};
pub use step::{run_step, Ending, Next, Phase, RunMove, StopCause};
