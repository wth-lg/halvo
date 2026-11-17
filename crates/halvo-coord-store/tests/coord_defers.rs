mod common;

use common::*;
use halvo_coord::items::drive::log_text;
use halvo_coord::types::{AgentId, Defers, Event, RunEnd, RunId, What, WorkflowId};
use halvo_coord_store::coordinator;
use std::num::NonZeroU32;

fn ended(seq: u32, run_n: u32, defers: Option<Defers>) -> Event {
    Event {
        workflow: WorkflowId::parse(&wf(1)).unwrap(),
        agent: AgentId::parse(&agent(1)).unwrap(),
        seq: NonZeroU32::new(seq).unwrap(),
        day: TODAY,
        at: noon(TODAY),
        what: What::RunEnded {
            run: RunId::parse(&run(run_n)).unwrap(),
            cause: RunEnd::Completed,
            defers,
        },
    }
}

/// The record of the one event of `log_text`, as halvo-coord writes it.
fn record_text(event: &Event) -> String {
    let body = log_text(&event.workflow, &event.agent, std::slice::from_ref(event));
    let start = body.find(r#""records":["#).unwrap() + r#""records":["#.len();
    body[start..body.len() - 2].to_owned()
}

#[test]
fn a_run_ended_with_both_defer_values_in_halvo_coords_row_text_is_stored_as_written() {
    let events = [
        ended(
            1,
            1,
            Some(Defers {
                launch: true,
                ticks_failed: 2,
            }),
        ),
        ended(
            2,
            2,
            Some(Defers {
                launch: false,
                ticks_failed: u32::MAX,
            }),
        ),
        ended(3, 3, None),
    ];
    let workflow = WorkflowId::parse(&wf(1)).unwrap();
    let body = log_text(&workflow, &AgentId::parse(&agent(1)).unwrap(), &events);
    let mut store = store();
    assert_eq!(
        coordinator(&mut store, &request("coordinator_write", &body)),
        stored(3)
    );
    let written: Vec<String> = events.iter().map(record_text).collect();
    assert!(
        written[0].ends_with(r#""defer_launch":"yes","defer_ticks_failed":2}"#),
        "{}",
        written[0]
    );
    assert!(
        written[1].ends_with(r#""defer_launch":"no","defer_ticks_failed":4294967295}"#),
        "{}",
        written[1]
    );
    assert!(
        written[2].ends_with(r#""defer_launch":null,"defer_ticks_failed":null}"#),
        "{}",
        written[2]
    );
    assert_eq!(
        rows(&store),
        written,
        "the store holds what halvo-coord wrote"
    );
}
