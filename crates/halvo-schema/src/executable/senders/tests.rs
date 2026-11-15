use std::num::NonZeroUsize;
use std::path::PathBuf;

use serde_json::Value;

use super::{is_sender, reply_senders, SendersError};

const ATEXT: &str = "!#$%&'*+-/=?^_`{|}~.";

fn read(slot: &str) -> Result<Vec<String>, SendersError> {
    reply_senders(slot)
}

fn list(addresses: &[&str]) -> Result<Vec<String>, SendersError> {
    Ok(addresses.iter().map(|a| (*a).to_owned()).collect())
}

fn at(piece: usize) -> NonZeroUsize {
    NonZeroUsize::new(piece).expect("a piece counted from 1")
}

fn empty(piece: usize) -> Result<Vec<String>, SendersError> {
    Err(SendersError::Empty(at(piece)))
}

fn not_an_address(piece: usize) -> Result<Vec<String>, SendersError> {
    Err(SendersError::NotAnAddress(at(piece)))
}

fn not_ascii(piece: usize) -> Result<Vec<String>, SendersError> {
    Err(SendersError::NotAscii(at(piece)))
}

fn twice(earlier: usize, later: usize) -> Result<Vec<String>, SendersError> {
    Err(SendersError::Twice {
        earlier: at(earlier),
        later: at(later),
    })
}

fn of_length(length: usize) -> String {
    let domain = "@people.test";
    format!("{}{domain}", "a".repeat(length - domain.len()))
}

#[test]
fn the_slot_is_cut_at_commas_trimmed_of_ascii_spaces_in_its_order() {
    let rows = [
        ("ann@people.test", list(&["ann@people.test"])),
        (
            "ann@people.test, bob@people.test",
            list(&["ann@people.test", "bob@people.test"]),
        ),
        (
            "bob@people.test,ann@people.test",
            list(&["bob@people.test", "ann@people.test"]),
        ),
        (
            "   cy@people.test  ,  Ann@People.test,bob@people.test   ",
            list(&["cy@people.test", "Ann@People.test", "bob@people.test"]),
        ),
    ];
    for (slot, senders) in rows {
        assert_eq!(read(slot), senders, "{slot}");
    }
}

#[test]
fn the_seed_lists_joined_with_a_comma_and_a_space_read_back() {
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tools")
        .join("fixture-server")
        .join("variants")
        .join("data");
    for seed in 1..=3 {
        let text = std::fs::read_to_string(data.join(format!("seed-{seed}.json"))).expect("a seed");
        let seed: Value = serde_json::from_str(&text).expect("JSON");
        let slots = seed["bindings"]["reply"]["slots"]
            .as_array()
            .expect("the slots");
        let senders: Vec<String> = slots
            .iter()
            .find(|slot| slot["name"] == "reply_senders")
            .and_then(|slot| slot["value"].as_array())
            .expect("the slot reply_senders")
            .iter()
            .map(|sender| sender.as_str().expect("an address").to_owned())
            .collect();
        assert_eq!(senders.len(), 2);
        assert_eq!(read(&senders.join(", ")), Ok(senders));
    }
}

#[test]
fn an_empty_piece_refuses_the_slot_with_its_number() {
    let rows = [
        ("", 1),
        (" ", 1),
        (",", 1),
        ("ann@people.test,", 2),
        ("ann@people.test, ", 2),
        (",ann@people.test", 1),
        (" , ann@people.test", 1),
        ("ann@people.test,,bob@people.test", 2),
        ("ann@people.test, ,bob@people.test", 2),
        ("a@people.test, b@people.test, c@people.test,", 4),
    ];
    for (slot, piece) in rows {
        assert_eq!(read(slot), empty(piece), "{slot:?}");
    }
}

#[test]
fn a_piece_that_is_no_address_refuses_the_slot_with_its_number() {
    let long = of_length(321);
    for slot in [
        "Ann Berg",
        "ann",
        "@people.test",
        "ann@",
        "@",
        "ann@@people.test",
        "ann@people@test",
        "ann people@test",
        "ann@people.test bob@people.test",
        "ann@people.test; bob@people.test",
        "Ann <ann@people.test>",
        "ann\t@people.test",
        long.as_str(),
    ] {
        assert_eq!(read(slot), not_an_address(1), "{slot:?}");
    }
    assert_eq!(read("bob@people.test, Ann Berg"), not_an_address(2));
    assert_eq!(read("a@people.test, b@people.test, ann"), not_an_address(3));
    let at_bound = of_length(320);
    assert_eq!(read(&at_bound), Ok(vec![at_bound.clone()]));
}

#[test]
fn a_piece_that_holds_a_byte_outside_ascii_has_a_value_of_its_own() {
    let long = format!("é{}", of_length(320));
    let rows = [
        ("ännchen@people.test", 1),
        ("ann@péople.test", 1),
        ("Ann Bérg", 1),
        ("é", 1),
        ("ann@people\u{a0}.test", 1),
        ("ann@people.test\u{a0}x", 1),
        ("bob@people.test, élan@people.test", 2),
        ("bob@people.test, ann@people.test, \u{1f600}", 3),
        (long.as_str(), 1),
    ];
    for (slot, piece) in rows {
        assert_eq!(read(slot), not_ascii(piece), "{slot:?}");
    }
    for c in ['\u{80}', '\u{ff}', '\u{100}', '\u{10ffff}'] {
        for slot in [format!("a{c}@people.test"), format!("ann@peo{c}ple.test")] {
            assert_eq!(read(&slot), not_ascii(1), "{slot:?}");
        }
    }
}

fn white_space() -> Vec<char> {
    (0..=u32::from(char::MAX))
        .filter_map(char::from_u32)
        .filter(|c| c.is_whitespace())
        .collect()
}

#[test]
fn the_white_space_of_char_is_whitespace_is_trimmed_from_the_ends_of_each_piece() {
    let white = white_space();
    assert_eq!(white.len(), 25);
    let (ann, bob) = ("ann@people.test", "bob@people.test");
    for c in white {
        for piece in [
            format!("{c}{ann}"),
            format!("{ann}{c}"),
            format!("{c}{ann}{c}"),
            format!(" {c}{c}{ann}{c} "),
        ] {
            let rows = [
                (piece.clone(), list(&[ann])),
                (format!("{piece}, {bob}"), list(&[ann, bob])),
                (format!("{bob},{piece}"), list(&[bob, ann])),
            ];
            for (slot, senders) in rows {
                assert_eq!(read(&slot), senders, "{slot:?}");
            }
        }
        let rows = [
            (format!("{c}"), 1),
            (format!("{ann},{c}"), 2),
            (format!("{c} ,{ann}"), 1),
        ];
        for (slot, piece) in rows {
            assert_eq!(read(&slot), empty(piece), "{slot:?}");
        }
        let at_bound = of_length(320);
        assert_eq!(read(&format!("{c}{at_bound}{c}")), Ok(vec![at_bound]));
        let over = of_length(321);
        assert_eq!(read(&format!("{c}{over}{c}")), not_an_address(1));
    }
}

#[test]
fn white_space_inside_a_piece_and_other_bytes_at_its_ends_refuse_the_slot() {
    for c in white_space() {
        let fault = if c.is_ascii() {
            not_an_address(1)
        } else {
            not_ascii(1)
        };
        for slot in [format!("ann{c}@people.test"), format!("ann@people{c}.test")] {
            assert_eq!(read(&slot), fault, "{slot:?}");
        }
    }
    let rows = [
        ('\0', not_an_address(1)),
        ('\u{1f}', not_an_address(1)),
        ('\u{7f}', not_an_address(1)),
        ('\u{200b}', not_ascii(1)),
        ('\u{feff}', not_ascii(1)),
    ];
    for (c, fault) in rows {
        assert!(!c.is_whitespace());
        for slot in [format!("{c}ann@people.test"), format!("ann@people.test{c}")] {
            assert_eq!(read(&slot), fault, "{slot:?}");
        }
    }
}

#[test]
fn an_address_twice_without_ascii_case_refuses_the_slot_with_both_numbers() {
    let rows = [
        ("ann@people.test, ann@people.test", (1, 2)),
        ("ann@people.test, ANN@People.TEST", (1, 2)),
        ("ann@people.test, bob@people.test, Ann@people.test", (1, 3)),
        ("bob@people.test, ann@people.test, BOB@people.test", (1, 3)),
        (
            "a@people.test, b@people.test, c@people.test, B@people.test",
            (2, 4),
        ),
    ];
    for (slot, (earlier, later)) in rows {
        assert_eq!(read(slot), twice(earlier, later), "{slot}");
    }
    assert_eq!(read("élan@people.test, ÉLAN@people.test"), not_ascii(1));
}

#[test]
fn the_first_fault_in_the_order_of_the_text_is_given() {
    let rows = [
        ("Ann Berg, , ann@people.test", not_an_address(1)),
        (", Ann Berg", empty(1)),
        ("ann@people.test, ANN@people.test, Ann Berg", twice(1, 2)),
        (
            "ann@people.test, Ann Berg, ann@people.test",
            not_an_address(2),
        ),
        ("ann@people.test, ann@people.test,", twice(1, 2)),
        (
            "bob@people.test, ännchen@people.test, Ann Berg",
            not_ascii(2),
        ),
        (
            "bob@people.test, Ann Berg, ännchen@people.test",
            not_an_address(2),
        ),
        ("bob@people.test, , ännchen@people.test", empty(2)),
    ];
    for (slot, fault) in rows {
        assert_eq!(read(slot), fault, "{slot}");
    }
}

#[test]
fn the_rule_for_a_sender_takes_printable_ascii_but_spaces_and_specials() {
    for byte in 0u8..=127 {
        let c = char::from(byte);
        let taken = c.is_ascii_alphanumeric() || ATEXT.contains(c);
        let local = format!("a{c}b@people.test");
        assert_eq!(is_sender(&local), taken, "{byte} in the local part");
        let domain = format!("ann@peo{c}ple.test");
        assert_eq!(is_sender(&domain), taken, "{byte} in the domain");
    }
    for text in ["a@b", "'@b", "a.b@c.d", "ann+x@people.test"] {
        assert!(is_sender(text), "{text}");
    }
    for text in ["", "@", "a@", "@b", "ab", "a@b@c", "é@b", "a@é", "a,b@c"] {
        assert!(!is_sender(text), "{text:?}");
    }
    assert!(is_sender(&of_length(320)));
    assert!(!is_sender(&of_length(321)));
}

#[test]
fn texts() {
    let rows = [
        (
            SendersError::Empty(at(1)),
            "the list of senders holds an empty entry",
        ),
        (
            SendersError::NotAnAddress(at(2)),
            "an entry of the list of senders is not an address",
        ),
        (
            SendersError::NotAscii(at(3)),
            "an entry of the list of senders holds a character outside ASCII, which Halvo does not take",
        ),
        (
            SendersError::Twice {
                earlier: at(4),
                later: at(5),
            },
            "the list of senders names one address twice",
        ),
    ];
    for (error, text) in rows {
        assert_eq!(error.to_string(), text);
    }
    assert_eq!(
        format!(
            "{:?}",
            SendersError::Twice {
                earlier: at(1),
                later: at(2)
            }
        ),
        "Twice { earlier: 1, later: 2 }"
    );
}
