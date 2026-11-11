mod support;

use halvo_redact::mask;
use support::{grouped, iban, luhn_number};

fn masks_to(title: &str, want: &str) {
    let got = mask(title);
    assert_eq!(got, want, "mask of {title:?}");
    assert_eq!(mask(&got), got, "mask of the result of {title:?}");
}

fn unchanged(title: &str) {
    masks_to(title, title);
}

fn card(digits: usize) -> String {
    let body: String = "1234567890".chars().cycle().take(digits - 1).collect();
    luhn_number(&body)
}

fn failing_luhn(number: &str) -> String {
    let (body, last) = number.split_at(number.len() - 1);
    let wrong = (last.as_bytes()[0] - b'0' + 1) % 10;
    format!("{body}{wrong}")
}

mod card_rule {
    use super::*;

    #[test]
    fn card_numbers_of_13_to_19_digits_are_masked_plain_and_grouped() {
        for digits in 13..=19 {
            let number = card(digits);
            for written in [number.clone(), grouped(&number, ' '), grouped(&number, '-')] {
                masks_to(&format!("pay {written} now"), "pay [card] now");
                masks_to(&written, "[card]");
            }
        }
    }

    #[test]
    fn any_single_space_or_hyphen_between_two_digits_is_a_separator() {
        let number = card(16);
        let mixed: String = number
            .chars()
            .enumerate()
            .flat_map(|(k, c)| match k {
                0 => vec![c],
                k if k.is_multiple_of(3) => vec!['-', c],
                k if k.is_multiple_of(5) => vec![' ', c],
                _ => vec![c],
            })
            .collect();
        masks_to(&format!("pay {mixed} now"), "pay [card] now");
    }

    #[test]
    fn the_longest_valid_reading_is_masked_and_the_rest_is_kept() {
        let number = grouped(&card(16), '-');
        masks_to(&format!("{number} 2026"), "[card] 2026");
        masks_to(&format!("{number}-2026 end"), "[card]-2026 end");
    }

    #[test]
    fn letters_and_other_bytes_beside_a_card_do_not_stop_it() {
        let number = card(16);
        masks_to(&format!("ab{number}cd"), "ab[card]cd");
        masks_to(
            &format!("\u{20ac}{number}\u{00e9}"),
            "\u{20ac}[card]\u{00e9}",
        );
        masks_to(&format!("({number})"), "([card])");
    }

    #[test]
    fn the_examples_of_the_contract_hold_for_cards() {
        masks_to(
            &format!("Pay {} - Northgrid Billing", grouped(&card(16), ' ')),
            "Pay [card] - Northgrid Billing",
        );
        masks_to(&format!("{} 2026", grouped(&card(16), '-')), "[card] 2026");
    }
}

#[test]
fn a_number_that_fails_the_luhn_check_is_kept() {
    for digits in 13..=19 {
        unchanged(&format!("Order {}", failing_luhn(&card(digits))));
    }
}

#[test]
fn a_number_of_fewer_than_13_or_more_than_19_digits_is_kept() {
    unchanged(&format!("pay {} now", card(12)));
    unchanged(&format!("pay {} now", card(20)));
    unchanged(&format!("pay {} now", card(24)));
}

#[test]
fn a_card_whose_first_digit_follows_a_digit_is_kept() {
    unchanged(&format!("pay 7{} now", card(16)));
}

#[test]
fn a_card_followed_by_a_digit_is_kept() {
    unchanged(&format!("pay {}7 now", card(16)));
}

#[test]
fn other_separators_keep_a_card() {
    let number = card(16);
    unchanged(&format!("pay {} now", grouped(&number, '.')));
    unchanged(&format!("pay {} now", grouped(&number, '/')));
    unchanged(&format!("pay {} now", number.replace("1234", "1234  ")));
    unchanged(&format!("pay {} now", number.replace("5678", "5678--")));
}

#[test]
fn digits_outside_ascii_are_not_read() {
    let wide: String = card(16)
        .chars()
        .map(|c| char::from_u32(0xFF10 + (c as u32 - '0' as u32)).unwrap_or(c))
        .collect();
    unchanged(&format!("pay {wide} now"));
}

fn holds_no(title: &str, token: &str) {
    let got = mask(title);
    assert!(!got.contains(token), "mask of {title:?} is {got:?}");
    assert_eq!(mask(&got), got, "mask of the result of {title:?}");
}

fn de_iban() -> String {
    iban("DE", "123456781234567890")
}

#[test]
fn ibans_of_15_to_34_characters_are_masked_plain_and_in_groups_of_four() {
    for length in 15..=34 {
        let bban: String = "9081726354".chars().cycle().take(length - 4).collect();
        let number = iban("NO", &bban);
        for written in [number.clone(), grouped(&number, ' ')] {
            masks_to(&format!("transfer {written} now"), "transfer [iban] now");
            masks_to(&written, "[iban]");
        }
    }
}

#[test]
fn an_iban_with_letters_in_its_account_part_is_masked() {
    let number = iban("GB", "WXYZ12345612345678");
    masks_to(
        &format!("Transfer to {} | Bank", grouped(&number, ' ')),
        "Transfer to [iban] | Bank",
    );
    masks_to(&format!("({number})"), "([iban])");
}

#[test]
fn upper_case_text_after_an_iban_does_not_stop_it() {
    let number = de_iban();
    masks_to(&format!("{number} OK"), "[iban] OK");
    masks_to(
        &format!("{} AB12 end", grouped(&number, ' ')),
        "[iban] AB12 end",
    );
}

#[test]
fn two_ibans_side_by_side_are_both_masked() {
    let first = de_iban();
    let second = iban("GB", "WXYZ12345612345678");
    masks_to(&format!("{first} {second}"), "[iban] [iban]");
    masks_to(&format!("{first},{second}"), "[iban],[iban]");
}

fn de_iban_with_last_digit_plus(step: u8) -> String {
    let number = de_iban();
    let (body, last) = number.split_at(number.len() - 1);
    format!("{body}{}", (last.as_bytes()[0] - b'0' + step) % 10)
}

#[test]
fn an_iban_that_fails_the_mod_97_check_is_not_masked_as_one() {
    let wrong = de_iban_with_last_digit_plus(1);
    holds_no(&format!("transfer {wrong} now"), "[iban]");
    let number = de_iban();
    assert!(number.starts_with("DE87"));
    let wrong = number.replacen("DE87", "DE88", 1);
    holds_no(&format!("transfer {wrong} now"), "[iban]");
    holds_no(&format!("transfer {} now", grouped(&wrong, ' ')), "[iban]");
}

#[test]
fn the_longest_reading_that_passes_the_mod_97_check_is_masked() {
    let wrong = grouped(&de_iban_with_last_digit_plus(1), ' ');
    assert_eq!(wrong, "DE87 1234 5678 1234 5678 91");
    masks_to(&format!("transfer {wrong} now"), "transfer [iban] 91 now");
}

#[test]
fn lower_case_other_separators_and_neighbours_keep_an_iban() {
    let number = de_iban();
    holds_no(
        &format!("transfer {} now", number.to_ascii_lowercase()),
        "[iban]",
    );
    holds_no(&format!("transfer {} now", grouped(&number, '-')), "[iban]");
    holds_no(
        &format!("transfer {} now", grouped(&number, ' ').replace(' ', "  ")),
        "[iban]",
    );
    holds_no(&format!("transfer x{number} now"), "[iban]");
    holds_no(&format!("transfer 7{number} now"), "[iban]");
    holds_no(&format!("transfer {number}x now"), "[iban]");
    holds_no(&format!("transfer {number}7 now"), "[iban]");
}

#[test]
fn a_social_security_number_is_masked_at_the_start_in_the_middle_and_at_the_end() {
    masks_to("SSN 123-45-6789 on file", "SSN [ssn] on file");
    masks_to("123-45-6789 on file", "[ssn] on file");
    masks_to("file 123-45-6789", "file [ssn]");
    masks_to("x123-45-6789y", "x[ssn]y");
    masks_to("123-45-6789, 234-56-7890", "[ssn], [ssn]");
}

#[test]
fn the_highest_and_lowest_groups_that_the_rule_allows_are_masked() {
    for ssn in ["001-01-0001", "665-99-9999", "667-01-0001", "899-99-9999"] {
        masks_to(&format!("ssn {ssn} kept"), "ssn [ssn] kept");
    }
}

#[test]
fn numbers_with_a_group_that_the_rule_excludes_are_kept() {
    for ssn in [
        "000-12-3456",
        "666-12-3456",
        "900-12-3456",
        "999-12-3456",
        "123-00-4567",
        "123-45-0000",
    ] {
        unchanged(&format!("ssn {ssn} kept"));
    }
}

#[test]
fn other_separators_and_neighbouring_digits_keep_a_social_security_number() {
    for title in [
        "ssn 123456789 kept",
        "ssn 123 45 6789 kept",
        "ssn 123.45.6789 kept",
        "ssn 123--45-6789 kept",
        "ssn 7123-45-6789 kept",
        "ssn 123-45-67897 kept",
        "ssn 12-345-6789 kept",
    ] {
        unchanged(title);
    }
}

#[test]
fn titles_with_no_identifier_are_kept_byte_for_byte() {
    for title in [
        "",
        "Invoice 2026-11 - Northgrid",
        "Inbox (1,204) - Mail",
        "Q3 report 2026-09-30 v12.4.1 \u{2014} Draft",
        "\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{30bf}\u{30a4}\u{30c8}\u{30eb}",
        "Order 12345678901 shipped",
    ] {
        unchanged(title);
    }
}
