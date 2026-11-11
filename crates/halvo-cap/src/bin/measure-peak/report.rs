use halvo_cap::{Exit, Limit, Outcome};

pub fn exit_word(exit: Exit) -> String {
    match exit {
        Exit::Code(code) => code.to_string(),
        Exit::Signal(signal) => format!("signal-{signal}"),
        Exit::NotStarted => "not-started".to_string(),
    }
}

pub fn run_line(n: u64, outcome: &Outcome) -> String {
    format!(
        "run n={n} exit={} peak_bytes={} capped={} start={} end={}",
        exit_word(outcome.exit),
        outcome.peak_bytes,
        if outcome.capped { "yes" } else { "no" },
        outcome.start.0,
        outcome.end.0
    )
}

pub fn median(peaks: &[u64]) -> u64 {
    let mut sorted = peaks.to_vec();
    sorted.sort_unstable();
    sorted.get(sorted.len() / 2).copied().unwrap_or(0)
}

pub fn median_passes(median: u64, limit: Limit) -> bool {
    median <= limit.bytes()
}

#[derive(Debug, PartialEq, Eq)]
pub struct Checks {
    pub exit: bool,
    pub cap: bool,
    pub median: bool,
}

impl Checks {
    pub fn of(outcomes: &[Outcome], limit: Limit) -> Checks {
        let peaks: Vec<u64> = outcomes.iter().map(|o| o.peak_bytes).collect();
        Checks {
            exit: outcomes.iter().all(|o| o.exit == Exit::Code(0)),
            cap: outcomes.iter().all(|o| !o.capped),
            median: median_passes(median(&peaks), limit),
        }
    }

    pub fn all_ok(&self) -> bool {
        self.exit && self.cap && self.median
    }

    pub fn line(&self) -> String {
        format!(
            "checks exit={} cap={} median={}",
            word(self.exit),
            word(self.cap),
            word(self.median)
        )
    }
}

pub fn word(ok: bool) -> &'static str {
    if ok {
        "ok"
    } else {
        "miss"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use halvo_cap::UnixNanos;

    fn outcome(exit: Exit, peak_bytes: u64, capped: bool) -> Outcome {
        Outcome {
            exit,
            peak_bytes,
            capped,
            start: UnixNanos(1),
            end: UnixNanos(2),
        }
    }

    #[test]
    fn the_median_of_an_even_count_is_the_higher_middle_value() {
        assert_eq!(median(&[5, 1, 3]), 3);
        assert_eq!(median(&[4, 1, 3, 2]), 3);
        assert_eq!(median(&[9]), 9);
        assert_eq!(median(&[8, 2]), 8);
    }

    #[test]
    fn a_median_at_or_below_the_limit_passes_and_a_median_above_it_misses() {
        let limit = Limit::new(1_000_000).unwrap();
        assert!(median_passes(500_000, limit));
        assert!(median_passes(999_999, limit));
        assert!(median_passes(1_000_000, limit));
        assert!(!median_passes(1_000_001, limit));
    }

    #[test]
    fn each_check_misses_on_its_own_cause() {
        let limit = Limit::new(100).unwrap();
        let fine = outcome(Exit::Code(0), 50, false);
        assert!(Checks::of(&[fine, fine, fine], limit).all_ok());
        assert_eq!(
            Checks::of(&[fine, outcome(Exit::Signal(9), 50, false), fine], limit),
            Checks {
                exit: false,
                cap: true,
                median: true
            }
        );
        assert_eq!(
            Checks::of(&[fine, outcome(Exit::NotStarted, 0, false), fine], limit),
            Checks {
                exit: false,
                cap: true,
                median: true
            }
        );
        assert_eq!(
            Checks::of(&[fine, outcome(Exit::Code(0), 50, true), fine], limit),
            Checks {
                exit: true,
                cap: false,
                median: true
            }
        );
        let high = outcome(Exit::Code(0), 101, false);
        assert_eq!(
            Checks::of(&[fine, high, high], limit),
            Checks {
                exit: true,
                cap: true,
                median: false
            }
        );
        assert_eq!(
            Checks::of(&[fine, high, high], limit).line(),
            "checks exit=ok cap=ok median=miss"
        );
    }

    #[test]
    fn a_run_line_has_the_form_of_the_contract() {
        assert_eq!(
            run_line(2, &outcome(Exit::Signal(9), 7, true)),
            "run n=2 exit=signal-9 peak_bytes=7 capped=yes start=1 end=2"
        );
        assert_eq!(exit_word(Exit::Code(-1073741571)), "-1073741571");
        assert_eq!(exit_word(Exit::NotStarted), "not-started");
    }
}
