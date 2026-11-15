/// What the description of the main run loop says of the dialog's repeating reading (ruling
/// RD-107, Addition 3, point 5): the timer whose callout is `[HalvoDialogHold reading:]`, read
/// alone, for a show with no report after a second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Timer {
    /// No such timer is in the run loop: it was never added, or it was invalidated.
    Absent,
    /// A valid timer whose next fire date lies `next` seconds from the read: one that has fired
    /// has a next fire date within one interval of the read, and one that never fired keeps the
    /// date of its first firing, which lies behind the read.
    Valid { next: f64, run: bool },
    /// An entry that the read cannot take apart.
    Unread,
}

const CALLOUT: &str = "[HalvoDialogHold reading:]";

/// The first entry of the dialog's timer in `text`, the description of the main run loop, for
/// a timer of `interval` seconds that was added at least two intervals before the read.
pub fn timer(text: &str, interval: f64) -> Timer {
    let Some(entry) = text.lines().find(|line| line.contains(CALLOUT)) else {
        return Timer::Absent;
    };
    let valid = entry.contains("valid = Yes");
    let next = entry
        .split_once("next fire date = ")
        .and_then(|(_, rest)| rest.split_once(" ("))
        .and_then(|(_, rest)| rest.split_once(" @"))
        .and_then(|(next, _)| next.trim().parse::<f64>().ok());
    match (valid, next) {
        (true, Some(next)) => Timer::Valid {
            next,
            run: next > -2.0 * interval,
        },
        (false, _) if entry.contains("valid = No") => Timer::Absent,
        _ => Timer::Unread,
    }
}

/// The words of a read of the timer.
pub fn words(timer: Timer) -> String {
    match timer {
        Timer::Absent => "no timer of the dialog's reading in the main run loop".into(),
        Timer::Valid { next, run: true } => {
            format!(
                "the timer of the dialog's reading has run (its next firing {next:.3} s from now)"
            )
        }
        Timer::Valid { next, run: false } => format!(
            "the timer of the dialog's reading has not run (its first firing was {:.3} s ago)",
            -next
        ),
        Timer::Unread => "an entry of the dialog's timer that this read cannot take apart".into(),
    }
}
