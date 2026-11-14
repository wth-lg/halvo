use halvo_schema::executable::body::{
    Accepted, CheckpointsReply, CoordinatorCall, CoordinatorReply, RevReply, RunnerCall,
    RunnerReply, StateReply, Stored, CHECKPOINTS_REPLY_MAX,
};
use halvo_schema::executable::names::{CheckpointState, LeftReason, ListedState, MarkState};
use halvo_schema::executable::record::AgentCheckpoint;
use halvo_schema::executable::text::OwnValue;
use halvo_schema::executable::{Refusal, RefusalCause};
use halvo_schema::request::{
    self, write_reply, CoordinatorRole, CoordinatorWrite, Decoded, ReplyResult, RunnerRequest,
    RunnerRole, REPLY_MAX,
};
use halvo_schema::RequestId;
use serde_json::value::RawValue;

const W: &str = "30303030303030303030303030303030";
const A: &str = "40404040404040404040404040404040";
const R: &str = "50505050505050505050505050505050";
const R2: &str = "51515151515151515151515151515151";
const R3: &str = "52525252525252525252525252525252";
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
fn a_checkpoint_has_at_most_three_rows_so_a_gone_or_left_row_has_seq_2_or_3() {
    for (state, left) in [("gone", "null"), ("left", r#""changed""#)] {
        let rest = format!(r#""sha256":null,"bytes":null,"state":"{state}","left":{left}"#);
        for seq in [2, 3] {
            let text = row(R, seq, &rest);
            assert_eq!(
                AgentCheckpoint::parse(text.as_bytes())
                    .unwrap()
                    .to_json()
                    .unwrap(),
                text
            );
        }
        for seq in [4, 5, 4_294_967_295] {
            expect(
                AgentCheckpoint::parse(row(R, seq, &rest).as_bytes()),
                "AgentCheckpoint",
                "seq_of_state",
                "seq",
            );
        }
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
    for key in ["workflow", "agent"] {
        for value in ["0", "false", "{}", "[]"] {
            let body = swap(
                &list_body("null"),
                &format!(r#""{key}":null"#),
                &format!(r#""{key}":{value}"#),
            );
            expect(
                CoordinatorCall::parse(body.as_bytes()),
                "Null",
                "not_null",
                key,
            );
        }
    }
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
                    r#""workbook":["/data/books/bills.xlsx"]"#,
                    r#""workbook":[]"#,
                )],
                "null",
            ),
            "ListedCheckpoint",
            "empty_path",
            "workbook",
        ),
        (
            checkpoints(
                &[swap(
                    &listed_made(R),
                    &format!(r#""copy":["bills.halvo-checkpoint-{R}.xlsx"]"#),
                    r#""copy":[]"#,
                )],
                "null",
            ),
            "ListedCheckpoint",
            "empty_path",
            "copy",
        ),
        (
            checkpoints(
                &[listed_made(R), listed_made(R3), listed_planned(R2)],
                "null",
            ),
            "CheckpointsReply",
            "run_order",
            "checkpoints",
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
    let text = checkpoints(&[listed_planned(R), second_workbook.clone()], "null");
    assert!(CoordinatorReply::parse(text.as_bytes()).is_ok(), "{text}");
    let text = checkpoints(
        &[listed_planned(R), second_workbook, listed_made(R3)],
        "null",
    );
    assert_eq!(
        CoordinatorReply::parse(text.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        text
    );
}

fn with_copy(len: usize, piece: impl Fn(usize) -> String) -> String {
    let copy: Vec<String> = (0..len.div_ceil(1024))
        .map(|i| format!("\"{}\"", piece((len - i * 1024).min(1024))))
        .collect();
    checkpoints(
        &[swap(
            &listed_made(R),
            &format!(r#""copy":["bills.halvo-checkpoint-{R}.xlsx"]"#),
            &format!(r#""copy":[{}]"#, copy.join(",")),
        )],
        "null",
    )
}

fn plain(len: usize) -> String {
    with_copy(len, |n| "c".repeat(n))
}

fn copy_at_the_bound() -> usize {
    let mut len = CHECKPOINTS_REPLY_MAX - (plain(1).len() - 1);
    while plain(len).len() != CHECKPOINTS_REPLY_MAX
        || plain(len + 1).len() != CHECKPOINTS_REPLY_MAX + 1
    {
        len -= 1;
    }
    len
}

fn refused_by_the_bound(input: &[u8]) {
    let refusal = CoordinatorReply::parse(input).unwrap_err();
    assert_eq!(
        (refusal.cause(), refusal.type_name(), refusal.rule()),
        (RefusalCause::Length, "CoordinatorReply", "reply_bytes"),
        "{} bytes",
        input.len()
    );
}

#[test]
fn a_list_reply_holds_at_most_the_bound_of_a_reply_with_the_envelope_that_the_core_writes() {
    let exact = plain(copy_at_the_bound());
    assert_eq!(exact.len(), CHECKPOINTS_REPLY_MAX);
    let raw = RawValue::from_string(exact.clone()).unwrap();
    let id = RequestId::from_random([0x40; 16]);
    let enveloped = write_reply(Some(&id), Ok(&ReplyResult::Raw(raw))).unwrap();
    assert_eq!(enveloped.len(), REPLY_MAX);
    assert_eq!(
        CoordinatorReply::parse(exact.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        exact
    );
    let over = plain(copy_at_the_bound() + 1);
    assert_eq!(over.len(), CHECKPOINTS_REPLY_MAX + 1);
    refused_by_the_bound(over.as_bytes());
}

#[test]
fn a_list_reply_is_counted_on_the_bytes_given_before_anything_is_read() {
    let exact = plain(copy_at_the_bound());
    let inside = exact.replacen(r#","next""#, r#", "next""#, 1);
    let through_serde = serde_json::from_str::<CoordinatorReply>(&inside).unwrap_err();
    assert!(
        through_serde
            .to_string()
            .contains("length: CoordinatorReply: -: reply_bytes"),
        "{through_serde}"
    );
    assert!(serde_json::from_str::<CoordinatorReply>(&exact).is_ok());
    for padded in [format!(" {exact}"), format!("{exact}\n"), inside] {
        assert_eq!(padded.len(), CHECKPOINTS_REPLY_MAX + 1);
        refused_by_the_bound(padded.as_bytes());
    }
    let wide = format!("{exact}{}", " ".repeat(391));
    refused_by_the_bound(wide.as_bytes());
    let escape_of_c = format!("{}u0063", '\\');
    assert_eq!(escape_of_c.len(), 6);
    let escaped = with_copy(60_000, |n| escape_of_c.repeat(n));
    let compact = plain(60_000);
    assert!(
        compact.len() < CHECKPOINTS_REPLY_MAX && escaped.len() > CHECKPOINTS_REPLY_MAX,
        "{} {}",
        compact.len(),
        escaped.len()
    );
    assert!(CoordinatorReply::parse(compact.as_bytes()).is_ok());
    refused_by_the_bound(escaped.as_bytes());
    let not_json = format!("x{exact}");
    refused_by_the_bound(not_json.as_bytes());
    let over = plain(copy_at_the_bound() + 1);
    let broken = format!("{}x", &over[..over.len() - 1]);
    assert_eq!(broken.len(), CHECKPOINTS_REPLY_MAX + 1);
    refused_by_the_bound(broken.as_bytes());
    let short_broken = format!("{}x", &exact[..exact.len() - 1]);
    let refusal = CoordinatorReply::parse(short_broken.as_bytes()).unwrap_err();
    assert_eq!(
        (refusal.cause(), refusal.rule()),
        (RefusalCause::Malformed, "json")
    );
}

#[test]
fn the_writer_of_a_list_reply_counts_the_compact_text_that_it_writes() {
    let len = copy_at_the_bound();
    let exact = plain(len);
    let CoordinatorReply::Checkpoints(mut reply) =
        CoordinatorReply::parse(exact.as_bytes()).unwrap()
    else {
        panic!("not a list reply")
    };
    assert_eq!(reply.to_json().unwrap(), exact);
    reply.checkpoints[0].copy = OwnValue::new(&"c".repeat(len + 1));
    let refusal = reply.to_json().unwrap_err();
    assert_eq!(
        (refusal.cause(), refusal.type_name(), refusal.rule()),
        (RefusalCause::Length, "CheckpointsReply", "json_length")
    );
}

fn refused_by_its_own_bound<T: std::fmt::Debug>(
    result: Result<T, Refusal>,
    type_name: &str,
    len: usize,
) {
    let refusal = result.expect_err("refused");
    assert_eq!(
        (refusal.cause(), refusal.type_name(), refusal.rule()),
        (RefusalCause::Length, type_name, "reply_bytes"),
        "{len} bytes"
    );
}

fn compact_page(len: usize) -> String {
    let mut copy = len - (plain(1).len() - 1);
    while plain(copy).len() > len {
        copy -= 1;
    }
    let page = plain(copy);
    assert_eq!(page.len(), len);
    page
}

fn reviewers_page(workbook: &str) -> String {
    checkpoints(
        &[format!(
            r#"{{"workflow":"{W}","agent":"{A}","run":"{R}","folder":["/d"],"workbook":[{workbook}],"copy":["c.xlsx"],"state":"made","sha256":"{SHA}","bytes":1,"at":1794339000000000001,"ended":"no","effect":null,"superseded":"no"}}"#
        )],
        "null",
    )
}

#[test]
fn the_reviewers_padded_and_escaped_pages_are_refused_by_the_list_reply_s_own_parse() {
    let small = reviewers_page(r#""/d/a.xlsx""#);
    assert_eq!(
        CheckpointsReply::parse(small.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        small
    );
    let padded = small.replacen(',', &format!(",{}", " ".repeat(CHECKPOINTS_REPLY_MAX)), 1);
    let piece = format!("\"{}\"", format!("{}u0061", '\\').repeat(1024));
    let escaped_page = |n: usize| reviewers_page(&vec![piece.as_str(); n].join(","));
    let escaped = escaped_page(60);
    assert_eq!((padded.len(), escaped.len()), (261_458, 369_199));
    let half = escaped_page(30);
    assert!(half.len() < CHECKPOINTS_REPLY_MAX);
    assert!(CheckpointsReply::parse(half.as_bytes()).is_ok());
    for input in [&padded, &escaped] {
        refused_by_its_own_bound(
            CheckpointsReply::parse(input.as_bytes()),
            "CheckpointsReply",
            input.len(),
        );
        refused_by_the_bound(input.as_bytes());
    }
}

#[test]
fn the_verifiers_five_pages_padded_past_the_bound_are_refused_by_the_list_reply_s_own_parse() {
    let page = compact_page(60_000);
    let room = CHECKPOINTS_REPLY_MAX - page.len();
    let inside =
        |pad: String| page.replacen("{\"checkpoints\":", &format!("{{\"checkpoints\":{pad}"), 1);
    for input in [
        format!("{}{page}", " ".repeat(room)),
        format!("{page}{}", " ".repeat(room)),
        inside(" ".repeat(room)),
    ] {
        assert_eq!(input.len(), CHECKPOINTS_REPLY_MAX);
        assert!(CheckpointsReply::parse(input.as_bytes()).is_ok());
        assert!(CoordinatorReply::parse(input.as_bytes()).is_ok());
    }
    let cases = [
        ("c1141", format!("{}{page}", " ".repeat(room + 1))),
        ("c1143", format!("{page}{}", " ".repeat(room + 1))),
        ("c1145", inside(" ".repeat(room + 1))),
        ("c1146", format!("{page}{}", "\n".repeat(room + 1))),
        ("c1147", format!("{}{page}", "\t".repeat(room + 400))),
    ];
    let lens: Vec<usize> = cases.iter().map(|(_, input)| input.len()).collect();
    assert_eq!(lens, [261_068, 261_068, 261_068, 261_068, 261_467]);
    for (case, input) in &cases {
        let refusal = CheckpointsReply::parse(input.as_bytes()).expect_err(case);
        assert_eq!(
            (refusal.cause(), refusal.type_name(), refusal.rule()),
            (RefusalCause::Length, "CheckpointsReply", "reply_bytes"),
            "{case}"
        );
        refused_by_the_bound(input.as_bytes());
    }
}

#[test]
fn a_compact_list_reply_at_the_bound_is_taken_by_its_own_parse_and_one_byte_more_is_refused_unread()
{
    let exact = plain(copy_at_the_bound());
    assert_eq!(exact.len(), CHECKPOINTS_REPLY_MAX);
    assert_eq!(
        CheckpointsReply::parse(exact.as_bytes())
            .unwrap()
            .to_json()
            .unwrap(),
        exact
    );
    let over = plain(copy_at_the_bound() + 1);
    let not_json = format!("x{exact}");
    let broken = format!("{}x", &over[..over.len() - 1]);
    for input in [over, not_json, broken] {
        assert_eq!(input.len(), CHECKPOINTS_REPLY_MAX + 1);
        refused_by_its_own_bound(
            CheckpointsReply::parse(input.as_bytes()),
            "CheckpointsReply",
            input.len(),
        );
    }
}

fn counted_on_the_bytes_given<T: std::fmt::Debug>(
    text: &str,
    type_name: &str,
    parse: fn(&[u8]) -> Result<T, Refusal>,
) {
    let room = CHECKPOINTS_REPLY_MAX - text.len();
    let at = format!("{{{}{}", " ".repeat(room), &text[1..]);
    assert_eq!(at.len(), CHECKPOINTS_REPLY_MAX);
    assert!(parse(at.as_bytes()).is_ok(), "{type_name}");
    assert!(
        CoordinatorReply::parse(at.as_bytes()).is_ok(),
        "{type_name}"
    );
    for over in [
        format!("{}{text}", " ".repeat(room + 1)),
        format!("{text}{}", "\n".repeat(room + 1)),
        format!("{{{}{}", "\t".repeat(room + 1), &text[1..]),
    ] {
        refused_by_its_own_bound(parse(over.as_bytes()), type_name, over.len());
        refused_by_the_bound(over.as_bytes());
    }
}

#[test]
fn every_other_reply_of_a_coordinator_body_is_counted_on_the_bytes_given_by_its_own_parse() {
    counted_on_the_bytes_given(r#"{"rev":1}"#, "RevReply", RevReply::parse);
    counted_on_the_bytes_given(r#"{"state":"active"}"#, "StateReply", StateReply::parse);
    counted_on_the_bytes_given(r#"{"ok":"yes"}"#, "Accepted", Accepted::parse);
    counted_on_the_bytes_given(r#"{"stored":1}"#, "Stored", Stored::parse);
}

#[test]
fn the_serde_reader_of_a_reply_counts_the_text_of_the_value_and_is_given_no_byte_outside_it() {
    let page = compact_page(260_966);
    for outside in [
        format!("{}{page}", " ".repeat(102)),
        format!("{page}{}", " ".repeat(102)),
    ] {
        assert_eq!(outside.len(), CHECKPOINTS_REPLY_MAX + 1);
        refused_by_the_bound(outside.as_bytes());
        assert!(serde_json::from_str::<CoordinatorReply>(&outside).is_ok());
    }
    let inside = page.replacen(
        "{\"checkpoints\":",
        &format!("{{\"checkpoints\":{}", " ".repeat(102)),
        1,
    );
    assert_eq!(inside.len(), CHECKPOINTS_REPLY_MAX + 1);
    let refusal = serde_json::from_str::<CoordinatorReply>(&inside)
        .unwrap_err()
        .to_string();
    assert!(
        refusal.contains("length: CoordinatorReply: -: reply_bytes"),
        "{refusal}"
    );
    let stored = format!("{}{{\"stored\":1}}", " ".repeat(CHECKPOINTS_REPLY_MAX));
    refused_by_its_own_bound(
        RunnerReply::parse(stored.as_bytes()),
        "RunnerReply",
        stored.len(),
    );
    assert!(serde_json::from_str::<RunnerReply>(&stored).is_ok());
}

#[test]
fn a_reply_is_counted_in_bytes_and_every_carriage_return_counts() {
    let wide = |len: usize| {
        with_copy(len, |n| {
            format!("{}{}", "c".repeat(n % 2), "\u{e9}".repeat(n / 2))
        })
    };
    let at = wide(copy_at_the_bound());
    assert_eq!(at.len(), CHECKPOINTS_REPLY_MAX);
    assert!(CheckpointsReply::parse(at.as_bytes()).is_ok());
    let over = wide(copy_at_the_bound() + 1);
    assert_eq!(over.len(), CHECKPOINTS_REPLY_MAX + 1);
    assert!(over.chars().count() < CHECKPOINTS_REPLY_MAX / 2 + 1_000);
    refused_by_its_own_bound(
        CheckpointsReply::parse(over.as_bytes()),
        "CheckpointsReply",
        over.len(),
    );
    refused_by_the_bound(over.as_bytes());
    let stored = r#"{"stored":1}"#;
    let returns = |len: usize| format!("{stored}{}", "\r".repeat(len - stored.len()));
    let at = returns(CHECKPOINTS_REPLY_MAX);
    assert!(Stored::parse(at.as_bytes()).is_ok());
    assert!(CoordinatorReply::parse(at.as_bytes()).is_ok());
    assert!(RunnerReply::parse(at.as_bytes()).is_ok());
    let over = returns(CHECKPOINTS_REPLY_MAX + 1);
    assert_eq!(over.len(), CHECKPOINTS_REPLY_MAX + 1);
    refused_by_its_own_bound(Stored::parse(over.as_bytes()), "Stored", over.len());
    refused_by_the_bound(over.as_bytes());
    refused_by_its_own_bound(
        RunnerReply::parse(over.as_bytes()),
        "RunnerReply",
        over.len(),
    );
    let wide_tail = format!(
        "{stored}{}",
        "\u{e9}".repeat((CHECKPOINTS_REPLY_MAX + 1 - stored.len()) / 2)
    );
    let wide_tail = format!(
        "{wide_tail}{}",
        " ".repeat(CHECKPOINTS_REPLY_MAX + 1 - wide_tail.len())
    );
    assert_eq!(wide_tail.len(), CHECKPOINTS_REPLY_MAX + 1);
    refused_by_the_bound(wide_tail.as_bytes());
    refused_by_its_own_bound(
        RunnerReply::parse(wide_tail.as_bytes()),
        "RunnerReply",
        wide_tail.len(),
    );
}
