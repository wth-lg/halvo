use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::*;
use crate::case::{Form, Kind};
use crate::forms::{self, paragraph};
use crate::model::{Sort, Tally};
use crate::targets::{parse, Targets};
use crate::trial::{completed, rule, Row, Rule};

const REPO_TARGETS: &str = include_str!("../../../../docs/r5/targets.toml");

fn targets() -> Targets {
    parse(REPO_TARGETS).expect("the targets file of the repository parses")
}

fn edited(from: &str, to: &str) -> String {
    assert!(REPO_TARGETS.contains(from), "{from}");
    REPO_TARGETS.replacen(from, to, 1)
}

fn refused_input(text: &str) -> String {
    match parse(text) {
        Err(Refusal::Input(detail)) => detail,
        other => panic!("not refused with input: {other:?}"),
    }
}

#[test]
fn the_targets_file_of_the_repository_gives_the_five_keys_of_the_trial() {
    assert_eq!(
        targets(),
        Targets {
            dev_benign: 8,
            dev_fault: 8,
            dev_critical: 7,
            dev_cost_budget: 1,
            calendar_days_min: 240,
        }
    );
}

type Field = fn(&Targets) -> u64;

#[test]
fn each_key_that_the_trial_reads_comes_from_its_own_line() {
    let read: [(&str, &str, Field); 5] = [
        ("dev_benign = 8\n", "dev_benign = 11\n", |t| t.dev_benign),
        ("dev_fault = 8\n", "dev_fault = 11\n", |t| t.dev_fault),
        ("dev_critical = 7\n", "dev_critical = 11\n", |t| {
            t.dev_critical
        }),
        ("dev_cost_budget = 1\n", "dev_cost_budget = 11\n", |t| {
            t.dev_cost_budget
        }),
        (
            "calendar_days_min = 240\n",
            "calendar_days_min = 11\n",
            |t| t.calendar_days_min,
        ),
    ];
    for (from, to, field) in read {
        let changed = parse(&edited(from, to)).expect("an edited value parses");
        assert_eq!(field(&changed), 11, "{to}");
    }
}

#[test]
fn the_targets_reader_refuses_an_extra_key() {
    let detail = refused_input(&edited(
        "dev_cost_budget = 1\n",
        "dev_cost_budget = 1\ndev_spare = 1\n",
    ));
    assert_eq!(
        detail,
        "targets line 11 is not heldout_benign = <integer> of [gate]"
    );
}

#[test]
fn the_targets_reader_refuses_a_missing_key_that_the_trial_does_not_read() {
    let detail = refused_input(&edited("value_min = 4\n", ""));
    assert_eq!(
        detail,
        "targets line 5 is not value_min = <integer> of [rule]"
    );
}

#[test]
fn the_targets_reader_refuses_two_keys_swapped() {
    let detail = refused_input(&edited(
        "dev_benign = 8\ndev_fault = 8\n",
        "dev_fault = 8\ndev_benign = 8\n",
    ));
    assert_eq!(
        detail,
        "targets line 7 is not dev_benign = <integer> of [gate]"
    );
}

#[test]
fn the_targets_reader_refuses_a_key_in_the_wrong_table() {
    let moved = edited("calendar_days_min = 240\n", "").replacen(
        "[routine]\n",
        "[routine]\ncalendar_days_min = 240\n",
        1,
    );
    let detail = refused_input(&moved);
    assert_eq!(
        detail,
        "targets line 15 is not calendar_days_min = <integer> of [gate]"
    );
}

#[test]
fn the_targets_reader_refuses_an_extra_table() {
    let detail = refused_input(&format!("{REPO_TARGETS}[extra]\n"));
    assert_eq!(detail, "targets line 26 is not of the form of L8.1");
}

#[test]
fn the_targets_reader_refuses_a_value_that_is_not_an_integer_of_the_line_form() {
    for to in [
        "dev_benign = 08\n",
        "dev_benign = -8\n",
        "dev_benign = 8.0\n",
        "dev_benign =8\n",
    ] {
        refused_input(&edited("dev_benign = 8\n", to));
    }
}

#[test]
fn the_targets_reader_refuses_text_that_is_not_lf_text() {
    refused_input(&REPO_TARGETS.replace('\n', "\r\n"));
    refused_input(REPO_TARGETS.trim_end_matches('\n'));
}

fn tally(assisted: u32, harm: u32) -> Tally {
    Tally {
        occurrences: 8,
        drifts: 1,
        assisted,
        harm,
        ..Tally::default()
    }
}

fn passing() -> Vec<Row> {
    let mut rows = Vec::new();
    for _ in 0..8 {
        rows.push(Row {
            kind: Kind::Benign,
            plain: tally(1, 0),
            admit: tally(1, 0),
        });
    }
    for _ in 0..8 {
        rows.push(Row {
            kind: Kind::Fault,
            plain: tally(3, 3),
            admit: tally(3, 2),
        });
    }
    rows
}

#[test]
fn the_rule_is_admit_when_both_clauses_hold() {
    assert_eq!(rule(&passing(), &targets()), Rule::Admit);
}

#[test]
fn the_rule_is_admit_with_wins_in_dev_critical_fault_cases_and_a_tie_in_the_rest() {
    let mut rows = passing();
    rows[8].admit.harm = 3;
    assert_eq!(rule(&rows, &targets()), Rule::Admit);
}

#[test]
fn the_rule_is_plain_with_wins_in_fewer_than_dev_critical_fault_cases() {
    let mut rows = passing();
    rows[8].admit.harm = 3;
    rows[9].admit.harm = 3;
    assert_eq!(rule(&rows, &targets()), Rule::Plain);
}

#[test]
fn the_rule_is_plain_when_admit_has_more_harm_than_plain_in_one_fault_case() {
    let mut rows = passing();
    rows[8].admit.harm = 4;
    assert_eq!(rule(&rows, &targets()), Rule::Plain);
}

#[test]
fn the_rule_is_admit_when_the_benign_assisted_of_admit_is_that_of_plain_plus_the_budget() {
    let mut rows = passing();
    rows[0].admit.assisted = 2;
    assert_eq!(rule(&rows, &targets()), Rule::Admit);
}

#[test]
fn the_rule_is_plain_when_the_benign_assisted_of_admit_passes_that_of_plain_plus_the_budget() {
    let mut rows = passing();
    rows[0].admit.assisted = 2;
    rows[1].admit.assisted = 2;
    assert_eq!(rule(&rows, &targets()), Rule::Plain);
}

#[test]
fn the_rule_takes_dev_critical_from_the_targets() {
    let mut raised = targets();
    raised.dev_critical = 9;
    assert_eq!(rule(&passing(), &raised), Rule::Plain);
}

#[test]
fn the_rule_takes_dev_cost_budget_from_the_targets() {
    let mut rows = passing();
    rows[0].admit.assisted = 2;
    rows[1].admit.assisted = 2;
    let mut raised = targets();
    raised.dev_cost_budget = 2;
    assert_eq!(rule(&rows, &raised), Rule::Admit);
}

#[test]
fn the_rule_reads_harm_on_the_fault_cases_alone() {
    let mut rows = passing();
    for row in &mut rows[..8] {
        row.plain.harm = 5;
    }
    for row in &mut rows[8..] {
        row.admit.harm = 3;
    }
    assert_eq!(rule(&rows, &targets()), Rule::Plain);
}

#[test]
fn the_rule_sums_assisted_on_the_benign_cases_alone() {
    let mut rows = passing();
    for row in &mut rows[8..] {
        row.admit.assisted = 9;
    }
    assert_eq!(rule(&rows, &targets()), Rule::Admit);
}

#[test]
fn a_line_with_failed_above_0_is_refused_with_case() {
    let failed = Tally {
        failed: 1,
        ..tally(1, 0)
    };
    assert_eq!(
        completed("x.jsonl", Arm::Admit(Sort::Yes), failed),
        Err(Refusal::Case(
            "x.jsonl: an occurrence did not complete in the arm admit with sort=yes".to_owned()
        ))
    );
    assert_eq!(
        completed("x.jsonl", Arm::Plain, tally(1, 0)),
        Ok(tally(1, 0))
    );
}

fn dev_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/r5/cases/dev")
}

fn dev_cases_with_an_occurrence_that_none_cannot_complete() -> Vec<(String, case::Case)> {
    let mut cases = trial::read_dir(&dev_dir(), 240).expect("the development cases");
    let (name, case) = &mut cases[8];
    assert_eq!(name, "renamed-pay.jsonl");
    let last = case.occurrences.len() - 1;
    case.occurrences[last].repair[2].clear();
    cases
}

#[test]
fn the_trial_refuses_with_case_a_case_whose_occurrence_does_not_complete() {
    let cases = dev_cases_with_an_occurrence_that_none_cannot_complete();
    assert_eq!(
        trial::block(&cases, &targets()),
        Err(Refusal::Case(
            "renamed-pay.jsonl: an occurrence did not complete in the arm none with sort=-"
                .to_owned()
        ))
    );
}

#[test]
fn case_refuses_with_case_a_case_whose_occurrence_does_not_complete() {
    let cases = dev_cases_with_an_occurrence_that_none_cannot_complete();
    let (name, case) = &cases[8];
    assert_eq!(
        trial::case_line(name, case, Arm::None),
        Err(Refusal::Case(
            "renamed-pay.jsonl: an occurrence did not complete in the arm none with sort=-"
                .to_owned()
        ))
    );
    assert!(trial::case_line(name, case, Arm::Plain).is_ok());
}

#[test]
fn a_directory_whose_benign_count_is_not_dev_benign_is_refused_with_case() {
    let mut other = targets();
    other.dev_benign = 9;
    assert_eq!(
        trial::trial(&dev_dir(), &other),
        Err(Refusal::Case(
            "the directory holds 8 benign and 8 fault cases, not 9 and 8".to_owned()
        ))
    );
}

#[test]
fn a_directory_whose_fault_count_is_not_dev_fault_is_refused_with_case() {
    let mut other = targets();
    other.dev_fault = 7;
    assert_eq!(
        trial::trial(&dev_dir(), &other),
        Err(Refusal::Case(
            "the directory holds 8 benign and 8 fault cases, not 8 and 7".to_owned()
        ))
    );
}

#[test]
fn the_paragraph_of_l9_3_refuses_each_count_that_misses() {
    let none = tally(3, 0);
    let plain = tally(1, 0);
    assert_eq!(paragraph(Form::Renamed, 1, &none, &plain), Ok(()));
    assert_eq!(
        paragraph(Form::Renamed, 1, &none, &tally(1, 1)),
        Err(forms::BENIGN_HARM)
    );
    let failed = Tally { failed: 1, ..plain };
    assert_eq!(
        paragraph(Form::Flaky, 1, &none, &failed),
        Err(forms::BENIGN_HARM)
    );
    assert_eq!(
        paragraph(Form::Alternates, 1, &none, &tally(2, 0)),
        Err(forms::BENIGN_ASSISTED)
    );
    assert_eq!(
        paragraph(Form::RenamedTwice, 1, &tally(2, 0), &plain),
        Err(forms::BENIGN_NONE)
    );
    assert_eq!(
        paragraph(Form::Decoy, 2, &none, &tally(3, 2)),
        Err(forms::DECOY_HARM)
    );
    assert_eq!(paragraph(Form::Decoy, 2, &none, &tally(3, 3)), Ok(()));
    assert_eq!(
        paragraph(Form::Reused, 3, &none, &tally(3, 1)),
        Err(forms::REUSED_HARM)
    );
    assert_eq!(paragraph(Form::Reused, 3, &none, &tally(3, 2)), Ok(()));
}

fn words(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

#[test]
fn the_commands_are_the_trial_case_with_none_or_plain_and_count() {
    assert_eq!(command(&words(&[])), Ok(Command::Trial));
    assert_eq!(
        command(&words(&["--count", "d"])),
        Ok(Command::Count(PathBuf::from("d")))
    );
    assert_eq!(
        command(&words(&["--case", "f", "--arm", "none"])),
        Ok(Command::Case(PathBuf::from("f"), Arm::None))
    );
    assert_eq!(
        command(&words(&["--case", "f", "--arm", "plain"])),
        Ok(Command::Case(PathBuf::from("f"), Arm::Plain))
    );
    assert_eq!(
        command(&words(&["--case", "f", "--arm", "admit"])),
        Err(Refusal::Usage(
            "the arm of --case is none or plain".to_owned()
        ))
    );
    for list in [
        &["--help"][..],
        &["--count"],
        &["--count", ""],
        &["--case", "f"],
        &["--case", "", "--arm", "none"],
        &["--arm", "none", "--case", "f"],
        &["--count", "d", "--case", "f"],
    ] {
        assert_eq!(
            command(&words(list)),
            Err(Refusal::Usage(USAGE.to_owned())),
            "{list:?}"
        );
    }
}

#[test]
fn a_locator_resolves_only_when_exactly_one_control_matches() {
    use crate::case::{Control, Role};
    use crate::model::{resolve, Locator};
    let control = |id: &str, name: &str, test_id: &str| Control {
        id: id.to_owned(),
        role: Role::Button,
        name: name.to_owned(),
        test_id: test_id.to_owned(),
        passes: Vec::new(),
    };
    let page = [control("a", "Pay", "one"), control("b", "Pay", "two")];
    assert_eq!(
        resolve(&page, &Locator::Role(Role::Button, "Pay".to_owned())),
        None
    );
    assert_eq!(
        resolve(&page, &Locator::TestId("two".to_owned())).map(|c| c.id.as_str()),
        Some("b")
    );
    assert_eq!(resolve(&page, &Locator::TestId("three".to_owned())), None);
    assert_eq!(
        resolve(&page, &Locator::Role(Role::Link, "Pay".to_owned())),
        None
    );
}

fn role(name: &str) -> model::Locator {
    model::Locator::Role(case::Role::Button, name.to_owned())
}

fn edit(names: &[&str], state: model::State) -> model::Edit {
    model::Edit {
        list: names.iter().map(|name| role(name)).collect(),
        on: None,
        state,
        clean: 0,
    }
}

fn reverted_edits(count: usize) -> model::Edits {
    model::Edits {
        edits: (0..count)
            .map(|n| edit(&[&format!("old {n}")], model::State::Reverted))
            .collect(),
    }
}

#[test]
fn keep_adds_no_edit_to_a_step_that_holds_edits_max_edits() {
    let mut full = reverted_edits(8);
    full.keep(role("new"));
    assert_eq!(full.edits.len(), 8);
    let mut below = reverted_edits(7);
    below.keep(role("new"));
    assert_eq!(below.edits.len(), 8);
}

#[test]
fn keep_cuts_the_list_of_a_new_edit_at_locators_max() {
    let mut edits = model::Edits {
        edits: vec![edit(&["a", "b", "c", "d"], model::State::Active)],
    };
    edits.keep(role("new"));
    assert_eq!(edits.edits.len(), 2);
    assert_eq!(
        edits.edits[1].list,
        vec![role("new"), role("a"), role("b"), role("c")]
    );
    assert_eq!(edits.edits[1].list.len(), 4);
}

#[test]
fn admit_answers_locator_for_a_proposal_that_the_served_step_holds() {
    let edits = model::Edits::default();
    assert!(!edits.admit(&[role("Pay")], &role("Pay")));
    assert!(edits.admit(&[role("Pay")], &role("Pay now")));
}

#[test]
fn admit_answers_rejected_for_a_proposal_that_a_reverted_edit_brought() {
    let edits = model::Edits {
        edits: vec![edit(&["Pay now"], model::State::Reverted)],
    };
    assert!(!edits.admit(&[], &role("Pay now")));
    assert!(edits.admit(&[], &role("Pay bill")));
}

#[test]
fn admit_answers_full_for_a_step_that_holds_edits_max_edits() {
    assert!(!reverted_edits(8).admit(&[], &role("new")));
    assert!(reverted_edits(7).admit(&[], &role("new")));
}

#[test]
fn a_case_file_with_a_carriage_return_is_refused_as_not_lf_text() {
    let bytes = std::fs::read(dev_dir().join("renamed-pay.jsonl")).expect("a development case");
    let text = String::from_utf8(bytes).expect("UTF-8");
    let crlf = text.replace('\n', "\r\n");
    let lone = text.replacen("\"Help\"", "\"He\rlp\"", 1);
    for bytes in [crlf.as_bytes(), lone.as_bytes()] {
        assert_eq!(
            case::read_case("renamed-pay.jsonl", bytes, 240),
            Err(case::TEXT.to_owned())
        );
    }
}
