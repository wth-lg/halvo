use crate::present::guard::Guard;
use crate::present::moment::{Decision, Event, Moment, Pair};
use std::time::{Duration, Instant};

const ARM: Duration = Duration::from_millis(500);
const TRUE: Pair = Pair {
    active: true,
    key: true,
};
const PAIRS: [Pair; 4] = [
    Pair {
        active: false,
        key: false,
    },
    Pair {
        active: false,
        key: true,
    },
    Pair {
        active: true,
        key: false,
    },
    TRUE,
];

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// One show: its moment and its hold, with the times of the steps from `t0`.
#[derive(Clone, Copy)]
struct Show {
    t0: Instant,
    moment: Moment,
    guard: Guard,
    reports: usize,
}

impl Show {
    fn new() -> Show {
        Show {
            t0: Instant::now(),
            moment: Moment::default(),
            guard: Guard::new(ARM),
            reports: 0,
        }
    }

    fn at(&self, n: u64) -> Instant {
        self.t0 + ms(n)
    }

    fn event(&mut self, event: Event, read: Option<Pair>, at: u64) -> Decision {
        let now = self.at(at);
        let decision = self.moment.decide(&mut self.guard, event, read, now);
        if decision.reports() {
            self.reports += 1;
        }
        decision
    }

    fn become_(&mut self, read: Pair, at: u64) -> Decision {
        self.event(Event::Become, Some(read), at)
    }

    fn resign(&mut self, at: u64) -> Decision {
        self.event(Event::Resign, None, at)
    }

    fn reading(&mut self, read: Pair, at: u64) -> Decision {
        self.event(Event::Reading, Some(read), at)
    }

    fn press(&mut self, read: Pair, modal: bool, at: u64) -> Decision {
        self.event(Event::Press { modal }, Some(read), at)
    }

    fn returned(&mut self, at: u64) {
        self.guard.reported(self.at(at));
    }

    /// Whether a press with the pair true in the modal window at `at` answers, on a copy.
    fn answers(&self, at: u64) -> bool {
        let mut copy = *self;
        copy.press(TRUE, true, at) == Decision::Answer
    }

    /// A show reported at a become notice at 0, whose report returned at 0.
    fn reported() -> Show {
        let mut show = Show::new();
        assert_eq!(show.become_(TRUE, 0), Decision::Moment { first: true });
        show.returned(0);
        show
    }
}

#[test]
fn moment_every_order_of_the_two_become_notices_reports_at_the_first_that_reads_both() {
    for first in PAIRS {
        for second in PAIRS {
            let mut show = Show::new();
            let one = show.become_(first, 0);
            let two = show.become_(second, 10);
            let label = format!("{first:?} then {second:?}");
            match (first.both(), second.both()) {
                (true, both) => {
                    assert_eq!(one, Decision::Moment { first: true }, "{label}");
                    let want = if both {
                        Decision::Moment { first: false }
                    } else {
                        Decision::Nothing
                    };
                    assert_eq!(two, want, "{label}");
                }
                (false, true) => {
                    assert_eq!(one, Decision::Nothing, "{label}");
                    assert_eq!(two, Decision::Moment { first: true }, "{label}");
                }
                (false, false) => {
                    assert_eq!(
                        (one, two),
                        (Decision::Nothing, Decision::Nothing),
                        "{label}"
                    );
                }
            }
            assert_eq!(
                show.reports,
                usize::from(first.both() || second.both()),
                "{label}"
            );
        }
    }
}

#[test]
fn moment_the_pair_turns_true_with_no_notice_and_the_repeating_reading_takes_the_moment() {
    let mut show = Show::new();
    assert_eq!(
        show.become_(PAIRS[1], 0),
        Decision::Nothing,
        "key, the app not active"
    );
    assert_eq!(show.reading(PAIRS[1], 50), Decision::Nothing);
    assert_eq!(show.reading(TRUE, 100), Decision::Moment { first: true });
    assert!(!show.answers(150), "the report has not returned");
    show.returned(200);
    assert!(!show.answers(699));
    assert!(show.answers(700));
    assert_eq!(show.reports, 1);
}

#[test]
fn moment_the_first_moment_makes_the_report_whether_a_notice_a_reading_or_a_press_took_it() {
    for (decision, reports) in [
        (Decision::Moment { first: true }, true),
        (Decision::Refused { moment: Some(true) }, true),
        (Decision::Moment { first: false }, false),
        (
            Decision::Refused {
                moment: Some(false),
            },
            false,
        ),
        (Decision::Refused { moment: None }, false),
        (Decision::Answer, false),
        (Decision::Nothing, false),
    ] {
        assert_eq!(decision.reports(), reports, "{decision:?}");
    }
    let mut show = Show::new();
    let first = show.press(TRUE, true, 300);
    assert!(
        first.reports(),
        "the first moment taken at a press: {first:?}"
    );
    assert_eq!(show.reports, 1);
}

#[test]
fn moment_the_pair_turns_true_with_no_notice_and_no_reading_and_a_press_takes_the_moment() {
    let mut show = Show::new();
    assert_eq!(
        show.become_(PAIRS[2], 0),
        Decision::Nothing,
        "active, the window not key"
    );
    assert_eq!(
        show.press(TRUE, true, 300),
        Decision::Refused { moment: Some(true) },
        "the press that took the moment"
    );
    assert_eq!(
        show.press(TRUE, true, 320),
        Decision::Refused { moment: None },
        "a press before the report returned"
    );
    show.returned(400);
    assert!(!show.answers(899), "arm_ms minus 1 after the return");
    assert!(show.answers(900), "arm_ms after the return");
    assert_eq!(show.reports, 1);
}

#[test]
fn moment_a_press_that_reads_both_for_the_first_time_outside_the_modal_window_takes_the_moment() {
    let mut show = Show::new();
    assert_eq!(
        show.press(TRUE, false, 300),
        Decision::Refused { moment: Some(true) },
        "the moment is taken before the modal fact is read, and the press is refused"
    );
    assert_eq!(show.reports, 1, "the press made the report");
}

#[test]
fn moment_after_a_moment_taken_outside_the_modal_window_a_press_arm_ms_after_the_return_answers() {
    // The report returns only after a report was made, as the alert's waiting thread calls it.
    let mut show = Show::new();
    show.press(TRUE, false, 300);
    if show.reports == 1 {
        show.returned(400);
    }
    assert_eq!(
        show.press(TRUE, true, 899),
        Decision::Refused { moment: None },
        "arm_ms minus 1 after the return"
    );
    assert_eq!(
        show.press(TRUE, true, 1_398),
        Decision::Refused { moment: None },
        "arm_ms minus 1 after the press that the hold refused"
    );
    assert_eq!(
        show.press(TRUE, true, 1_898),
        Decision::Answer,
        "arm_ms after the last refused press, in the modal window"
    );
    let mut show = Show::new();
    show.press(TRUE, false, 300);
    if show.reports == 1 {
        show.returned(400);
    }
    assert_eq!(
        show.press(TRUE, true, 900),
        Decision::Answer,
        "arm_ms after the return, in the modal window"
    );
}

#[test]
fn moment_readings_of_true_after_true_leave_the_hold_where_it_was() {
    let mut show = Show::reported();
    assert_eq!(show.reading(TRUE, 300), Decision::Nothing);
    assert_eq!(show.reading(TRUE, 450), Decision::Nothing);
    assert!(show.answers(500));
}

#[test]
fn moment_a_resign_forces_not_true_and_the_next_true_reading_starts_the_hold_anew() {
    for later in [Event::Reading, Event::Press { modal: true }] {
        let mut show = Show::reported();
        assert_eq!(show.resign(200), Decision::Nothing);
        let took = show.event(later, Some(TRUE), 300);
        let want = match later {
            Event::Reading => Decision::Moment { first: false },
            _ => Decision::Refused {
                moment: Some(false),
            },
        };
        assert_eq!(took, want, "{later:?}");
        assert!(!show.answers(799), "{later:?}");
        assert!(show.answers(800), "{later:?}");
        assert_eq!(show.reports, 1);
    }
}

#[test]
fn moment_a_real_become_notice_while_the_stored_pair_is_true_starts_the_hold_anew() {
    let mut show = Show::reported();
    assert_eq!(show.become_(TRUE, 400), Decision::Moment { first: false });
    assert!(!show.answers(899));
    assert!(show.answers(900));
    assert_eq!(show.reports, 1);
}

#[test]
fn moment_a_press_with_the_pair_false_after_the_hold_starts_it_anew_and_the_next_true_is_a_moment()
{
    let mut show = Show::reported();
    assert_eq!(
        show.press(PAIRS[1], true, 600),
        Decision::Refused { moment: None },
        "the pair false after the hold had passed"
    );
    assert!(
        !show.answers(601),
        "1 ms later, the pair true again with no notice"
    );
    let mut show = Show::reported();
    show.press(PAIRS[1], true, 600);
    assert_eq!(
        show.press(TRUE, true, 700),
        Decision::Refused {
            moment: Some(false)
        },
        "the pair true with no notice: the press takes a moment"
    );
    assert!(!show.answers(1_199), "arm_ms minus 1 after it");
    assert!(show.answers(1_200), "arm_ms after it");
}

#[test]
fn moment_a_press_that_the_modal_check_refuses_after_the_hold_starts_it_anew() {
    let mut show = Show::reported();
    assert_eq!(
        show.press(TRUE, false, 600),
        Decision::Refused { moment: None }
    );
    assert!(!show.answers(601), "1 ms after the refused press");
    assert!(!show.answers(1_099), "arm_ms minus 1 after it");
    assert!(show.answers(1_100), "arm_ms after it");
}

#[test]
fn moment_a_run_of_presses_inside_the_hold_never_answers_and_the_first_after_arm_does() {
    let mut show = Show::reported();
    for step in 1..=10 {
        assert_eq!(
            show.press(TRUE, true, step * 100),
            Decision::Refused { moment: None },
            "press {step}"
        );
    }
    assert!(!show.answers(1_499));
    assert_eq!(show.press(TRUE, true, 1_500), Decision::Answer);
}

#[test]
fn moment_a_second_moment_before_the_report_returned_reports_once_and_holds_from_the_return() {
    let mut show = Show::new();
    assert_eq!(show.become_(TRUE, 0), Decision::Moment { first: true });
    assert_eq!(show.resign(10), Decision::Nothing);
    assert_eq!(show.become_(TRUE, 20), Decision::Moment { first: false });
    assert!(!show.answers(30), "before the report returned");
    show.returned(100);
    assert!(!show.answers(599));
    assert!(show.answers(600));
    assert_eq!(show.reports, 1);
}

#[test]
fn moment_a_clock_reading_earlier_than_the_start_moves_nothing_back() {
    let mut show = Show::new();
    assert_eq!(show.become_(TRUE, 1_000), Decision::Moment { first: true });
    show.returned(1_000);
    assert_eq!(
        show.press(TRUE, true, 500),
        Decision::Refused { moment: None }
    );
    assert!(!show.answers(1_499));
    assert!(show.answers(1_500));
}

#[test]
fn moment_a_show_whose_pair_never_reads_both_answers_no_press_and_reports_nothing() {
    let mut show = Show::new();
    for (i, pair) in PAIRS[..3].iter().enumerate() {
        let at = 1_000 * (i as u64 + 1);
        show.become_(*pair, at);
        show.reading(*pair, at + 50);
        assert_eq!(
            show.press(*pair, true, at + 600),
            Decision::Refused { moment: None },
            "{pair:?}"
        );
    }
    assert_eq!(show.reports, 0);
}

#[test]
fn moment_a_resign_forces_not_true_whatever_pair_comes_with_it() {
    let mut show = Show::reported();
    assert_eq!(
        show.event(Event::Resign, Some(TRUE), 200),
        Decision::Nothing
    );
    assert_eq!(show.reading(TRUE, 300), Decision::Moment { first: false });
    assert!(!show.answers(799));
    assert!(show.answers(800));
}
