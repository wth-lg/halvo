mod common;

use common::*;
use halvo_coord_store::{coordinator, CoordReply, Host, MemoryStore};
use halvo_schema::coordinator::body::History;
use halvo_schema::coordinator::record::CoordEvent;
use halvo_schema::ErrorName;

fn read(store: &mut MemoryStore, after: Option<u32>) -> Result<(Vec<u32>, u32), ErrorName> {
    match coordinator(store, &history(after))? {
        CoordReply::History(h) => Ok((h.records.iter().map(|r| r.seq.get()).collect(), h.last)),
        other => panic!("not a history: {other:?}"),
    }
}

fn fill(store: &mut MemoryStore, n: u32) {
    let mut seq = 1;
    while seq <= n {
        let records: Vec<String> = (seq..=n.min(seq + 7)).map(|s| row(s, TODAY)).collect();
        let sent = coordinator(store, &log(&records));
        assert_eq!(sent, stored(records.len() as u32));
        seq += records.len() as u32;
    }
}

#[test]
fn an_empty_store_answers_no_record_and_last_0() {
    let mut store = store();
    assert_eq!(read(&mut store, None), Ok((vec![], 0)));
    assert_eq!(read(&mut store, Some(5)), Ok((vec![], 0)));
}

#[test]
fn pages_of_64_in_ascending_seq() {
    let mut store = store();
    fill(&mut store, 70);
    assert_eq!(read(&mut store, None), Ok(((1..=64).collect(), 70)));
    assert_eq!(read(&mut store, Some(0)), Ok(((1..=64).collect(), 70)));
    assert_eq!(read(&mut store, Some(64)), Ok(((65..=70).collect(), 70)));
    assert_eq!(read(&mut store, Some(6)), Ok(((7..=70).collect(), 70)));
    assert_eq!(read(&mut store, Some(70)), Ok((vec![], 70)));
    assert_eq!(read(&mut store, Some(200)), Ok((vec![], 70)));
}

#[test]
fn a_page_of_exactly_64_and_one_of_none() {
    let mut store = store();
    fill(&mut store, 64);
    assert_eq!(read(&mut store, None), Ok(((1..=64).collect(), 64)));
    assert_eq!(read(&mut store, Some(64)), Ok((vec![], 64)));
}

#[test]
fn the_records_are_the_stored_rows() {
    let mut store = store();
    fill(&mut store, 3);
    let Ok(CoordReply::History(h)) = coordinator(&mut store, &history(None)) else {
        panic!("no history");
    };
    let expected: Vec<CoordEvent> = rows(&store)
        .iter()
        .map(|r| CoordEvent::parse(r.as_bytes()).expect("a row"))
        .collect();
    assert_eq!(h.records, expected);
    let text = CoordReply::History(h.clone()).to_json().expect("written");
    assert_eq!(History::parse(text.as_bytes()), Ok(h));
}

#[test]
fn rows_of_another_agent_are_not_read() {
    let mut store = store();
    fill(&mut store, 2);
    let other = log_body(2, 2, &[hold_set(2, 2, 1, TODAY, noon(TODAY))]);
    assert_eq!(
        coordinator(&mut store, &request("coordinator_write", &other)),
        stored(1)
    );
    assert_eq!(read(&mut store, None), Ok((vec![1, 2], 2)));
    let theirs = request("coordinator_write", &history_body(2, 2, None));
    let Ok(CoordReply::History(h)) = coordinator(&mut store, &theirs) else {
        panic!("no history");
    };
    assert_eq!(h.records.len(), 1);
    assert_eq!(h.last, 1);
    assert_eq!(h.records[0].agent.as_str(), agent(2));
}

#[test]
fn gaps_are_answered_in_ascending_seq() {
    let mut store = store();
    assert_eq!(
        Host::commit(
            &mut store,
            vec![
                typed(row(5, TODAY)),
                typed(row(1, TODAY)),
                typed(row(4, TODAY))
            ]
        ),
        Ok(())
    );
    assert_eq!(read(&mut store, None), Ok((vec![1, 4, 5], 5)));
    assert_eq!(read(&mut store, Some(1)), Ok((vec![4, 5], 5)));
    assert_eq!(read(&mut store, Some(2)), Ok((vec![4, 5], 5)));
}

#[test]
fn the_stored_last_is_that_of_the_rows_that_stand() {
    let (mut store, clock) = store_at(noon(TODAY), 0);
    fill(&mut store, 3);
    clock.set(noon(TODAY + 93));
    assert_eq!(store.expire(), 3);
    assert_eq!(read(&mut store, None), Ok((vec![], 0)));
    assert_eq!(
        coordinator(&mut store, &log(&[row(4, TODAY + 93)])),
        Err(declared("seq"))
    );
    assert_eq!(
        coordinator(&mut store, &log(&[row(1, TODAY + 93)])),
        stored(1)
    );
}

#[test]
fn the_read_leaves_out_rows_past_their_bound_with_no_expiry() {
    let (mut store, clock) = store_at(noon(TODAY), 0);
    fill(&mut store, 3);
    clock.set(noon(TODAY + 93));
    assert_eq!(read(&mut store, None), Ok((vec![], 0)));
    assert_eq!(
        coordinator(&mut store, &log(&[row(4, TODAY + 93)])),
        Err(declared("seq"))
    );
    assert_eq!(
        coordinator(&mut store, &log(&[row(1, TODAY + 93)])),
        stored(1)
    );
}

#[test]
fn takes_the_keys_of_the_body_in_any_order() {
    let mut store = store();
    fill(&mut store, 1);
    let body = format!(
        r#"{{"after":null,"agent":"{}","workflow":"{}","op":"coord_history"}}"#,
        agent(1),
        wf(1)
    );
    let Ok(CoordReply::History(h)) = coordinator(&mut store, &request("coordinator_write", &body))
    else {
        panic!("no history");
    };
    assert_eq!(h.records.len(), 1);
}

#[test]
fn a_history_of_no_stored_workflow_or_of_another_agent_is_refused() {
    let mut store = store();
    store.put_workflow(dismissed(3));
    store.put_workflow(workflow(4, None));
    for (w, a, error) in [
        (9, 1, ErrorName::NoWorkflow),
        (1, 2, declared("binding")),
        (3, 1, declared("binding")),
        (4, 1, declared("binding")),
    ] {
        let body = history_body(w, a, None);
        assert_eq!(
            coordinator(&mut store, &request("coordinator_write", &body)),
            Err(error),
            "workflow {w} agent {a}"
        );
    }
}

#[test]
fn a_malformed_history_is_refused() {
    let mut store = store();
    let good = history_body(1, 1, None);
    for body in [
        good.replacen(r#""after":null"#, r#""after":-1"#, 1),
        good.replacen(r#""after":null"#, r#""after":"1""#, 1),
        good.replacen(r#""op""#, r#""other":1,"op""#, 1),
        good.replacen("coord_history", "coord_histories", 1),
        good.replacen(r#","after":null"#, "", 1),
    ] {
        assert_eq!(
            coordinator(&mut store, &request("coordinator_write", &body)),
            Err(ErrorName::Malformed),
            "{body}"
        );
    }
    assert_eq!(
        coordinator(&mut store, &request("runner", &good)),
        Err(ErrorName::Role)
    );
}
