mod common;

use common::*;
use halvo_coord_store::{coordinator, Host, MemoryStore};
use halvo_schema::{ErrorName, Instant};

fn send(
    store: &mut MemoryStore,
    records: &[String],
) -> Result<halvo_coord_store::CoordReply, ErrorName> {
    coordinator(store, &log(records))
}

#[test]
fn stores_a_body_and_answers_its_count() {
    let mut store = store();
    let records = [row(1, TODAY), row(2, TODAY), row(3, TODAY)];
    assert_eq!(send(&mut store, &records), stored(3));
    assert_eq!(rows(&store), records);
    assert_eq!(send(&mut store, &[row(4, TODAY)]), stored(1));
    assert_eq!(rows(&store).len(), 4);
}

#[test]
fn commits_what_the_typed_writer_writes_and_never_the_bytes() {
    let mut store = store();
    let given = row(1, TODAY)
        .replacen(r#"{"v":1,"#, "{ ", 1)
        .replacen(r#""origin":null}"#, r#""origin" : null , "v" : 1 }"#, 1)
        .replacen(
            &format!(r#""agent":"{}""#, agent(1)),
            &format!(r#""agent":"4{}""#, &agent(1)[1..]),
            1,
        );
    assert_ne!(given, row(1, TODAY));
    assert_eq!(canonical(&given), row(1, TODAY));
    assert_eq!(send(&mut store, &[given]), stored(1));
    assert_eq!(rows(&store), [row(1, TODAY)]);
}

#[test]
fn takes_the_keys_of_the_body_in_any_order() {
    let mut store = store();
    let body = format!(
        r#"{{"records":[{}],"agent":"{}","op":"coord_log","workflow":"{}"}}"#,
        row(1, TODAY),
        agent(1),
        wf(1)
    );
    assert_eq!(
        coordinator(&mut store, &request("coordinator_write", &body)),
        stored(1)
    );
}

#[test]
fn a_malformed_body_stores_nothing() {
    let long = format!(
        r#"{{"op":"coord_log","workflow":"{}","agent":"{}","records":[{}]{}}}"#,
        wf(1),
        agent(1),
        row(1, TODAY),
        " ".repeat(4_096)
    );
    let deleted = format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","seq":1,"day":{TODAY},"at":{},"event":"run_ended","run":"{}","slot":null,"item":null,"kind":null,"effect":null,"approval":null,"digest":null,"answer":null,"cause":"deleted","trigger":null,"origin":null}}"#,
        wf(1),
        agent(1),
        noon(TODAY),
        run(1)
    );
    let nine: Vec<String> = (1..=9).map(|n| row(n, TODAY)).collect();
    let bodies = [
        "[]".to_owned(),
        r#""coord_log""#.to_owned(),
        log_body(1, 1, &[]),
        log_body(1, 1, &nine),
        log_body(
            1,
            1,
            &[row(1, TODAY), hold_set(2, 2, 2, TODAY, noon(TODAY))],
        ),
        log_body(1, 1, &[deleted]),
        log_body(1, 1, &[row(1, TODAY)]).replacen("coord_log", "coord_logs", 1),
        log_body(1, 1, &[row(1, TODAY)]).replacen(r#""op""#, r#""other":1,"op""#, 1),
        log_body(1, 1, &[row(1, TODAY).replacen(r#""v":1"#, r#""v":2"#, 1)]),
        long,
    ];
    for body in bodies {
        let mut store = store();
        assert_eq!(
            coordinator(&mut store, &request("coordinator_write", &body)),
            Err(ErrorName::Malformed),
            "{body}"
        );
        assert!(rows(&store).is_empty(), "{body}");
    }
}

#[test]
fn a_body_of_no_stored_workflow_or_of_another_agent_stores_nothing() {
    let mut store = store();
    store.put_workflow(dismissed(3));
    store.put_workflow(workflow(4, None));
    let cases = [
        (9, 1, ErrorName::NoWorkflow),
        (1, 2, declared("binding")),
        (3, 1, declared("binding")),
        (4, 1, declared("binding")),
    ];
    for (w, a, error) in cases {
        let body = log_body(w, a, &[hold_set(w, a, 1, TODAY, noon(TODAY))]);
        assert_eq!(
            coordinator(&mut store, &request("coordinator_write", &body)),
            Err(error),
            "workflow {w} agent {a}"
        );
    }
    assert!(rows(&store).is_empty());
}

#[test]
fn each_seq_is_the_stored_last_plus_one() {
    let mut store = store();
    let seq = Err(declared("seq"));
    assert_eq!(send(&mut store, &[row(2, TODAY)]), seq, "the first is 1");
    assert_eq!(
        send(&mut store, &[row(1, TODAY), row(3, TODAY)]),
        seq,
        "a gap inside"
    );
    assert_eq!(
        send(&mut store, &[row(1, TODAY), row(1, TODAY)]),
        seq,
        "a repeat inside"
    );
    assert!(rows(&store).is_empty());
    assert_eq!(send(&mut store, &[row(1, TODAY), row(2, TODAY)]), stored(2));
    assert_eq!(
        send(&mut store, &[row(1, TODAY), row(2, TODAY)]),
        seq,
        "a repeat"
    );
    assert_eq!(
        send(&mut store, &[row(2, TODAY), row(3, TODAY)]),
        seq,
        "an overlap"
    );
    assert_eq!(send(&mut store, &[row(4, TODAY)]), seq, "a gap");
    assert_eq!(rows(&store).len(), 2);
    assert_eq!(send(&mut store, &[row(3, TODAY)]), stored(1));
    let other = log_body(2, 2, &[hold_set(2, 2, 1, TODAY, noon(TODAY))]);
    assert_eq!(
        coordinator(&mut store, &request("coordinator_write", &other)),
        stored(1),
        "each agent counts from 1"
    );
    assert_eq!(rows(&store).len(), 4);
}

#[test]
fn no_seq_follows_the_largest() {
    let mut store = store();
    assert_eq!(
        Host::commit(&mut store, vec![typed(row(u32::MAX, TODAY))]),
        Ok(())
    );
    assert_eq!(send(&mut store, &[row(1, TODAY)]), Err(declared("seq")));
    assert_eq!(rows(&store).len(), 1);
}

#[test]
fn day_refusals() {
    let late = at(TODAY, 23 * 3_600 + 1_800, WEST);
    let (mut store, _) = store_at(late, WEST);
    let utc_day = TODAY + 1;
    let refused = [
        hold_set(1, 1, 1, utc_day, late),
        hold_set(1, 1, 1, TODAY - 1, late),
        hold_set(1, 1, 1, TODAY + 1, at(TODAY + 1, 3_600, WEST)),
        hold_set(1, 1, 1, TODAY - 93, at(TODAY - 93, 3_600, WEST)),
        hold_set(1, 1, 1, 0, at(0, 3_600, WEST)),
    ];
    for record in refused {
        assert_eq!(
            send(&mut store, std::slice::from_ref(&record)),
            Err(ErrorName::Malformed),
            "{record}"
        );
        assert!(rows(&store).is_empty(), "{record}");
    }
    assert_eq!(
        send(
            &mut store,
            &[
                hold_set(1, 1, 1, TODAY, late),
                hold_set(1, 1, 2, utc_day, late)
            ]
        ),
        Err(ErrorName::Malformed)
    );
    assert_eq!(
        send(
            &mut store,
            &[
                hold_set(1, 1, 1, TODAY, late),
                hold_set(1, 1, 2, TODAY - 1, late)
            ]
        ),
        Err(ErrorName::Malformed)
    );
    assert!(rows(&store).is_empty());
    assert_eq!(
        send(
            &mut store,
            &[
                hold_set(1, 1, 1, TODAY, late),
                hold_set(1, 1, 2, TODAY - 92, at(TODAY - 92, 0, WEST))
            ]
        ),
        stored(2)
    );
    assert_eq!(Host::clock(&store).day(Instant(late)), TODAY);
}

#[test]
fn a_refused_body_leaves_nothing_in_the_way_of_the_next() {
    let mut store = store();
    assert_eq!(
        send(&mut store, &[row(1, TODAY), row(2, TODAY + 1)]),
        Err(ErrorName::Malformed)
    );
    assert_eq!(send(&mut store, &[row(1, TODAY), row(2, TODAY)]), stored(2));
    assert_eq!(rows(&store), [row(1, TODAY), row(2, TODAY)]);
}

#[test]
fn a_cancelled_lapse_of_an_item_that_no_run_holds_is_stored() {
    let mut store = store();
    let raised = format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","seq":1,"day":{TODAY},"at":{},"event":"item_raised","run":null,"slot":null,"item":"{}","kind":"setup","effect":null,"approval":null,"digest":null,"answer":null,"cause":null,"trigger":null,"origin":null}}"#,
        wf(1),
        agent(1),
        noon(TODAY),
        item(1)
    );
    let lapsed = format!(
        r#"{{"v":1,"workflow":"{}","agent":"{}","seq":2,"day":{TODAY},"at":{},"event":"item_lapsed","run":null,"slot":null,"item":"{}","kind":"setup","effect":null,"approval":null,"digest":null,"answer":null,"cause":"cancelled","trigger":null,"origin":null}}"#,
        wf(1),
        agent(1),
        noon(TODAY) + NS,
        item(1)
    );
    assert_eq!(send(&mut store, &[raised, lapsed]), stored(2));
}

const EAST: i64 = 5 * 3_600;

#[test]
fn a_negative_at_is_refused_east_of_utc() {
    let (mut store, _) = store_at(at(10, 3_600, EAST), EAST);
    assert_eq!(
        send(&mut store, &[hold_set(1, 1, 1, 0, -1)]),
        Err(ErrorName::Malformed)
    );
    assert!(rows(&store).is_empty());
}

#[test]
fn a_negative_at_is_refused_west_of_utc() {
    let (mut store, _) = store_at(at(10, 3_600, WEST), WEST);
    assert_eq!(
        send(&mut store, &[hold_set(1, 1, 1, 0, -1)]),
        Err(ErrorName::Malformed)
    );
    assert!(rows(&store).is_empty());
}

#[test]
fn a_negative_at_in_the_second_record_refuses_the_body() {
    let (mut store, _) = store_at(at(10, 3_600, EAST), EAST);
    let first = hold_set(1, 1, 1, 10, at(10, 1_800, EAST));
    assert_eq!(
        send(&mut store, &[first.clone(), hold_set(1, 1, 2, 0, -1)]),
        Err(ErrorName::Malformed)
    );
    assert!(rows(&store).is_empty());
    assert_eq!(send(&mut store, &[first]), stored(1));
}

#[test]
fn the_reply_of_a_stored_body_is_the_count() {
    let mut store = store();
    let one = send(&mut store, &[row(1, TODAY)]).expect("stored");
    assert_eq!(one.to_json().expect("written"), r#"{"stored":1}"#);
    let eight: Vec<String> = (2..=9).map(|n| row(n, TODAY)).collect();
    let reply = send(&mut store, &eight).expect("stored");
    assert_eq!(reply.to_json().expect("written"), r#"{"stored":8}"#);
}

#[test]
fn the_day_tie_comes_before_the_seq_check() {
    let mut store = store();
    assert_eq!(send(&mut store, &[row(1, TODAY), row(2, TODAY)]), stored(2));
    let stale_and_off_day = hold_set(1, 1, 1, TODAY - 1, noon(TODAY));
    assert_eq!(
        send(&mut store, &[stale_and_off_day]),
        Err(ErrorName::Malformed)
    );
    assert_eq!(rows(&store).len(), 2);
}

#[test]
fn an_op_of_neither_body_stores_nothing() {
    let mut store = store();
    let body = log_body(1, 1, &[row(1, TODAY)]).replacen("coord_log", "coord_delete", 1);
    assert_eq!(
        coordinator(&mut store, &request("coordinator_write", &body)),
        Err(ErrorName::Malformed)
    );
    assert!(rows(&store).is_empty());
}
