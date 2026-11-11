mod common;

use common::{entries, read, refused, refused_as_array, values, with, HEX_A, HEX_B, MAX63};
use halvo_schema::request::{
    decode, decode_reply, write_reply, AnalysisRole, AnalysisWrite, CoordinatorBody,
    CoordinatorRole, CoordinatorWrite, Decision, DecodeError, Decoded, ErrorName, ExtBody,
    ExtRequest, ExtRole, InterfaceDecision, InterfaceRead, InterfaceRole, Read, Reply, ReplyResult,
    Role, RunnerBody, RunnerRequest, RunnerRole, ANALYSIS_WRITE_MAX, INTERFACE_REQUEST_MAX,
};
use halvo_schema::{
    Episode, EvidencePage, Position, RequestId, StoreId, SummaryId, Text, WorkflowId, WorkflowsPage,
};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

const EXAMPLE: &str = r#"{"v":1,"id":"7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d","request":"analysis_write","body":{"store":"4a20c017c73d6c2ba45ee3b27fa724f4","episodes":[{"id":"c41d7e90a2b34f658791a0b2c3d4e5f6","category":"utility_bill_payment","site":{"token":"northwind-power","source":"title"},"app":"com.google.Chrome","day":20767,"part":"evening","minutes":5,"keys":null,"pointer":null,"segments":[4410,4411],"expires":1795478400000000000,"start":1794339000000000000,"end":1794339135000000000,"steps":[{"app":{"value":"com.google.Chrome","layer":"floor"},"title":{"value":"Pay bill - Northwind Power","layer":"floor"},"input":{"value":"click","layer":"floor"},"site":null,"text":null}]}],"commit":{"seg":4411,"idx":212}}}"#;
const EPISODE: &str = r#"{"id":"c41d7e90a2b34f658791a0b2c3d4e5f6","category":"utility_bill_payment","site":{"token":"northwind-power","source":"title"},"app":"com.google.Chrome","day":20767,"part":"evening","minutes":5,"keys":null,"pointer":null,"segments":[4410,4411],"expires":1795478400000000000,"start":1794339000000000000,"end":1794339135000000000,"steps":[{"app":{"value":"com.google.Chrome","layer":"floor"},"title":{"value":"Pay bill - Northwind Power","layer":"floor"},"input":{"value":"click","layer":"floor"},"site":null,"text":null}]}"#;
const LONGEST: Position = Position {
    seg: MAX63,
    idx: MAX63,
};

#[derive(Debug, Serialize, Deserialize)]
struct Probe {
    probe: u32,
}

impl RunnerBody for Probe {
    type Reply = ();
}

impl CoordinatorBody for Probe {
    type Reply = ();
}

#[derive(Debug)]
struct Learning;

impl ExtRole for Learning {
    const NAME: &'static str = "learning";
}

impl ExtBody<Learning> for Probe {
    type Reply = ();
}

macro_rules! bad_role {
    ($name:ident, $text:expr) => {
        #[derive(Debug)]
        struct $name;
        impl ExtRole for $name {
            const NAME: &'static str = $text;
        }
        impl ExtBody<$name> for Probe {
            type Reply = ();
        }
    };
}

bad_role!(AsRequestName, "interface_decision");
bad_role!(AsRunner, "runner");
bad_role!(AsRoleName, "coordinator");
bad_role!(Empty, "");
bad_role!(Upper, "Learning");
bad_role!(Hyphen, "learn-ing");
bad_role!(TooLong, "abcdefghijklmnopqrstuvwxyz0123456");

fn rid() -> RequestId {
    RequestId::parse("7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d").unwrap()
}

fn wid() -> WorkflowId {
    WorkflowId::parse(HEX_A).unwrap()
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap()
}

fn error(bytes: &[u8]) -> DecodeError {
    match decode(bytes) {
        Err(e) => e,
        Ok(d) => panic!("decoded {d:?}"),
    }
}

fn reply_error(bytes: &str) -> DecodeError {
    match decode_reply(bytes.as_bytes()) {
        Err(e) => e,
        Ok(r) => panic!("decoded {r:?}"),
    }
}

fn example_write() -> AnalysisWrite {
    let mut write = AnalysisWrite::new(
        &AnalysisRole::claim(),
        rid(),
        StoreId::parse("4a20c017c73d6c2ba45ee3b27fa724f4").unwrap(),
    );
    write.push(read::<Episode>(EPISODE)).unwrap();
    write.commit(Position {
        seg: 4411,
        idx: 212,
    });
    write
}

#[test]
fn the_analysis_write_of_the_contract_is_written_and_read_byte_for_byte() {
    let write = example_write();
    let bytes = write.to_bytes().unwrap();
    assert_eq!(text(bytes.clone()), EXAMPLE);
    assert_eq!(write.to_bytes().unwrap(), bytes);
    assert_eq!(write.id(), &rid());
    assert_eq!(write.store().as_str(), "4a20c017c73d6c2ba45ee3b27fa724f4");
    assert_eq!(write.episodes().len(), 1);
    assert_eq!(
        write.commit_position(),
        Some(Position {
            seg: 4411,
            idx: 212
        })
    );
    match decode(EXAMPLE.as_bytes()).unwrap() {
        Decoded::AnalysisWrite(w) => {
            assert_eq!(w.to_bytes().unwrap(), bytes);
            assert_eq!(w.episodes(), write.episodes());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_interface_requests_have_their_forms() {
    let interface = InterfaceRole::claim();
    let sid = SummaryId::parse(HEX_B).unwrap();
    let cases = [
        (
            InterfaceRead::workflows(&interface, rid(), None).to_bytes(),
            r#"{"v":1,"id":"7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d","request":"interface_read","body":{"read":"workflows","after":null}}"#.to_owned(),
        ),
        (
            InterfaceRead::workflows(&interface, rid(), Some(wid())).to_bytes(),
            format!(r#"{{"v":1,"id":"7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d","request":"interface_read","body":{{"read":"workflows","after":"{HEX_A}"}}}}"#),
        ),
        (
            InterfaceRead::evidence(&interface, rid(), wid(), Some(sid.clone())).to_bytes(),
            format!(r#"{{"v":1,"id":"7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d","request":"interface_read","body":{{"read":"evidence","workflow":"{HEX_A}","after":"{HEX_B}"}}}}"#),
        ),
        (
            InterfaceRead::evidence(&interface, rid(), wid(), None).to_bytes(),
            format!(r#"{{"v":1,"id":"7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d","request":"interface_read","body":{{"read":"evidence","workflow":"{HEX_A}","after":null}}}}"#),
        ),
    ];
    for (bytes, want) in cases {
        let bytes = bytes.unwrap();
        assert_eq!(text(bytes.clone()), want);
        match decode(&bytes).unwrap() {
            Decoded::InterfaceRead(r) => assert_eq!(r.to_bytes().unwrap(), bytes),
            other => panic!("{other:?}"),
        }
    }
    let read = InterfaceRead::evidence(&interface, rid(), wid(), Some(sid.clone()));
    assert_eq!(
        read.read(),
        &Read::Evidence {
            workflow: wid(),
            after: Some(sid)
        }
    );
    for (decision, name, built) in [
        (
            Decision::Approve,
            "approve",
            InterfaceDecision::approve(&interface, rid(), wid()),
        ),
        (
            Decision::Dismiss,
            "dismiss",
            InterfaceDecision::dismiss(&interface, rid(), wid()),
        ),
        (
            Decision::Delete,
            "delete",
            InterfaceDecision::delete(&interface, rid(), wid()),
        ),
    ] {
        let bytes = built.to_bytes().unwrap();
        assert_eq!(
            text(bytes.clone()),
            format!(
                r#"{{"v":1,"id":"7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d","request":"interface_decision","body":{{"decision":"{name}","workflow":"{HEX_A}"}}}}"#
            )
        );
        assert_eq!(built.decision(), decision);
        assert_eq!(built.workflow(), &wid());
        match decode(&bytes).unwrap() {
            Decoded::InterfaceDecision(d) => {
                assert_eq!(d.to_bytes().unwrap(), bytes);
                assert_eq!(d.decision(), decision);
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn the_bodies_of_later_runs_are_written_with_their_names() {
    let runner = RunnerRequest::new(&RunnerRole::claim(), rid(), Probe { probe: 1 });
    assert_eq!(
        text(runner.to_bytes().unwrap()),
        r#"{"v":1,"id":"7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d","request":"runner","body":{"probe":1}}"#
    );
    assert_eq!(runner.body().probe, 1);
    let coordinator = CoordinatorWrite::new(&CoordinatorRole::claim(), rid(), Probe { probe: 1 });
    assert_eq!(
        text(coordinator.to_bytes().unwrap()),
        r#"{"v":1,"id":"7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d","request":"coordinator_write","body":{"probe":1}}"#
    );
    assert_eq!(coordinator.id(), &rid());
    let ext = ExtRequest::new(&Learning, rid(), Probe { probe: 1 });
    assert_eq!(
        text(ext.to_bytes().unwrap()),
        r#"{"v":1,"id":"7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d","request":"learning","body":{"probe":1}}"#
    );
    let big = RunnerRequest::new(&RunnerRole::claim(), rid(), Probe { probe: 1 });
    assert!(big.to_bytes().is_ok());
}

#[test]
fn a_further_role_takes_only_a_name_that_no_request_or_role_holds() {
    let malformed = Err(ErrorName::Malformed);
    assert_eq!(
        ExtRequest::new(&AsRequestName, rid(), Probe { probe: 1 }).to_bytes(),
        malformed
    );
    assert_eq!(
        ExtRequest::new(&AsRunner, rid(), Probe { probe: 1 }).to_bytes(),
        malformed
    );
    assert_eq!(
        ExtRequest::new(&AsRoleName, rid(), Probe { probe: 1 }).to_bytes(),
        malformed
    );
    assert_eq!(
        ExtRequest::new(&Empty, rid(), Probe { probe: 1 }).to_bytes(),
        malformed
    );
    assert_eq!(
        ExtRequest::new(&Upper, rid(), Probe { probe: 1 }).to_bytes(),
        malformed
    );
    assert_eq!(
        ExtRequest::new(&Hyphen, rid(), Probe { probe: 1 }).to_bytes(),
        malformed
    );
    assert_eq!(
        ExtRequest::new(&TooLong, rid(), Probe { probe: 1 }).to_bytes(),
        malformed
    );
}

fn episode_len(e: &Episode) -> usize {
    serde_json::to_string(e).unwrap().len()
}

fn episode_of_len(target: usize) -> Episode {
    let mut e = read::<Episode>(EPISODE);
    let mut step = e.steps[0].clone();
    step.title.as_mut().unwrap().value = Text::cut("");
    e.steps.clear();
    loop {
        e.steps.push(step.clone());
        let bare = episode_len(&e);
        if bare + 1024 * e.steps.len() >= target {
            let mut need = target - bare;
            for s in e.steps.iter_mut() {
                let take = need.min(1024);
                s.title.as_mut().unwrap().value = Text::cut(&"t".repeat(take));
                need -= take;
            }
            assert_eq!(episode_len(&e), target);
            return e;
        }
    }
}

#[test]
fn push_counts_the_longest_position_and_never_lets_a_write_pass_its_bound() {
    let store = StoreId::parse(HEX_B).unwrap();
    let mut empty = AnalysisWrite::new(&AnalysisRole::claim(), rid(), store.clone());
    empty.commit(LONGEST);
    let base = empty.to_bytes().unwrap().len();
    assert_eq!(
        AnalysisWrite::new(&AnalysisRole::claim(), rid(), store.clone())
            .to_bytes()
            .unwrap()
            .len(),
        base - 53 + 4
    );

    let mut write = AnalysisWrite::new(&AnalysisRole::claim(), rid(), store.clone());
    write.push(episode_of_len(30_000)).unwrap();
    write.push(episode_of_len(20_000)).unwrap();
    let room = ANALYSIS_WRITE_MAX - base - 30_000 - 1 - 20_000 - 1;
    let over = episode_of_len(room + 1);
    let before = write.to_bytes().unwrap();
    let returned = write.push(over.clone()).unwrap_err();
    assert_eq!(returned, over);
    assert_eq!(write.to_bytes().unwrap(), before);
    assert_eq!(write.episodes().len(), 2);
    write.push(episode_of_len(room)).unwrap();
    assert_eq!(write.to_bytes().unwrap().len(), ANALYSIS_WRITE_MAX - 49);
    write.commit(LONGEST);
    assert_eq!(write.to_bytes().unwrap().len(), ANALYSIS_WRITE_MAX);
    let last = read::<Episode>(EPISODE);
    assert_eq!(write.push(last.clone()).unwrap_err(), last);
    let bytes = write.to_bytes().unwrap();
    match decode(&bytes).unwrap() {
        Decoded::AnalysisWrite(mut w) => {
            assert_eq!(w.to_bytes().unwrap(), bytes);
            assert!(w.push(read::<Episode>(EPISODE)).is_err());
        }
        other => panic!("{other:?}"),
    }

    let mut alone = AnalysisWrite::new(&AnalysisRole::claim(), rid(), store);
    let mut huge = read::<Episode>(EPISODE);
    let step = huge.steps[0].clone();
    huge.steps = vec![step; 1000];
    assert!(episode_len(&huge) + base > ANALYSIS_WRITE_MAX);
    assert!(alone.push(huge).is_err());
    assert!(alone.episodes().is_empty());
}

#[test]
fn a_value_that_its_reader_refuses_is_not_written() {
    let mut write = example_write();
    write.commit(Position {
        seg: MAX63 + 1,
        idx: 0,
    });
    assert_eq!(write.to_bytes(), Err(ErrorName::Malformed));
    write.commit(Position {
        seg: 0,
        idx: u64::MAX,
    });
    assert_eq!(write.to_bytes(), Err(ErrorName::Malformed));
    write.commit(LONGEST);
    assert!(write.to_bytes().is_ok());
    let mut bad = read::<Episode>(EPISODE);
    bad.minutes = 7;
    write.push(bad).unwrap();
    assert_eq!(write.to_bytes(), Err(ErrorName::Malformed));
}

fn line(request: &str, body: &str) -> String {
    format!(r#"{{"v":1,"id":"{HEX_A}","request":"{request}","body":{body}}}"#)
}

#[test]
fn decode_makes_its_checks_in_the_order_of_the_contract() {
    let id = Some(RequestId::parse(HEX_A).unwrap());
    let read_body = r#"{"read":"workflows","after":null}"#;
    let cases: Vec<(String, Option<RequestId>, ErrorName)> = vec![
        ("not json".to_owned(), None, ErrorName::Malformed),
        ("[]".to_owned(), None, ErrorName::Malformed),
        ("{\"v\":1".to_owned(), None, ErrorName::Malformed),
        (
            line("interface_read", read_body).replace("\"v\":1", "\"v\":1.0"),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line("interface_read", read_body).replace("\"v\":1", "\"v\":1e0"),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line("interface_read", read_body).replace("\"v\":1", "\"v\":\"1\""),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line("interface_read", read_body).replace("\"v\":1,", ""),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line("interface_read", read_body).replace("\"v\":1", "\"v\":2"),
            id.clone(),
            ErrorName::Version,
        ),
        (
            line("interface_read", read_body).replace("\"v\":1", "\"v\":-1"),
            id.clone(),
            ErrorName::Version,
        ),
        (
            line("interface_read", read_body).replace("\"v\":1", "\"v\":99999999999999999999999"),
            id.clone(),
            ErrorName::Version,
        ),
        (
            r#"{"v":2,"request":5}"#.to_owned(),
            None,
            ErrorName::Version,
        ),
        (
            line("interface_read", read_body)
                .replace("\"request\":\"interface_read\"", "\"request\":5"),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line("interface_read", read_body).replace("\"request\":\"interface_read\",", ""),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line(
                "interface_read",
                &format!("{read_body}{}", " ".repeat(4096)),
            ),
            id.clone(),
            ErrorName::TooLarge,
        ),
        (
            line(
                "interface_decision",
                &format!("{{\"bad\":{}1}}", " ".repeat(4096)),
            ),
            id.clone(),
            ErrorName::TooLarge,
        ),
        (
            line(
                "analysis_write",
                &format!("{{\"bad\":{}1}}", " ".repeat(65500)),
            ),
            id.clone(),
            ErrorName::TooLarge,
        ),
        (
            line("interface_read", r#"{"read":"workflows"}"#),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line(
                "interface_read",
                r#"{"read":"workflows","after":null,"x":1}"#,
            ),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line("interface_read", r#"{"read":"all","after":null}"#),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line("interface_read", r#"{"read":"evidence","after":null}"#),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line(
                "interface_decision",
                &format!(r#"{{"decision":"approve","workflow":"{HEX_A}","state":"active"}}"#),
            ),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line(
                "interface_decision",
                &format!(r#"{{"decision":"activate","workflow":"{HEX_A}"}}"#),
            ),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line(
                "interface_decision",
                r#"{"decision":"approve","workflow":"x"}"#,
            ),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line(
                "analysis_write",
                r#"{"store":"x","episodes":[],"commit":null}"#,
            ),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line(
                "analysis_write",
                &format!(r#"{{"store":"{HEX_B}","episodes":[]}}"#),
            ),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line(
                "analysis_write",
                &format!(r#"{{"store":"{HEX_B}","episodes":[],"commit":null,"state":"approved"}}"#),
            ),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line(
                "analysis_write",
                &format!(
                    r#"{{"store":"{HEX_B}","episodes":[{}],"commit":null}}"#,
                    EPISODE.replace(
                        "com.google.Chrome\",\"day",
                        "/Applications/Chrome.app\",\"day"
                    )
                ),
            ),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line(
                "analysis_write",
                &format!(
                    r#"{{"store":"{HEX_B}","episodes":[],"commit":{{"seg":9223372036854775808,"idx":0}}}}"#
                ),
            ),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line("runner", "{}").replace("\"body\":{}", "\"body\":{},\"extra\":1"),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line("runner", "{}").replace(",\"body\":{}", ""),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            line("runner", "{}").replace(&format!("\"id\":\"{HEX_A}\""), "\"id\":\"x\""),
            None,
            ErrorName::Malformed,
        ),
        (
            line("runner", "{}").replace(&format!("\"id\":\"{HEX_A}\","), ""),
            None,
            ErrorName::Malformed,
        ),
        (
            line("runner", "{}").replace("\"v\":1,", &format!("\"v\":1,\"id\":\"{HEX_B}\",")),
            None,
            ErrorName::Malformed,
        ),
        (
            line("runner", "{}").replace("\"v\":1,", "\"v\":1,\"v\":1,"),
            id.clone(),
            ErrorName::Malformed,
        ),
        (line("analysis", "{}"), id.clone(), ErrorName::Malformed),
        (line("interface", "{}"), id.clone(), ErrorName::Malformed),
        (line("coordinator", "{}"), id.clone(), ErrorName::Malformed),
        (line("Learning", "{}"), id.clone(), ErrorName::Malformed),
        (line("learn-ing", "{}"), id.clone(), ErrorName::Malformed),
        (line("", "{}"), id.clone(), ErrorName::Malformed),
        (
            line(&"a".repeat(33), "{}"),
            id.clone(),
            ErrorName::Malformed,
        ),
    ];
    for (input, want_id, want) in cases {
        let got = error(input.as_bytes());
        assert_eq!(
            (got.error, &got.id),
            (want, &want_id),
            "{}",
            &input[..input.len().min(160)]
        );
    }
    let mut bytes = line("runner", "{}").into_bytes();
    bytes.push(0xff);
    assert_eq!(error(&bytes).error, ErrorName::Malformed);
}

#[test]
fn decode_returns_the_bodies_of_later_runs_as_their_bytes() {
    let huge = format!("[{}0]", "0,".repeat(200_000));
    let cases = [
        ("runner", r#"{ "probe" : 1 }"#, Some(Role::Runner)),
        (
            "coordinator_write",
            r#"{"op":"set_state","x":[1,2]}"#,
            Some(Role::Coordinator),
        ),
        ("coordinator_write", "5", Some(Role::Coordinator)),
        ("runner", &huge, Some(Role::Runner)),
        ("learning", r#"{"probe":1}"#, None),
        ("a", "null", None),
        ("abcdefghijklmnopqrstuvwxyz012345", "\"x\"", None),
    ];
    for (name, body, role) in cases {
        let input = line(name, body);
        match decode(input.as_bytes()).unwrap() {
            Decoded::Raw(raw) => {
                assert_eq!(raw.name(), name);
                assert_eq!(raw.body().get(), body);
                assert_eq!(raw.id().as_str(), HEX_A);
                assert_eq!(raw.role(), role);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(decode(input.as_bytes()).unwrap().role(), role);
    }
    assert_eq!(
        decode(EXAMPLE.as_bytes()).unwrap().role(),
        Some(Role::Analysis)
    );
    let read = InterfaceRead::workflows(&InterfaceRole::claim(), rid(), None)
        .to_bytes()
        .unwrap();
    assert_eq!(decode(&read).unwrap().role(), Some(Role::Interface));
    let decision = InterfaceDecision::delete(&InterfaceRole::claim(), rid(), wid())
        .to_bytes()
        .unwrap();
    assert_eq!(decode(&decision).unwrap().role(), Some(Role::Interface));
    assert_eq!(decode(&decision).unwrap().id(), &rid());
    let reordered = format!(
        r#"{{"body":{{"read":"workflows","after":null}},"request":"interface_read","id":"{HEX_A}","v":1}}"#
    );
    assert!(matches!(
        decode(reordered.as_bytes()).unwrap(),
        Decoded::InterfaceRead(_)
    ));
}

const W1: &str = "10000000000000000000000000000000";

const WORKFLOW: &str = r#"{"v":1,"id":"10000000000000000000000000000000","state":"proposed","category":"email","site":null,"app":"com.apple.mail","trigger":"manual","recurrence":{"count":1,"period":null,"first":20767,"last":20767},"confidence":50,"agent":null}"#;

fn workflows_item() -> String {
    format!(r#"{{"workflow":{WORKFLOW},"summaries":["{HEX_B}"],"hint":null}}"#)
}

fn workflows_page() -> String {
    format!(r#"{{"workflows":[{}],"next":"{W1}"}}"#, workflows_item())
}

fn evidence_item() -> String {
    format!(
        r#"{{"v":1,"summary":"{HEX_B}","segments":[1],"expires":10,"start":1,"end":2,"hint":null,"steps":[]}}"#
    )
}

fn evidence_page() -> String {
    format!(r#"{{"evidence":[{}],"next":null}}"#, evidence_item())
}

fn reply(id: &str, key: &str, value: &str) -> String {
    format!(r#"{{"v":1,"id":{id},"{key}":{value}}}"#)
}

#[test]
fn decode_reply_reads_each_result_and_each_error_name() {
    let quoted = format!("\"{HEX_A}\"");
    let rid_a = RequestId::parse(HEX_A).unwrap();
    let r = decode_reply(reply(&quoted, "ok", "null").as_bytes()).unwrap();
    assert_eq!(r.id, Some(rid_a.clone()));
    assert!(matches!(r.outcome, Ok(ReplyResult::Null)));
    let r = decode_reply(reply(&quoted, "ok", &workflows_page()).as_bytes()).unwrap();
    match r.outcome {
        Ok(ReplyResult::Workflows(p)) => assert_eq!(p, read::<WorkflowsPage>(&workflows_page())),
        other => panic!("{other:?}"),
    }
    let r = decode_reply(reply(&quoted, "ok", &evidence_page()).as_bytes()).unwrap();
    match r.outcome {
        Ok(ReplyResult::Evidence(p)) => assert_eq!(p, read::<EvidencePage>(&evidence_page())),
        other => panic!("{other:?}"),
    }
    for raw in [
        r#"{"probe":1}"#,
        "5",
        "\"x\"",
        "[]",
        "true",
        r#"{"workflows":[],"next":null,"more":1}"#,
        r#"{"workflows":[]}"#,
        r#"{"evidence":[],"later":null}"#,
    ] {
        match decode_reply(reply(&quoted, "ok", raw).as_bytes())
            .unwrap()
            .outcome
        {
            Ok(ReplyResult::Raw(value)) => assert_eq!(value.get(), raw),
            other => panic!("{raw}: {other:?}"),
        }
    }
    for name in ErrorName::ALL {
        let r = decode_reply(reply("null", "error", &format!("\"{}\"", name.as_str())).as_bytes())
            .unwrap();
        assert_eq!(r.id, None);
        assert_eq!(r.outcome.unwrap_err(), *name);
    }
    let names: Vec<&str> = ErrorName::ALL.iter().map(|n| n.as_str()).collect();
    assert_eq!(
        names,
        [
            "malformed",
            "version",
            "role",
            "too_large",
            "store",
            "no_workflow",
            "state",
            "unsupported"
        ]
    );
}

#[test]
fn decode_reply_refuses_what_c3_does_not_write() {
    let quoted = format!("\"{HEX_A}\"");
    let id = Some(RequestId::parse(HEX_A).unwrap());
    let unsorted = workflows_page().replace(
        &format!("\"next\":\"{W1}\""),
        &format!("\"next\":\"{HEX_A}\""),
    );
    let hint_without_support = workflows_page().replace(
        &format!("\"summaries\":[\"{HEX_B}\"],\"hint\":null"),
        "\"summaries\":[],\"hint\":\"northwind-power\"",
    );
    assert_ne!(hint_without_support, workflows_page());
    let cases: Vec<(String, Option<RequestId>, ErrorName)> = vec![
        (
            reply(&quoted, "ok", &hint_without_support),
            id.clone(),
            ErrorName::Malformed,
        ),
        ("nope".to_owned(), None, ErrorName::Malformed),
        (
            reply(&quoted, "error", "\"oops\""),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            reply(&quoted, "error", "5"),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            reply(&quoted, "error", "null"),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            reply(&quoted, "ok", &unsorted),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            reply(&quoted, "ok", r#"{"workflows":5,"next":null}"#),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            reply(&quoted, "ok", r#"{"evidence":[],"next":"x"}"#),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            reply(&quoted, "ok", r#"{"workflows":[],"next":null,"next":null}"#),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            format!(r#"{{"v":1,"id":{quoted},"ok":null,"error":"role"}}"#),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            format!(r#"{{"v":1,"id":{quoted}}}"#),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            r#"{"v":1,"ok":null}"#.to_owned(),
            None,
            ErrorName::Malformed,
        ),
        (
            r#"{"v":1,"id":"x","ok":null}"#.to_owned(),
            None,
            ErrorName::Malformed,
        ),
        (
            format!(r#"{{"v":1,"id":{quoted},"ok":null,"extra":1}}"#),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            format!(r#"{{"v":2,"id":{quoted},"ok":null}}"#),
            id.clone(),
            ErrorName::Version,
        ),
        (
            format!(r#"{{"v":1.0,"id":{quoted},"ok":null}}"#),
            id.clone(),
            ErrorName::Malformed,
        ),
        (
            format!(r#"{{"id":{quoted},"ok":null}}"#),
            id.clone(),
            ErrorName::Malformed,
        ),
    ];
    for (input, want_id, want) in cases {
        let got = reply_error(&input);
        assert_eq!((got.error, &got.id), (want, &want_id), "{input}");
    }
}

#[test]
fn the_reply_writer_writes_v_id_then_ok_or_error() {
    let id = RequestId::parse(HEX_A).unwrap();
    let quoted = format!("\"{HEX_A}\"");
    assert_eq!(
        text(write_reply(Some(&id), Ok(&ReplyResult::Null)).unwrap()),
        reply(&quoted, "ok", "null")
    );
    assert_eq!(
        text(write_reply(None, Err(ErrorName::Malformed)).unwrap()),
        r#"{"v":1,"id":null,"error":"malformed"}"#
    );
    let page = ReplyResult::Workflows(read::<WorkflowsPage>(&workflows_page()));
    assert_eq!(
        text(write_reply(Some(&id), Ok(&page)).unwrap()),
        reply(&quoted, "ok", &workflows_page())
    );
    let page = ReplyResult::Evidence(read::<EvidencePage>(&evidence_page()));
    assert_eq!(
        text(write_reply(Some(&id), Ok(&page)).unwrap()),
        reply(&quoted, "ok", &evidence_page())
    );
    let raw = ReplyResult::Raw(RawValue::from_string(r#"{"probe":1}"#.to_owned()).unwrap());
    assert_eq!(
        text(write_reply(Some(&id), Ok(&raw)).unwrap()),
        reply(&quoted, "ok", r#"{"probe":1}"#)
    );
    for name in ErrorName::ALL {
        let bytes = write_reply(Some(&id), Err(*name)).unwrap();
        let back: Reply = decode_reply(&bytes).unwrap();
        assert_eq!(
            (back.id, back.outcome.unwrap_err()),
            (Some(id.clone()), *name)
        );
    }
    for result in [ReplyResult::Null, page, raw] {
        let bytes = write_reply(None, Ok(&result)).unwrap();
        let back = decode_reply(&bytes).unwrap();
        let again = write_reply(back.id.as_ref(), back.outcome.as_ref().map_err(|e| *e)).unwrap();
        assert_eq!(again, bytes);
    }
}

#[test]
fn the_reply_writer_refuses_a_result_that_decode_reply_refuses() {
    let id = RequestId::parse(HEX_A).unwrap();
    let mut page = read::<WorkflowsPage>(&workflows_page());
    page.next = Some(WorkflowId::parse(HEX_A).unwrap());
    assert_eq!(
        write_reply(Some(&id), Ok(&ReplyResult::Workflows(page))),
        Err(ErrorName::Malformed)
    );
    let mut page = read::<EvidencePage>(&evidence_page());
    page.evidence[0].segments.clear();
    assert_eq!(
        write_reply(Some(&id), Ok(&ReplyResult::Evidence(page))),
        Err(ErrorName::Malformed)
    );
    for shaped in [
        r#"{"workflows":5,"next":null}"#,
        r#"{"evidence":[],"next":"x"}"#,
    ] {
        let raw = ReplyResult::Raw(RawValue::from_string(shaped.to_owned()).unwrap());
        assert_eq!(write_reply(Some(&id), Ok(&raw)), Err(ErrorName::Malformed));
    }
}

#[test]
fn the_bounds_are_those_of_the_contract() {
    assert_eq!(ANALYSIS_WRITE_MAX, 65_500);
    assert_eq!(INTERFACE_REQUEST_MAX, 4_096);
    let roles: Vec<&str> = Role::ALL.iter().map(|r| r.as_str()).collect();
    assert_eq!(roles, ["analysis", "interface", "runner", "coordinator"]);
}

const STEP: &str = r#"{"app":{"value":"com.google.Chrome","layer":"floor"},"title":{"value":"Pay bill - Northwind Power","layer":"floor"},"input":{"value":"click","layer":"floor"},"site":null,"text":null}"#;

fn field(line: &str, key: &str) -> String {
    let found = entries(line).into_iter().find(|(k, _)| k == key);
    found.expect(key).1.get().to_owned()
}

fn not_malformed(cases: &[(&str, String)]) -> Vec<String> {
    cases
        .iter()
        .filter(|(_, input)| {
            !matches!(
                decode(input.as_bytes()),
                Err(DecodeError { error: ErrorName::Malformed, id: Some(ref id) }) if id.as_str() == HEX_A
            )
        })
        .map(|(label, _)| (*label).to_owned())
        .collect()
}

#[test]
fn a_read_body_is_refused_as_an_array() {
    refused_as_array::<Read>(r#"["workflows",null]"#);
    refused::<Read>(&format!(r#"["evidence","{HEX_A}",null]"#));
}

#[test]
fn decode_refuses_an_interface_body_in_the_array_form() {
    let mut cases: Vec<(&str, String)> = [
        ("an approval", format!(r#"["approve","{HEX_A}"]"#)),
        ("a dismissal", format!(r#"["dismiss","{HEX_A}"]"#)),
        ("a deletion", format!(r#"["delete","{HEX_A}"]"#)),
    ]
    .map(|(label, body)| (label, line("interface_decision", &body)))
    .into();
    cases.extend(
        [
            ("the read workflows", r#"["workflows",null]"#.to_owned()),
            (
                "the read evidence",
                format!(r#"["evidence","{HEX_A}",null]"#),
            ),
        ]
        .map(|(label, body)| (label, line("interface_read", &body))),
    );
    assert_eq!(not_malformed(&cases), Vec::<String>::new());
}

#[test]
fn decode_refuses_an_analysis_write_with_a_part_in_the_array_form() {
    let body = field(EXAMPLE, "body");
    let one = |episode: String| with(&body, "episodes", &format!("[{episode}]"));
    let tagged = with(STEP, "app", r#"["com.google.Chrome","floor"]"#);
    let cases = [
        ("the body", values(&body)),
        ("an episode", one(values(EPISODE))),
        (
            "the site of an episode",
            one(with(EPISODE, "site", r#"["title","northwind-power"]"#)),
        ),
        (
            "the key counts of an episode",
            one(with(EPISODE, "keys", "[1,2,3,4,5,6,7]")),
        ),
        (
            "a step",
            one(with(EPISODE, "steps", &format!("[{}]", values(STEP)))),
        ),
        (
            "a tagged value of a step",
            one(with(EPISODE, "steps", &format!("[{tagged}]"))),
        ),
        ("the commit position", with(&body, "commit", "[4411,212]")),
    ]
    .map(|(label, body)| (label, line("analysis_write", &body)));
    assert_eq!(not_malformed(&cases), Vec::<String>::new());
}

#[test]
fn decode_reply_and_the_writer_refuse_a_page_item_in_the_array_form() {
    let quoted = format!("\"{HEX_A}\"");
    let workflows = |item: String| format!(r#"{{"workflows":[{item}],"next":"{W1}"}}"#);
    let evidence = |item: String| format!(r#"{{"evidence":[{item}],"next":null}}"#);
    let pages = [
        ("a workflows item", workflows(values(&workflows_item()))),
        (
            "the workflow of an item",
            workflows(with(&workflows_item(), "workflow", &values(WORKFLOW))),
        ),
        ("an evidence item", evidence(values(&evidence_item()))),
        (
            "a step of an evidence item",
            evidence(with(
                &evidence_item(),
                "steps",
                &format!("[{}]", values(STEP)),
            )),
        ),
    ];
    let mut wrong = Vec::new();
    for (label, page) in pages {
        let read = decode_reply(reply(&quoted, "ok", &page).as_bytes());
        if !matches!(
            read,
            Err(DecodeError {
                error: ErrorName::Malformed,
                ..
            })
        ) {
            wrong.push(format!("decode_reply read {label}"));
        }
        let raw = ReplyResult::Raw(RawValue::from_string(page).unwrap());
        if write_reply(None, Ok(&raw)) != Err(ErrorName::Malformed) {
            wrong.push(format!("write_reply wrote {label}"));
        }
    }
    assert_eq!(wrong, Vec::<String>::new());
}
