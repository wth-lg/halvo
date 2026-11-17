//! The loop that a run waits in: on macOS the process main thread's run loop,
//! turned in slices of at most one tick.

use std::time::Duration;

/// One firing of the supervisor's timer: the longest slice that the loop is
/// turned before the supervisor reads its inputs again.
pub const TICK: Duration = Duration::from_millis(100);

/// The longest that the self-check allows between the queue of a block on the
/// main run loop and its run.
pub const SELF_CHECK_BOUND: Duration = Duration::from_millis(250);

/// How long the self-check waits for its block.
pub const SELF_CHECK_WINDOW: Duration = Duration::from_secs(2);

/// The loop that the supervisor turns while a run waits.
pub trait MainLoop {
    /// Turns the loop for at most `wait`: on macOS the main run loop runs its
    /// sources and blocks for that long.
    fn turn(&mut self, wait: Duration);

    /// The loop's monotonic time.
    fn now(&self) -> Duration;
}

/// The slice that a wait until `until` turns next at `now`: `None` once
/// `until` has passed, else at most [`TICK`].
pub fn slice(now: Duration, until: Duration) -> Option<Duration> {
    let left = until.checked_sub(now).filter(|left| !left.is_zero())?;
    Some(left.min(TICK))
}

/// The self-check's line and status: `run_loop_block_ms=<n>` with the time
/// from the queue of the block to its run, or the window when it never ran,
/// and 0 when the block ran within [`SELF_CHECK_BOUND`], else 1.
pub fn self_check_answer(ran_after: Option<Duration>) -> (String, i32) {
    let shown = ran_after.unwrap_or(SELF_CHECK_WINDOW);
    let status = match ran_after {
        Some(after) if after <= SELF_CHECK_BOUND => halvo_control::exit::CLEAN,
        Some(_) | None => halvo_control::exit::ERROR,
    };
    (format!("run_loop_block_ms={}", shown.as_millis()), status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slice_is_at_most_one_tick_and_none_after_the_end() {
        let at = Duration::from_secs;
        assert_eq!(slice(at(0), at(5)), Some(TICK));
        assert_eq!(
            slice(Duration::from_millis(4_950), at(5)),
            Some(Duration::from_millis(50))
        );
        assert_eq!(slice(at(5), at(5)), None);
        assert_eq!(slice(at(6), at(5)), None);
    }

    #[test]
    fn the_self_check_passes_a_block_that_ran_within_250_ms_only() {
        assert_eq!(
            self_check_answer(Some(Duration::from_millis(3))),
            ("run_loop_block_ms=3".to_owned(), 0)
        );
        assert_eq!(
            self_check_answer(Some(SELF_CHECK_BOUND)),
            ("run_loop_block_ms=250".to_owned(), 0)
        );
        assert_eq!(
            self_check_answer(Some(Duration::from_millis(251))),
            ("run_loop_block_ms=251".to_owned(), 1)
        );
        assert_eq!(
            self_check_answer(None),
            ("run_loop_block_ms=2000".to_owned(), 1)
        );
    }
}
