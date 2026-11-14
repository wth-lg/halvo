use std::time::{Duration, Instant};

/// Where a press of a hosted case is timed from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum From {
    /// The show's report on screen.
    Report,
    /// The time at which the press before it ran on the dialog's thread.
    Previous,
}

/// When a press timed `at` milliseconds from `from` is due: from the report, or from the time
/// `previous` at which the press before it ran on the dialog's thread, the report when there was
/// none.
pub fn due(report: Instant, previous: Option<Instant>, from: From, at: u64) -> Instant {
    let base = match from {
        From::Report => report,
        From::Previous => previous.unwrap_or(report),
    };
    base + Duration::from_millis(at)
}

/// The failure of a press timed from the press before it that must fall inside that press's
/// hold: the line "the schedule slipped" when the press ran `arm` or more after the press before
/// it, by their own times on the dialog's thread.
pub fn slipped(previous: Instant, ran: Instant, arm: Duration) -> Option<String> {
    let gap = ran.saturating_duration_since(previous);
    (gap >= arm).then(|| {
        format!(
            "the schedule slipped: the press ran {} ms after the press before it, and arm_ms is {}",
            gap.as_millis(),
            arm.as_millis()
        )
    })
}
