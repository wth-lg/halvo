//! The Board: what a run reports, kept by the supervisor and read by the timed
//! run's report and status. Every text on it is a fixed kind plus an OS code.

use halvo_control::onboarding::{GrantState, Permission};
use halvo_control::proto::{self, PasswordGuard, Platform, SourceName};

use super::counters::Snapshot;
use super::report::FailedSource;
use crate::source::{CaptureError, HealthReason, SourceCounters, SourceState, StopCause};

/// The most bytes that the Board holds, its fixed parts and its heap together:
/// the response cap (contracts.md 0.8, docs/baseline/ledger.d/daemon-2.txt).
pub const BOARD_MAX_BYTES: usize = 262_144;

/// The capture state that follows the sources (contracts.md C2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capture {
    /// No source has been asked to start.
    Starting,
    /// Every source that the run wants runs.
    Running,
    /// Some sources that the run wants run, and some do not.
    Partial,
    /// No source that the run wants runs.
    Faulted,
    /// The supervisor stopped the sources while the drain fails.
    Stopped,
    /// Desired is running and the lease lapsed or was never claimed: no
    /// source runs (contracts.md C14 item 3).
    NoIndicator,
    /// A stop for capture off met a source that did not stop: from that
    /// answer on until the process ends (ruling PL-3).
    Pausing,
}

/// The drain as the supervisor reads it: failing by the owner's last report,
/// and the hold that the owner publishes in its snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Drain {
    pub failing: bool,
    pub hold: bool,
}

impl Drain {
    /// Whether the sources stop: while the drain fails or the owner holds.
    pub fn stops_the_sources(self) -> bool {
        self.failing || self.hold
    }
}

/// One source's line on the Board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
    pub name: SourceName,
    pub state: SourceState,
    pub unstopped: bool,
}

/// What the last life of the data directory lost, as the start read it in the
/// run marker: the trigger of the stop that ended that life and what it did
/// not store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastLife {
    pub kind: proto::StopKind,
    pub lost: proto::LostRecord,
}

/// What a run reports.
#[derive(Debug, Clone, PartialEq)]
pub struct Board {
    pub platform: Platform,
    pub snapshot: Option<Snapshot>,
    pub sources: Vec<SourceEntry>,
    pub enabled: Vec<SourceName>,
    pub capture: Capture,
    pub drain: Drain,
    pub permissions: Vec<(Permission, GrantState)>,
    pub granted_at_start: Option<Vec<Permission>>,
    pub figures: SourceCounters,
    pub failed: Vec<(SourceName, CaptureError)>,
    pub password_guard: PasswordGuard,
    /// The loss of the last life that the start read, until a delete of
    /// everything takes it back.
    pub last_life: Option<LastLife>,
}

impl Board {
    /// The Board of a run on `platform` before any source starts: every
    /// source of the system off, in the fixed order of its names.
    pub fn new(platform: Platform) -> Board {
        Board {
            platform,
            snapshot: None,
            sources: SourceName::of(platform)
                .iter()
                .map(|name| SourceEntry {
                    name: *name,
                    state: SourceState::Off,
                    unstopped: false,
                })
                .collect(),
            enabled: Vec::new(),
            capture: Capture::Starting,
            drain: Drain::default(),
            permissions: Vec::new(),
            granted_at_start: None,
            figures: SourceCounters::default(),
            failed: Vec::new(),
            password_guard: match platform {
                Platform::Macos => PasswordGuard::Os,
                Platform::Windows => PasswordGuard::Halvo,
            },
            last_life: None,
        }
    }

    /// Sets the line of `name`.
    pub fn set_source(&mut self, name: SourceName, state: &SourceState, unstopped: bool) {
        if let Some(entry) = self.sources.iter_mut().find(|entry| entry.name == name) {
            entry.state = state.clone();
            entry.unstopped = unstopped;
        }
    }

    /// Records the first failure of `name` in this run; a later failure of the
    /// same source keeps the first, so the Board holds one line per source.
    pub fn record_failure(&mut self, name: SourceName, error: CaptureError) {
        if !self.failed.iter().any(|(failed, _)| *failed == name) {
            self.failed.push((name, error));
        }
    }

    /// The `failed_source=` entries of the report, one per failed source, in
    /// the order of the failures.
    pub fn failed_sources(&self) -> Vec<FailedSource> {
        self.failed
            .iter()
            .map(|(name, error)| FailedSource {
                name: name.as_str(),
                error: error.to_string(),
            })
            .collect()
    }

    /// The capture state of a run that wants `wanted`, by the lines of the
    /// sources: off sources are outside the required set.
    pub fn capture_of(&self, wanted: &[SourceName]) -> Capture {
        if wanted.is_empty() {
            return Capture::Starting;
        }
        let required: Vec<&SourceEntry> = self
            .sources
            .iter()
            .filter(|entry| wanted.contains(&entry.name) && entry.state != SourceState::Off)
            .collect();
        let runs = |entry: &&&SourceEntry| {
            matches!(
                entry.state,
                SourceState::Running | SourceState::Impaired { .. } | SourceState::NotReported
            )
        };
        let running = required.iter().filter(runs).count();
        if required.is_empty() || running == required.len() {
            Capture::Running
        } else if running == 0 {
            Capture::Faulted
        } else {
            Capture::Partial
        }
    }

    /// The lines of control-2's status for every source of the system, in its
    /// fixed order, with the mark `unstopped`.
    pub fn source_lines(&self) -> Vec<proto::SourceLine> {
        self.sources
            .iter()
            .map(|entry| proto::SourceLine {
                name: entry.name,
                state: line_state(&entry.state),
                unstopped: entry.unstopped,
            })
            .collect()
    }

    /// The bytes that the Board holds: its fixed part and what its lists and
    /// texts hold on the heap.
    pub fn bytes(&self) -> usize {
        let errors: usize = self
            .failed
            .iter()
            .map(|(_, error)| error_bytes(error))
            .sum::<usize>()
            + self
                .sources
                .iter()
                .map(|entry| match &entry.state {
                    SourceState::Failed { error, .. } => error_bytes(error),
                    _ => 0,
                })
                .sum::<usize>();
        std::mem::size_of::<Board>()
            + self.sources.capacity() * std::mem::size_of::<SourceEntry>()
            + self.enabled.capacity() * std::mem::size_of::<SourceName>()
            + self.permissions.capacity() * std::mem::size_of::<(Permission, GrantState)>()
            + self.granted_at_start.as_ref().map_or(0, |granted| {
                granted.capacity() * std::mem::size_of::<Permission>()
            })
            + self.failed.capacity() * std::mem::size_of::<(SourceName, CaptureError)>()
            + errors
    }
}

fn error_bytes(error: &CaptureError) -> usize {
    match error {
        CaptureError::Start(text) => text.capacity(),
        _ => 0,
    }
}

/// The token of C2.3 for each reason of the source health, by an exhaustive
/// match.
pub const fn reason_token(reason: HealthReason) -> proto::Reason {
    match reason {
        HealthReason::Permission => proto::Reason::Permission,
        HealthReason::NoKeyEvents => proto::Reason::NoKeyEvents,
        HealthReason::SecureInputElsewhere => proto::Reason::SecureInputElsewhere,
        HealthReason::TitleReadErrors => proto::Reason::TitleReadErrors,
        HealthReason::StreamBlank => proto::Reason::StreamBlank,
        HealthReason::StreamSuspended => proto::Reason::StreamSuspended,
        HealthReason::ThreadEnded => proto::Reason::ThreadEnded,
        HealthReason::WriterGone => proto::Reason::WriterGone,
        HealthReason::ItemClosed => proto::Reason::ItemClosed,
        HealthReason::Timeout => proto::Reason::Timeout,
        HealthReason::OsError => proto::Reason::OsError,
        HealthReason::NotImplemented => proto::Reason::NotImplemented,
        HealthReason::RunLoopStalled => proto::Reason::RunLoopStalled,
    }
}

fn line_state(state: &SourceState) -> proto::SourceState {
    match state {
        SourceState::NotReported | SourceState::Running => proto::SourceState::Running,
        SourceState::Impaired { reason } => proto::SourceState::Impaired {
            reason: reason_token(*reason),
            error: None,
        },
        SourceState::Failed { reason, .. } => proto::SourceState::Failed {
            reason: reason_token(*reason),
            error: None,
        },
        SourceState::Stopped(cause) => proto::SourceState::Stopped {
            by_user: *cause == StopCause::User,
        },
        SourceState::Off => proto::SourceState::Off,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::daemon::counters::Counters;
    use crate::daemon::report::TimedRunReport;
    use crate::daemon::SecureInput;

    fn every_reason() -> [HealthReason; 13] {
        [
            HealthReason::Permission,
            HealthReason::NoKeyEvents,
            HealthReason::SecureInputElsewhere,
            HealthReason::TitleReadErrors,
            HealthReason::StreamBlank,
            HealthReason::StreamSuspended,
            HealthReason::ThreadEnded,
            HealthReason::WriterGone,
            HealthReason::ItemClosed,
            HealthReason::Timeout,
            HealthReason::OsError,
            HealthReason::NotImplemented,
            HealthReason::RunLoopStalled,
        ]
    }

    #[test]
    fn the_report_lines_come_from_the_board() {
        let mut board = Board::new(Platform::Macos);
        let mut snapshot = Counters::default().snapshot(0, false);
        snapshot.discarded_by_barrier = 7;
        board.snapshot = Some(snapshot);
        board.record_failure(SourceName::Foreground, CaptureError::ThreadEnded);
        board.record_failure(
            SourceName::Foreground,
            CaptureError::Timeout {
                stage: "later",
                ms: 1,
            },
        );
        let report: TimedRunReport = crate::daemon::report_of_board(
            &board,
            &crate::sink::channel(1).0.counts(),
            SecureInput::DoNotHold,
        );
        let text = report.render();
        assert!(text.contains("\nfailed_sources=1\n"), "{text}");
        assert!(
            text.contains(
                "\nfailed_source=foreground: the capture thread had ended before the source was shut down\n"
            ),
            "{text}"
        );
        assert!(text.contains("\nevents_discarded=7\n"), "{text}");
        assert!(text.contains("\nsecure_input_keystrokes=0\n"), "{text}");
    }

    #[test]
    fn the_board_holds_no_unbounded_text() {
        let mut board = Board::new(Platform::Windows);
        board.snapshot = Some(Counters::default().snapshot(u64::MAX, true));
        for name in SourceName::WINDOWS {
            for _ in 0..10_000 {
                board.record_failure(
                    name,
                    CaptureError::Start("the capture source failed to start".to_owned()),
                );
            }
            board.set_source(
                name,
                &SourceState::Failed {
                    reason: HealthReason::OsError,
                    error: CaptureError::Start("the capture source failed to start".to_owned()),
                },
                true,
            );
        }
        board.enabled = SourceName::WINDOWS.to_vec();
        board.permissions = Permission::R1_MACOS
            .iter()
            .map(|permission| (*permission, GrantState::NotGranted))
            .collect();
        board.granted_at_start = Some(Permission::R1_MACOS.to_vec());
        assert_eq!(board.failed.len(), SourceName::WINDOWS.len());
        assert_eq!(board.sources.len(), SourceName::WINDOWS.len());
        assert!(
            board.bytes() <= BOARD_MAX_BYTES,
            "the Board holds {} bytes",
            board.bytes()
        );
        assert!(
            board.bytes() < 8 * 1024,
            "the Board holds {} bytes",
            board.bytes()
        );
        let lines = board.source_lines();
        assert_eq!(lines.len(), 6);
        let _ = Duration::ZERO;
    }

    #[test]
    fn each_reason_of_the_source_health_prints_its_token() {
        let tokens = [
            "permission",
            "no_key_events",
            "secure_input_elsewhere",
            "title_read_errors",
            "stream_blank",
            "stream_suspended",
            "thread_ended",
            "writer_gone",
            "item_closed",
            "timeout",
            "os_error",
            "not_implemented",
            "run_loop_stalled",
        ];
        for (reason, token) in every_reason().into_iter().zip(tokens) {
            let mut board = Board::new(Platform::Macos);
            board.set_source(
                SourceName::Input,
                &SourceState::Failed {
                    reason,
                    error: CaptureError::ThreadEnded,
                },
                false,
            );
            let line = &board.source_lines()[2];
            assert_eq!(
                line.state.reason().map(|reason| reason.as_str()),
                Some(token)
            );
            let text = serde_json::to_string(line).unwrap_or_default();
            assert!(text.contains(&format!("\"{token}\"")), "{reason:?}: {text}");
        }
    }

    #[test]
    fn the_board_lists_every_source_of_the_system_in_its_fixed_order() {
        for platform in [Platform::Macos, Platform::Windows] {
            let board = Board::new(platform);
            let names: Vec<SourceName> =
                board.source_lines().iter().map(|line| line.name).collect();
            assert_eq!(names, SourceName::of(platform), "{platform:?}");
            assert!(board
                .source_lines()
                .iter()
                .all(|line| line.state == proto::SourceState::Off));
        }
        assert_eq!(
            Board::new(Platform::Macos).password_guard,
            PasswordGuard::Os
        );
    }

    #[test]
    fn the_capture_state_follows_the_sources() {
        let wanted = [SourceName::Foreground, SourceName::Input];
        let mut board = Board::new(Platform::Macos);
        let failed = SourceState::Failed {
            reason: HealthReason::OsError,
            error: CaptureError::ThreadEnded,
        };
        board.set_source(SourceName::Foreground, &failed, false);
        board.set_source(SourceName::Input, &failed, false);
        assert_eq!(board.capture_of(&wanted), Capture::Faulted);
        board.set_source(SourceName::Input, &SourceState::Running, false);
        assert_eq!(board.capture_of(&wanted), Capture::Partial);
        board.set_source(SourceName::Foreground, &SourceState::Running, false);
        assert_eq!(board.capture_of(&wanted), Capture::Running);
    }
}
