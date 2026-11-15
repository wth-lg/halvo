#![cfg(target_os = "windows")]

#[path = "browser/evidence.rs"]
mod evidence;

use evidence::{
    after_start_state_met, cold_evidence, newest_mark_of, second_control_allowed, AfterStart,
    Letter, Mark, Reading, Seen, Typed,
};

fn seen(what: &'static str, password: bool, element: Option<&str>, at: u32) -> Seen {
    Seen {
        what,
        password,
        element: element.map(str::to_string),
        at,
    }
}

const CHROME_PAGE: &str = "framework=Chrome window_class=Chrome_WidgetWin_1 file=chrome.exe kind=Chrome is_password=0 protected=0 pid_read=1 file_read=ok";
const CHROME_INPUT: &str = "framework=Chrome window_class=Chrome_WidgetWin_1 file=chrome.exe kind=Chrome is_password=1 protected=1 pid_read=1 file_read=ok";

fn recorded(read_at: u32) -> Vec<Seen> {
    vec![
        seen("event", false, Some(CHROME_PAGE), read_at - 172),
        seen("event", true, Some(CHROME_INPUT), read_at - 12),
        seen("read", true, Some(CHROME_PAGE), read_at),
    ]
}

fn run_130() -> Vec<Seen> {
    recorded(10_000 - 1_828)
}

fn run_142() -> Vec<Seen> {
    recorded(10_000 - 1_844)
}

#[test]
fn a_kept_reading_of_the_page_after_a_reading_of_the_input_is_a_reading_of_the_password_field() {
    assert_eq!(cold_evidence(&run_130(), 10_000, 10_000), Ok(1));
    assert_eq!(cold_evidence(&run_142(), 10_000, 10_000), Ok(1));
}

#[test]
fn a_reading_of_the_page_with_no_reading_of_the_input_before_it_is_no_evidence() {
    let mut marks = run_130();
    marks.remove(1);
    marks.insert(1, seen("read", true, None, 8_160));
    assert!(cold_evidence(&marks, 10_000, 10_000).is_err());
}

#[test]
fn a_reading_of_the_page_after_a_clear_mark_is_no_evidence() {
    let mut marks = run_130();
    marks.insert(2, seen("read", false, None, 8_165));
    marks.push(seen("read", true, Some(CHROME_PAGE), 8_180));
    assert!(cold_evidence(&marks, 10_000, 10_000).is_err());
}

#[test]
fn a_reading_of_the_page_after_a_focus_move_is_no_evidence() {
    for moved in ["event", "latch-input", "latch-other"] {
        let mut marks = run_130();
        marks.insert(2, seen(moved, true, None, 8_165));
        assert!(cold_evidence(&marks, 10_000, 10_000).is_err(), "{moved}");
    }
}

const PAGE: &str = "framework=Chrome window_class=Chrome_WidgetWin_1 file=msedge.exe kind=Edge is_password=0 protected=0";

fn run_28() -> Vec<Seen> {
    vec![
        seen("event", false, Some(PAGE), 9_500),
        seen("read", true, None, 9_700),
        seen("event", true, None, 9_800),
        seen("read", true, None, 9_900),
    ]
}

#[test]
fn the_marks_of_run_28_are_a_reading_of_the_password_field() {
    assert_eq!(cold_evidence(&run_28(), 10_000, 10_000), Ok(1));
}

#[test]
fn a_mark_that_names_a_plain_element_in_the_span_is_no_evidence() {
    let mut marks = run_28();
    marks.push(seen("event", true, Some(PAGE), 9_950));
    assert!(cold_evidence(&marks, 10_000, 10_000).is_err());
}

#[test]
fn a_mark_that_reads_another_state_in_the_span_is_no_evidence() {
    let mut marks = run_28();
    marks.push(seen("read", false, None, 9_950));
    assert!(cold_evidence(&marks, 10_000, 10_000).is_err());
}

#[test]
fn a_span_with_no_mark_before_the_letter_is_no_evidence() {
    let marks = vec![seen("read", true, None, 10_100)];
    assert!(cold_evidence(&marks, 10_000, 10_200).is_err());
}

#[test]
fn the_marks_up_to_the_deciding_point_are_read_and_no_later_one() {
    let mut marks = run_28();
    marks.push(seen("read", true, None, 10_130));
    marks.push(seen("read", false, None, 10_400));
    assert_eq!(cold_evidence(&marks, 10_000, 10_200), Ok(2));
    assert!(cold_evidence(&marks, 10_000, 10_400).is_err());
}

#[test]
fn the_older_mark_of_the_page_is_what_the_old_pick_read() {
    assert_eq!(newest_mark_of(&run_28(), "msedge.exe"), PAGE);
}

#[test]
fn the_span_is_read_across_the_wrap_of_the_tick_count() {
    let marks = vec![
        seen("read", true, None, u32::MAX - 10),
        seen("read", true, None, 20),
    ];
    assert_eq!(cold_evidence(&marks, 5, 30), Ok(2));
}

fn letters_from(first: u32, step: u32) -> Vec<Letter> {
    (0..5)
        .map(|k| Letter {
            input: first.wrapping_add(k * step),
            recorded: false,
            not_cleared_in_time: true,
        })
        .collect()
}

fn mark(what: &'static str, reading: Reading, began: u32, at: u32) -> Mark {
    Mark {
        what,
        reading: Some(reading),
        another_element: false,
        began,
        at: Some(at),
    }
}

fn late_reads(first: u32) -> Vec<Mark> {
    vec![
        mark("event", Reading::Clear, first - 500, first - 500),
        mark("read", Reading::Clear, first - 500, first - 480),
        mark("read", Reading::Clear, first + 120, first + 305),
        mark("read", Reading::Clear, first + 310, first + 450),
    ]
}

#[test]
fn a_short_control_whose_reads_were_asked_on_time_and_answered_late_takes_a_second_control() {
    let first = 10_000;
    assert_eq!(
        second_control_allowed(&letters_from(first, 40), &late_reads(first), true, 0, 0),
        Ok(())
    );
}

#[test]
fn the_reckoned_ticks_of_chromes_run_4_take_a_second_control() {
    let first = 729_570;
    let marks = vec![
        mark("event", Reading::Clear, 729_296, 729_296),
        mark("read", Reading::Clear, 729_296, 729_296),
        mark("read", Reading::Clear, 729_690, 729_875),
        mark("read", Reading::Clear, 729_875, 730_000),
    ];
    assert_eq!(
        second_control_allowed(&letters_from(first, 40), &marks, true, 0, 0),
        Ok(())
    );
}

#[test]
fn each_condition_taken_away_refuses_the_second_control() {
    let first = 10_000;
    let letters = letters_from(first, 40);
    let marks = late_reads(first);
    let mut other_rule = letters.clone();
    other_rule[2].not_cleared_in_time = false;
    let mut latch = marks.clone();
    latch.push(Mark {
        what: "latch-input",
        reading: None,
        another_element: false,
        began: first + 60,
        at: None,
    });
    let mut password = marks.clone();
    password.push(mark("event", Reading::Password, first + 90, first + 90));
    let mut unknown = marks.clone();
    unknown[2].reading = Some(Reading::Unknown);
    let mut another = marks.clone();
    another[2].another_element = true;
    let mut asked_late = marks.clone();
    asked_late[2] = mark("read", Reading::Clear, first + 200, first + 305);
    let mut answered_early = marks.clone();
    answered_early[3] = mark("read", Reading::Clear, first + 310, first + 360);
    let mut recorded = letters.clone();
    recorded[0].recorded = true;
    let mut waited_on_password = marks.clone();
    waited_on_password[3].reading = Some(Reading::Password);
    let cases: [(&str, Result<(), &str>); 11] = [
        (
            "the read waited on answered Password after the letters",
            second_control_allowed(&letters, &waited_on_password, true, 0, 0),
        ),
        (
            "another rule",
            second_control_allowed(&other_rule, &marks, true, 0, 0),
        ),
        (
            "a latch open before",
            second_control_allowed(&letters, &marks, true, 1, 0),
        ),
        (
            "a latch among the letters",
            second_control_allowed(&letters, &latch, true, 0, 0),
        ),
        (
            "the gate not Clear before",
            second_control_allowed(&letters, &marks, false, 0, 0),
        ),
        (
            "a Password event among the letters",
            second_control_allowed(&letters, &password, true, 0, 0),
        ),
        (
            "an Unknown read among the letters",
            second_control_allowed(&letters, &unknown, true, 0, 0),
        ),
        (
            "a read of another element",
            second_control_allowed(&letters, &another, true, 0, 0),
        ),
        (
            "a read not asked on time",
            second_control_allowed(&letters, &asked_late, true, 0, 0),
        ),
        (
            "a read answered within 250 ms",
            second_control_allowed(&letters, &answered_early, true, 0, 0),
        ),
        (
            "the sink differs from the seam",
            second_control_allowed(&recorded, &marks, true, 0, 0),
        ),
    ];
    for (name, verdict) in cases {
        assert!(verdict.is_err(), "{name}: {verdict:?}");
    }
    assert_eq!(
        second_control_allowed(&recorded, &marks, true, 0, 1),
        Ok(()),
        "four missing letters of the same case"
    );
}

#[test]
fn drawn_short_controls_take_a_second_control_only_while_all_four_conditions_hold() {
    let mut draw: u64 = 0x5eed_0317;
    let mut next = move |below: u32| {
        draw = draw
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((draw >> 33) % u64::from(below)) as u32
    };
    for case in 0..400 {
        let first = 1_000_000u32.wrapping_add(next(u32::MAX / 2));
        let step = 30 + next(40);
        let letters = letters_from(first, step);
        let last = letters[4].input;
        let latency = 260 + next(200);
        let mut marks = vec![mark("event", Reading::Clear, first - 400, first - 400)];
        let mut began = first + 120 + next(20);
        let mut answered = began + latency;
        marks.push(mark("read", Reading::Clear, began, answered));
        while answered < last + 120 + 400 {
            began = answered + next(25);
            answered = began + latency;
            marks.push(mark("read", Reading::Clear, began, answered));
        }
        let allowed = second_control_allowed(&letters, &marks, true, 0, 0);
        assert_eq!(
            allowed,
            Ok(()),
            "case {case}: step {step} latency {latency}"
        );
        let mut broken = marks.clone();
        let refused = match next(4) {
            0 => {
                let mut other = letters.clone();
                other[next(5) as usize].not_cleared_in_time = false;
                second_control_allowed(&other, &marks, true, 0, 0)
            }
            1 => second_control_allowed(&letters, &marks, true, 1 + next(3) as usize, 0),
            2 => second_control_allowed(&letters, &marks, false, 0, 0),
            _ => {
                broken[1].reading = Some(Reading::Password);
                second_control_allowed(&letters, &broken, true, 0, 0)
            }
        };
        assert!(
            refused.is_err(),
            "case {case}: a condition taken away was allowed"
        );
    }
}

fn after_start_marks() -> Vec<Mark> {
    vec![
        mark("start", Reading::Unknown, 1_495_765, 1_495_765),
        mark("event", Reading::Unknown, 1_495_796, 1_495_796),
        mark("read", Reading::Unknown, 1_495_796, 1_495_796),
        mark("event", Reading::Unknown, 1_496_390, 1_496_390),
        mark("read", Reading::Unknown, 1_496_390, 1_496_390),
    ]
}

fn withheld_letters() -> Vec<Typed> {
    vec![
        Typed {
            recorded: false,
            state_at_the_key: true,
        };
        5
    ]
}

fn met<'a>(letters: &'a [Typed], marks: &'a [Mark]) -> AfterStart<'a> {
    AfterStart {
        letters,
        marks,
        gate_unknown_before: true,
        open_latches_before: 0,
        front_tick: 1_497_000,
        handler_counts_unchanged: true,
        page_moved_its_focus: true,
    }
}

#[test]
fn the_marks_of_run_43662069989_with_a_front_tick_after_them_meet_the_state_after_the_start() {
    let letters = withheld_letters();
    let marks = after_start_marks();
    assert_eq!(after_start_state_met(&met(&letters, &marks)), Ok(()));
}

#[test]
fn each_condition_of_the_state_after_the_start_taken_away_refuses_the_reopened_page() {
    let letters = withheld_letters();
    let marks = after_start_marks();
    let mut recorded = letters.clone();
    recorded[3].recorded = true;
    let mut other_rule = letters.clone();
    other_rule[1].state_at_the_key = false;
    let mut latch = marks.clone();
    latch.push(Mark {
        what: "latch-other",
        reading: None,
        another_element: false,
        began: 1_496_000,
        at: None,
    });
    let mut event_after_the_front = marks.clone();
    event_after_the_front.push(mark("event", Reading::Clear, 1_497_100, 1_497_100));
    let mut read_after_the_front = marks.clone();
    read_after_the_front.push(mark("read", Reading::Unknown, 1_497_050, 1_497_200));
    let cases: [(&str, AfterStart); 9] = [
        ("no letter", met(&[], &marks)),
        ("a letter recorded", met(&recorded, &marks)),
        ("a letter by another rule", met(&other_rule, &marks)),
        ("a latch mark since the start", met(&letters, &latch)),
        (
            "an event since the front",
            met(&letters, &event_after_the_front),
        ),
        (
            "an own read since the front",
            met(&letters, &read_after_the_front),
        ),
        (
            "the gate not Unknown",
            AfterStart {
                gate_unknown_before: false,
                ..met(&letters, &marks)
            },
        ),
        (
            "a latch open before",
            AfterStart {
                open_latches_before: 1,
                ..met(&letters, &marks)
            },
        ),
        (
            "the handler counts moved",
            AfterStart {
                handler_counts_unchanged: false,
                ..met(&letters, &marks)
            },
        ),
    ];
    for (name, seen) in cases {
        assert!(after_start_state_met(&seen).is_err(), "{name}");
    }
    assert!(
        after_start_state_met(&AfterStart {
            page_moved_its_focus: false,
            ..met(&letters, &marks)
        })
        .is_err(),
        "no record of the page's own move"
    );
}
