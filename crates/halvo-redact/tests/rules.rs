mod support;

use halvo_redact::mask;
use support::{grouped, luhn_number};

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
