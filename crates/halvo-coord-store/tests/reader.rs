mod common;

use common::*;
use halvo_coord_store::{coordinator, CoordReply, MemoryStore};
use halvo_schema::ErrorName;

fn send(store: &mut MemoryStore, records: &[String]) -> Result<CoordReply, ErrorName> {
    coordinator(store, &log(records))
}

fn read_back(store: &mut MemoryStore) -> (Vec<String>, u32) {
    match coordinator(store, &history(None)) {
        Ok(CoordReply::History(h)) => (
            h.records
                .iter()
                .map(|r| r.to_json().expect("written"))
                .collect(),
            h.last,
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_hold_sign_in_refused_is_stored_and_read_with_the_run_that_it_answers() {
    let mut store = store();
    let records = [
        hold_row(1, "hold_set", "sign_in_refused", Some(1)),
        hold_row(2, "hold_cleared", "sign_in_refused", Some(1)),
    ];
    assert_eq!(send(&mut store, &records), stored(2));
    assert_eq!(rows(&store), records);
    assert_eq!(read_back(&mut store), (records.to_vec(), 2));
    assert_eq!(send(&mut store, &[row(3, TODAY)]), stored(1));
}

fn lapsed(seq: u32, kind: &str, cause: &str) -> String {
    format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","seq":{seq},"day":{TODAY},"at":{},"event":"item_lapsed","run":null,"slot":null,"item":"{}","kind":"{kind}","effect":null,"approval":null,"digest":null,"answer":null,"cause":"{cause}","trigger":null,"origin":null}}"#,
        wf(1),
        agent(1),
        noon(TODAY),
        item(1)
    )
}

#[test]
fn the_lapse_answer_lost_of_a_confirmation_is_stored_and_read() {
    let mut store = store();
    let records = [
        confirm_answered(1, TODAY, 1),
        lapsed(2, "confirm", "answer_lost"),
    ];
    assert_eq!(send(&mut store, &records), stored(2));
    assert_eq!(read_back(&mut store), (records.to_vec(), 2));
    assert_eq!(send(&mut store, &[row(3, TODAY)]), stored(1));
}

#[test]
fn the_lapse_answer_lost_of_another_kind_is_refused() {
    for kind in [
        "sign_in",
        "link_account",
        "stuck",
        "check",
        "setup",
        "ready",
        "show",
    ] {
        let mut store = store();
        assert_eq!(
            send(&mut store, &[lapsed(1, kind, "answer_lost")]),
            Err(ErrorName::Malformed),
            "{kind}"
        );
        assert!(rows(&store).is_empty());
    }
}

#[test]
fn the_hold_sign_in_refused_without_its_run_is_refused() {
    for event in ["hold_set", "hold_cleared"] {
        let mut store = store();
        assert_eq!(
            send(&mut store, &[hold_row(1, event, "sign_in_refused", None)]),
            Err(ErrorName::Malformed),
            "{event}"
        );
        assert!(rows(&store).is_empty());
    }
    for (event, cause) in [
        ("hold_set", "person"),
        ("hold_set", "failures"),
        ("hold_cleared", "person"),
        ("hold_cleared", "failures"),
    ] {
        let mut store = store();
        assert_eq!(
            send(&mut store, &[hold_row(1, event, cause, Some(1))]),
            Err(ErrorName::Malformed),
            "{event} {cause} names no run"
        );
    }
}
