//! The six tests of U3.1's second pull request on the derived store, opened with its key: the three privacy lines
//! learn_scrub, learn_expired and learn_listed (the coordinator's word of 2026-11-21, way C) and the three refusal
//! tests of contracts.md L5.4.

use std::collections::BTreeSet;
use std::path::PathBuf;

use halvo_derived::{Cause, Host, IdSource, Refusal, Store};
use halvo_schema::executable::RunnerReply;
use halvo_schema::kinds::registry;
use halvo_schema::{
    KindEntry, Position, Proposal, RecurrenceStep, RowRead, StoreId, SummaryId, SummaryRead,
    TypedRecord, WorkflowId, WriterStep,
};
use serde_json::json;

use crate::case::Verb;
use crate::delete::{derived_workflow_line, seed_cases, seed_derived, MARKERS};
use crate::derived::{episode, process_key, Day, DerivedHost};
use crate::host::PlayHost;
use crate::privacy::{expired, listed, scrub};
use crate::stub::{compiled_step, id, play_all, Arm, Play};

fn scratch(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary");
    let root = exe
        .ancestors()
        .nth(3)
        .expect("the target directory")
        .join("tmp")
        .join("r5-privacy-derived");
    halvo_test_dir::make(&root, name).expect("a test directory")
}

/// The rows of the family `learn` whose bytes hold the workflow's id as a JSON string or its marker, each as
/// (kind, state), the state `-` for a kind without one.
fn rows_of(host: &mut DerivedHost, workflow: &str, marker: &str) -> Vec<(&'static str, String)> {
    let quoted = format!("\"{workflow}\"");
    let mut found = Vec::new();
    for entry in halvo_schema::learning::KINDS {
        for json in host.learn_rows(entry.name).expect("a keyed read") {
            if json.contains(&quoted) || json.contains(marker) {
                let state = serde_json::from_str::<serde_json::Value>(&json)
                    .ok()
                    .and_then(|v| v["state"].as_str().map(str::to_owned))
                    .unwrap_or_else(|| "-".to_owned());
                found.push((entry.name, state));
            }
        }
    }
    found.sort();
    found
}

/// S1, learn_scrub: after the delete of the first workflow no learned row of it stands, read through the store's own
/// calls with the key, while the second workflow's rows stand, and the removal went through halvo-derived's own
/// delete call alone: across the one call the store commits one transaction that raises the revision of `learn`,
/// with no request to the family's handlers, no batch and no open (`privacy::scrub`).
#[test]
fn learn_scrub_the_store_s_own_delete_alone_removes_the_first_workflow_s_learned_rows() {
    let cases = seed_cases();
    let (mut play, first) =
        seed_derived(&cases, &scratch("scrub")).expect("the seeded derived store");
    let second = play.workflow().to_owned();
    let (first_id, second_id) = (
        WorkflowId::parse(&first).expect("the first workflow's id"),
        WorkflowId::parse(&second).expect("the second workflow's id"),
    );
    assert!(!rows_of(&mut play.host, &first, MARKERS[0]).is_empty());
    let ok = scrub(&mut play.host, &first_id, &second_id, &mut |host, id| {
        host.delete_workflow(id)
    })
    .expect("the check runs");
    assert!(ok, "learn_scrub misses");
    assert_eq!(rows_of(&mut play.host, &first, MARKERS[0]), []);
}

/// S2, learn_expired: on a clock one day past the 93 days a learn_run and a learn_routine row are gone at the next
/// open, counted by the rows that a read on the last day of the 93 hands out after that open, and on the 93rd day
/// itself they stand (`privacy::expired`).
#[test]
fn learn_expired_a_learn_run_and_a_learn_routine_row_go_one_day_past_93_days_and_stand_on_the_93rd()
{
    let cases = seed_cases();
    let (mut play, _) =
        seed_derived(&cases, &scratch("expired")).expect("the seeded derived store");
    assert!(
        expired(&mut play.host).expect("the check runs"),
        "learn_expired misses"
    );
}

/// S3, learn_listed: the store's counts per class, which its report to its host carries after each committed
/// transaction, change across the transactions of the family `learn` alone by the gate's count of the learned rows by
/// class (`privacy::listed`).
#[test]
fn learn_listed_the_store_s_counts_by_class_count_every_learned_row() {
    let cases = seed_cases();
    let (mut play, _) = seed_derived(&cases, &scratch("listed")).expect("the seeded derived store");
    assert!(
        listed(&mut play.host).expect("the check runs"),
        "learn_listed misses"
    );
}

/// L5.4 delete_leaves_others: the decision `delete` of workflow A removes no row of the family that names workflow
/// B, and B's agent is still served its learned step (the line of `learn-gate delete --what workflow --store
/// derived`).
#[test]
fn refusal_delete_leaves_others() {
    let (line, ok) =
        derived_workflow_line(&scratch("leaves-others")).expect("the line of the derived store");
    assert!(ok, "{line}");
    let other_rows: u32 = line
        .split(' ')
        .find_map(|word| word.strip_prefix("other_rows="))
        .and_then(|n| n.parse().ok())
        .expect("a count of the other rows");
    assert!(other_rows > 0, "{line}");
    assert_eq!(
        line,
        format!(
            "delete what=workflow store=derived rows_left=0 other_rows={other_rows} other_served=yes result=ok"
        )
    );
}

/// L5.4 delete_takes_revoked: a compile of workflow A's agent under a newer playbook number withdraws its older
/// edits (L4.2), so A holds a `reverted` and a `withdrawn` learn_edit row; the decision `delete` of A takes them with
/// the others, and no learned row holds A's id or marker after it.
#[test]
fn refusal_delete_takes_revoked() {
    let cases = seed_cases();
    let (mut play, first) =
        seed_derived(&cases, &scratch("takes-revoked")).expect("the seeded derived store");
    let host = &mut play.host;
    let (agent, run) = (id('4', 1), id('5', 900));
    let body = |value: serde_json::Value| value.to_string().into_bytes();
    host.coordinator(&body(json!({"op": "start_run", "workflow": first, "agent": agent, "run": run, "mode": "compile"})))
        .expect("a compile run of the first agent");
    let RunnerReply::Load(load) = host
        .runner(&body(json!({"op": "load", "agent": agent, "run": run})))
        .expect("a load")
    else {
        panic!("a load answered with another reply");
    };
    let number = load.playbook.expect("a playbook").number.get() + 1;
    let control = &cases[0].occurrences[0].page[0];
    let step = compiled_step(1, Verb::Click, control).expect("a compiled step");
    host.runner(&body(json!({"op": "put_playbook", "agent": agent, "run": run, "number": number, "steps": [step]})))
        .expect("the put_playbook of the compile");
    let states: BTreeSet<String> = rows_of(host, &first, MARKERS[0])
        .into_iter()
        .filter(|(kind, _)| *kind == "learn_edit")
        .map(|(_, state)| state)
        .collect();
    assert!(
        states.contains("reverted") && states.contains("withdrawn"),
        "{states:?}"
    );
    let id = WorkflowId::parse(&first).expect("the first workflow's id");
    assert!(host.delete_workflow(&id).expect("the decision delete"));
    assert_eq!(rows_of(host, &first, MARKERS[0]), []);
}

struct NoProposal;

impl RecurrenceStep for NoProposal {
    fn propose(&self, _: &dyn SummaryRead) -> Vec<Proposal> {
        Vec::new()
    }
}

struct Ids(u64);

impl IdSource for Ids {
    fn fill(&mut self, bytes: &mut [u8]) {
        for chunk in bytes.chunks_mut(8) {
            self.0 += 1;
            let word = self.0.to_be_bytes();
            chunk.copy_from_slice(&word[..chunk.len()]);
        }
    }
}

/// A step of the family `learn` that returns one record after every batch.
struct OneRecord(TypedRecord);

impl WriterStep for OneRecord {
    fn after_commit(&self, _: &dyn SummaryRead, _: &dyn RowRead) -> Vec<TypedRecord> {
        vec![self.0.clone()]
    }
}

/// L5.4 step_writes_no_edit: a test step of the family `learn` that returns one learn_edit record, a copy of a
/// stored row of the seeded workflow, is refused by the store with the cause own origin, and the store's
/// learn_edit rows are the same after the batch.
#[test]
fn refusal_step_writes_no_edit() {
    let root = scratch("step-edit");
    let key = process_key();
    let cases = seed_cases();
    let day = cases[0].occurrences[cases[0].occurrences.len() - 1].day;
    let edits = {
        let host = DerivedHost::new(root.clone(), key.clone(), cases[0].occurrences[0].day);
        let mut play = Play::with_host(&cases[0], Arm::Admit, 1, host);
        play_all(&mut play, false).expect("the seeded case plays on the derived store");
        play.host.learn_rows("learn_edit").expect("a keyed read")
    };
    assert!(!edits.is_empty());
    let record = TypedRecord {
        kind: "learn_edit",
        json: edits[0].clone(),
    };
    let clock = Day::default();
    clock.set(day);
    let entries: Vec<&'static KindEntry> = registry().collect();
    let host =
        Host::new(Box::new(clock), Box::new(NoProposal)).step("learn", Box::new(OneRecord(record)));
    let mut store = Store::open(&root, &key, &entries, host, &mut Ids(1 << 40))
        .expect("the store opens again with its key");
    let summary = SummaryId::parse(&id('7', 50)).expect("a summary id");
    let source = StoreId::parse(&id('8', 1)).expect("the gate's store id");
    let outcome = store
        .commit_batch(
            &source,
            &[episode(summary, 9, day, 1_000).expect("an episode")],
            Position { seg: 1_000, idx: 0 },
            &mut Ids(1 << 41),
        )
        .expect("the batch commits");
    let steps: Vec<Option<String>> = outcome
        .steps
        .iter()
        .map(|step| step.as_ref().err().map(ToString::to_string))
        .collect();
    assert!(
        matches!(
            outcome.steps.as_slice(),
            [Err(Refusal {
                index: 0,
                cause: Cause::OwnOrigin
            })]
        ),
        "{steps:?}"
    );
    drop(store);
    let mut again = DerivedHost::new(root, key, day);
    assert_eq!(again.learn_rows("learn_edit").expect("a keyed read"), edits);
}
