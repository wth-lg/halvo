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
        let number = grouped(&luhn_number("623170586270777"), '-');
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
        masks_to(&format!("{} 2026", grouped(&card(16), '-')), "[card]");
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

fn key(prefix: &str, body: char, length: usize) -> String {
    format!("{prefix}{}", body.to_string().repeat(length))
}

#[test]
fn a_key_is_masked_at_the_start_in_the_middle_and_at_the_end() {
    let token = key("ghp_", 'a', 36);
    masks_to(&format!("{token} saved"), "[key] saved");
    masks_to(&format!("key {token} saved"), "key [key] saved");
    masks_to(&format!("key {token}"), "key [key]");
    masks_to(&format!("(\"{token}\")"), "(\"[key]\")");
}

#[test]
fn an_aws_key_takes_16_characters_of_a_to_z_and_2_to_7() {
    masks_to(
        &format!("Key {} saved", key("AKIA", 'Q', 16)),
        "Key [key] saved",
    );
    for prefix in ["ASIA", "ABIA", "ACCA", "A3TA", "A3TZ", "A3T0", "A3T9"] {
        masks_to(
            &format!("key {} saved", key(prefix, '7', 16)),
            "key [key] saved",
        );
    }
    masks_to(
        &format!("key AKIA{}0 saved", "Q2".repeat(8)),
        "key [key]0 saved",
    );
    unchanged(&format!("key {} saved", key("AKIA", '9', 16)));
    unchanged(&format!("key {} saved", key("AKIA", '1', 16)));
    unchanged(&format!("key {} saved", key("A3T", '9', 17)));
    unchanged(&format!("key {} saved", key("A3Ta", 'Q', 16)));
}

#[test]
fn a_key_longer_than_its_rule_is_masked_up_to_the_rule_and_the_rest_is_kept() {
    masks_to(
        &format!("key {} saved", key("sk_live_", 'b', 120)),
        &format!("key [key]{} saved", "b".repeat(21)),
    );
    masks_to(
        &format!("key {} saved", key("sk_live_", 'b', 10)),
        "key [key] saved",
    );
}

#[test]
fn a_key_shorter_than_its_rule_is_kept() {
    unchanged(&format!("key {} saved", key("ghp_", 'a', 35)));
    unchanged(&format!("key {} saved", key("sk_test_", 'a', 9)));
    unchanged(&format!("key {} saved", key("glpat-", 'x', 19)));
}

#[test]
fn a_key_whose_prefix_follows_a_letter_or_digit_or_differs_in_case_is_kept() {
    unchanged(&format!("key x{} saved", key("ghp_", 'a', 36)));
    unchanged(&format!("key 7{} saved", key("ghp_", 'a', 36)));
    unchanged(&format!("key {} saved", key("GHP_", 'a', 36)));
    unchanged(&format!("key {} saved", key("Sk_live_", 'a', 20)));
}

#[test]
fn two_keys_side_by_side_are_both_masked() {
    let first = key("ghp_", 'a', 36);
    let second = key("gho_", 'b', 36);
    masks_to(&format!("{first} {second}"), "[key] [key]");
    masks_to(&format!("{first}{second}"), "[key][key]");
}

#[test]
fn the_rows_that_the_release_states_otherwise_are_corrected_or_gone() {
    masks_to(
        &format!("key {} saved", key("dapi", 'f', 32)),
        "key [key] saved",
    );
    unchanged(&format!("key {} saved", key("dapi", 'g', 32)));
    unchanged(&format!("key {} saved", key("npm_", 'a', 36)));
    unchanged(&format!("key {} saved", key("dor_v1_", 'a', 64)));
    masks_to(
        &format!("key {} saved", key("dop_v1_", 'a', 64)),
        "key [key] saved",
    );
}

#[test]
fn a_card_then_a_key_with_no_byte_between_are_both_masked() {
    let number = card(16);
    masks_to(&format!("{number}{}", key("AKIA", 'Q', 16)), "[card][key]");
}

fn gb_iban_with_a_card_reading_inside() -> String {
    iban("GB", &format!("WXYZ{}", luhn_number("2468013579246")))
}

mod scan_rule {
    use super::*;

    #[test]
    fn an_iban_that_touches_a_card_starts_where_the_card_ends() {
        let account = gb_iban_with_a_card_reading_inside();
        masks_to(&format!("{}{account}", card(16)), "[card][iban]");
        masks_to(
            "note 0085697852016451NL54 0123 4567 8901 2345 67 now",
            "note [card][iban] now",
        );
    }

    #[test]
    fn an_iban_that_touches_a_key_starts_where_the_key_ends() {
        masks_to(
            &format!(
                "note {}NO56 3456 7890 1234 5678 90 ok",
                key("glpat-", 'z', 20)
            ),
            "note [key][iban] ok",
        );
    }

    #[test]
    fn a_reading_that_starts_before_a_card_and_crosses_one_separator_keeps_none_of_it() {
        let crossing = luhn_number("202611053507152");
        let number = luhn_number(&format!("{}6618304", &crossing[8..]));
        masks_to(
            &format!("on 2026 1105 {}", grouped(&number, ' ')),
            "on [card]",
        );
        masks_to(
            &format!("ref 2026-1105-{} end", grouped(&number, '-')),
            "ref [card] end",
        );
    }

    #[test]
    fn a_key_body_that_runs_into_a_card_keeps_none_of_the_card() {
        masks_to(
            &format!(
                "key {}{} notes",
                key("sk_live_", 'b', 10),
                grouped(&card(16), ' ')
            ),
            "key [key] notes",
        );
    }

    #[test]
    fn the_classes_of_a_reading_that_crosses_one_separator_keep_nothing() {
        for (title, want) in [
            (
                "cards 0077-8932-8792-8 0042 1809 6792 9084 end",
                "cards [card] end",
            ),
            (
                "box 0000-491675-43583-0067 0500 6480 8194 now",
                "box [card] now",
            ),
            ("pay 4162382333 0055-3940-6067-4828 end", "pay [card] end"),
            ("note 2026 11 05 0086 1345 3032 7786 end", "note [card] end"),
            ("cards 555 0545 0073-6545-9425-3730 now", "cards [card] now"),
            ("note 819031190568 324-27-3006 ok", "note [card] ok"),
            (
                "cards 248-28-1017 8952-0017-4436-6666 ok",
                "cards [card] ok",
            ),
            (
                "pay CH421234567890123 0088-3481-8131-7601 ok",
                "pay [iban] ok",
            ),
            (
                "cards QR99 CH51 3456 7890 1234 5678 901 now",
                "cards [iban] now",
            ),
            ("cards RE65 0099-4994-0285-5593 ok", "cards RE[card] ok"),
            (
                "box AT89901234567890123456 FR32 5678 9012 3456 7890 1234 5678 90 now",
                "box [iban] now",
            ),
        ] {
            masks_to(title, want);
        }
    }

    #[test]
    fn an_iban_whose_end_a_card_reading_on_its_right_masks_is_masked() {
        let digits = "80467213590824";
        let tail = luhn_number(&format!("{digits}5"));
        masks_to(
            &format!(
                "ref {}{} end",
                iban("GB", &format!("WXYZ{digits}")),
                &tail[14..]
            ),
            "ref [iban] end",
        );
    }

    #[test]
    fn nine_identifiers_that_touch_are_masked_in_one_call() {
        let token = key("ghp_", 'a', 36);
        let title = format!(
            "{token}247-61-3058{token}247-61-3058{token}247-61-3058{token}{}{}",
            card(16),
            gb_iban_with_a_card_reading_inside()
        );
        masks_to(&title, "[key][ssn][key][ssn][key][ssn][key][card][iban]");
    }

    #[test]
    fn a_group_takes_the_token_of_its_first_reading_and_the_first_rule_at_that_byte() {
        masks_to(
            &format!("key GR1348941000026-{} saved", "q".repeat(13)),
            "key [key] saved",
        );
        let number = luhn_number("24761305860174");
        masks_to(
            &format!("pay 247-61-3058-6017-4{} now", &number[14..]),
            "pay [card] now",
        );
        masks_to("id 151-14-20044028163578 end", "id [ssn] end");
    }

    #[test]
    fn a_shorter_reading_at_a_start_holds_where_the_longest_does_not() {
        masks_to(
            &format!("pay {} 1070 now", grouped(&card(16), ' ')),
            "pay [card] 1070 now",
        );
        masks_to(
            &format!("transfer {} 8K0A now", grouped(&de_iban(), ' ')),
            "transfer [iban] 8K0A now",
        );
    }

    #[test]
    fn a_card_reading_may_start_among_the_digits_of_a_masked_card() {
        masks_to(
            &format!("{} 2026", grouped(&luhn_number("531418733333333"), '-')),
            "[card]",
        );
        masks_to(
            &format!("{} 2026", grouped(&luhn_number("623170586270777"), '-')),
            "[card] 2026",
        );
    }
}

#[test]
fn ibans_of_14_and_35_characters_are_not_masked() {
    unchanged(&format!("ref {} end", iban("NO", "1234567890")));
    unchanged(&format!(
        "ref {} end",
        iban("NO", "ABCD1234EFGH5678IJKL9012MNOP345")
    ));
}

#[test]
fn lower_case_letters_after_an_iban_are_no_part_of_it() {
    masks_to(
        &format!("transfer {} yx.", de_iban()),
        "transfer [iban] yx.",
    );
}

#[test]
fn a_run_that_breaks_one_condition_of_the_iban_form_is_kept() {
    for run in [
        "7B34WQ12RT34YU5614",
        "B734WQ12RT34YU5692",
        "BBX4WQ12RT34YU5605",
        "BB3XWQ12RT34YU5665",
    ] {
        unchanged(&format!("ref {run} end"));
    }
}

#[test]
fn a_key_or_an_iban_after_an_underscore_or_a_hyphen_is_masked() {
    let token = key("AKIA", 'Q', 16);
    masks_to(&format!("id_{token} x"), "id_[key] x");
    masks_to(&format!("id-{token} x"), "id-[key] x");
    masks_to(&format!("id_{} x", de_iban()), "id_[iban] x");
    masks_to(&format!("id-{} x", de_iban()), "id-[iban] x");
    masks_to(&format!("x {}-ok y", de_iban()), "x [iban]-ok y");
}

#[test]
fn a_run_of_13_zeros_passes_the_luhn_check_and_is_masked() {
    masks_to("x 0000000000000 y", "x [card] y");
}

#[test]
fn an_aws_key_body_with_an_8_is_kept() {
    unchanged(&format!("key AKIA{} saved", "Q8".repeat(8)));
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

#[test]
fn titles_built_from_pieces_of_every_rule_keep_the_properties_of_the_scan() {
    let pieces = [
        card(13),
        card(16),
        card(19),
        grouped(&card(16), ' '),
        grouped(&card(16), '-'),
        de_iban(),
        grouped(&de_iban(), ' '),
        gb_iban_with_a_card_reading_inside(),
        "247-61-3058".to_owned(),
        key("ghp_", 'a', 36),
        key("AKIA", 'Q', 16),
        key("sk_live_", 'b', 12),
        key("glpat-", 'z', 20),
        key("pypi-AgEIcHlwaS5vcmc", 'x', 60),
        " ".to_owned(),
        "-".to_owned(),
        "7".to_owned(),
        "x".to_owned(),
        "2026".to_owned(),
        "AB12".to_owned(),
        "[card]".to_owned(),
        "\u{e9}".to_owned(),
        "\u{200b}".to_owned(),
        "\u{65e5}\u{672c}".to_owned(),
    ];
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = |bound: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        usize::try_from(state % bound as u64).unwrap_or(0)
    };
    for _ in 0..5000 {
        let mut title = String::new();
        for _ in 0..=next(10) {
            title.push_str(&pieces[next(pieces.len())]);
        }
        let masked = mask(&title);
        assert!(masked.len() <= title.len(), "mask of {title:?} is longer");
        assert_eq!(mask(&masked), masked, "mask of the result of {title:?}");
    }
}

mod boundary {
    use super::*;

    const KEY_ROWS: [(&str, &str, usize); 28] = [
        ("AKIA", "Q2", 16),
        ("A3TA", "Q2", 16),
        ("ghp_", "Ab3", 36),
        ("gho_", "Ab3", 36),
        ("ghu_", "Ab3", 36),
        ("ghr_", "Ab3", 36),
        ("github_pat_", "Ab3_", 82),
        ("glpat-", "Ab3_-", 20),
        ("glptt-", "b3", 40),
        ("GR1348941", "Ab3_-", 20),
        ("sk_test_", "Ab3", 99),
        ("SG.", "Ab3=_-.", 66),
        ("pypi-AgEIcHlwaS5vcmc", "Ab3_-", 1000),
        ("AIza", "Ab3_-", 35),
        ("dop_v1_", "b3", 64),
        ("doo_v1_", "b3", 64),
        ("dp.pt.", "Ab3", 43),
        ("shpat_", "Bb3", 32),
        ("shpca_", "Bb3", 32),
        ("shppa_", "Bb3", 32),
        ("shpss_", "Bb3", 32),
        ("pul-", "b3", 40),
        ("lin_api_", "Ab3", 40),
        ("AGE-SECRET-KEY-1", "Q9", 58),
        ("hf_", "Ab", 34),
        ("rubygems_", "b3", 48),
        ("dapi", "b3", 32),
        ("SK", "Bb3", 32),
    ];

    const ASCII_LETTERS_AND_DIGITS: [(u8, u8); 3] = [(b'0', b'9'), (b'A', b'Z'), (b'a', b'z')];
    const ASCII_DIGITS: [(u8, u8); 1] = [(b'0', b'9')];
    const NO_BYTE: [(u8, u8); 0] = [];

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Side {
        Before,
        After,
    }

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Rule {
        Card,
        Iban,
        Ssn,
        Key,
    }

    fn refused(rule: Rule, side: Side) -> &'static [(u8, u8)] {
        match (rule, side) {
            (Rule::Key | Rule::Iban, Side::Before) => &ASCII_LETTERS_AND_DIGITS,
            (Rule::Iban, Side::After) => &ASCII_LETTERS_AND_DIGITS,
            (Rule::Card | Rule::Ssn, _) => &ASCII_DIGITS,
            (Rule::Key, Side::After) => &NO_BYTE,
        }
    }

    const ANOTHER_READING: [(Rule, Side, u8, &str, &str); 2] = [
        (
            Rule::Card,
            Side::Before,
            b'0',
            "[card] x",
            "a card reading of 17 digits from the 0 before the card passes the Luhn check",
        ),
        (
            Rule::Card,
            Side::After,
            b'8',
            "x [card]",
            "a card reading of 17 digits up to the 8 after the card passes the Luhn check",
        ),
    ];

    fn in_set(set: &[(u8, u8)], byte: u8) -> bool {
        set.iter().any(|&(low, high)| (low..=high).contains(&byte))
    }

    fn character_ending_in(byte: u8) -> String {
        let bytes = if byte < 0x80 {
            vec![byte]
        } else {
            vec![0xC2, byte]
        };
        String::from_utf8(bytes).expect("a character that ends in the byte")
    }

    fn character_starting_with(byte: u8) -> String {
        let bytes = match byte {
            0x00..=0x7F => vec![byte],
            0xC2..=0xDF => vec![byte, 0x80],
            0xE0 => vec![byte, 0xA0, 0x80],
            0xE1..=0xEF => vec![byte, 0x80, 0x80],
            0xF0 => vec![byte, 0x90, 0x80, 0x80],
            _ => vec![byte, 0x80, 0x80, 0x80],
        };
        String::from_utf8(bytes).expect("a character that starts with the byte")
    }

    #[test]
    fn the_boundaries_of_each_rule_hold_by_the_bytes_that_the_contract_names() {
        let mut identifiers = vec![
            (Rule::Card, luhn_number("123456789012345"), "[card]"),
            (Rule::Iban, iban("DE", "123456781234567890"), "[iban]"),
            (Rule::Ssn, "247-61-3058".to_owned(), "[ssn]"),
        ];
        for (prefix, pattern, max) in KEY_ROWS {
            let body: String = pattern.chars().cycle().take(max).collect();
            identifiers.push((Rule::Key, format!("{prefix}{body}"), "[key]"));
        }
        let before: Vec<u8> = (0x00..=0xBF).collect();
        let after: Vec<u8> = (0x00..=0x7F).chain(0xC2..=0xF4).collect();
        let mut cases = 0;
        let mut wrong = Vec::new();
        for (rule, identifier, token) in &identifiers {
            for (side, bytes) in [(Side::Before, &before), (Side::After, &after)] {
                for &byte in bytes {
                    cases += 1;
                    let (title, masked) = match side {
                        Side::Before => {
                            let c = character_ending_in(byte);
                            (format!("{c}{identifier} x"), format!("{c}{token} x"))
                        }
                        Side::After => {
                            let c = character_starting_with(byte);
                            (format!("x {identifier}{c}"), format!("x {token}{c}"))
                        }
                    };
                    let other = ANOTHER_READING
                        .iter()
                        .find(|e| e.0 == *rule && e.1 == side && e.2 == byte);
                    let (want, why) = match other {
                        Some(&(_, _, _, output, why)) => (output.to_owned(), why),
                        None if in_set(refused(*rule, side), byte) => {
                            (title.clone(), "item 2 refuses the byte")
                        }
                        None => (masked, "item 2 allows the byte"),
                    };
                    let got = mask(&title);
                    if got != want {
                        wrong.push(format!(
                            "{rule:?} {side:?} {byte:#04x} in {title:?}: {got:?}, want {want:?} ({why})"
                        ));
                    }
                }
            }
        }
        assert_eq!(cases, identifiers.len() * (before.len() + after.len()));
        assert_eq!(cases, 31 * (192 + 179));
        assert!(
            wrong.is_empty(),
            "{} of {cases} cases differ: {}",
            wrong.len(),
            wrong.join("; ")
        );
    }
}
