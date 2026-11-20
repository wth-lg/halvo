#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Lead {
    Tab,
    Click,
    TabTab,
    Enter,
    Nothing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Wait {
    Sleeps,
    ServesSent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Landing {
    Password,
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Checks {
    Stall,
    Enter,
    TabOut,
    ControlsAndSink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Phase {
    pub(super) test: &'static str,
    pub(super) name: &'static str,
    pub(super) lead: Lead,
    pub(super) wait: Wait,
    pub(super) stall_ms: u64,
    pub(super) rounds: u32,
    pub(super) letters_through_the_wait: bool,
    pub(super) letters_after: u32,
    pub(super) start_in_password: bool,
    pub(super) self_focus: bool,
    pub(super) two_step: bool,
    pub(super) other_front: bool,
    pub(super) landing: Landing,
    pub(super) checks: Checks,
}

pub(super) const fn phase(
    test: &'static str,
    name: &'static str,
    lead: Lead,
    wait: Wait,
    stall_ms: u64,
) -> Phase {
    Phase {
        test,
        name,
        lead,
        wait,
        stall_ms,
        rounds: if stall_ms >= 2_000 { 5 } else { 20 },
        letters_through_the_wait: true,
        letters_after: 0,
        start_in_password: false,
        self_focus: false,
        two_step: false,
        other_front: false,
        landing: Landing::Password,
        checks: Checks::Stall,
    }
}

pub(super) const CONTROL_LETTERS: u32 = 5;

pub(super) const WAITS_MS: [u64; 5] = [150, 200, 300, 600, 2_000];

const TAB_SLEEPS: [(&str, &str); 5] = [
    ("stall-tab-sleeps-150", "tab_sleeps_150ms"),
    ("stall-tab-sleeps-200", "tab_sleeps_200ms"),
    ("stall-tab-sleeps-300", "tab_sleeps_300ms"),
    ("stall-tab-sleeps-600", "tab_sleeps_600ms"),
    ("stall-tab-sleeps-2000", "tab_sleeps_2000ms"),
];

const TAB_SERVES: [(&str, &str); 5] = [
    ("stall-tab-serves-150", "tab_serves_150ms"),
    ("stall-tab-serves-200", "tab_serves_200ms"),
    ("stall-tab-serves-300", "tab_serves_300ms"),
    ("stall-tab-serves-600", "tab_serves_600ms"),
    ("stall-tab-serves-2000", "tab_serves_2000ms"),
];

const CLICK_SLEEPS: [(&str, &str); 5] = [
    ("stall-click-sleeps-150", "click_sleeps_150ms"),
    ("stall-click-sleeps-200", "click_sleeps_200ms"),
    ("stall-click-sleeps-300", "click_sleeps_300ms"),
    ("stall-click-sleeps-600", "click_sleeps_600ms"),
    ("stall-click-sleeps-2000", "click_sleeps_2000ms"),
];

const CLICK_SERVES: [(&str, &str); 5] = [
    ("stall-click-serves-150", "click_serves_150ms"),
    ("stall-click-serves-200", "click_serves_200ms"),
    ("stall-click-serves-300", "click_serves_300ms"),
    ("stall-click-serves-600", "click_serves_600ms"),
    ("stall-click-serves-2000", "click_serves_2000ms"),
];

pub(super) const NATIVE_PHASES: [(&str, &str); 2] =
    [("stall-mask", "mask"), ("stall-stale-focus", "stale-focus")];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    TabSleeps,
    TabServes,
    ClickSleeps,
    ClickServes,
    SecondFieldAndOtherFront,
    EnterTabOutAndTwoStep,
    SelfFocus,
}

impl Kind {
    pub(super) fn test(self) -> &'static str {
        match self {
            Kind::TabSleeps => "stall-tab-sleeps",
            Kind::TabServes => "stall-tab-serves",
            Kind::ClickSleeps => "stall-click-sleeps",
            Kind::ClickServes => "stall-click-serves",
            Kind::SecondFieldAndOtherFront => "stall-second-field-other-front",
            Kind::EnterTabOutAndTwoStep => "stall-enter-tab-out-two-step",
            Kind::SelfFocus => "stall-self-focus",
        }
    }

    pub(super) fn limit_secs(self) -> u64 {
        match self {
            Kind::ClickSleeps | Kind::ClickServes | Kind::SelfFocus => 330,
            _ => 300,
        }
    }

    pub(super) fn phases(self) -> Vec<Phase> {
        let at_each_wait = |lead: Lead, wait: Wait, labels: [(&'static str, &'static str); 5]| {
            WAITS_MS
                .into_iter()
                .zip(labels)
                .map(|(stall_ms, (test, name))| phase(test, name, lead, wait, stall_ms))
                .collect()
        };
        let waits = [Wait::ServesSent, Wait::Sleeps];
        match self {
            Kind::TabSleeps => at_each_wait(Lead::Tab, Wait::Sleeps, TAB_SLEEPS),
            Kind::TabServes => at_each_wait(Lead::Tab, Wait::ServesSent, TAB_SERVES),
            Kind::ClickSleeps => at_each_wait(Lead::Click, Wait::Sleeps, CLICK_SLEEPS),
            Kind::ClickServes => at_each_wait(Lead::Click, Wait::ServesSent, CLICK_SERVES),
            Kind::SecondFieldAndOtherFront => waits
                .into_iter()
                .map(|wait| Phase {
                    letters_through_the_wait: false,
                    ..phase("stall-tab-tab", "tab_tab_300ms", Lead::TabTab, wait, 300)
                })
                .chain(waits.into_iter().map(|wait| Phase {
                    other_front: true,
                    letters_through_the_wait: false,
                    ..phase(
                        "stall-other-front",
                        "other_front_300ms",
                        Lead::Tab,
                        wait,
                        300,
                    )
                }))
                .collect(),
            Kind::EnterTabOutAndTwoStep => vec![
                Phase {
                    letters_through_the_wait: false,
                    letters_after: 4,
                    landing: Landing::Plain,
                    checks: Checks::Enter,
                    ..phase(
                        "stall-enter",
                        "enter_then_five_letters",
                        Lead::Enter,
                        Wait::Sleeps,
                        0,
                    )
                },
                Phase {
                    letters_through_the_wait: false,
                    start_in_password: true,
                    landing: Landing::Plain,
                    checks: Checks::TabOut,
                    ..phase(
                        "stall-tab-out",
                        "tab_out_then_a_letter",
                        Lead::Tab,
                        Wait::Sleeps,
                        0,
                    )
                },
                Phase {
                    two_step: true,
                    letters_through_the_wait: false,
                    ..phase("stall-two-step", "two_step", Lead::Nothing, Wait::Sleeps, 0)
                },
            ],
            Kind::SelfFocus => [
                (Wait::ServesSent, 150),
                (Wait::ServesSent, 300),
                (Wait::ServesSent, 600),
                (Wait::Sleeps, 300),
            ]
            .into_iter()
            .map(|(wait, stall_ms)| Phase {
                self_focus: true,
                letters_through_the_wait: false,
                checks: Checks::ControlsAndSink,
                ..phase(
                    "stall-self-focus",
                    "self_focus",
                    Lead::Nothing,
                    wait,
                    stall_ms,
                )
            })
            .collect(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Totals {
    pub(crate) rounds: u32,
    pub(crate) counted: u32,
    pub(crate) letters_sent: u32,
    pub(crate) letters_landed: u32,
    pub(crate) letters_recorded: u32,
    pub(crate) tabs_recorded: u32,
    pub(crate) other_recorded: u32,
    pub(crate) late_moved: u64,
    pub(crate) withheld_set: u32,
    pub(crate) rounds_with_a_recorded_letter: u32,
    pub(crate) rounds_not_withheld_by_the_password_field: u32,
    pub(crate) sink_differs: u32,
    pub(crate) keystroke_events: u32,
}

impl Totals {
    pub(super) fn add(&mut self, other: Totals) {
        self.rounds += other.rounds;
        self.counted += other.counted;
        self.letters_sent += other.letters_sent;
        self.letters_landed += other.letters_landed;
        self.letters_recorded += other.letters_recorded;
        self.tabs_recorded += other.tabs_recorded;
        self.other_recorded += other.other_recorded;
        self.late_moved += other.late_moved;
        self.withheld_set += other.withheld_set;
        self.rounds_with_a_recorded_letter += other.rounds_with_a_recorded_letter;
        self.rounds_not_withheld_by_the_password_field +=
            other.rounds_not_withheld_by_the_password_field;
        self.sink_differs += other.sink_differs;
        self.keystroke_events += other.keystroke_events;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Step {
    pub(super) session: u32,
    pub(super) at: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ControlRead {
    pub(super) step: Step,
    pub(super) recorded: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Controls {
    pub(super) before: ControlRead,
    pub(super) after: ControlRead,
    pub(super) late_at_the_end: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Rounds {
        totals: Totals,
        checks: Checks,
    },
    Native {
        rounds: u32,
        counted: u32,
        recorded: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PhaseRun {
    pub(super) test: &'static str,
    pub(super) name: &'static str,
    pub(super) wait: Wait,
    pub(super) step: Step,
    pub(super) outcome: Outcome,
}

pub(super) fn bracketed(step: Step, controls: &Controls) -> Result<(), String> {
    let (before, after) = (controls.before, controls.after);
    if before.step.session != step.session || after.step.session != step.session {
        return Err(format!(
            "the controls read the fixture and source of session {} and {}, not its own session {}",
            before.step.session, after.step.session, step.session
        ));
    }
    if !(before.step.at < step.at && step.at < after.step.at) {
        return Err(format!(
            "the controls do not bracket it: control_before at step {}, the phase at step {}, control_after at step {}",
            before.step.at, step.at, after.step.at
        ));
    }
    if before.recorded != CONTROL_LETTERS || after.recorded != CONTROL_LETTERS {
        return Err(format!(
            "a control recorded fewer than {CONTROL_LETTERS} letters of the plain field: control_before={} control_after={}",
            before.recorded, after.recorded
        ));
    }
    Ok(())
}

fn of_rounds(totals: &Totals, checks: Checks) -> Result<(), &'static str> {
    if totals.sink_differs > 0 {
        return Err(
            "the sink held another number of keystroke events than the seam recorded in a round",
        );
    }
    let all_counted = totals.counted == totals.rounds;
    let failed = match checks {
        Checks::Stall => [
            (totals.counted == 0, "no round counted"),
            (
                totals.letters_recorded > 0,
                "a letter typed ahead of the program was recorded in a counted round",
            ),
            (
                totals.other_recorded > 0,
                "a key of another class than Tab was recorded in a counted round",
            ),
            (
                totals.late_moved > 0,
                "the source's late value moved during the phase",
            ),
            (!all_counted, "INCONCLUSIVE: not every round counted"),
        ]
        .into_iter()
        .find(|(failed, _)| *failed),
        Checks::Enter => [
            (
                totals.letters_recorded > 0,
                "a letter after Enter was recorded",
            ),
            (
                totals.withheld_set == 0 && totals.counted > 0,
                "keys_withheld did not read yes",
            ),
            (totals.late_moved > 0, "the source's late value moved"),
            (!all_counted, "INCONCLUSIVE: not every round counted"),
        ]
        .into_iter()
        .find(|(failed, _)| *failed),
        Checks::TabOut => [
            (
                totals.letters_recorded > 0,
                "the letter after the Tab out was recorded",
            ),
            (
                totals.tabs_recorded > 0,
                "the Tab out of the password field was recorded",
            ),
            (
                totals.other_recorded > 0,
                "a key of another class was recorded",
            ),
            (totals.late_moved > 0, "the source's late value moved"),
            (!all_counted, "INCONCLUSIVE: not every round counted"),
        ]
        .into_iter()
        .find(|(failed, _)| *failed),
        Checks::ControlsAndSink => None,
    };
    failed.map_or(Ok(()), |(_, why)| Err(why))
}

fn of_native(rounds: u32, counted: u32, recorded: u32, late: u64) -> Result<(), String> {
    if counted < rounds.saturating_sub(1).max(1) {
        return Err(format!(
            "INCONCLUSIVE: counted {counted} rounds of {rounds}"
        ));
    }
    if recorded > 0 {
        return Err("a letter of a password field was recorded".to_string());
    }
    if late > 0 {
        return Err("the source's late value moved".to_string());
    }
    Ok(())
}

pub(super) fn verdict(run: &PhaseRun, controls: &Controls) -> Result<(), String> {
    bracketed(run.step, controls)
        .and_then(|()| match run.outcome {
            Outcome::Rounds { totals, checks } => {
                of_rounds(&totals, checks).map_err(str::to_string)
            }
            Outcome::Native {
                rounds,
                counted,
                recorded,
            } => of_native(rounds, counted, recorded, controls.late_at_the_end),
        })
        .map_err(|why| format!("{} {} {:?}: {why}", run.test, run.name, run.wait))
}

pub(super) const REPLACED: [(&str, Phase); 27] = [
    (
        "stall::tab_then_letters_while_the_program_sleeps_150_ms",
        phase(
            "stall-tab-sleeps-150",
            "tab_sleeps_150ms",
            Lead::Tab,
            Wait::Sleeps,
            150,
        ),
    ),
    (
        "stall::tab_then_letters_while_the_program_sleeps_200_ms",
        phase(
            "stall-tab-sleeps-200",
            "tab_sleeps_200ms",
            Lead::Tab,
            Wait::Sleeps,
            200,
        ),
    ),
    (
        "stall::tab_then_letters_while_the_program_sleeps_300_ms",
        phase(
            "stall-tab-sleeps-300",
            "tab_sleeps_300ms",
            Lead::Tab,
            Wait::Sleeps,
            300,
        ),
    ),
    (
        "stall::tab_then_letters_while_the_program_sleeps_600_ms",
        phase(
            "stall-tab-sleeps-600",
            "tab_sleeps_600ms",
            Lead::Tab,
            Wait::Sleeps,
            600,
        ),
    ),
    (
        "stall::tab_then_letters_while_the_program_sleeps_2000_ms",
        phase(
            "stall-tab-sleeps-2000",
            "tab_sleeps_2000ms",
            Lead::Tab,
            Wait::Sleeps,
            2000,
        ),
    ),
    (
        "stall::tab_then_letters_while_the_program_serves_sent_messages_150_ms",
        phase(
            "stall-tab-serves-150",
            "tab_serves_150ms",
            Lead::Tab,
            Wait::ServesSent,
            150,
        ),
    ),
    (
        "stall::tab_then_letters_while_the_program_serves_sent_messages_200_ms",
        phase(
            "stall-tab-serves-200",
            "tab_serves_200ms",
            Lead::Tab,
            Wait::ServesSent,
            200,
        ),
    ),
    (
        "stall::tab_then_letters_while_the_program_serves_sent_messages_300_ms",
        phase(
            "stall-tab-serves-300",
            "tab_serves_300ms",
            Lead::Tab,
            Wait::ServesSent,
            300,
        ),
    ),
    (
        "stall::tab_then_letters_while_the_program_serves_sent_messages_600_ms",
        phase(
            "stall-tab-serves-600",
            "tab_serves_600ms",
            Lead::Tab,
            Wait::ServesSent,
            600,
        ),
    ),
    (
        "stall::tab_then_letters_while_the_program_serves_sent_messages_2000_ms",
        phase(
            "stall-tab-serves-2000",
            "tab_serves_2000ms",
            Lead::Tab,
            Wait::ServesSent,
            2000,
        ),
    ),
    (
        "stall::click_then_letters_while_the_program_sleeps_150_ms",
        phase(
            "stall-click-sleeps-150",
            "click_sleeps_150ms",
            Lead::Click,
            Wait::Sleeps,
            150,
        ),
    ),
    (
        "stall::click_then_letters_while_the_program_sleeps_200_ms",
        phase(
            "stall-click-sleeps-200",
            "click_sleeps_200ms",
            Lead::Click,
            Wait::Sleeps,
            200,
        ),
    ),
    (
        "stall::click_then_letters_while_the_program_sleeps_300_ms",
        phase(
            "stall-click-sleeps-300",
            "click_sleeps_300ms",
            Lead::Click,
            Wait::Sleeps,
            300,
        ),
    ),
    (
        "stall::click_then_letters_while_the_program_sleeps_600_ms",
        phase(
            "stall-click-sleeps-600",
            "click_sleeps_600ms",
            Lead::Click,
            Wait::Sleeps,
            600,
        ),
    ),
    (
        "stall::click_then_letters_while_the_program_sleeps_2000_ms",
        phase(
            "stall-click-sleeps-2000",
            "click_sleeps_2000ms",
            Lead::Click,
            Wait::Sleeps,
            2000,
        ),
    ),
    (
        "stall::click_then_letters_while_the_program_serves_sent_messages_150_ms",
        phase(
            "stall-click-serves-150",
            "click_serves_150ms",
            Lead::Click,
            Wait::ServesSent,
            150,
        ),
    ),
    (
        "stall::click_then_letters_while_the_program_serves_sent_messages_200_ms",
        phase(
            "stall-click-serves-200",
            "click_serves_200ms",
            Lead::Click,
            Wait::ServesSent,
            200,
        ),
    ),
    (
        "stall::click_then_letters_while_the_program_serves_sent_messages_300_ms",
        phase(
            "stall-click-serves-300",
            "click_serves_300ms",
            Lead::Click,
            Wait::ServesSent,
            300,
        ),
    ),
    (
        "stall::click_then_letters_while_the_program_serves_sent_messages_600_ms",
        phase(
            "stall-click-serves-600",
            "click_serves_600ms",
            Lead::Click,
            Wait::ServesSent,
            600,
        ),
    ),
    (
        "stall::click_then_letters_while_the_program_serves_sent_messages_2000_ms",
        phase(
            "stall-click-serves-2000",
            "click_serves_2000ms",
            Lead::Click,
            Wait::ServesSent,
            2000,
        ),
    ),
    (
        "stall::tab_tab_and_a_letter_across_a_second_field_while_the_program_waits_after_the_first_tab",
        Phase {
            letters_through_the_wait: false,
            ..phase(
                "stall-tab-tab",
                "tab_tab_300ms",
                Lead::TabTab,
                Wait::ServesSent,
                300,
            )
        },
    ),
    (
        "stall::tab_tab_and_a_letter_across_a_second_field_while_the_program_waits_after_the_first_tab",
        Phase {
            letters_through_the_wait: false,
            ..phase(
                "stall-tab-tab",
                "tab_tab_300ms",
                Lead::TabTab,
                Wait::Sleeps,
                300,
            )
        },
    ),
    (
        "stall::tab_and_letters_while_a_window_of_another_thread_comes_to_the_front",
        Phase {
            other_front: true,
            letters_through_the_wait: false,
            ..phase(
                "stall-other-front",
                "other_front_300ms",
                Lead::Tab,
                Wait::ServesSent,
                300,
            )
        },
    ),
    (
        "stall::tab_and_letters_while_a_window_of_another_thread_comes_to_the_front",
        Phase {
            other_front: true,
            letters_through_the_wait: false,
            ..phase(
                "stall-other-front",
                "other_front_300ms",
                Lead::Tab,
                Wait::Sleeps,
                300,
            )
        },
    ),
    (
        "stall::enter_with_no_focus_move_then_five_letters_records_no_letter",
        Phase {
            letters_through_the_wait: false,
            letters_after: 4,
            landing: Landing::Plain,
            checks: Checks::Enter,
            ..phase(
                "stall-enter",
                "enter_then_five_letters",
                Lead::Enter,
                Wait::Sleeps,
                0,
            )
        },
    ),
    (
        "stall::a_second_tab_out_of_the_password_field_then_a_letter_records_nothing",
        Phase {
            letters_through_the_wait: false,
            start_in_password: true,
            landing: Landing::Plain,
            checks: Checks::TabOut,
            ..phase(
                "stall-tab-out",
                "tab_out_then_a_letter",
                Lead::Tab,
                Wait::Sleeps,
                0,
            )
        },
    ),
    (
        "stall::the_program_moves_the_focus_in_two_steps_and_no_letter_is_recorded",
        Phase {
            two_step: true,
            letters_through_the_wait: false,
            ..phase(
                "stall-two-step",
                "two_step",
                Lead::Nothing,
                Wait::Sleeps,
                0,
            )
        },
    ),
];

pub(super) const REPLACED_NATIVE: [(&str, (&str, &str)); 2] = [
    (
        "stall::an_edit_masked_while_it_has_the_focus_records_no_letter_after_the_mask",
        NATIVE_PHASES[0],
    ),
    (
        "stall::a_focus_notice_that_the_system_does_not_deliver_changes_nothing",
        NATIVE_PHASES[1],
    ),
];

const OWN: u32 = 4;
const OTHER: u32 = 9;

fn control(session: u32, at: u32) -> ControlRead {
    ControlRead {
        step: Step { session, at },
        recorded: CONTROL_LETTERS,
    }
}

fn controls(before: ControlRead, after: ControlRead) -> Controls {
    Controls {
        before,
        after,
        late_at_the_end: 0,
    }
}

fn passing() -> Totals {
    Totals {
        rounds: 2,
        counted: 2,
        withheld_set: 1,
        ..Totals::default()
    }
}

fn run(session: u32, at: u32, outcome: Outcome) -> PhaseRun {
    PhaseRun {
        test: "stall-tab-sleeps-150",
        name: "tab_sleeps_150ms",
        wait: Wait::Sleeps,
        step: Step { session, at },
        outcome,
    }
}

fn rounds(session: u32, at: u32) -> PhaseRun {
    run(
        session,
        at,
        Outcome::Rounds {
            totals: passing(),
            checks: Checks::Stall,
        },
    )
}

#[test]
#[ignore = "a made-up phase of a shared session: 50-windows-functional runs it by its exact name"]
fn a_phase_between_the_two_controls_of_its_own_session_passes() {
    let pair = controls(control(OWN, 0), control(OWN, 3));
    for at in [1, 2] {
        assert_eq!(verdict(&rounds(OWN, at), &pair), Ok(()));
    }
}

#[test]
#[ignore = "a made-up phase of a shared session: 50-windows-functional runs it by its exact name"]
fn a_phase_that_the_controls_of_another_session_bracket_fails() {
    let lent = controls(control(OTHER, 0), control(OTHER, 3));
    let got = verdict(&rounds(OWN, 1), &lent);
    assert!(
        got.as_ref()
            .is_err_and(|why| why.contains("not its own session 4")),
        "a phase of session {OWN} passed on the controls of session {OTHER}"
    );
}

#[test]
#[ignore = "a made-up phase of a shared session: 50-windows-functional runs it by its exact name"]
fn a_phase_whose_two_controls_come_from_two_sessions_fails() {
    for pair in [
        controls(control(OWN, 0), control(OTHER, 3)),
        controls(control(OTHER, 0), control(OWN, 3)),
    ] {
        assert!(
            verdict(&rounds(OWN, 1), &pair).is_err(),
            "a phase passed on a control of another session"
        );
    }
}

#[test]
#[ignore = "a made-up phase of a shared session: 50-windows-functional runs it by its exact name"]
fn a_phase_before_the_before_control_or_after_the_after_control_fails() {
    let pair = controls(control(OWN, 1), control(OWN, 3));
    for at in [0, 1, 3, 4] {
        let got = verdict(&rounds(OWN, at), &pair);
        assert!(
            got.as_ref()
                .is_err_and(|why| why.contains("the controls do not bracket it")),
            "a phase at step {at} passed on controls at steps 1 and 3"
        );
    }
}

#[test]
#[ignore = "a made-up phase of a shared session: 50-windows-functional runs it by its exact name"]
fn a_phase_whose_control_recorded_fewer_than_five_letters_fails() {
    for (before, after) in [(4, 5), (5, 4), (0, 0)] {
        let pair = controls(
            ControlRead {
                recorded: before,
                ..control(OWN, 0)
            },
            ControlRead {
                recorded: after,
                ..control(OWN, 3)
            },
        );
        let got = verdict(&rounds(OWN, 1), &pair);
        assert!(
            got.as_ref().is_err_and(
                |why| why.contains(&format!("control_before={before} control_after={after}"))
            ),
            "a phase passed on controls that recorded {before} and {after} letters"
        );
    }
}

#[test]
#[ignore = "a made-up phase of a shared session: 50-windows-functional runs it by its exact name"]
fn each_check_of_a_phase_fails_its_own_fault_in_each_family() {
    let pair = controls(control(OWN, 0), control(OWN, 2));
    let good = passing();
    let faults = [
        (
            Checks::Stall,
            Totals {
                sink_differs: 1,
                ..good
            },
            "the sink held",
        ),
        (
            Checks::Stall,
            Totals { counted: 0, ..good },
            "no round counted",
        ),
        (
            Checks::Stall,
            Totals {
                letters_recorded: 1,
                ..good
            },
            "a letter typed ahead",
        ),
        (
            Checks::Stall,
            Totals {
                other_recorded: 1,
                ..good
            },
            "another class than Tab",
        ),
        (
            Checks::Stall,
            Totals {
                late_moved: 1,
                ..good
            },
            "moved during the phase",
        ),
        (
            Checks::Stall,
            Totals { counted: 1, ..good },
            "not every round counted",
        ),
        (
            Checks::Enter,
            Totals {
                sink_differs: 1,
                ..good
            },
            "the sink held",
        ),
        (
            Checks::Enter,
            Totals {
                letters_recorded: 1,
                ..good
            },
            "a letter after Enter",
        ),
        (
            Checks::Enter,
            Totals {
                withheld_set: 0,
                ..good
            },
            "keys_withheld did not read yes",
        ),
        (
            Checks::Enter,
            Totals {
                late_moved: 1,
                ..good
            },
            "late value moved",
        ),
        (
            Checks::Enter,
            Totals { counted: 1, ..good },
            "not every round counted",
        ),
        (
            Checks::TabOut,
            Totals {
                sink_differs: 1,
                ..good
            },
            "the sink held",
        ),
        (
            Checks::TabOut,
            Totals {
                letters_recorded: 1,
                ..good
            },
            "the letter after the Tab out",
        ),
        (
            Checks::TabOut,
            Totals {
                tabs_recorded: 1,
                ..good
            },
            "the Tab out of the password field",
        ),
        (
            Checks::TabOut,
            Totals {
                other_recorded: 1,
                ..good
            },
            "a key of another class was",
        ),
        (
            Checks::TabOut,
            Totals {
                late_moved: 1,
                ..good
            },
            "late value moved",
        ),
        (
            Checks::TabOut,
            Totals { counted: 1, ..good },
            "not every round counted",
        ),
        (
            Checks::ControlsAndSink,
            Totals {
                sink_differs: 1,
                ..good
            },
            "the sink held",
        ),
    ];
    for checks in [
        Checks::Stall,
        Checks::Enter,
        Checks::TabOut,
        Checks::ControlsAndSink,
    ] {
        let clean = run(
            OWN,
            1,
            Outcome::Rounds {
                totals: good,
                checks,
            },
        );
        assert_eq!(verdict(&clean, &pair), Ok(()), "{checks:?}");
    }
    let mut missed = Vec::new();
    for (index, (checks, totals, why)) in faults.into_iter().enumerate() {
        let got = verdict(&run(OWN, 1, Outcome::Rounds { totals, checks }), &pair);
        if !got.as_ref().is_err_and(|got| got.contains(why)) {
            missed.push(index);
        }
    }
    assert!(
        missed.is_empty(),
        "faults that passed by their index: {missed:?}"
    );
}

#[test]
#[ignore = "a made-up phase of a shared session: 50-windows-functional runs it by its exact name"]
fn a_native_phase_fails_each_of_its_own_faults() {
    let pair = controls(control(OWN, 0), control(OWN, 2));
    let native = |rounds, counted, recorded| {
        run(
            OWN,
            1,
            Outcome::Native {
                rounds,
                counted,
                recorded,
            },
        )
    };
    assert_eq!(verdict(&native(5, 4, 0), &pair), Ok(()));
    assert_eq!(verdict(&native(1, 1, 0), &pair), Ok(()));
    let late = Controls {
        late_at_the_end: 1,
        ..pair
    };
    let faults = [
        (native(5, 3, 0), pair, "counted 3 rounds of 5"),
        (native(1, 0, 0), pair, "counted 0 rounds of 1"),
        (
            native(5, 5, 1),
            pair,
            "a letter of a password field was recorded",
        ),
        (native(5, 5, 0), late, "late value moved"),
    ];
    let mut missed = Vec::new();
    for (index, (phase, pair, why)) in faults.into_iter().enumerate() {
        if !verdict(&phase, &pair)
            .as_ref()
            .is_err_and(|got| got.contains(why))
        {
            missed.push(index);
        }
    }
    assert!(
        missed.is_empty(),
        "faults that passed by their index: {missed:?}"
    );
}

#[test]
#[ignore = "a made-up phase of a shared session: 50-windows-functional runs it by its exact name"]
fn the_kinds_hold_one_phase_for_each_test_and_sub_case_that_they_replace() {
    let built: Vec<Phase> = [
        Kind::TabSleeps,
        Kind::TabServes,
        Kind::ClickSleeps,
        Kind::ClickServes,
        Kind::SecondFieldAndOtherFront,
        Kind::EnterTabOutAndTwoStep,
    ]
    .into_iter()
    .flat_map(Kind::phases)
    .collect();
    let replaced: Vec<Phase> = REPLACED.iter().map(|(_, phase)| *phase).collect();
    let tests: std::collections::BTreeSet<&str> = REPLACED
        .iter()
        .map(|(test, _)| *test)
        .chain(REPLACED_NATIVE.iter().map(|(test, _)| *test))
        .collect();
    assert_eq!(
        (replaced.len() + REPLACED_NATIVE.len(), tests.len()),
        (29, 27),
        "the table of the replaced tests"
    );
    let missing: Vec<usize> = (0..replaced.len())
        .filter(|i| built.iter().filter(|p| **p == replaced[*i]).count() != 1)
        .collect();
    assert!(
        missing.is_empty() && built.len() == replaced.len(),
        "phases of the replaced tests that the kinds do not hold once, by their index: {missing:?}; the kinds hold {} phases",
        built.len()
    );
    let native: Vec<(&str, &str)> = REPLACED_NATIVE.iter().map(|(_, phase)| *phase).collect();
    assert_eq!(native, NATIVE_PHASES.to_vec(), "the native phases");
}
