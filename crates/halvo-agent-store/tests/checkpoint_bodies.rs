mod common;

use common::*;
use halvo_agent_store::{MemoryStore, AGENT_KINDS};
use halvo_schema::executable::body::CheckpointsReply;
use halvo_schema::executable::names::{CheckpointState, EffectState, LeftReason, YesNo};
use halvo_schema::executable::{AgentCheckpoint, CoordinatorReply};
use halvo_schema::ErrorName;
use serde_json::{json, Value};

const FOLDER: &str = "/data/books";

fn book(n: u32) -> String {
    format!("{FOLDER}/book-{n}.xlsx")
}

fn copy_of(r: u32, n: u32) -> String {
    format!("book-{n}.halvo-checkpoint-{}.xlsx", run(r))
}

fn all_rows(store: &MemoryStore) -> Vec<Vec<String>> {
    AGENT_KINDS
        .iter()
        .map(|(kind, _)| rows(store, kind))
        .collect()
}

fn planned(r: u32, n: u32) -> Value {
    json!({
        "v": 1, "workflow": wf(1), "agent": agent(1), "run": run(r), "seq": 1, "day": ROW_DAY,
        "at": ROW_AT, "folder": [FOLDER], "workbook": [book(n)], "copy": [copy_of(r, n)],
        "sha256": null, "bytes": null, "state": "planned", "left": null,
    })
}

fn made(r: u32, n: u32) -> Value {
    let mut row = planned(r, n);
    row["seq"] = json!(2);
    row["state"] = json!("made");
    row["sha256"] = json!(digest(n));
    row["bytes"] = json!(4096 + n);
    row
}

fn as_agent_2(mut row: Value) -> Value {
    row["workflow"] = json!(wf(2));
    row["agent"] = json!(agent(2));
    row
}

fn put_checkpoint(
    store: &mut MemoryStore,
    row: Value,
) -> Result<halvo_schema::executable::RunnerReply, ErrorName> {
    let body = json!({"op": "checkpoint", "agent": row["agent"].clone(), "run": row["run"].clone(), "record": row});
    runner(store, body)
}

fn checkpoints(store: &MemoryStore) -> Vec<AgentCheckpoint> {
    rows(store, "agent_checkpoint")
        .iter()
        .map(|r| AgentCheckpoint::parse(r.as_bytes()).expect("a stored checkpoint row"))
        .collect()
}

fn list(store: &mut MemoryStore, after: Option<u32>) -> CheckpointsReply {
    let body =
        json!({"op": "list_checkpoints", "workflow": null, "agent": null, "after": after.map(run)});
    match coordinator(store, body) {
        Ok(CoordinatorReply::Checkpoints(reply)) => reply,
        other => panic!("a list of checkpoints: {other:?}"),
    }
}

fn mark(
    store: &mut MemoryStore,
    w: u32,
    a: u32,
    r: u32,
    n: u32,
    state: &str,
    left: Option<&str>,
) -> Result<CoordinatorReply, ErrorName> {
    coordinator(
        store,
        json!({
            "op": "mark_checkpoint", "workflow": wf(w), "agent": agent(a), "run": run(r),
            "workbook": [book(n)], "state": state, "left": left,
        }),
    )
}

fn stored_reply(n: u32) -> CoordinatorReply {
    CoordinatorReply::parse(format!(r#"{{"stored":{n}}}"#).as_bytes()).expect("a stored reply")
}

fn edit(r: u32, e: u32, seq: u32, state: &str, n: u32) -> Value {
    let mut row = row(r, e, seq, state, "local_edit", 1, None, None);
    for field in row["kept"].as_array_mut().expect("the kept fields") {
        if field["name"] == "file" {
            field["value"] = json!([book(n)]);
        }
    }
    row
}

fn committed_edit(store: &mut MemoryStore, r: u32, e: u32, n: u32) {
    for (seq, state) in [(1, "cleared"), (2, "releasing"), (3, "committed")] {
        assert_eq!(put(store, edit(r, e, seq, state, n)), Ok(stored(1)));
    }
}

fn spreadsheet() -> MemoryStore {
    store_with("spreadsheet_fill")
}

#[test]
fn checkpoint_stores_the_planned_row_and_then_the_made_row() {
    let mut store = spreadsheet();
    assert_eq!(put_checkpoint(&mut store, planned(1, 1)), Ok(stored(1)));
    assert_eq!(put_checkpoint(&mut store, planned(1, 1)), Ok(stored(1)));
    assert_eq!(checkpoints(&store).len(), 1);
    assert_eq!(put_checkpoint(&mut store, made(1, 1)), Ok(stored(1)));
    let later = at(NOW.at.0 + NANOS_PER_DAY);
    let body = json!({"op": "checkpoint", "agent": agent(1), "run": run(1), "record": made(1, 1)});
    assert_eq!(runner_at(&mut store, later, body), Ok(stored(1)));
    let mut rows = checkpoints(&store);
    rows.sort_by_key(|c| c.seq);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0],
        AgentCheckpoint::parse(planned(1, 1).to_string().as_bytes()).expect("a planned row")
    );
    assert_eq!(
        rows[1],
        AgentCheckpoint::parse(made(1, 1).to_string().as_bytes()).expect("a made row")
    );
}

#[test]
fn checkpoint_refuses_a_row_that_does_not_follow_the_checkpoint_s_newest_row() {
    let mut store = spreadsheet();
    let before = all_rows(&store);
    assert_eq!(
        put_checkpoint(&mut store, made(1, 1)),
        Err(declared("effect"))
    );
    assert_eq!(all_rows(&store), before);
    assert_eq!(put_checkpoint(&mut store, planned(1, 1)), Ok(stored(1)));
    let mut other_copy = made(1, 1);
    other_copy["copy"] = json!(["book-1.other.xlsx"]);
    let mut other_folder = made(1, 1);
    other_folder["folder"] = json!(["/data"]);
    let mut other_plan = planned(1, 1);
    other_plan["copy"] = json!(["book-1.other.xlsx"]);
    for row in [other_copy, other_folder, other_plan] {
        assert_eq!(
            put_checkpoint(&mut store, row.clone()),
            Err(declared("effect")),
            "{row}"
        );
    }
    assert_eq!(put_checkpoint(&mut store, made(1, 1)), Ok(stored(1)));
    assert_eq!(
        put_checkpoint(&mut store, planned(1, 1)),
        Err(declared("effect"))
    );
    let mut other_digest = made(1, 1);
    other_digest["sha256"] = json!(digest(9));
    assert_eq!(
        put_checkpoint(&mut store, other_digest),
        Err(declared("effect"))
    );
    assert_eq!(put_checkpoint(&mut store, planned(1, 2)), Ok(stored(1)));
    assert_eq!(checkpoints(&store).len(), 3);
}

#[test]
fn checkpoint_is_held_to_the_rules_of_every_runner_body() {
    let mut store = unstarted("spreadsheet_fill");
    assert_eq!(
        put_checkpoint(&mut store, planned(1, 1)),
        Err(declared("binding"))
    );
    start(&mut store, 1);
    let mut other_workflow = planned(1, 1);
    other_workflow["workflow"] = json!(wf(2));
    assert_eq!(
        put_checkpoint(&mut store, other_workflow),
        Err(declared("binding"))
    );
    let mut off_day = planned(1, 1);
    off_day["day"] = json!(ROW_DAY + 1);
    assert_eq!(put_checkpoint(&mut store, off_day), Err(declared("day")));
    assert!(checkpoints(&store).is_empty());
    assert_eq!(put_checkpoint(&mut store, planned(1, 1)), Ok(stored(1)));
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    assert_eq!(
        put_checkpoint(&mut store, made(1, 1)),
        Err(declared("effect"))
    );
    assert_eq!(checkpoints(&store).len(), 1);
    let mut gone = made(1, 1);
    gone["state"] = json!("gone");
    assert_eq!(put_checkpoint(&mut store, gone), Err(ErrorName::Malformed));
}

fn two_agents() -> MemoryStore {
    let mut store = spreadsheet();
    store.put_workflow(workflow(&wf(2), "spreadsheet_fill", "approved", None));
    create(&mut store, 2, 2).expect("create_agent of the second agent");
    store
}

#[test]
fn list_checkpoints_reads_the_newest_planned_or_made_row_of_each_checkpoint_over_all_agents() {
    let mut store = two_agents();
    assert_eq!(start_at(&mut store, NOW, 2, 2, 3), Ok(accepted()));
    start(&mut store, 2);
    assert_eq!(
        put_checkpoint(&mut store, as_agent_2(planned(3, 1))),
        Ok(stored(1))
    );
    assert_eq!(
        put_checkpoint(&mut store, as_agent_2(made(3, 1))),
        Ok(stored(1))
    );
    assert_eq!(put_checkpoint(&mut store, planned(2, 2)), Ok(stored(1)));
    assert_eq!(put_checkpoint(&mut store, planned(2, 1)), Ok(stored(1)));
    assert_eq!(put_checkpoint(&mut store, planned(1, 1)), Ok(stored(1)));
    assert_eq!(put_checkpoint(&mut store, made(1, 1)), Ok(stored(1)));
    assert_eq!(put_checkpoint(&mut store, planned(1, 3)), Ok(stored(1)));
    assert_eq!(
        mark(&mut store, 1, 1, 1, 3, "gone", None),
        Ok(stored_reply(1))
    );

    let reply = list(&mut store, None);
    let keys: Vec<(String, String, String, CheckpointState)> = reply
        .checkpoints
        .iter()
        .map(|c| {
            let state = match c.state {
                halvo_schema::executable::names::ListedState::Planned => CheckpointState::Planned,
                halvo_schema::executable::names::ListedState::Made => CheckpointState::Made,
            };
            (
                c.agent.as_str().to_owned(),
                c.run.as_str().to_owned(),
                c.workbook.joined(),
                state,
            )
        })
        .collect();
    assert_eq!(
        keys,
        [
            (agent(1), run(1), book(1), CheckpointState::Made),
            (agent(1), run(2), book(1), CheckpointState::Planned),
            (agent(1), run(2), book(2), CheckpointState::Planned),
            (agent(2), run(3), book(1), CheckpointState::Made),
        ]
    );
    assert_eq!(reply.next, None);
    let first = &reply.checkpoints[0];
    assert_eq!(first.workflow.as_str(), wf(1));
    assert_eq!(first.folder.joined(), FOLDER);
    assert_eq!(first.copy.joined(), copy_of(1, 1));
    assert_eq!(
        first.sha256.as_ref().map(|d| d.to_string()),
        Some(digest(1))
    );
    assert_eq!(first.bytes, Some(4097));
    assert_eq!(first.at.0, ROW_AT);
    assert_eq!(reply.checkpoints[1].sha256, None);
    assert_eq!(reply.checkpoints[3].workflow.as_str(), wf(2));

    let after = list(&mut store, Some(1));
    assert_eq!(after.checkpoints.len(), 3);
    assert!(after
        .checkpoints
        .iter()
        .all(|c| c.run.as_str() > run(1).as_str()));
    assert!(list(&mut store, Some(3)).checkpoints.is_empty());
}

#[test]
fn list_checkpoints_says_whether_the_run_ended_and_the_state_of_its_edit_of_the_path() {
    let mut store = spreadsheet();
    assert_eq!(put_checkpoint(&mut store, planned(1, 1)), Ok(stored(1)));
    assert_eq!(put_checkpoint(&mut store, made(1, 1)), Ok(stored(1)));
    assert_eq!(put_checkpoint(&mut store, planned(1, 2)), Ok(stored(1)));
    let listed = list(&mut store, None);
    assert!(listed.checkpoints.iter().all(|c| c.ended == YesNo::No));
    assert!(listed.checkpoints.iter().all(|c| c.effect.is_none()));

    assert_eq!(put(&mut store, edit(1, 1, 1, "cleared", 1)), Ok(stored(1)));
    assert_eq!(
        put(&mut store, edit(1, 1, 2, "releasing", 1)),
        Ok(stored(1))
    );
    let listed = list(&mut store, None);
    assert_eq!(listed.checkpoints[0].effect, Some(EffectState::Releasing));
    assert_eq!(listed.checkpoints[1].effect, None);

    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    let listed = list(&mut store, None);
    assert!(listed.checkpoints.iter().all(|c| c.ended == YesNo::Yes));
    assert_eq!(listed.checkpoints[0].effect, Some(EffectState::Unknown));
    assert_eq!(listed.checkpoints[1].effect, None);
}

#[test]
fn list_checkpoints_reads_the_cautious_state_when_two_edits_of_a_run_name_the_path() {
    let mut store = spreadsheet();
    assert_eq!(put_checkpoint(&mut store, planned(1, 1)), Ok(stored(1)));
    committed_edit(&mut store, 1, 1, 1);
    assert_eq!(put(&mut store, edit(1, 2, 1, "cleared", 1)), Ok(stored(1)));
    assert_eq!(
        put(&mut store, edit(1, 2, 2, "releasing", 1)),
        Ok(stored(1))
    );
    assert_eq!(
        list(&mut store, None).checkpoints[0].effect,
        Some(EffectState::Releasing)
    );
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    assert_eq!(
        list(&mut store, None).checkpoints[0].effect,
        Some(EffectState::Unknown)
    );
}

#[test]
fn list_checkpoints_says_superseded_when_a_later_run_of_the_agent_committed_an_edit_of_the_path() {
    let mut store = two_agents();
    start(&mut store, 2);
    start(&mut store, 4);
    assert_eq!(start_at(&mut store, NOW, 2, 2, 3), Ok(accepted()));
    let later = |mut row: Value| {
        row["at"] = json!(ROW_AT + 1);
        row
    };
    assert_eq!(put_checkpoint(&mut store, planned(1, 1)), Ok(stored(1)));
    assert_eq!(put_checkpoint(&mut store, planned(1, 2)), Ok(stored(1)));
    assert_eq!(
        put_checkpoint(&mut store, later(planned(2, 1))),
        Ok(stored(1))
    );
    assert_eq!(
        put_checkpoint(&mut store, later(planned(4, 2))),
        Ok(stored(1))
    );
    assert_eq!(
        put_checkpoint(&mut store, later(as_agent_2(planned(3, 1)))),
        Ok(stored(1))
    );
    let superseded = |store: &mut MemoryStore| -> Vec<YesNo> {
        list(store, None)
            .checkpoints
            .iter()
            .map(|c| c.superseded)
            .collect()
    };
    assert_eq!(superseded(&mut store), [YesNo::No; 5]);

    let mut other = edit(3, 9, 1, "cleared", 1);
    other = as_agent_2(other);
    for (seq, state) in [(1, "cleared"), (2, "releasing"), (3, "committed")] {
        let mut row = other.clone();
        row["seq"] = json!(seq);
        row["state"] = json!(state);
        assert_eq!(put(&mut store, row), Ok(stored(1)));
    }
    assert_eq!(superseded(&mut store), [YesNo::No; 5]);

    assert_eq!(put(&mut store, edit(4, 4, 1, "cleared", 2)), Ok(stored(1)));
    assert_eq!(
        put(&mut store, edit(4, 4, 2, "releasing", 2)),
        Ok(stored(1))
    );
    assert_eq!(superseded(&mut store), [YesNo::No; 5]);

    committed_edit(&mut store, 2, 2, 1);
    assert_eq!(
        superseded(&mut store),
        [YesNo::Yes, YesNo::No, YesNo::No, YesNo::No, YesNo::No]
    );
    committed_edit(&mut store, 1, 1, 1);
    assert_eq!(
        superseded(&mut store),
        [YesNo::Yes, YesNo::No, YesNo::No, YesNo::No, YesNo::No]
    );
}

#[test]
fn list_checkpoints_pages_by_whole_runs_and_names_the_run_to_ask_after() {
    let mut store = spreadsheet();
    let long = format!("{FOLDER}/{}.xlsx", "w".repeat(9_000));
    for r in 1..=40 {
        start(&mut store, r);
        let mut row = planned(r, 1);
        row["workbook"] = json!(halvo_schema::executable::OwnValue::new(&long));
        assert_eq!(put_checkpoint(&mut store, row), Ok(stored(1)), "run {r}");
    }
    let mut seen = Vec::new();
    let mut after = None;
    let mut pages = 0;
    loop {
        let reply = list(&mut store, after);
        pages += 1;
        let text = CoordinatorReply::Checkpoints(reply.clone())
            .to_json()
            .expect("a page that the writer takes");
        assert!(text.len() <= halvo_schema::executable::body::CHECKPOINTS_REPLY_MAX);
        assert!(!reply.checkpoints.is_empty());
        seen.extend(reply.checkpoints.iter().map(|c| c.run.as_str().to_owned()));
        match reply.next {
            None => break,
            Some(next) => {
                assert_eq!(Some(next.as_str()), seen.last().map(String::as_str));
                after = (1..=40).find(|r| run(*r) == next.as_str());
            }
        }
    }
    assert!(pages > 1, "{pages}");
    assert_eq!(seen, (1..=40).map(run).collect::<Vec<_>>());
}

#[test]
fn mark_checkpoint_stores_the_next_row_of_a_planned_or_made_checkpoint_alone() {
    let mut store = spreadsheet().with_utc_offset(WEST);
    assert_eq!(put_checkpoint(&mut store, planned(1, 1)), Ok(stored(1)));
    assert_eq!(put_checkpoint(&mut store, made(1, 1)), Ok(stored(1)));
    assert_eq!(put_checkpoint(&mut store, planned(1, 2)), Ok(stored(1)));
    let night = at(local(TODAY, 23 * 3600, WEST));
    let body = json!({
        "op": "mark_checkpoint", "workflow": wf(1), "agent": agent(1), "run": run(1),
        "workbook": [book(1)], "state": "gone", "left": null,
    });
    assert_eq!(coordinator_at(&mut store, night, body), Ok(stored_reply(1)));
    assert_eq!(
        mark(&mut store, 1, 1, 1, 2, "left", Some("changed")),
        Ok(stored_reply(1))
    );
    let before = all_rows(&store);
    assert_eq!(
        mark(&mut store, 1, 1, 1, 1, "left", Some("link")),
        Ok(stored_reply(0))
    );
    assert_eq!(
        mark(&mut store, 1, 1, 1, 2, "gone", None),
        Ok(stored_reply(0))
    );
    assert_eq!(
        mark(&mut store, 1, 1, 1, 7, "gone", None),
        Ok(stored_reply(0))
    );
    assert_eq!(
        mark(&mut store, 1, 1, 9, 1, "gone", None),
        Ok(stored_reply(0))
    );
    assert_eq!(all_rows(&store), before);

    let mut rows = checkpoints(&store);
    rows.sort_by_key(|c| (c.workbook.joined(), c.seq));
    let gone = &rows[2];
    assert_eq!(
        (gone.seq.get(), gone.state, gone.left),
        (3, CheckpointState::Gone, None)
    );
    assert_eq!(gone.sha256.as_ref().map(|d| d.to_string()), Some(digest(1)));
    assert_eq!(gone.bytes, Some(4097));
    assert_eq!((gone.day, gone.at), (TODAY, night.at));
    assert_eq!(
        (gone.folder.joined(), gone.copy.joined()),
        (FOLDER.to_owned(), copy_of(1, 1))
    );
    let left = &rows[4];
    assert_eq!(
        (left.seq.get(), left.state, left.left),
        (2, CheckpointState::Left, Some(LeftReason::Changed))
    );
    assert_eq!((left.sha256.clone(), left.bytes), (None, None));
    assert!(list(&mut store, None).checkpoints.is_empty());
    assert_eq!(
        put_checkpoint(&mut store, made(1, 2)),
        Err(declared("effect"))
    );
}

#[test]
fn mark_checkpoint_of_another_agent_s_checkpoint_or_workflow_is_refused() {
    let mut store = two_agents();
    assert_eq!(put_checkpoint(&mut store, planned(1, 1)), Ok(stored(1)));
    let before = all_rows(&store);
    assert_eq!(
        mark(&mut store, 2, 2, 1, 1, "gone", None),
        Err(declared("binding"))
    );
    assert_eq!(
        mark(&mut store, 1, 2, 1, 1, "gone", None),
        Err(declared("binding"))
    );
    assert_eq!(
        mark(&mut store, 9, 1, 1, 1, "gone", None),
        Err(ErrorName::NoWorkflow)
    );
    assert_eq!(all_rows(&store), before);
    assert_eq!(
        mark(&mut store, 1, 1, 1, 1, "gone", None),
        Ok(stored_reply(1))
    );
}

#[test]
fn mark_checkpoint_of_an_agent_that_is_no_longer_the_workflow_s_marks_its_rows() {
    let mut store = spreadsheet();
    assert_eq!(put_checkpoint(&mut store, planned(1, 1)), Ok(stored(1)));
    store.put_workflow(workflow(
        &wf(1),
        "spreadsheet_fill",
        "approved",
        Some(&agent(5)),
    ));
    assert_eq!(
        mark(&mut store, 1, 1, 1, 1, "left", Some("unreadable")),
        Ok(stored_reply(1))
    );
}

fn long_book(n: u32) -> Value {
    json!(halvo_schema::executable::OwnValue::new(&format!(
        "{FOLDER}/{n}-{}.xlsx",
        "w".repeat(9_000)
    )))
}

fn long_planned(r: u32, n: u32) -> Value {
    let mut row = planned(r, n);
    row["workbook"] = long_book(n);
    row
}

fn long_made(r: u32, n: u32) -> Value {
    let mut row = made(r, n);
    row["workbook"] = long_book(n);
    row
}

fn pages(store: &mut MemoryStore) -> Vec<CheckpointsReply> {
    let mut out = Vec::new();
    let mut after: Option<String> = None;
    loop {
        let body =
            json!({"op": "list_checkpoints", "workflow": null, "agent": null, "after": after});
        let reply = match coordinator(store, body) {
            Ok(CoordinatorReply::Checkpoints(reply)) => reply,
            other => panic!("a list of checkpoints: {other:?}"),
        };
        CoordinatorReply::Checkpoints(reply.clone())
            .to_json()
            .expect("a page that the writer takes");
        let next = reply.next.clone();
        out.push(reply);
        match next {
            None => return out,
            Some(run) => after = Some(run.as_str().to_owned()),
        }
    }
}

#[test]
fn a_planned_row_that_would_overflow_its_run_s_page_is_refused_with_length_and_a_later_run_is_listed(
) {
    let mut store = spreadsheet();
    let mut fitted = 0;
    let mut refused = None;
    for n in 1..=60 {
        match put_checkpoint(&mut store, long_planned(1, n)) {
            Ok(reply) => {
                assert_eq!(reply, stored(1));
                fitted = n;
            }
            Err(e) => {
                refused = Some((n, e));
                break;
            }
        }
    }
    let (n, error) = refused.expect("a planned row past one page is refused");
    assert_eq!(error, declared("length"));
    assert!(fitted > 1, "{fitted}");
    assert_eq!(checkpoints(&store).len(), fitted as usize);
    assert_eq!(
        put_checkpoint(&mut store, long_planned(1, n)),
        Err(declared("length"))
    );
    assert_eq!(checkpoints(&store).len(), fitted as usize);
    assert_eq!(
        put_checkpoint(&mut store, long_planned(1, 1)),
        Ok(stored(1)),
        "a repeat at the limit is answered as the first"
    );
    for k in 1..=fitted {
        assert_eq!(
            put_checkpoint(&mut store, long_made(1, k)),
            Ok(stored(1)),
            "the made row of {k}"
        );
    }
    start(&mut store, 2);
    assert_eq!(put_checkpoint(&mut store, planned(2, 1)), Ok(stored(1)));
    let listed: Vec<String> = pages(&mut store)
        .iter()
        .flat_map(|p| p.checkpoints.iter().map(|c| c.run.as_str().to_owned()))
        .collect();
    assert_eq!(
        listed.iter().filter(|r| **r == run(1)).count(),
        fitted as usize
    );
    assert!(listed.contains(&run(2)), "the later run is listed");
}

#[test]
fn a_run_planted_over_one_page_gives_a_page_that_the_writer_refuses_and_never_an_empty_page() {
    let mut store = spreadsheet();
    let rows: Vec<halvo_schema::TypedRecord> = (1..=40)
        .map(|n| halvo_schema::TypedRecord {
            kind: "agent_checkpoint",
            json: long_planned(1, n).to_string(),
        })
        .collect();
    store
        .typed_commit(rows)
        .expect("rows planted past the handler");
    let reply = list(&mut store, None);
    assert_eq!(reply.checkpoints.len(), 40);
    assert!(
        CoordinatorReply::Checkpoints(reply).to_json().is_err(),
        "the writer refuses the page"
    );
}

#[test]
fn a_run_whose_checkpoint_shares_the_first_instant_of_another_run_s_is_not_superseded_by_it() {
    let mut store = spreadsheet();
    start(&mut store, 2);
    assert_eq!(put_checkpoint(&mut store, planned(1, 1)), Ok(stored(1)));
    assert_eq!(put_checkpoint(&mut store, planned(2, 1)), Ok(stored(1)));
    committed_edit(&mut store, 2, 2, 1);
    let listed = list(&mut store, None);
    let superseded: Vec<(String, YesNo)> = listed
        .checkpoints
        .iter()
        .map(|c| (c.run.as_str().to_owned(), c.superseded))
        .collect();
    assert_eq!(superseded, [(run(1), YesNo::No), (run(2), YesNo::No)]);
}

#[test]
fn of_a_rejected_and_a_refused_edit_of_one_path_the_list_gives_rejected() {
    let mut store = spreadsheet();
    assert_eq!(put_checkpoint(&mut store, planned(1, 1)), Ok(stored(1)));
    assert_eq!(put(&mut store, edit(1, 1, 1, "cleared", 1)), Ok(stored(1)));
    assert_eq!(put(&mut store, edit(1, 2, 1, "refused", 1)), Ok(stored(1)));
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    assert_eq!(last_of(&store, &effect(1)).state, EffectState::Rejected);
    assert_eq!(last_of(&store, &effect(2)).state, EffectState::Refused);
    assert_eq!(
        list(&mut store, None).checkpoints[0].effect,
        Some(EffectState::Rejected)
    );
}

const LIMIT: usize = halvo_schema::executable::body::CHECKPOINTS_REPLY_MAX;

fn path_of(k: u32, len: usize) -> String {
    format!("{FOLDER}/{k}-{}.xlsx", "w".repeat(len))
}

fn row_of(r: u32, k: u32, path: &str, made: bool) -> Value {
    let mut row = planned(r, k);
    row["workbook"] = json!(halvo_schema::executable::OwnValue::new(path));
    row["at"] = json!(ROW_AT + i64::from(r));
    if made {
        row["seq"] = json!(2);
        row["state"] = json!("made");
        row["sha256"] = json!(digest(k));
        row["bytes"] = json!(u32::MAX);
    }
    row
}

fn edit_of(r: u32, e: u32, seq: u32, state: &str, path: &str) -> Value {
    let mut row = row(r, e, seq, state, "local_edit", 1, None, None);
    for field in row["kept"].as_array_mut().expect("the kept fields") {
        if field["name"] == "file" {
            field["value"] = json!(halvo_schema::executable::OwnValue::new(path));
        }
    }
    row
}

fn page_len(entries: &[Value]) -> usize {
    let reply = json!({"checkpoints": entries, "next": run(1)});
    match CoordinatorReply::parse(reply.to_string().as_bytes()) {
        Ok(CoordinatorReply::Checkpoints(page)) => {
            page.to_json().expect("a page under the bound").len()
        }
        other => panic!("a page of checkpoints: {other:?}"),
    }
}

fn widest_entry(k: u32, path: &str) -> Value {
    json!({
        "workflow": wf(1), "agent": agent(1), "run": run(1), "folder": [FOLDER],
        "workbook": halvo_schema::executable::OwnValue::new(path), "copy": [copy_of(1, k)],
        "state": "made", "sha256": "f".repeat(64), "bytes": u32::MAX, "at": i64::MIN,
        "ended": "yes", "effect": "committed", "superseded": "yes",
    })
}

fn reckoning(paths: &[(u32, String)]) -> usize {
    let empty = page_len(&[]);
    let entries: usize = paths
        .iter()
        .map(|(k, p)| page_len(&[widest_entry(*k, p)]) - empty)
        .sum();
    empty + entries + paths.len().saturating_sub(1)
}

fn fill(store: &mut MemoryStore, r: u32, paths: &[(u32, String)]) -> Vec<(u32, String)> {
    let mut kept = Vec::new();
    for (k, path) in paths {
        match put_checkpoint(store, row_of(r, *k, path, false)) {
            Ok(reply) => {
                assert_eq!(reply, stored(1));
                kept.push((*k, path.clone()));
            }
            Err(e) => {
                assert_eq!(e, declared("length"), "{k}");
                break;
            }
        }
    }
    kept
}

fn make_and_commit(store: &mut MemoryStore, r: u32, kept: &[(u32, String)]) {
    for (k, path) in kept {
        assert_eq!(
            put_checkpoint(store, row_of(r, *k, path, true)),
            Ok(stored(1))
        );
        let e = r * 1000 + k;
        for (seq, state) in [(1, "cleared"), (2, "releasing"), (3, "committed")] {
            assert_eq!(
                put(store, edit_of(r, e, seq, state, path)),
                Ok(stored(1)),
                "{k} {state}"
            );
        }
    }
}

#[test]
fn a_run_filled_to_its_bound_with_long_paths_and_made_at_the_widest_lists_in_a_page_that_the_writer_takes(
) {
    let mut store = spreadsheet();
    let mut paths: Vec<(u32, String)> = (1..=5).map(|k| (k, path_of(k, 48_000))).collect();
    let base = reckoning(&paths);
    let empty = page_len(&[]);
    let width = |len: usize| page_len(&[widest_entry(6, &path_of(6, len))]) - empty;
    let from = (LIMIT + 10)
        .saturating_sub(base + width(1))
        .saturating_sub(64);
    let mut tuned = None;
    for len in from..from + 256 {
        let over = (base + width(len) + 1).saturating_sub(LIMIT);
        if (7..=12).contains(&over) {
            let mut trial = paths.clone();
            trial.push((6, path_of(6, len)));
            assert_eq!(reckoning(&trial), base + width(len) + 1);
            tuned = Some(trial);
            break;
        }
    }
    paths = tuned.expect("a path length that brings the sixth row 7 to 12 bytes over the bound");
    paths.push((7, path_of(7, 48_000)));
    let kept = fill(&mut store, 1, &paths);
    make_and_commit(&mut store, 1, &kept);
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    start(&mut store, 2);
    let later = fill(&mut store, 2, &paths);
    make_and_commit(&mut store, 2, &later);
    let first = list(&mut store, None);
    assert!(first.checkpoints.iter().all(|c| c.run.as_str() == run(1)));
    assert!(first.checkpoints.iter().all(|c| c.ended == YesNo::Yes
        && c.superseded == YesNo::Yes
        && c.effect == Some(EffectState::Committed)
        && c.bytes == Some(u32::MAX)));
    for page in pages(&mut store) {
        assert!(!page.checkpoints.is_empty());
    }
}

#[test]
fn a_run_filled_to_its_bound_with_short_paths_and_made_at_the_widest_bytes_lists_in_a_page_that_the_writer_takes(
) {
    let mut store = spreadsheet();
    let paths: Vec<(u32, String)> = (1..=400).map(|k| (k, path_of(k, 1_000))).collect();
    let kept = fill(&mut store, 1, &paths);
    assert!(kept.len() < paths.len(), "the run meets its bound");
    for (k, path) in &kept {
        assert_eq!(
            put_checkpoint(&mut store, row_of(1, *k, path, true)),
            Ok(stored(1))
        );
    }
    for page in pages(&mut store) {
        assert!(!page.checkpoints.is_empty());
    }
}
