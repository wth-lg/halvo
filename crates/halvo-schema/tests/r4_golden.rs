use std::collections::BTreeSet;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use halvo_schema::coordinator::body::{CoordHistory, CoordLog, History, Stored};
use halvo_schema::coordinator::names::{
    BindResult, CheckAnswer, DoneAnswer, EffectAnswer, HoldSetCause, OtherKind, OtherLapse, RunEnd,
    Trigger,
};
use halvo_schema::coordinator::record::{
    Answered, BindAnswer, Cleared, CoordEvent, Entry, ItemId, Lapsed, LinkAnswer, Raised,
    SignInAnswer, What,
};
use halvo_schema::coordinator::KINDS;
use halvo_schema::executable::{ApprovalId, Digest, EffectId, RunId, WebOrigin};
use halvo_schema::kinds::check_record;
use halvo_schema::record::Instant;
use halvo_schema::request::{
    decode, decode_reply, write_reply, CoordinatorRole, CoordinatorWrite, Decoded, ErrorName,
    ReplyResult,
};
use halvo_schema::{AgentId, RequestId, WorkflowId};
use serde_json::value::RawValue;

const DAY: u32 = 20767;
const AT: i64 = 1_794_339_000_000_000_000;
const MINUTE: i64 = 60_000_000_000;

fn run(n: u8) -> RunId {
    RunId::parse(&format!("9f8e7d6c5b4a39281706f5e4d3c2b1a{n}")).unwrap()
}

fn item(n: u8) -> ItemId {
    ItemId::parse(&format!("7e6d5c4b3a2918070f1e2d3c4b5a697{n}")).unwrap()
}

fn entry() -> Entry {
    Entry {
        effect: EffectId::parse("5c4b3a29180f7e6d5c4b3a29180f7e6d").unwrap(),
        approval: ApprovalId::parse("6d5c4b3a29180f7e6d5c4b3a29180f7e").unwrap(),
        digest: Digest::parse(&"ab".repeat(32)).unwrap(),
        answer: EffectAnswer::AllowOnce,
    }
}

fn origin(host: &str) -> WebOrigin {
    WebOrigin::parse(&format!("https://{host}:443")).unwrap()
}

fn row(seq: u32, day: u32, what: What) -> CoordEvent {
    CoordEvent {
        workflow: WorkflowId::parse("0f1e2d3c4b5a69788796a5b4c3d2e1f0").unwrap(),
        agent: AgentId::parse("1a2b3c4d5e6f708192a3b4c5d6e7f809").unwrap(),
        seq: NonZeroU32::new(seq).unwrap(),
        day,
        at: Instant(AT + i64::from(seq) * MINUTE + i64::from(day - DAY) * 1440 * MINUTE),
        what,
    }
}

fn records() -> Vec<CoordEvent> {
    let whats = [
        (
            DAY,
            What::RunStarted {
                run: run(0),
                trigger: Trigger::Schedule,
                slot: Some(DAY),
            },
        ),
        (
            DAY,
            What::ItemRaised {
                item: item(1),
                raised: Raised::Confirm(run(0)),
            },
        ),
        (
            DAY,
            What::ItemAnswered {
                item: item(1),
                answered: Answered::Confirm(entry()),
            },
        ),
        (
            DAY,
            What::ItemReleased {
                item: item(1),
                entry: entry(),
            },
        ),
        (
            DAY,
            What::ItemRaised {
                item: item(8),
                raised: Raised::SignIn(run(0)),
            },
        ),
        (
            DAY,
            What::ItemAnswered {
                item: item(8),
                answered: Answered::SignIn(SignInAnswer::First(DoneAnswer::Done)),
            },
        ),
        (
            DAY,
            What::ItemAnswered {
                item: item(8),
                answered: Answered::SignIn(SignInAnswer::Second(BindAnswer {
                    answer: BindResult::Allow,
                    origin: origin("northwind-power.test"),
                })),
            },
        ),
        (
            DAY,
            What::ItemRaised {
                item: item(2),
                raised: Raised::LinkAccount(run(0)),
            },
        ),
        (
            DAY,
            What::ItemAnswered {
                item: item(2),
                answered: Answered::LinkAccount(LinkAnswer {
                    answer: DoneAnswer::Done,
                    origin: origin("login.northwind-power.test"),
                }),
            },
        ),
        (
            DAY,
            What::RunEnded {
                run: run(0),
                cause: RunEnd::Completed,
            },
        ),
        (
            DAY + 1,
            What::ItemRaised {
                item: item(3),
                raised: Raised::Ready,
            },
        ),
        (
            DAY + 1,
            What::ItemLapsed {
                item: item(3),
                lapsed: Lapsed::Other {
                    kind: OtherKind::Ready,
                    cause: OtherLapse::Check,
                },
            },
        ),
        (
            DAY + 1,
            What::ItemRaised {
                item: item(4),
                raised: Raised::Check(None),
            },
        ),
        (
            DAY + 1,
            What::ItemAnswered {
                item: item(4),
                answered: Answered::Check(CheckAnswer::WentThrough),
            },
        ),
        (
            DAY + 1,
            What::HoldCleared {
                cleared: Cleared::Check {
                    run: run(0),
                    answer: CheckAnswer::WentThrough,
                },
            },
        ),
        (
            DAY + 1,
            What::RunStarted {
                run: run(1),
                trigger: Trigger::Person,
                slot: None,
            },
        ),
        (
            DAY + 1,
            What::ItemRaised {
                item: item(5),
                raised: Raised::Stuck(run(1)),
            },
        ),
        (
            DAY + 1,
            What::ItemLapsed {
                item: item(5),
                lapsed: Lapsed::Other {
                    kind: OtherKind::Stuck,
                    cause: OtherLapse::Cancelled,
                },
            },
        ),
        (
            DAY + 1,
            What::RunEnded {
                run: run(1),
                cause: RunEnd::Cancelled,
            },
        ),
        (
            DAY + 1,
            What::HoldSet {
                cause: HoldSetCause::Failures,
            },
        ),
        (
            DAY + 2,
            What::SlotSkipped {
                slot: Some(DAY + 2),
            },
        ),
        (
            DAY + 2,
            What::ItemRaised {
                item: item(6),
                raised: Raised::Setup,
            },
        ),
        (
            DAY + 2,
            What::ItemAnswered {
                item: item(6),
                answered: Answered::Setup,
            },
        ),
    ];
    whats
        .into_iter()
        .enumerate()
        .map(|(at, (day, what))| row(u32::try_from(at + 1).unwrap(), day, what))
        .collect()
}

fn id() -> RequestId {
    RequestId::parse("2c3d4e5f60718293a4b5c6d7e8f90a1b").unwrap()
}

fn history_id() -> RequestId {
    RequestId::parse("3d4e5f60718293a4b5c6d7e8f90a1b2c").unwrap()
}

fn log_body() -> CoordLog {
    CoordLog {
        workflow: records()[0].workflow.clone(),
        agent: records()[0].agent.clone(),
        records: records()[..3].to_vec(),
    }
}

fn history_body() -> CoordHistory {
    CoordHistory {
        workflow: records()[0].workflow.clone(),
        agent: records()[0].agent.clone(),
        after: Some(5),
    }
}

fn history_reply() -> History {
    History {
        records: records()[5..7].to_vec(),
        last: 23,
    }
}

fn raw(text: String) -> ReplyResult {
    ReplyResult::Raw(RawValue::from_string(text).unwrap())
}

fn request_lines() -> Vec<String> {
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
    vec![
        text(
            CoordinatorWrite::new(&CoordinatorRole::claim(), id(), log_body())
                .to_bytes()
                .unwrap(),
        ),
        text(
            write_reply(
                Some(&id()),
                Ok(&raw(Stored { stored: 3 }.to_json().unwrap())),
            )
            .unwrap(),
        ),
        text(write_reply(Some(&id()), Err(ErrorName::read("seq").unwrap())).unwrap()),
        text(write_reply(Some(&id()), Err(ErrorName::read("binding").unwrap())).unwrap()),
        text(
            CoordinatorWrite::new(&CoordinatorRole::claim(), history_id(), history_body())
                .to_bytes()
                .unwrap(),
        ),
        text(
            write_reply(
                Some(&history_id()),
                Ok(&raw(history_reply().to_json().unwrap())),
            )
            .unwrap(),
        ),
    ]
}

fn committed_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

fn committed(name: &str) -> String {
    let bytes = std::fs::read(committed_path(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.ends_with('\n'), "{name}: no LF at its end");
    assert!(!text.contains('\r'), "{name}: a CR");
    assert!(!text.starts_with('\u{feff}'), "{name}: a byte-order mark");
    text
}

fn written_equals_committed(name: &str, lines: &[String]) {
    let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
    let file = name.replace('/', "-");
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("r4-golden-{}-{file}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join(&file);
    std::fs::write(&out, &text).unwrap();
    let committed = std::fs::read(committed_path(name)).unwrap_or_default();
    assert!(
        committed == text.as_bytes(),
        "{name} differs from what the writers write, which stands at {}",
        out.display()
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

fn keys_of(value: &serde_json::Value, out: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, inner) in map {
                out.insert(key.clone());
                keys_of(inner, out);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|item| keys_of(item, out)),
        _ => {}
    }
}

fn names_file() -> BTreeSet<String> {
    committed("names.txt").lines().map(str::to_owned).collect()
}

#[test]
fn r4_golden_records() {
    let values = records();
    let lines: Vec<String> = values.iter().map(|r| r.to_json().unwrap()).collect();
    written_equals_committed("r4/records.jsonl", &lines);

    let text = committed("r4/records.jsonl");
    let entry = &KINDS[0];
    for (line, value) in text.lines().zip(&values) {
        assert_eq!(
            &CoordEvent::parse(line.as_bytes()).unwrap(),
            value,
            "{line}"
        );
        assert_eq!(check_record(entry, line), Ok(()), "{line}");
    }
    assert_eq!(text.lines().count(), values.len());
    assert_eq!(text.lines().nth(6), Some(entry.example));

    let mut keys = BTreeSet::new();
    for line in text.lines() {
        keys_of(&serde_json::from_str(line).unwrap(), &mut keys);
    }
    let names = names_file();
    let missing: Vec<&String> = keys.difference(&names).collect();
    assert!(
        missing.is_empty(),
        "keys missing from names.txt: {missing:?}"
    );
}

#[test]
fn r4_golden_requests() {
    let lines = request_lines();
    written_equals_committed("r4/requests.jsonl", &lines);

    let text = committed("r4/requests.jsonl");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 6);

    let Ok(Decoded::Raw(log)) = decode(lines[0].as_bytes()) else {
        panic!("line 1 is no request that decode returns undecoded");
    };
    assert_eq!(log.name(), "coordinator_write");
    assert_eq!(
        CoordLog::parse(log.body().get().as_bytes()).unwrap(),
        log_body()
    );

    let reply = decode_reply(lines[1].as_bytes()).unwrap();
    assert_eq!(reply.id.as_ref(), Some(&id()));
    let Ok(ReplyResult::Raw(result)) = reply.outcome else {
        panic!("line 2 is no reply with a raw result");
    };
    assert_eq!(
        Stored::parse(result.get().as_bytes()).unwrap(),
        Stored { stored: 3 }
    );

    for (line, name) in [(lines[2], "seq"), (lines[3], "binding")] {
        let reply = decode_reply(line.as_bytes()).unwrap();
        assert_eq!(reply.outcome.unwrap_err().as_str(), name);
    }

    let Ok(Decoded::Raw(history)) = decode(lines[4].as_bytes()) else {
        panic!("line 5 is no request that decode returns undecoded");
    };
    assert_eq!(
        CoordHistory::parse(history.body().get().as_bytes()).unwrap(),
        history_body()
    );
    let reply = decode_reply(lines[5].as_bytes()).unwrap();
    assert_eq!(reply.id.as_ref(), Some(&history_id()));
    let Ok(ReplyResult::Raw(result)) = reply.outcome else {
        panic!("line 6 is no reply with a raw result");
    };
    assert_eq!(
        History::parse(result.get().as_bytes()).unwrap(),
        history_reply()
    );

    let mut keys = BTreeSet::new();
    for line in &lines {
        keys_of(&serde_json::from_str(line).unwrap(), &mut keys);
    }
    for left_out in ["v", "id", "request", "body", "ok", "error"] {
        keys.remove(left_out);
    }
    let names = names_file();
    let missing: Vec<&String> = keys.difference(&names).collect();
    assert!(
        missing.is_empty(),
        "keys missing from names.txt: {missing:?}"
    );
}
