//! The identifier core of the workflow run. The episode step reads the records of one call, masks every window
//! title first, orders the records by their instant, drops what lies between run marks, cuts episodes and asks a
//! stage for each episode's category. The recurrence step proposes a workflow for a key that `K` bounded summaries
//! hold. Stage A names an episode's category by the words of its titles and of its sites. Stage B, the worker and
//! the `propose` command belong to later units.
#![forbid(unsafe_code)]

pub mod episode;
pub mod recurrence;
pub mod stage;

pub use episode::{episode_step, Call, Outcome, Stage, FLICKER, GAP, LOOK_BACK, RESOLUTION};
pub use recurrence::{Proposer, K};
pub use stage::StageA;
