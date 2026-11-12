use halvo_schema::executable::body::{
    CoordinatorCall, CoordinatorReply, RunnerCall, RunnerReply, CHECKPOINTS_REPLY_MAX,
};
use halvo_schema::executable::names::{CheckpointState, LeftReason, ListedState, MarkState};
use halvo_schema::executable::record::AgentCheckpoint;
use halvo_schema::executable::{Refusal, RefusalCause};
use halvo_schema::request::{
    self, CoordinatorRole, CoordinatorWrite, Decoded, RunnerRequest, RunnerRole, REPLY_MAX,
};
use halvo_schema::RequestId;

const W: &str = "30303030303030303030303030303030";
const A: &str = "40404040404040404040404040404040";
const R: &str = "50505050505050505050505050505050";
const R2: &str = "51515151515151515151515151515151";
const SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const PLANNED: &str = r#""sha256":null,"bytes":null,"state":"planned","left":null"#;

fn made() -> String {
    format!(r#""sha256":"{SHA}","bytes":18432,"state":"made","left":null"#)
}

fn paths(run: &str) -> String {
    format!(
        r#""folder":["/data/books"],"workbook":["/data/books/bills.xlsx"],"copy":["bills.halvo-checkpoint-{run}.xlsx"]"#
    )
}

fn row(run: &str, seq: u32, rest: &str) -> String {
    format!(
        r#"{{"v":1,"workflow":"{W}","agent":"{A}","run":"{run}","seq":{seq},"day":20768,"at":1794339000000000002,{},{rest}}}"#,
        paths(run)
    )
}

fn listed(run: &str, rest: &str) -> String {
    format!(
        r#"{{"workflow":"{W}","agent":"{A}","run":"{run}",{},{rest}}}"#,
        paths(run)
    )
}

fn listed_made(run: &str) -> String {
    listed(
        run,
        &format!(
            r#""state":"made","sha256":"{SHA}","bytes":18432,"at":1794339000000000002,"ended":"yes","effect":"committed","superseded":"no""#
        ),
    )
}

fn listed_planned(run: &str) -> String {
    listed(
        run,
        r#""state":"planned","sha256":null,"bytes":null,"at":1794339000000000001,"ended":"no","effect":null,"superseded":"no""#,
    )
}

fn checkpoints(rows: &[String], next: &str) -> String {
    format!(r#"{{"checkpoints":[{}],"next":{next}}}"#, rows.join(","))
}

fn checkpoint_body(record: &str) -> String {
    format!(r#"{{"op":"checkpoint","agent":"{A}","run":"{R}","record":{record}}}"#)
}

fn list_body(after: &str) -> String {
    format!(r#"{{"op":"list_checkpoints","workflow":null,"agent":null,"after":{after}}}"#)
}

fn mark_body(rest: &str) -> String {
    format!(
        r#"{{"op":"mark_checkpoint","workflow":"{W}","agent":"{A}","run":"{R}","workbook":["/data/books/bills.xlsx"],{rest}}}"#
    )
}

fn expect(result: Result<impl std::fmt::Debug, Refusal>, type_name: &str, rule: &str, key: &str) {
    let refusal = result.expect_err("refused");
    assert_eq!(refusal.cause(), RefusalCause::Malformed, "{refusal}");
    assert_eq!(
        (
            refusal.type_name(),
            refusal.rule(),
            refusal.key().unwrap_or("-")
        ),
        (type_name, rule, key)
    );
}

fn swap(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "{from} in {text}");
    text.replacen(from, to, 1)
}

#[test]
fn the_closed_lists_of_a_checkpoint_are_the_names_of_the_contract() {
    assert_eq!(
        CheckpointState::ALL
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>(),
        ["planned", "made", "gone", "left"]
    );
    assert_eq!(
        ListedState::ALL
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>(),
        ["planned", "made"]
    );
    assert_eq!(
        MarkState::ALL
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>(),
        ["gone", "left"]
    );
    assert_eq!(
        LeftReason::ALL
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>(),
        ["changed", "link", "not_file", "name", "unreadable"]
    );
}

#[test]
fn a_checkpoint_row_of_each_state_reads_and_writes_back_to_the_same_bytes() {
    for text in [
        row(R, 1, PLANNED),
        row(R, 2, &made()),
        row(
            R,
            3,
            &format!(r#""sha256":"{SHA}","bytes":18432,"state":"gone","left":null"#),
        ),
        row(
            R,
            2,
            r#""sha256":null,"bytes":null,"state":"gone","left":null"#,
        ),
        row(
            R2,
            2,
            r#""sha256":null,"bytes":null,"state":"left","left":"changed""#,
        ),
        row(
            R2,
            3,
            &format!(r#""sha256":"{SHA}","bytes":0,"state":"left","left":"unreadable""#),
        ),
    ] {
        let read =
            AgentCheckpoint::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(read.to_json().unwrap(), text);
    }
}

#[test]
fn a_checkpoint_row_is_refused_for_each_rule_of_its_shape() {
    let made_row = row(R, 2, &made());
    let cases: Vec<(String, &str, &str)> = vec![
        (row(R, 2, PLANNED), "seq_of_state", "seq"),
        (row(R, 1, &made()), "seq_of_state", "seq"),
        (
            row(
                R,
                1,
                r#""sha256":null,"bytes":null,"state":"gone","left":null"#,
            ),
            "seq_of_state",
            "seq",
        ),
        (
            row(
                R,
                3,
                r#""sha256":null,"bytes":null,"state":"made","left":null"#,
            ),
            "seq_of_state",
            "seq",
        ),
        (
            row(
                R,
                2,
                r#""sha256":null,"bytes":null,"state":"made","left":null"#,
            ),
            "made_without_digest",
            "sha256",
        ),
        (
            row(
                R,
                1,
                &format!(r#""sha256":"{SHA}","bytes":1,"state":"planned","left":null"#),
            ),
            "planned_with_digest",
            "sha256",
        ),
        (
            swap(&made_row, r#""bytes":18432"#, r#""bytes":null"#),
            "digest_and_bytes",
            "bytes",
        ),
        (
            row(
                R,
                1,
                r#""sha256":null,"bytes":7,"state":"planned","left":null"#,
            ),
            "digest_and_bytes",
            "bytes",
        ),
        (
            swap(&made_row, r#""left":null"#, r#""left":"changed""#),
            "left_of_state",
            "left",
        ),
        (
            row(
                R,
                3,
                r#""sha256":null,"bytes":null,"state":"left","left":null"#,
            ),
            "left_of_state",
            "left",
        ),
        (
            swap(&made_row, r#""folder":["/data/books"]"#, r#""folder":[]"#),
            "empty_path",
            "folder",
        ),
        (
            swap(
                &made_row,
                r#""workbook":["/data/books/bills.xlsx"]"#,
                r#""workbook":[]"#,
            ),
            "empty_path",
            "workbook",
        ),
        (
            swap(
                &made_row,
                &format!(r#""copy":["bills.halvo-checkpoint-{R}.xlsx"]"#),
                r#""copy":[]"#,
            ),
            "empty_path",
            "copy",
        ),
    ];
    for (text, rule, key) in cases {
        expect(
            AgentCheckpoint::parse(text.as_bytes()),
            "AgentCheckpoint",
            rule,
            key,
        );
    }
    expect(
        AgentCheckpoint::parse(
            swap(&made_row, r#""state":"made""#, r#""state":"kept""#).as_bytes(),
        ),
        "CheckpointState",
        "unknown_name",
        "state",
    );
    expect(
        AgentCheckpoint::parse(swap(&made_row, r#""left":null"#, r#""left":"moved""#).as_bytes()),
        "LeftReason",
        "unknown_name",
        "left",
    );
    expect(
        AgentCheckpoint::parse(swap(&made_row, r#""bytes":18432"#, r#""bytes":-1"#).as_bytes()),
        "count",
        "not_a_count",
        "bytes",
    );
    expect(
        AgentCheckpoint::parse(swap(&made_row, SHA, "bbbb").as_bytes()),
        "Digest",
        "hex64",
        "sha256",
    );
    expect(
        AgentCheckpoint::parse(swap(&made_row, r#","left":null"#, "").as_bytes()),
        "AgentCheckpoint",
        "missing_key",
        "left",
    );
}

#[test]
fn a_checkpoint_body_reads_writes_back_and_travels_in_a_runner_request() {
    for record in [row(R, 1, PLANNED), row(R, 2, &made())] {
        let text = checkpoint_body(&record);
        let call = RunnerCall::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(call.op(), "checkpoint");
        assert_eq!(call.to_json().unwrap(), text);
        assert_eq!((call.agent().as_str(), call.run().as_str()), (A, R));
        let RunnerCall::Checkpoint(body) = &call else {
            panic!("{text}")
        };
        assert_eq!(body.record.to_json().unwrap(), record);
        let request = RunnerRequest::new(
            &RunnerRole::claim(),
            RequestId::from_random([7; 16]),
            call.clone(),
        );
        let Ok(Decoded::Raw(raw)) = request::decode(&request.to_bytes().unwrap()) else {
            panic!("{text}")
        };
        assert_eq!(
            RunnerCall::parse(raw.body().get().as_bytes()).unwrap(),
            call
        );
        let stored = RunnerReply::parse(br#"{"stored":1}"#).unwrap();
        assert!(stored.answers(&call));
        let load = RunnerReply::parse(br#"{"steps":[]}"#).unwrap();
        assert!(!load.answers(&call));
    }
}

#[test]
fn a_checkpoint_body_holds_a_planned_or_made_row_of_its_own_run() {
    for rest in [
        r#""sha256":null,"bytes":null,"state":"gone","left":null"#,
        r#""sha256":null,"bytes":null,"state":"left","left":"link""#,
    ] {
        expect(
            RunnerCall::parse(checkpoint_body(&row(R, 2, rest)).as_bytes()),
            "PutCheckpoint",
            "state_of_runner",
            "record",
        );
    }
    expect(
        RunnerCall::parse(checkpoint_body(&row(R2, 1, PLANNED)).as_bytes()),
        "PutCheckpoint",
        "record_of_another_run",
        "record",
    );
    let other_agent = swap(
        &checkpoint_body(&row(R, 1, PLANNED)),
        &format!(r#""op":"checkpoint","agent":"{A}""#),
        r#""op":"checkpoint","agent":"41414141414141414141414141414141""#,
    );
    expect(
        RunnerCall::parse(other_agent.as_bytes()),
        "PutCheckpoint",
        "record_of_another_run",
        "record",
    );
    expect(
        RunnerCall::parse(checkpoint_body(&row(R, 2, PLANNED)).as_bytes()),
        "AgentCheckpoint",
        "seq_of_state",
        "seq",
    );
}

#[test]
fn the_two_coordinator_bodies_of_the_list_read_write_back_and_travel_in_a_coordinator_write() {
    let bodies = [
        (list_body("null"), "list_checkpoints"),
        (list_body(&format!("\"{R}\"")), "list_checkpoints"),
        (
            mark_body(r#""state":"gone","left":null"#),
            "mark_checkpoint",
        ),
        (
            mark_body(r#""state":"left","left":"not_file""#),
            "mark_checkpoint",
        ),
    ];
    for (text, op) in bodies {
        let call =
            CoordinatorCall::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
        assert_eq!(call.op(), op);
        assert_eq!(call.to_json().unwrap(), text);
        let write = CoordinatorWrite::new(
            &CoordinatorRole::claim(),
            RequestId::from_random([9; 16]),
            call.clone(),
        );
        let Ok(Decoded::Raw(raw)) = request::decode(&write.to_bytes().unwrap()) else {
            panic!("{op}")
        };
        assert_eq!(
            CoordinatorCall::parse(raw.body().get().as_bytes()).unwrap(),
            call
        );
    }
}

#[test]
fn a_list_body_names_no_workflow_and_no_agent() {
    expect(
        CoordinatorCall::parse(
            swap(
                &list_body("null"),
                r#""workflow":null"#,
                &format!(r#""workflow":"{W}""#),
            )
            .as_bytes(),
        ),
        "Null",
        "not_null",
        "workflow",
    );
    expect(
        CoordinatorCall::parse(
            swap(
                &list_body("null"),
                r#""agent":null"#,
                &format!(r#""agent":"{A}""#),
            )
            .as_bytes(),
        ),
        "Null",
        "not_null",
        "agent",
    );
    expect(
        CoordinatorCall::parse(swap(&list_body("null"), r#","agent":null"#, "").as_bytes()),
        "ListCheckpoints",
        "missing_key",
        "agent",
    );
    expect(
        CoordinatorCall::parse(list_body(r#""5050""#).as_bytes()),
        "RunId",
        "hex32",
        "after",
    );
}

#[test]
fn a_mark_body_names_its_reason_exactly_when_it_leaves_the_copy() {
    for (rest, rule, key) in [
        (r#""state":"left","left":null"#, "left_of_state", "left"),
        (
            r#""state":"gone","left":"changed""#,
            "left_of_state",
            "left",
        ),
    ] {
        expect(
            CoordinatorCall::parse(mark_body(rest).as_bytes()),
            "MarkCheckpoint",
            rule,
            key,
        );
    }
    for (state, name) in [("planned", "MarkState"), ("made", "MarkState")] {
        expect(
            CoordinatorCall::parse(
                mark_body(&format!(r#""state":"{state}","left":null"#)).as_bytes(),
            ),
            name,
            "unknown_name",
            "state",
        );
    }
    let empty = swap(
        &mark_body(r#""state":"gone","left":null"#),
        r#""workbook":["/data/books/bills.xlsx"]"#,
        r#""workbook":[]"#,
    );
    expect(
        CoordinatorCall::parse(empty.as_bytes()),
        "MarkCheckpoint",
        "empty_path",
        "workbook",
    );
}

#[test]
fn the_list_reply_and_the_stored_reply_answer_their_own_calls() {
    let list = CoordinatorCall::parse(list_body("null").as_bytes()).unwrap();
    let mark =
        CoordinatorCall::parse(mark_body(r#""state":"gone","left":null"#).as_bytes()).unwrap();
    let end = CoordinatorCall::parse(
        format!(r#"{{"op":"end_run","workflow":"{W}","agent":"{A}","run":"{R}"}}"#).as_bytes(),
    )
    .unwrap();
    for text in [
        checkpoints(&[], "null"),
        checkpoints(&[listed_planned(R), listed_made(R2)], &format!("\"{R2}\"")),
    ] {
        let reply =
            CoordinatorReply::parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e}: {text}"));
        let CoordinatorReply::Checkpoints(_) = &reply else {
            panic!("{text}")
        };
        assert_eq!(reply.to_json().unwrap(), text);
        assert!(reply.answers(&list));
        assert!(!reply.answers(&mark) && !reply.answers(&end));
    }
    for (count, answers) in [(0, true), (1, true), (2, false)] {
        let text = format!(r#"{{"stored":{count}}}"#);
        let reply = CoordinatorReply::parse(text.as_bytes()).unwrap();
        let CoordinatorReply::Stored(_) = &reply else {
            panic!("{text}")
        };
        assert_eq!(reply.to_json().unwrap(), text);
        assert_eq!(reply.answers(&mark), answers, "{text}");
        assert!(!reply.answers(&list) && !reply.answers(&end));
    }
    let accepted = CoordinatorReply::parse(br#"{"ok":"yes"}"#).unwrap();
    assert!(!accepted.answers(&list) && !accepted.answers(&mark));
}

#[test]
fn a_list_reply_is_refused_for_each_rule_of_its_rows() {
    let cases: Vec<(String, &str, &str, &str)> = vec![
        (
            checkpoints(&[listed_made(R2), listed_planned(R)], "null"),
            "CheckpointsReply",
            "run_order",
            "checkpoints",
        ),
        (
            checkpoints(&[listed_made(R), listed_planned(R)], "null"),
            "CheckpointsReply",
            "repeated_name",
            "checkpoints",
        ),
        (
            checkpoints(
                &[swap(
                    &listed_made(R),
                    r#""sha256":"bbbb"#,
                    r#""sha256":null,"x":"bbbb"#,
                )],
                "null",
            ),
            "ListedCheckpoint",
            "unknown_key",
            "checkpoints",
        ),
        (
            checkpoints(
                &[swap(
                    &listed_made(R),
                    &format!(r#""sha256":"{SHA}""#),
                    r#""sha256":null"#,
                )],
                "null",
            ),
            "ListedCheckpoint",
            "digest_and_bytes",
            "bytes",
        ),
        (
            checkpoints(
                &[swap(
                    &listed_made(R),
                    &format!(r#""sha256":"{SHA}","bytes":18432"#),
                    r#""sha256":null,"bytes":null"#,
                )],
                "null",
            ),
            "ListedCheckpoint",
            "made_without_digest",
            "sha256",
        ),
        (
            checkpoints(
                &[swap(
                    &listed_planned(R),
                    r#""sha256":null,"bytes":null"#,
                    &format!(r#""sha256":"{SHA}","bytes":3"#),
                )],
                "null",
            ),
            "ListedCheckpoint",
            "planned_with_digest",
            "sha256",
        ),
        (
            checkpoints(
                &[swap(
                    &listed_made(R),
                    r#""effect":"committed""#,
                    r#""effect":"releasing""#,
                )],
                "null",
            ),
            "ListedCheckpoint",
            "releasing_after_end",
            "effect",
        ),
        (
            checkpoints(
                &[swap(
                    &listed_made(R),
                    r#""folder":["/data/books"]"#,
                    r#""folder":[]"#,
                )],
                "null",
            ),
            "ListedCheckpoint",
            "empty_path",
            "folder",
        ),
        (
            checkpoints(
                &[swap(
                    &listed_made(R),
                    r#""state":"made""#,
                    r#""state":"gone""#,
                )],
                "null",
            ),
            "ListedState",
            "unknown_name",
            "state",
        ),
        (
            checkpoints(
                &[swap(&listed_made(R), r#""ended":"yes""#, r#""ended":true"#)],
                "null",
            ),
            "YesNo",
            "not_a_string",
            "ended",
        ),
    ];
    for (text, type_name, rule, key) in cases {
        expect(
            CoordinatorReply::parse(text.as_bytes()),
            type_name,
            rule,
            key,
        );
    }
    let releasing_open = swap(
        &swap(
            &listed_made(R),
            r#""effect":"committed""#,
            r#""effect":"releasing""#,
        ),
        r#""ended":"yes""#,
        r#""ended":"no""#,
    );
    let text = checkpoints(&[releasing_open], "null");
    assert_eq!(
        CoordinatorReply::parse(text.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        text
    );
    let second_workbook = swap(
        &listed_made(R),
        r#""workbook":["/data/books/bills.xlsx"]"#,
        r#""workbook":["/data/books/rent.xlsx"]"#,
    );
    let text = checkpoints(&[listed_planned(R), second_workbook], "null");
    assert!(CoordinatorReply::parse(text.as_bytes()).is_ok(), "{text}");
}

#[test]
fn a_list_reply_holds_at_most_the_bound_of_a_reply_beside_its_envelope() {
    assert_eq!(CHECKPOINTS_REPLY_MAX, REPLY_MAX - 53);
    let with_copy = |len: usize| {
        let copy: Vec<String> = (0..len.div_ceil(1024))
            .map(|i| format!("\"{}\"", "c".repeat((len - i * 1024).min(1024))))
            .collect();
        checkpoints(
            &[swap(
                &listed_made(R),
                &format!(r#""copy":["bills.halvo-checkpoint-{R}.xlsx"]"#),
                &format!(r#""copy":[{}]"#, copy.join(",")),
            )],
            "null",
        )
    };
    let base = with_copy(1).len() - 1;
    let mut len = CHECKPOINTS_REPLY_MAX - base;
    while with_copy(len).len() != CHECKPOINTS_REPLY_MAX
        || with_copy(len + 1).len() != CHECKPOINTS_REPLY_MAX + 1
    {
        len -= 1;
    }
    let exact = with_copy(len);
    assert_eq!(exact.len(), CHECKPOINTS_REPLY_MAX);
    assert_eq!(
        CoordinatorReply::parse(exact.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        exact
    );
    let over = with_copy(len + 1);
    assert_eq!(over.len(), CHECKPOINTS_REPLY_MAX + 1);
    let refusal = CoordinatorReply::parse(over.as_bytes()).unwrap_err();
    assert_eq!(
        (refusal.cause(), refusal.type_name(), refusal.rule()),
        (RefusalCause::Length, "CheckpointsReply", "json_length")
    );
}
