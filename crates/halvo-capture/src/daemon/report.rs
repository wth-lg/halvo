//! The report of a bounded timed run, in the lines of contracts.md C5.1.

use crate::event::Modality;
use crate::sink::KindCounts;
use crate::source::{HeldSamples, SuppressedKeys};

/// A source that failed at its start, after it, or at its stop, with the
/// Display text of its error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedSource {
    pub name: &'static str,
    pub error: String,
}

/// The six lines of a run with `--hold-secure-input`, from the input source's
/// counters of its two phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HoldLines {
    pub released_keystrokes: u64,
    pub held_keystrokes: u64,
    pub suppressed: SuppressedKeys,
    pub held_samples: HeldSamples,
    pub window_pointer: u64,
    pub keys_typed: bool,
}

/// The result of a bounded timed run. It has no `Default`: every figure is
/// one that the run read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedRunReport {
    pub screen_frame: u64,
    pub keystroke: u64,
    pub pointer: u64,
    pub foreground_app: u64,
    pub window_title: u64,
    pub system_audio: u64,
    pub microphone: u64,
    pub camera: u64,
    pub secure_input_keystrokes: u64,
    pub segments_written: u64,
    pub events_received: u64,
    pub events_persisted: u64,
    pub events_discarded: u64,
    /// The sink's counts of each kind, in the order of [`Modality::ALL`].
    pub sink_counts: [KindCounts; 8],
    pub tap_disabled_timeout: u64,
    pub tap_disabled_user_input: u64,
    pub rss_peak_bytes: u64,
    pub failed_sources: Vec<FailedSource>,
    pub hold: Option<HoldLines>,
}

impl TimedRunReport {
    /// The count of `kind`.
    pub fn count(&self, kind: Modality) -> u64 {
        match kind {
            Modality::ScreenFrame => self.screen_frame,
            Modality::Keystroke => self.keystroke,
            Modality::Pointer => self.pointer,
            Modality::ForegroundApp => self.foreground_app,
            Modality::WindowTitle => self.window_title,
            Modality::SystemAudio => self.system_audio,
            Modality::Microphone => self.microphone,
            Modality::Camera => self.camera,
        }
    }

    /// The lines of contracts.md C5.1 in their order, each ended by LF.
    pub fn render(&self) -> String {
        let mut lines = Vec::new();
        for kind in Modality::ALL {
            lines.push(format!(
                "modality={} count={}",
                kind.as_str(),
                self.count(kind)
            ));
        }
        lines.push(format!(
            "secure_input_keystrokes={}",
            self.secure_input_keystrokes
        ));
        lines.push(format!("segments_written={}", self.segments_written));
        lines.push(format!("events_received={}", self.events_received));
        lines.push(format!("events_persisted={}", self.events_persisted));
        lines.push(format!("events_discarded={}", self.events_discarded));
        for (kind, counts) in Modality::ALL.into_iter().zip(self.sink_counts) {
            lines.push(format!(
                "sink_counts kind={} accepted={} dropped={} coalesced={} refused={}",
                kind.as_str(),
                counts.accepted,
                counts.dropped,
                counts.coalesced,
                counts.refused
            ));
        }
        lines.push(format!(
            "tap_disabled_timeout={}",
            self.tap_disabled_timeout
        ));
        lines.push(format!(
            "tap_disabled_user_input={}",
            self.tap_disabled_user_input
        ));
        lines.push(format!("rss_peak_bytes={}", self.rss_peak_bytes));
        lines.push(format!("failed_sources={}", self.failed_sources.len()));
        for failed in &self.failed_sources {
            lines.push(format!("failed_source={}: {}", failed.name, failed.error));
        }
        if let Some(hold) = &self.hold {
            lines.push(format!(
                "secure_released_keystrokes={}",
                hold.released_keystrokes
            ));
            lines.push(format!("secure_held_keystrokes={}", hold.held_keystrokes));
            lines.push(format!(
                "secure_held_suppressed={}/{}/{}",
                hold.suppressed.down, hold.suppressed.up, hold.suppressed.modifier
            ));
            lines.push(format!(
                "secure_input_held={}/{}",
                hold.held_samples.held, hold.held_samples.taken
            ));
            lines.push(format!("secure_window_pointer={}", hold.window_pointer));
            lines.push(format!(
                "secure_window_keys_typed={}",
                if hold.keys_typed { "yes" } else { "no" }
            ));
        }
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }
}

/// What a timed run ends with: its report, the exit status of
/// `halvo_control::exit`, and the loss line of contracts.md C3.2 when the
/// status is 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub report: TimedRunReport,
    pub status: i32,
    pub loss: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::CaptureError;

    fn fixture_with_sink_lines(name: &str, sink: &[KindCounts; 8]) -> String {
        let path = format!(
            "{}/../../scripts/r1-gate/fixtures/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).unwrap().replace('\r', "");
        let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let after = lines
            .iter()
            .position(|line| line.starts_with("events_discarded="))
            .expect("the fixture has no events_discarded line")
            + 1;
        let sink_lines = Modality::ALL.into_iter().zip(sink).map(|(kind, counts)| {
            format!(
                "sink_counts kind={} accepted={} dropped={} coalesced={} refused={}",
                kind.as_str(),
                counts.accepted,
                counts.dropped,
                counts.coalesced,
                counts.refused
            )
        });
        lines.splice(after..after, sink_lines);
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    fn sink_of(counts: [u64; 8]) -> [KindCounts; 8] {
        counts.map(|accepted| KindCounts {
            accepted,
            ..KindCounts::default()
        })
    }

    fn report(
        counts: [u64; 8],
        failed_sources: Vec<FailedSource>,
        hold: Option<HoldLines>,
    ) -> TimedRunReport {
        let total = counts.iter().sum();
        TimedRunReport {
            screen_frame: counts[0],
            keystroke: counts[1],
            pointer: counts[2],
            foreground_app: counts[3],
            window_title: counts[4],
            system_audio: counts[5],
            microphone: counts[6],
            camera: counts[7],
            secure_input_keystrokes: 0,
            segments_written: 4,
            events_received: total,
            events_persisted: total,
            events_discarded: 0,
            sink_counts: sink_of(counts),
            tap_disabled_timeout: 0,
            tap_disabled_user_input: 0,
            rss_peak_bytes: 25_264_128,
            failed_sources,
            hold,
        }
    }

    #[test]
    fn the_passing_fixture_renders_from_typed_values() {
        let counts = [10, 5, 7, 3, 2, 0, 0, 0];
        let typed = report(counts, Vec::new(), None);
        assert_eq!(
            typed.render(),
            fixture_with_sink_lines("c5-pass.txt", &sink_of(counts))
        );
    }

    #[test]
    fn the_failing_input_fixture_renders_from_an_error_the_code_raises() {
        let counts = [10, 0, 0, 3, 2, 0, 0, 0];
        let failed = FailedSource {
            name: "input",
            error: CaptureError::PermissionDenied(Modality::Keystroke).to_string(),
        };
        let typed = report(counts, vec![failed], None);
        assert_eq!(
            typed.render(),
            fixture_with_sink_lines("c5-fail-input.txt", &sink_of(counts))
        );
    }

    #[test]
    fn the_hold_fixture_renders_its_six_lines_from_typed_values() {
        let counts = [10, 3, 15, 3, 2, 0, 0, 0];
        let hold = HoldLines {
            released_keystrokes: 3,
            held_keystrokes: 0,
            suppressed: SuppressedKeys::default(),
            held_samples: HeldSamples {
                held: 40,
                taken: 40,
            },
            window_pointer: 12,
            keys_typed: true,
        };
        let typed = report(counts, Vec::new(), Some(hold));
        assert_eq!(
            typed.render(),
            fixture_with_sink_lines("c5-hold-pass.txt", &sink_of(counts))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_peak_line_is_a_positive_byte_count() {
        let mut typed = report([0; 8], Vec::new(), None);
        typed.rss_peak_bytes = crate::daemon::signal::peak_rss_bytes().unwrap();
        let text = typed.render();
        let lines: Vec<&str> = text.lines().collect();
        let at = lines
            .iter()
            .position(|line| line.starts_with("rss_peak_bytes="))
            .expect("no peak line");
        let bytes: u64 = lines[at]["rss_peak_bytes=".len()..].parse().unwrap();
        assert!(
            bytes > 1_048_576,
            "a peak of {bytes} bytes is no byte count"
        );
        assert!(lines[at + 1].starts_with("failed_sources="));
        assert_eq!(lines[at - 1], "tap_disabled_user_input=0");
    }
}
