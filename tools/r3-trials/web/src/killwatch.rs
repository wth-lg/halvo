//! The row `relay_port_free` (the coordinator's ruling CP-145 (1)): when the driver is killed, the
//! browser is gone before the relay's port is free, and nothing of the run is left (A13.7). The
//! row promises no seconds; a kill reads ok, a miss with its cause, or open, at any load.
//!
//! The watch reads, in each pass, first whether a process of the run still runs and then whether
//! the port can be bound; when the port can be bound and a process was running, it reads the
//! processes once more. It ends nothing while it watches: at `REPORT_AT` it reports what still
//! runs and watches on, and only at `CAP` does the row end the run's processes itself.

use std::time::Duration;

/// How long the watch may last from the kill before the row ends the run's processes itself:
/// ten times the watch that the row had before (fixed by CP-145 (1) (d) before any run).
pub const CAP: Duration = Duration::from_secs(300);

/// When the watch reports what still runs, once, and watches on.
pub const REPORT_AT: Duration = Duration::from_secs(30);

/// The wait between two passes.
pub const PASS_WAIT: Duration = Duration::from_millis(2);

/// What one pass read, in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    /// A process of the run ran; the port was taken.
    RunningTaken,
    /// A process of the run ran; the port was free; at the read after it a process still ran.
    FreeWhileRunning,
    /// A process of the run ran; the port was free; at the read after it none ran.
    FreeThenGone,
    /// No process of the run ran; the port was taken.
    GoneTaken,
    /// No process of the run ran; the port was already free.
    GoneFree,
    /// The process table could not be read: no end of the run is shown, and the watch goes on.
    Unread,
}

impl Pass {
    /// Whether this pass shows the order (or that it cannot be known), so the watch is done.
    pub fn settles(self) -> bool {
        !matches!(self, Pass::RunningTaken | Pass::Unread)
    }

    /// Whether this pass read the run gone.
    pub fn gone(self) -> bool {
        matches!(self, Pass::FreeThenGone | Pass::GoneTaken | Pass::GoneFree)
    }
}

/// What one kill reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A pass read no process of the run and then the port still taken, and the row's own end
    /// after the watch found nothing to end and left nothing.
    Ok,
    /// A miss: `free_first`, `tree_left` or `left_behind`.
    Miss(&'static str),
    /// Open: `one_pass`, both events first seen in one pass, so their order is not known.
    Open(&'static str),
}

impl Outcome {
    /// The word of the run line's `result=`.
    pub fn result(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Miss(_) => "miss",
            Outcome::Open(_) => "open",
        }
    }

    /// The word of the run line's `cause=`.
    pub fn cause(self) -> &'static str {
        match self {
            Outcome::Ok => "-",
            Outcome::Miss(c) | Outcome::Open(c) => c,
        }
    }
}

/// What a kill reads from the passes of its watch, in order, and the row's own end after it
/// (`left_before`: what that end found; `left_after`: what it left). Passes that end with a
/// process of the run still running mean the watch reached its cap; when they end with a table
/// that could not be read, the kill is open with `table_unread`, never ok.
pub fn decide(passes: &[Pass], left_before: usize, left_after: usize) -> Outcome {
    let left = left_before > 0 || left_after > 0;
    for p in passes {
        let open_or_ok = match p {
            Pass::RunningTaken | Pass::Unread => continue,
            Pass::FreeWhileRunning => return Outcome::Miss("free_first"),
            Pass::FreeThenGone | Pass::GoneFree => Outcome::Open("one_pass"),
            Pass::GoneTaken => Outcome::Ok,
        };
        return if left {
            Outcome::Miss("left_behind")
        } else {
            open_or_ok
        };
    }
    if passes.last() == Some(&Pass::Unread) {
        return Outcome::Open("table_unread");
    }
    Outcome::Miss("tree_left")
}

/// What a call of the row prints in place of its cell line (CP-145 (1) (e)): `watch_open` when no
/// kill of the call was a miss and one was open; `None`, the cell line, otherwise (a call with a
/// miss prints its cell line, whose ok count is then below its runs).
pub fn call_notrun(misses: usize, opens: usize) -> Option<&'static str> {
    (misses == 0 && opens > 0).then_some("watch_open")
}

/// What the watch read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Watched {
    pub passes: Vec<Pass>,
    /// Milliseconds from the kill to the pass that read the run gone.
    pub gone_ms: Option<u128>,
    /// Milliseconds from the kill to the first read of the port free (read on after the order is
    /// shown until it is free or `REPORT_AT` has passed).
    pub free_ms: Option<u128>,
    /// The longest gap between the starts of two passes.
    pub max_gap_ms: u128,
    /// Whether the watch reached `CAP` with a process of the run running.
    pub capped: bool,
}

/// The readers of one watch: whether a process of the run runs (`None`: the process table could
/// not be read), whether the port can be bound,
/// the milliseconds since the kill, the wait between passes, and the report of what still runs
/// (called at `REPORT_AT` and at `CAP` with the milliseconds).
pub struct Readers<'a> {
    pub running: &'a mut dyn FnMut() -> Option<bool>,
    pub free: &'a mut dyn FnMut() -> bool,
    pub now_ms: &'a mut dyn FnMut() -> u128,
    pub pause: &'a mut dyn FnMut(),
    pub report: &'a mut dyn FnMut(u128),
}

/// The watch of one kill (see the module). It ends nothing.
pub fn watch(r: Readers<'_>, cap: Duration, report_at: Duration) -> Watched {
    let (cap, report_at) = (cap.as_millis(), report_at.as_millis());
    let mut w = Watched::default();
    let mut reported = false;
    let mut last: Option<u128> = None;
    loop {
        let start = (r.now_ms)();
        if let Some(l) = last {
            w.max_gap_ms = w.max_gap_ms.max(start - l);
        }
        last = Some(start);
        let pass = match (r.running)() {
            None => Pass::Unread,
            Some(true) => {
                if (r.free)() {
                    // a second read that cannot be read shows nothing: the pass is unread
                    match (r.running)() {
                        Some(true) => Pass::FreeWhileRunning,
                        Some(false) => Pass::FreeThenGone,
                        None => Pass::Unread,
                    }
                } else {
                    Pass::RunningTaken
                }
            }
            Some(false) => {
                if (r.free)() {
                    Pass::GoneFree
                } else {
                    Pass::GoneTaken
                }
            }
        };
        let at = (r.now_ms)();
        if matches!(
            pass,
            Pass::FreeWhileRunning | Pass::FreeThenGone | Pass::GoneFree
        ) {
            w.free_ms.get_or_insert(at);
        }
        if pass.gone() {
            w.gone_ms = Some(at);
        }
        w.passes.push(pass);
        if pass.settles() {
            break;
        }
        if !reported && at >= report_at {
            (r.report)(at);
            reported = true;
        }
        if at >= cap {
            (r.report)(at);
            w.capped = true;
            return w;
        }
        (r.pause)();
    }
    // the order is shown: read the port on, for the line's free_ms, until it is free or
    // `report_at` from the kill has passed
    while w.free_ms.is_none() {
        let at = (r.now_ms)();
        if at >= report_at {
            break;
        }
        if (r.free)() {
            w.free_ms = Some((r.now_ms)());
            break;
        }
        (r.pause)();
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn a_kill_reads_ok_a_miss_with_its_cause_or_open() {
        use Pass::*;
        let cases: [(&str, &[Pass], usize, usize, Outcome); 13] = [
            (
                "an unread pass, then gone",
                &[Unread, GoneTaken],
                0,
                0,
                Outcome::Ok,
            ),
            (
                "still unread at the cap",
                &[RunningTaken, Unread],
                0,
                0,
                Outcome::Open("table_unread"),
            ),
            (
                "tree gone, port taken",
                &[RunningTaken, GoneTaken],
                0,
                0,
                Outcome::Ok,
            ),
            (
                "tree gone at the first pass",
                &[GoneTaken],
                0,
                0,
                Outcome::Ok,
            ),
            (
                "port free while a process ran",
                &[RunningTaken, FreeWhileRunning],
                0,
                0,
                Outcome::Miss("free_first"),
            ),
            (
                "a process still runs at the cap",
                &[RunningTaken, RunningTaken],
                0,
                0,
                Outcome::Miss("tree_left"),
            ),
            ("no pass at all", &[], 0, 0, Outcome::Miss("tree_left")),
            (
                "the row's end found one",
                &[GoneTaken],
                1,
                0,
                Outcome::Miss("left_behind"),
            ),
            (
                "the row's end left one",
                &[GoneTaken],
                0,
                1,
                Outcome::Miss("left_behind"),
            ),
            (
                "gone with the port already free",
                &[RunningTaken, GoneFree],
                0,
                0,
                Outcome::Open("one_pass"),
            ),
            (
                "free, then gone in one pass",
                &[FreeThenGone],
                0,
                0,
                Outcome::Open("one_pass"),
            ),
            (
                "one pass, and the end found one",
                &[FreeThenGone],
                2,
                0,
                Outcome::Miss("left_behind"),
            ),
            (
                "a free port while running wins over a left count",
                &[FreeWhileRunning],
                1,
                1,
                Outcome::Miss("free_first"),
            ),
        ];
        let wrong: Vec<_> = cases
            .iter()
            .filter(|(_, p, b, a, want)| decide(p, *b, *a) != *want)
            .map(|(name, p, b, a, want)| format!("{name}: {:?} want {want:?}", decide(p, *b, *a)))
            .collect();
        assert!(wrong.is_empty(), "{wrong:?}");
    }

    #[test]
    fn a_call_with_an_open_kill_and_no_miss_prints_watch_open() {
        assert_eq!(call_notrun(0, 1), Some("watch_open"));
        assert_eq!(call_notrun(1, 1), None);
        assert_eq!(call_notrun(2, 0), None);
        assert_eq!(call_notrun(0, 0), None);
    }

    /// A fake run: the run's processes stop at `gone_at`, the port is free from `free_at`; each
    /// pause is `PASS_WAIT`, each read takes no time. Records the order of the reads.
    fn fake(
        gone_at: u128,
        free_at: u128,
        cap: Duration,
    ) -> (Watched, Vec<&'static str>, Vec<u128>) {
        let clock = RefCell::new(0u128);
        let log = RefCell::new(Vec::new());
        let reports = RefCell::new(Vec::new());
        let w = watch(
            Readers {
                running: &mut || {
                    log.borrow_mut().push("tree");
                    Some(*clock.borrow() < gone_at)
                },
                free: &mut || {
                    log.borrow_mut().push("port");
                    *clock.borrow() >= free_at
                },
                now_ms: &mut || *clock.borrow(),
                pause: &mut || *clock.borrow_mut() += PASS_WAIT.as_millis(),
                report: &mut |at| reports.borrow_mut().push(at),
            },
            cap,
            REPORT_AT,
        );
        (w, log.into_inner(), reports.into_inner())
    }

    #[test]
    fn each_pass_reads_the_tree_before_the_port_and_the_tree_again_after_a_free_port() {
        let (w, log, _) = fake(1_000, 10, CAP);
        assert_eq!(&log[..2], ["tree", "port"], "{log:?}");
        assert_eq!(w.passes.last(), Some(&Pass::FreeWhileRunning));
        assert_eq!(&log[log.len() - 3..], ["tree", "port", "tree"]);
        assert_eq!(decide(&w.passes, 0, 0), Outcome::Miss("free_first"));
    }

    #[test]
    fn the_watch_waits_past_the_old_30_seconds_and_reports_what_runs_then() {
        // the run ends at 45 s, the port is never free: ok, with the report at 30 s
        let (w, _, reports) = fake(45_000, u128::MAX, CAP);
        assert_eq!(w.gone_ms, Some(45_000));
        assert!(!w.capped);
        assert_eq!(reports, [30_000]);
        assert_eq!(decide(&w.passes, 0, 0), Outcome::Ok);
        assert_eq!(w.max_gap_ms, PASS_WAIT.as_millis());
    }

    #[test]
    fn a_run_that_still_runs_at_the_cap_is_a_miss_with_a_second_report() {
        let (w, _, reports) = fake(u128::MAX, u128::MAX, CAP);
        assert!(w.capped);
        assert_eq!(reports, [30_000, 300_000]);
        assert_eq!(decide(&w.passes, 0, 0), Outcome::Miss("tree_left"));
        assert_eq!(CAP, Duration::from_secs(300));
    }

    #[test]
    fn after_the_order_the_port_is_read_on_for_free_ms() {
        let (w, _, _) = fake(100, 2_000, CAP);
        assert_eq!(w.gone_ms, Some(100));
        assert_eq!(w.free_ms, Some(2_000));
        assert_eq!(decide(&w.passes, 0, 0), Outcome::Ok);
    }
}
