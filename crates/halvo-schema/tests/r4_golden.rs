use std::collections::BTreeSet;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

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
    let held = leaf(&format!("r4-golden-{file}"));
    let out = held.path().join(&file);
    std::fs::write(&out, &text).unwrap();
    let committed = std::fs::read(committed_path(name)).unwrap_or_default();
    if committed != text.as_bytes() {
        held.keep();
        panic!(
            "{name} differs from what the writers write, which stands at {}",
            out.display()
        );
    }
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

struct Leaf(PathBuf);

impl Leaf {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Leaf {
    fn drop(&mut self) {
        if let Some(line) = remove(&self.0) {
            eprintln!("{line}");
        }
    }
}

fn remove(dir: &Path) -> Option<String> {
    let error = std::fs::remove_dir_all(dir).err()?;
    Some(format!("leaf not removed: {}: {error}", dir.display()))
}

fn leaf_name(word: &str, pid: u32, clock: u128, count: u64) -> String {
    format!("{word}-{pid}-{clock}-{count}")
}

fn make_leaf(dir: PathBuf) -> Result<Leaf, String> {
    let parent = dir
        .parent()
        .ok_or_else(|| format!("no parent: {}", dir.display()))?;
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(Leaf(dir))
}

fn leaf(word: &str) -> Leaf {
    static CLOCK: OnceLock<u128> = OnceLock::new();
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let clock = *CLOCK.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    });
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    let name = leaf_name(word, std::process::id(), clock, count);
    make_leaf(Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)).unwrap_or_else(|e| panic!("{e}"))
}

impl Leaf {
    fn keep(self) {
        std::mem::forget(self);
    }
}

#[test]
fn leaf_two_calls_give_two_paths() {
    let first = leaf("r4-golden-leaf");
    let second = leaf("r4-golden-leaf");
    assert_ne!(first.path(), second.path());
    assert!(first.path().is_dir() && second.path().is_dir());
    let stem = |held: &Leaf| {
        let name = held
            .path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        name[..name.rfind('-').unwrap()].to_owned()
    };
    assert_eq!(stem(&first), stem(&second));
}

#[test]
fn leaf_name_differs_by_the_clock_reading_the_counter_and_the_process_id() {
    assert_ne!(leaf_name("w", 7, 1, 0), leaf_name("w", 7, 2, 0));
    assert_ne!(leaf_name("w", 7, 1, 0), leaf_name("w", 7, 1, 1));
    assert_ne!(leaf_name("w", 7, 1, 0), leaf_name("w", 8, 1, 0));
    assert!(leaf_name("w", 7, 1, 0).starts_with("w-7-"));
}

#[test]
fn leaf_that_exists_fails_with_its_path() {
    let first = leaf("r4-golden-leaf");
    let error = make_leaf(first.path().to_path_buf())
        .err()
        .expect("a second making of one leaf fails");
    assert!(
        error.contains(&first.path().display().to_string()),
        "{error}"
    );
}

#[test]
fn leaf_is_gone_after_its_drop() {
    let held = leaf("r4-golden-leaf");
    let path = held.path().to_path_buf();
    std::fs::write(path.join("file"), "x").unwrap();
    drop(held);
    assert!(!path.exists(), "{}", path.display());
}

#[test]
fn leaf_removal_that_fails_gives_one_line_with_the_path() {
    let held = leaf("r4-golden-leaf");
    let path = held.path().to_path_buf();
    drop(held);
    let line = remove(&path).expect("a line for a removal that fails");
    assert!(
        line.contains(&path.display().to_string()) && !line.contains('\n'),
        "{line}"
    );
}

#[test]
fn leaf_lies_under_the_target_directory() {
    let held = leaf("r4-golden-leaf");
    assert!(
        held.path().starts_with(env!("CARGO_TARGET_TMPDIR")),
        "{}",
        held.path().display()
    );
}

#[test]
fn leaf_of_a_file_that_differs_stands_after_the_failure() {
    let caught = std::panic::catch_unwind(|| {
        written_equals_committed("no-such-golden-file", &["x".to_owned()])
    });
    let message = caught
        .err()
        .and_then(|payload| payload.downcast::<String>().ok())
        .expect("a failure with a message");
    let out = kept_path(&message).unwrap_or_else(|| panic!("no kept leaf in: {message}"));
    let leaf = out.parent().unwrap().to_path_buf();
    let held = Leaf(leaf.clone());
    assert!(out.is_file(), "{message}");
    drop(held);
    assert!(!leaf.exists(), "{}", leaf.display());
}

fn kept_path(message: &str) -> Option<PathBuf> {
    let (_, shown) = message.split_once("which stands at ")?;
    let out = PathBuf::from(shown);
    let leaf = out.parent()?;
    let named = leaf.file_name()?.to_str()?.starts_with("r4-golden-");
    (named && leaf.parent()? == Path::new(env!("CARGO_TARGET_TMPDIR"))).then_some(out)
}

#[test]
fn leaf_kept_path_reads_only_a_leaf_of_the_comparison() {
    let tmp = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let leaf = tmp.join("r4-golden-x-1-2-0");
    let shown = |path: &Path| {
        format!(
            "x differs from what the writers write, which stands at {}",
            path.display()
        )
    };
    assert_eq!(kept_path(&shown(&leaf.join("x"))), Some(leaf.join("x")));
    let refused = format!("{}: File exists (os error 17)", leaf.display());
    assert_eq!(kept_path(&refused), None);
    assert_eq!(kept_path(&shown(&tmp.join("x"))), None);
    assert_eq!(
        kept_path(&shown(&tmp.join("r4-export-names-1-2-0").join("x"))),
        None
    );
    let nested = leaf.join("r4-golden-y-1-2-0").join("x");
    assert_eq!(kept_path(&shown(&nested)), None);
}

const LEAF_CHILD: &str = "HALVO_LEAF_CHILD";

fn leaf_child(test: &str, role: &str, nocapture: bool) -> std::process::Output {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([test, "--exact", "--test-threads=1"])
        .env(LEAF_CHILD, role);
    if nocapture {
        command.arg("--nocapture");
    }
    let out = command.output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("test result: ok. 1 passed;"),
        "{role}: {}\n{stdout}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn leaf_parts(path: &Path) -> (String, u32, u128, u64) {
    let name = path.file_name().unwrap().to_str().unwrap();
    let mut parts = name.rsplitn(4, '-');
    let count = parts.next().unwrap_or_default().parse();
    let clock = parts.next().unwrap_or_default().parse();
    let pid = parts.next().unwrap_or_default().parse();
    let word = parts.next().unwrap_or_default().to_owned();
    match (pid, clock, count) {
        (Ok(pid), Ok(clock), Ok(count)) => (word, pid, clock, count),
        _ => panic!("not word-pid-clock-count: {name}"),
    }
}

fn leaf_clock_now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

#[test]
fn leaf_from_leaf_holds_the_word_the_process_id_and_the_clock() {
    if std::env::var(LEAF_CHILD).as_deref() != Ok("clock") {
        leaf_child(
            "leaf_from_leaf_holds_the_word_the_process_id_and_the_clock",
            "clock",
            false,
        );
        return;
    }
    let before = leaf_clock_now();
    let held = leaf("r4-golden-leaf-clock");
    let after = leaf_clock_now();
    let (word, pid, clock, _) = leaf_parts(held.path());
    assert_eq!(word, "r4-golden-leaf-clock");
    assert_eq!(pid, std::process::id());
    assert!(
        before <= clock && clock <= after,
        "{before} {clock} {after}"
    );
}

#[test]
fn leaf_refuses_the_directory_that_the_next_leaf_names() {
    if std::env::var(LEAF_CHILD).as_deref() != Ok("next") {
        leaf_child(
            "leaf_refuses_the_directory_that_the_next_leaf_names",
            "next",
            false,
        );
        return;
    }
    let first = leaf("r4-golden-leaf-next");
    let (word, pid, clock, count) = leaf_parts(first.path());
    let next = first
        .path()
        .with_file_name(format!("{word}-{pid}-{clock}-{}", count + 1));
    std::fs::create_dir(&next).unwrap();
    let _next = Leaf(next.clone());
    std::fs::write(next.join("kept"), "kept\n").unwrap();
    let caught = std::panic::catch_unwind(|| leaf("r4-golden-leaf-next"));
    let message = caught
        .err()
        .and_then(|payload| payload.downcast::<String>().ok())
        .expect("leaf() fails on the directory that it names");
    assert!(message.contains(&next.display().to_string()), "{message}");
    assert_eq!(
        std::fs::read_to_string(next.join("kept")).unwrap(),
        "kept\n"
    );
    assert_eq!(std::fs::read_dir(&next).unwrap().count(), 1);
}

#[test]
fn leaf_whose_path_is_gone_drops_without_a_panic() {
    let held = leaf("r4-golden-leaf-gone");
    std::fs::remove_dir_all(held.path()).unwrap();
    let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || drop(held)));
    assert!(dropped.is_ok());
}

#[test]
fn leaf_of_a_comparison_that_matches_is_gone() {
    let lines: Vec<String> = committed("names.txt").lines().map(str::to_owned).collect();
    written_equals_committed("names.txt", &lines);
    let own = format!("r4-golden-names.txt-{}-", std::process::id());
    let left: Vec<String> = std::fs::read_dir(env!("CARGO_TARGET_TMPDIR"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(&own))
        .collect();
    assert!(left.is_empty(), "{left:?}");
}
