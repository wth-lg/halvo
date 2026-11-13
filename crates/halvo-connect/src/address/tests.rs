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
fn an_empty_piece_refuses_the_slot() {
    for slot in [
        "",
        " ",
        ",",
        "ann@people.test,",
        "ann@people.test, ",
        ",ann@people.test",
        " , ann@people.test",
        "ann@people.test,,bob@people.test",
        "ann@people.test, ,bob@people.test",
    ] {
        assert_eq!(read(slot), Err(SendersError::Empty), "{slot:?}");
    }
}

#[test]
fn a_piece_that_is_no_address_refuses_the_slot() {
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
        "\tann@people.test",
        "ann@people.test\t",
        "ann@people.test\u{a0}",
        "ännchen@people.test",
        long.as_str(),
        "bob@people.test, Ann Berg",
    ] {
        assert_eq!(read(slot), Err(SendersError::NotAnAddress), "{slot:?}");
    }
    let at_bound = of_length(320);
    assert_eq!(read(&at_bound), Ok(vec![at_bound.clone()]));
}

#[test]
fn only_the_space_is_trimmed_and_other_ascii_white_space_refuses_the_slot() {
    let ann = "ann@people.test";
    for byte in ['\t', '\n', '\u{b}', '\u{c}', '\r'] {
        for piece in [
            format!("{byte}{ann}"),
            format!("{ann}{byte}"),
            format!("{byte}{ann}{byte}"),
            format!(" {byte}{ann}{byte} "),
        ] {
            for slot in [
                piece.clone(),
                format!("{piece}, bob@people.test"),
                format!("bob@people.test, {piece}"),
            ] {
                assert_eq!(read(&slot), Err(SendersError::NotAnAddress), "{slot:?}");
            }
        }
        let fits = format!("{}{byte}", of_length(319));
        assert_eq!(read(&fits), Err(SendersError::NotAnAddress), "{fits:?}");
    }
    let at_bound = of_length(320);
    assert_eq!(
        read(&format!("   {at_bound}  , bob@people.test")),
        Ok(vec![at_bound.clone(), "bob@people.test".to_owned()])
    );
    let over = of_length(321);
    assert_eq!(read(&format!(" {over} ")), Err(SendersError::NotAnAddress));
}

#[test]
fn an_address_twice_without_ascii_case_refuses_the_slot() {
    for slot in [
        "ann@people.test, ann@people.test",
        "ann@people.test, ANN@People.TEST",
        "ann@people.test, bob@people.test, Ann@people.test",
        "bob@people.test, ann@people.test, BOB@people.test",
    ] {
        assert_eq!(read(slot), Err(SendersError::Twice), "{slot}");
    }
    assert_eq!(
        read("élan@people.test, ÉLAN@people.test"),
        Err(SendersError::NotAnAddress)
    );
}

#[test]
fn the_first_fault_in_the_order_of_the_text_is_given() {
    let rows = [
        ("Ann Berg, , ann@people.test", SendersError::NotAnAddress),
        (", Ann Berg", SendersError::Empty),
        (
            "ann@people.test, ANN@people.test, Ann Berg",
            SendersError::Twice,
        ),
        (
            "ann@people.test, Ann Berg, ann@people.test",
            SendersError::NotAnAddress,
        ),
        ("ann@people.test, ann@people.test,", SendersError::Twice),
    ];
    for (slot, fault) in rows {
        assert_eq!(read(slot), Err(fault), "{slot}");
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
            SendersError::Empty,
            "the list of senders holds an empty entry",
        ),
        (
            SendersError::NotAnAddress,
            "an entry of the list of senders is not an address",
        ),
        (
            SendersError::Twice,
            "the list of senders names one address twice",
        ),
    ];
    for (error, text) in rows {
        assert_eq!(error.to_string(), text);
    }
    assert_eq!(format!("{:?}", SendersError::Twice), "Twice");
}
