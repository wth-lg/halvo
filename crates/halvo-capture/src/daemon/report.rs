//! The report of a bounded timed run.

/// The result of a bounded timed run: per-modality event counts and the
/// secure-input keystroke count. [`TimedRunReport::render`] prints the exact
/// lines the gate's timed-run and secure-input sub-checks parse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimedRunReport {
    pub seconds: u64,
    pub screen_frame: u64,
    pub keystroke: u64,
    pub pointer: u64,
    pub foreground_app: u64,
    pub window_title: u64,
    pub secure_input_keystrokes: u64,
}

impl TimedRunReport {
    /// Render the report in the gate's line format.
    pub fn render(&self) -> String {
        format!(
            "modality=screen_frame count={}\n\
             modality=keystroke count={}\n\
             modality=pointer count={}\n\
             modality=foreground_app count={}\n\
             modality=window_title count={}\n\
             secure_input_keystrokes={}\n",
            self.screen_frame,
            self.keystroke,
            self.pointer,
            self.foreground_app,
            self.window_title,
            self.secure_input_keystrokes,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_renders_the_gate_lines() {
        let report = TimedRunReport {
            seconds: 600,
            screen_frame: 3,
            keystroke: 2,
            pointer: 5,
            foreground_app: 1,
            window_title: 4,
            secure_input_keystrokes: 0,
        };
        let text = report.render();
        assert!(text.contains("modality=screen_frame count=3"));
        assert!(text.contains("modality=keystroke count=2"));
        assert!(text.contains("modality=foreground_app count=1"));
        assert!(text.contains("secure_input_keystrokes=0"));
    }
}
