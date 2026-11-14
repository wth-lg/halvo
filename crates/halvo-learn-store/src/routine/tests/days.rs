use serde_json::Value;

use super::super::days::occurrences;
use super::fixtures::*;

#[test]
fn a_summary_counts_when_its_category_and_site_key_equal_the_rows() {
    let row = fresh(1, DAY);
    let summaries = vec![
        summary(1, BILL, domain(HOME, "address_field"), BROWSER, 10),
        summary(2, BILL, domain(HOME, "sign_in"), MAIL, 11),
        summary(3, BILL, domain(OTHER, "address_field"), BROWSER, 12),
        summary(4, ORDER, domain(HOME, "address_field"), BROWSER, 13),
        summary(5, BILL, Value::Null, BROWSER, 14),
        summary(6, BILL, title(HASH), BROWSER, 15),
        summary(7, BILL, domain(HOME, "address_field"), MAIL, 16),
    ];
    assert_eq!(occurrences(&row, &summaries, &[]), vec![10, 16]);
}

#[test]
fn a_summary_of_the_other_source_of_the_same_domain_does_not_count() {
    let address = fresh(1, DAY);
    let sign_in = fresh_at(2, DAY, domain(HOME, "sign_in"), BROWSER);
    let summaries = vec![
        summary(1, BILL, domain(HOME, "address_field"), BROWSER, 20),
        summary(2, BILL, domain(HOME, "sign_in"), BROWSER, 21),
    ];
    assert_eq!(occurrences(&address, &summaries, &[]), vec![20]);
    assert_eq!(occurrences(&sign_in, &summaries, &[]), vec![21]);
}

#[test]
fn a_title_site_key_counts_against_the_same_hash_alone() {
    let row = fresh_at(1, DAY, title(HASH), BROWSER);
    let summaries = vec![
        summary(1, BILL, title(HASH), MAIL, 20),
        summary(2, BILL, title(HASH_TWO), BROWSER, 21),
        summary(3, BILL, domain(HOME, "address_field"), BROWSER, 22),
        summary(4, BILL, Value::Null, BROWSER, 23),
        summary(5, ORDER, title(HASH), BROWSER, 24),
    ];
    assert_eq!(occurrences(&row, &summaries, &[]), vec![20]);
}

#[test]
fn where_the_rows_site_key_is_null_a_summary_counts_by_category_and_app() {
    let row = fresh_at(1, DAY, Value::Null, BROWSER);
    let summaries = vec![
        summary(1, BILL, Value::Null, BROWSER, 30),
        summary(2, BILL, Value::Null, MAIL, 31),
        summary(3, BILL, domain(HOME, "address_field"), BROWSER, 32),
        summary(4, ORDER, Value::Null, BROWSER, 33),
        summary(5, BILL, title(HASH), BROWSER, 34),
    ];
    assert_eq!(occurrences(&row, &summaries, &[]), vec![30]);
}

#[test]
fn a_verified_replay_run_of_the_workflow_counts_and_no_other_run_does() {
    let row = fresh(1, DAY);
    let runs = vec![
        run(1, 1, 40, "replay", "verified"),
        run(1, 2, 41, "compile", "verified"),
        run(1, 3, 42, "replay", "unverified"),
        run(1, 4, 43, "replay", "started"),
        run(2, 5, 44, "replay", "verified"),
        run(1, 6, 45, "replay", "started"),
        run(1, 6, 45, "replay", "verified"),
        run_of(1, 9, 7, 46, "replay", "verified"),
        run(1, 8, 47, "compile", "unverified"),
    ];
    assert_eq!(occurrences(&row, &[], &runs), vec![40, 45, 46]);
}

#[test]
fn a_day_on_both_streams_counts_once_and_the_days_ascend() {
    let row = fresh(1, DAY);
    let summaries = vec![
        summary(1, BILL, domain(HOME, "address_field"), BROWSER, 50),
        summary(2, BILL, domain(HOME, "address_field"), BROWSER, 50),
        summary(3, BILL, domain(HOME, "address_field"), BROWSER, 53),
    ];
    let runs = vec![
        run(1, 1, 55, "replay", "verified"),
        run(1, 2, 50, "replay", "verified"),
        run(1, 3, 51, "replay", "verified"),
        run(1, 4, 51, "replay", "verified"),
    ];
    assert_eq!(occurrences(&row, &summaries, &runs), vec![50, 51, 53, 55]);
}

#[test]
fn a_summary_of_day_zero_is_no_occurrence() {
    let row = fresh(1, DAY);
    let summaries = vec![
        summary(1, BILL, domain(HOME, "address_field"), BROWSER, 0),
        summary(2, BILL, domain(HOME, "address_field"), BROWSER, 1),
    ];
    assert_eq!(occurrences(&row, &summaries, &[]), vec![1]);
}
