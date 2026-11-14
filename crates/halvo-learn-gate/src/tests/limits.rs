use halvo_schema::executable::ExecutableStep;
use serde_json::Value;

use crate::case::{read_case, read_dir, Case, Kind, Verb};
use crate::drift::{verdict, Gate};
use crate::page::{Control, Locator, Role};
use crate::store::Family;
use crate::stub::{compiled_step, play_all, play_case, recovered_step, Arm, Play, Tally};
use crate::tests::reader::{calendar_days_min, repo};

const TARGETS: &str = include_str!("../../../../docs/r5/targets.toml");
const RENAMED: &str = include_str!("../../../../fixtures/r5/cases/dev/renamed-pay.jsonl");

fn read(text: &str) -> Result<Case, String> {
    read_case("renamed-pay.jsonl", text.as_bytes(), calendar_days_min())
}

fn day_of(line: &str) -> u32 {
    let rest = line.strip_prefix("{\"day\":").expect("an occurrence line");
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().expect("a day")
}

fn with_day(line: &str, day: u32) -> String {
    format!(
        "{{\"day\":{day}{}",
        &line[7 + day_of(line).to_string().len()..]
    )
}

/// The case with `more` copies of its last occurrence appended, one day apart.
fn longer(more: u32) -> String {
    let lines: Vec<&str> = RENAMED.lines().collect();
    let last = lines[lines.len() - 1];
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    for i in 1..=more {
        out.push(with_day(last, day_of(last) + i));
    }
    out.join("\n") + "\n"
}

/// The case with its last occurrence moved to `span` days after its first.
fn spanning(span: u32) -> String {
    let lines: Vec<&str> = RENAMED.lines().collect();
    let first = day_of(lines[1]);
    let mut out: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
    let last = out.len() - 1;
    out[last] = with_day(lines[last], first + span);
    out.join("\n") + "\n"
}

#[test]
fn reader_takes_16_occurrences_and_refuses_17() {
    assert_eq!(RENAMED.lines().count(), 9);
    assert!(read(&longer(8)).is_ok(), "{:?}", read(&longer(8)));
    assert!(read(&longer(9)).is_err());
}

#[test]
fn reader_takes_a_span_of_calendar_days_min_and_refuses_one_day_less() {
    assert_eq!(calendar_days_min(), 240);
    assert!(read(&spanning(240)).is_ok(), "{:?}", read(&spanning(240)));
    assert!(read(&spanning(239)).is_err());
}

#[test]
fn drift_plays_a_case_whose_last_day_is_the_largest_day() {
    let lines: Vec<&str> = RENAMED.lines().collect();
    let first = day_of(lines[1]);
    let far = read(&spanning(u32::MAX - first)).expect("a case whose last day is 4294967295");
    assert_eq!(far.occurrences[far.occurrences.len() - 1].day, u32::MAX);
    let near = read(RENAMED).expect("the development case");
    for arm in Arm::ALL {
        let got = play_case(&far, arm, false).expect("every occurrence completes");
        let want = play_case(&near, arm, false).expect("every occurrence completes");
        assert_eq!(got.counts, want.counts, "{}", arm.as_str());
        assert_eq!(
            (got.reverted_good, got.edit_rows),
            (want.reverted_good, want.edit_rows)
        );
    }
}

fn button(id: &str, name: &str, test_id: &str) -> Control {
    Control {
        id: id.to_owned(),
        role: Role::Button,
        name: name.to_owned(),
        test_id: test_id.to_owned(),
        passes: Vec::new(),
    }
}

#[test]
fn page_locator_resolves_a_single_match_alone() {
    let page = [button("a", "Pay", "t-1"), button("b", "Pay", "t-2")];
    assert_eq!(
        Locator::Role(Role::Button, "Pay".into()).resolve(&page),
        None
    );
    let found = Locator::TestId("t-2".into())
        .resolve(&page)
        .map(|c| c.id.as_str());
    assert_eq!(found, Some("b"));
}

#[test]
fn stub_logs_a_timeout_before_the_retry_of_a_flaky_step() {
    let cases =
        read_dir(&repo().join("fixtures/r5/cases/dev"), calendar_days_min()).expect("the cases");
    let flaky = cases
        .iter()
        .find(|c| c.name == "flaky-amount")
        .expect("a flaky case");
    let mut play = Play::new(flaky, Arm::Admit, 1, crate::store::MemoryStore::new());
    play_all(&mut play, false).expect("a play");
    let timeouts = play
        .host
        .store
        .rows(Family::Agent)
        .all()
        .filter(|(kind, json)| *kind == "agent_log" && json.contains("\"cause\":\"timeout\""))
        .count();
    assert_eq!(timeouts, 1);
}

#[test]
fn stub_recovered_step_moves_a_held_locator_to_the_front() {
    let control = button("go", "Pay now", "pay-1");
    let step = compiled_step(1, Verb::Click, &control).expect("a compiled step");
    let served = ExecutableStep::parse(step.to_string().as_bytes()).expect("a step");
    let recovered = recovered_step(&served, &control).expect("a recovered step");
    let locators = recovered["target"]["locators"]
        .as_array()
        .expect("locators")
        .clone();
    let by: Vec<&str> = locators.iter().filter_map(|l| l["by"].as_str()).collect();
    assert_eq!(by, ["role", "test_id"]);
    assert_eq!(
        recovered["target"]["desc"],
        Value::String("button \"Pay now\"".into())
    );
}

#[test]
fn drift_counts_a_tie_of_harm_as_no_win() {
    let targets = crate::targets::parse(TARGETS.as_bytes()).expect("the targets file");
    let cases =
        read_dir(&repo().join("fixtures/r5/cases/dev"), calendar_days_min()).expect("the cases");
    let mut played: Vec<(Case, [Tally; 3])> = cases
        .into_iter()
        .map(|case| {
            let tallies = Arm::ALL.map(|arm| play_case(&case, arm, false).expect("a play"));
            (case, tallies)
        })
        .collect();
    let fault = played
        .iter_mut()
        .find(|(c, _)| c.kind == Kind::Fault)
        .expect("a fault case");
    fault.1[2].counts.harm = fault.1[1].counts.harm;
    let (lines, ok) = verdict(&played, &targets, Gate::Dev);
    assert_eq!(
        lines[1],
        "rollback cases=8 wins=7 losses=0 critical=7 cost=0 budget=1"
    );
    assert!(ok);
}
