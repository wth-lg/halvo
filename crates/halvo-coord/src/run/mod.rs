//! The run of the coordinator (contracts I5.6 to I5.8, I5.10): the moves of a run with
//! `run_step`, the stop and its kill, and `drive`, which carries out the moves of a run on the
//! ports, the board of the schedule and the guard's table.

mod check;
mod delete;
mod drive;
mod recover;
mod step;

pub use check::check_due;
pub(crate) use check::read_replies;
pub use delete::{delete_step, forgotten, Gone};
pub use drive::{drive, send_cancel, stop_unstamped, Act, Done, StopBy};
pub use recover::{recover, recover_step, Earlier, Life, Recovery};
pub(crate) use recover::{kept_runs, kill_kept, write_not_stopped};
pub use step::{run_step, Ending, Next, Phase, RunMove, StopCause};
