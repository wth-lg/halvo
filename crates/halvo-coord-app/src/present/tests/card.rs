use super::{field, pay, pay_fields};
use crate::present::card::{breaks, LINES, LINE_CHARS, WINDOWS_MARKS};
use crate::present::Card;
use halvo_coord::types::{Cause, Class, FieldFrom};
use halvo_coord::words;

#[test]
fn a_payment_card_has_the_title_the_lines_and_the_two_controls_of_its_class() {
    let card = pay();
    assert_eq!(card.title(), "Pay this?");
    assert_eq!(
        card.lines(),
        [
            "Pay to: City Power",
            "Amount: 42.10 USD",
            "Pay with: Card ending 4242",
            "Site: site1.example"
        ]
    );
    assert_eq!(
        card.text(),
        "Pay to: City Power\nAmount: 42.10 USD\nPay with: Card ending 4242\nSite: site1.example"
    );
    assert_eq!((card.reject(), card.allow()), ("Don't pay", "Pay"));
}

#[test]
fn a_card_of_more_than_one_effect_of_one_class_takes_the_many_entries_with_the_count() {
    let card = Card::new(&[Class::Payment, Class::Payment], &pay_fields()).unwrap();
    assert_eq!(card.title(), "Pay this?");
    assert_eq!(
        (card.reject(), card.allow()),
        ("Don't pay these 2", "Pay all 2")
    );
}

#[test]
fn a_card_of_more_than_one_class_takes_the_mixed_entries_and_keeps_each_action_line() {
    let fields = vec![
        field("It will make this payment", &[], FieldFrom::Halvo),
        field("Pay to", &["City Power"], FieldFrom::You),
        field("It will send this message", &[], FieldFrom::Halvo),
        field("To", &["Ann Lee"], FieldFrom::Site),
    ];
    let card = Card::new(&[Class::Payment, Class::Send, Class::Payment], &fields).unwrap();
    assert_eq!(card.title(), "Do these 3 things?");
    assert_eq!(
        (card.reject(), card.allow()),
        ("Don't do any of these 3", "Do all 3")
    );
    assert_eq!(
        card.lines(),
        [
            "It will make this payment",
            "Pay to: City Power",
            "It will send this message",
            "To: Ann Lee"
        ]
    );
}

#[test]
fn every_class_with_entries_gives_its_own_rejecting_text_first_and_two_different_controls() {
    for class in Class::ALL.iter().copied() {
        let key = |name: &str| format!("confirm.{}.{name}", class.name());
        let Ok(reject) = words::text(&key("reject"), &[]) else {
            assert_eq!(Card::new(&[class], &[]), Err(Cause::Malformed), "{class:?}");
            continue;
        };
        let one = Card::new(&[class], &[]).unwrap();
        assert_eq!(one.reject(), reject);
        assert_eq!(one.allow(), words::text(&key("allow"), &[]).unwrap());
        assert_eq!(one.title(), words::text(&key("title"), &[]).unwrap());
        let many = Card::new(&[class; 3], &[]).unwrap();
        let three = [("count", "3")];
        assert_eq!(
            many.reject(),
            words::text(&key("reject_many"), &three).unwrap()
        );
        assert_eq!(
            many.allow(),
            words::text(&key("allow_many"), &three).unwrap()
        );
        for card in [one, many] {
            assert_ne!(card.reject(), card.allow(), "{class:?}");
        }
    }
}

#[test]
fn a_card_is_refused_for_no_class_and_for_a_class_whose_entries_the_words_file_lacks() {
    assert_eq!(Card::new(&[], &pay_fields()), Err(Cause::Malformed));
    for class in [
        Class::Credential,
        Class::Read,
        Class::Prepare,
        Class::LocalEdit,
    ] {
        assert_eq!(
            Card::new(&[class], &pay_fields()),
            Err(Cause::Malformed),
            "{class:?}"
        );
    }
}

#[test]
fn a_field_is_taken_as_it_is_and_a_line_break_in_it_breaks_the_composed_form() {
    let fields = vec![
        field(
            "What to remove",
            &["Draft one", "Draft two"],
            FieldFrom::Site,
        ),
        field(
            "Message",
            &["Thanks.\nAmount: 1 USD\r\nend"],
            FieldFrom::Agent,
        ),
        field("Something changed", &[], FieldFrom::Halvo),
        field("Tab\there", &["a\u{0}b"], FieldFrom::Site),
    ];
    let card = Card::new(&[Class::Payment], &fields).unwrap();
    assert_eq!(
        card.lines(),
        [
            "What to remove: Draft oneDraft two",
            "Message: Thanks.\nAmount: 1 USD\r\nend",
            "Something changed",
            "Tab\there: a\u{0}b"
        ]
    );
    assert!(!card.whole());
    for (i, field) in fields.iter().enumerate() {
        let one = Card::new(&[Class::Payment], std::slice::from_ref(field)).unwrap();
        assert_eq!(one.whole(), i % 2 == 0, "field {i}");
    }
}

#[test]
fn a_mixed_card_is_refused_for_a_class_whose_entries_the_words_file_lacks() {
    for class in [
        Class::Credential,
        Class::Read,
        Class::Prepare,
        Class::LocalEdit,
    ] {
        assert_eq!(
            Card::new(&[Class::Payment, class], &pay_fields()),
            Err(Cause::Malformed),
            "{class:?}"
        );
        assert_eq!(
            Card::new(&[class, Class::Send, Class::Payment], &pay_fields()),
            Err(Cause::Malformed),
            "{class:?}"
        );
    }
}

#[test]
fn each_mark_of_the_closed_list_in_a_label_or_a_value_breaks_the_composed_form_and_stays() {
    let marks = [
        '\u{0}', '\u{9}', '\u{a}', '\u{d}', '\u{1f}', '\u{7f}', '\u{85}', '\u{9f}', '\u{2028}',
        '\u{2029}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}',
        '\u{2067}', '\u{2068}', '\u{2069}', '\u{200e}', '\u{200f}', '\u{061c}',
    ];
    for mark in marks {
        let code = u32::from(mark);
        assert!(breaks(mark), "U+{code:04X}");
        let value = format!("City Power{mark}Amount: 0.01 USD");
        let in_value = Card::new(
            &[Class::Payment],
            &[field("Pay to", &[&value], FieldFrom::Site)],
        )
        .unwrap();
        assert_eq!(
            in_value.lines(),
            [format!("Pay to: {value}")],
            "U+{code:04X}"
        );
        assert!(!in_value.whole(), "U+{code:04X} in a value");
        let in_label = Card::new(
            &[Class::Payment],
            &[field(
                &format!("Pay{mark}to"),
                &["City Power"],
                FieldFrom::Site,
            )],
        )
        .unwrap();
        assert!(!in_label.whole(), "U+{code:04X} in a label");
    }
    for plain in [
        ' ', 'a', '\u{7e}', '\u{a0}', '\u{200d}', '\u{2027}', '\u{202f}', '\u{2065}', '\u{206a}',
        '\u{61b}', '\u{61d}',
    ] {
        assert!(!breaks(plain), "U+{:04X}", u32::from(plain));
    }
}

#[test]
fn a_card_of_every_class_with_entries_keeps_the_composed_form_on_both_systems() {
    for class in Class::ALL.iter().copied() {
        for classes in [vec![class], vec![class; 3], vec![Class::Payment, class]] {
            let Ok(card) = Card::new(&classes, &pay_fields()) else {
                continue;
            };
            assert!(card.keeps(&[]), "{classes:?} on macOS");
            assert!(card.keeps(WINDOWS_MARKS), "{classes:?} on Windows");
            assert!(card.whole(), "{classes:?} on this system");
        }
    }
}

#[test]
fn a_title_or_a_control_text_with_a_mark_of_the_list_breaks_the_composed_form() {
    let lines = ["Pay to: City Power"];
    assert!(Card::of("Pay this?", &lines, "Don't pay", "Pay").whole());
    for mark in ['\n', '\u{2028}', '\u{202e}', '\u{0}'] {
        let broken = format!("Pay{mark}");
        assert!(
            !Card::of(&broken, &lines, "Don't pay", "Pay").whole(),
            "title"
        );
        assert!(
            !Card::of("Pay this?", &lines, &broken, "Pay").whole(),
            "reject"
        );
        assert!(
            !Card::of("Pay this?", &lines, "Don't pay", &broken).whole(),
            "allow"
        );
    }
}

#[test]
fn an_access_key_mark_in_a_control_text_breaks_the_composed_form_where_the_system_has_one() {
    let lines = ["Pay to: City Power"];
    for (reject, allow) in [
        ("Don't &pay", "Pay"),
        ("Don't pay", "&Pay"),
        ("Don't pay", "Pay &&"),
    ] {
        let card = Card::of("Pay this?", &lines, reject, allow);
        assert!(!card.keeps(WINDOWS_MARKS), "{reject} / {allow} on Windows");
        assert!(card.keeps(&[]), "{reject} / {allow} on macOS");
        assert_eq!(card.whole(), cfg!(not(windows)), "{reject} / {allow} here");
    }
    let marked_text = Card::of("Pay & go?", &["Pay to: A & B"], "Don't pay", "Pay");
    assert!(
        marked_text.keeps(WINDOWS_MARKS),
        "the mark counts in a control text alone"
    );
}

#[test]
fn the_placeholder_bounds_are_16_lines_and_120_characters() {
    assert_eq!((LINES, LINE_CHARS), (16, 120));
}

#[test]
fn a_card_over_its_bound_in_lines_or_in_characters_breaks_the_composed_form() {
    let line = "Pay to: City Power";
    assert!(Card::of("Pay this?", &vec![line; LINES], "Don't pay", "Pay").whole());
    assert!(!Card::of("Pay this?", &vec![line; LINES + 1], "Don't pay", "Pay").whole());
    let full = "\u{e9}".repeat(LINE_CHARS);
    let over = "\u{e9}".repeat(LINE_CHARS + 1);
    let other = "e".repeat(LINE_CHARS);
    assert!(
        Card::of(&full, &[&full], &full, &other).whole(),
        "each at its bound"
    );
    assert!(
        !Card::of("Pay this?", &[&over], "Don't pay", "Pay").whole(),
        "a line"
    );
    assert!(
        !Card::of(&over, &[line], "Don't pay", "Pay").whole(),
        "the title"
    );
    assert!(
        !Card::of("Pay this?", &[line], &over, "Pay").whole(),
        "reject"
    );
    assert!(
        !Card::of("Pay this?", &[line], "Don't pay", &over).whole(),
        "allow"
    );
    assert!(
        Card::of("Pay this?", &[], "Don't pay", "Pay").whole(),
        "no line"
    );
}

#[test]
fn a_value_cut_inside_a_number_reads_as_the_one_value_that_was_bound() {
    let card = Card::new(
        &[Class::Payment],
        &[field("Amount", &["Pay 1", "000.00 USD"], FieldFrom::Site)],
    )
    .unwrap();
    assert_eq!(card.lines(), ["Amount: Pay 1000.00 USD"]);
}

#[test]
fn a_piece_keeps_its_own_comma_and_space_and_a_field_with_no_value_is_its_label() {
    let card = Card::new(
        &[Class::Payment],
        &[
            field("Pay to", &["Power, Water", " and Gas"], FieldFrom::You),
            field("It will make this payment", &[], FieldFrom::Halvo),
        ],
    )
    .unwrap();
    assert_eq!(
        card.lines(),
        ["Pay to: Power, Water and Gas", "It will make this payment"]
    );
}
