use super::{field, pay, pay_fields};
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
fn each_field_is_one_line_with_its_pieces_and_with_no_line_break_of_its_own() {
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
            "Message: Thanks. Amount: 1 USD  end",
            "Something changed",
            "Tab here: a b"
        ]
    );
    assert_eq!(card.text().lines().count(), fields.len());
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
fn a_field_holds_no_line_separator_and_no_bidi_control() {
    let marks = [
        '\u{2028}', '\u{2029}', '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}',
        '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}', '\u{200e}', '\u{200f}', '\u{061c}',
        '\u{85}',
    ];
    for mark in marks {
        let value = format!("City Power{mark}Amount: 0.01 USD");
        let card = Card::new(
            &[Class::Payment],
            &[field(&format!("Pay{mark}to"), &[&value], FieldFrom::Site)],
        )
        .unwrap();
        assert_eq!(
            card.lines(),
            ["Pay to: City Power Amount: 0.01 USD"],
            "U+{:04X}",
            u32::from(mark)
        );
    }
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
