use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use halvo_control::client::{ClientError, Connector, DeleteOutcome, NoTransport};
use halvo_control::model::{StorageCap, StoreId};
use halvo_control::onboarding::{Permission, RequestOutcome};
use halvo_control::proto::{CapSet, DaemonStatus, ErrorCode, ErrorReply, InventoryReply};
use halvo_schema_core::request::{decode, Decoded, Read};
use halvo_schema_core::{Decision, WorkflowId};
use serde_json::{json, Value};

use super::control::{Control, Gated, Made, Places, MAX_CALLS_IN_FLIGHT, REPLY_BYTES_IN_FLIGHT};
use super::workflows::{decide, proposals, read, Listed, Row, WorkflowsView};

const STEP_TIMEOUT: Duration = Duration::from_secs(20);
const ABSENT_STEP_WAIT: Duration = Duration::from_millis(300);

#[derive(Debug, Default)]
struct Gate {
    connects: Vec<String>,
    released: usize,
}

#[derive(Debug, Clone, Default)]
struct HeldUntilReleased {
    gate: Arc<(Mutex<Gate>, Condvar)>,
}

impl HeldUntilReleased {
    fn gate(&self) -> std::sync::MutexGuard<'_, Gate> {
        self.gate.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn connects(&self) -> Vec<String> {
        self.gate().connects.clone()
    }

    fn release_one(&self) {
        self.gate().released += 1;
        self.gate.1.notify_all();
    }

    fn wait_for_connects(&self, n: usize, limit: Duration) -> bool {
        let until = Instant::now() + limit;
        let mut gate = self.gate();
        while gate.connects.len() < n {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            gate = self
                .gate
                .1
                .wait_timeout(gate, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }
}

impl Connector for HeldUntilReleased {
    type Transport = NoTransport;

    fn endpoint(&self) -> String {
        "held".to_string()
    }

    fn connect(&self, _: Duration) -> Result<NoTransport, ClientError> {
        let name = std::thread::current().name().unwrap_or("-").to_string();
        let mut gate = self.gate();
        gate.connects.push(name);
        self.gate.1.notify_all();
        let until = Instant::now() + STEP_TIMEOUT;
        while gate.released == 0 && Instant::now() < until {
            let left = until.saturating_duration_since(Instant::now());
            gate = self
                .gate
                .1
                .wait_timeout(gate, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        gate.released = gate.released.saturating_sub(1);
        Err(ClientError::Unreachable {
            endpoint: "held".to_string(),
        })
    }
}

type Call = JoinHandle<Result<halvo_control::proto::DaemonStatus, ClientError>>;

fn status_call(gated: &Arc<Gated<HeldUntilReleased>>, name: &str) -> Call {
    let made = gated.places().make();
    let gated = Arc::clone(gated);
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(move || gated.status(made, false))
        .expect("a thread starts")
}

fn four_in_flight() -> (HeldUntilReleased, Arc<Gated<HeldUntilReleased>>, Vec<Call>) {
    let held = HeldUntilReleased::default();
    let gated = Arc::new(Gated::new(held.clone()));
    let calls = (0..4)
        .map(|n| status_call(&gated, &format!("in-flight-{n}")))
        .collect();
    assert!(
        held.wait_for_connects(4, STEP_TIMEOUT),
        "four calls connect"
    );
    assert_eq!(gated.places().in_flight(), 4);
    (held, gated, calls)
}

fn release_all(held: &HeldUntilReleased, calls: Vec<Call>) {
    for _ in 0..calls.len() + 2 {
        held.release_one();
    }
    for call in calls {
        let _ = call.join().expect("a call ends");
    }
}

#[test]
fn a_fifth_call_waits_until_one_returns() {
    assert_eq!(MAX_CALLS_IN_FLIGHT, 4);
    assert_eq!(halvo_control::wire::REPLY_MAX_BYTES, 262_144);
    assert_eq!(REPLY_BYTES_IN_FLIGHT, 1_048_576);
    let (held, gated, mut calls) = four_in_flight();
    calls.push(status_call(&gated, "fifth"));
    assert!(
        !held.wait_for_connects(5, ABSENT_STEP_WAIT),
        "a fifth call connected while four were in flight: {:?}",
        held.connects()
    );
    assert_eq!(gated.places().in_flight(), 4);
    assert_eq!(gated.places().waiting(), 1);
    held.release_one();
    assert!(
        held.wait_for_connects(5, STEP_TIMEOUT),
        "the fifth call connects once one returns"
    );
    assert_eq!(held.connects()[4], "fifth");
    release_all(&held, calls);
    assert_eq!(gated.places().in_flight(), 0);
    assert_eq!(gated.places().waiting(), 0);
}

#[test]
fn waiting_calls_take_the_free_places_in_the_order_in_which_they_were_made() {
    let (held, gated, mut calls) = four_in_flight();
    let first = gated.places().make();
    let second = gated.places().make();
    let third = gated.places().make();
    let spawn = |made, name: &str| {
        let gated = Arc::clone(&gated);
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || gated.status(made, false))
            .expect("a thread starts")
    };
    calls.push(spawn(third, "third"));
    calls.push(spawn(second, "second"));
    calls.push(spawn(first, "first"));
    assert!(!held.wait_for_connects(5, ABSENT_STEP_WAIT));
    for n in 5..=7 {
        held.release_one();
        assert!(held.wait_for_connects(n, STEP_TIMEOUT), "call {n} connects");
    }
    assert_eq!(held.connects()[4..], ["first", "second", "third"]);
    release_all(&held, calls);
}

#[test]
fn the_deadline_of_a_call_runs_from_the_moment_it_is_made_and_counts_the_wait() {
    let (held, gated, calls) = four_in_flight();
    let made = gated.places().make();
    let started = Instant::now();
    let answer = gated.status(made, false);
    let waited = started.elapsed();
    assert!(
        matches!(answer, Err(ClientError::Deadline { seconds: 2 })),
        "{answer:?}"
    );
    assert!(
        waited >= Duration::from_millis(2_000)
            && waited < Duration::from_millis(2_000) + STEP_TIMEOUT,
        "waited {waited:?}"
    );
    assert_eq!(held.connects().len(), 4, "the late call never connected");
    assert_eq!(gated.places().waiting(), 0, "the late call left the line");
    release_all(&held, calls);
}

#[test]
fn a_call_that_is_made_and_dropped_before_it_enters_leaves_the_line() {
    let (held, gated, mut calls) = four_in_flight();
    let dropped = gated.places().make();
    calls.push(status_call(&gated, "after"));
    drop(dropped);
    held.release_one();
    assert!(
        held.wait_for_connects(5, STEP_TIMEOUT),
        "the call after the dropped one connects"
    );
    assert_eq!(held.connects()[4], "after");
    release_all(&held, calls);
}

#[derive(Debug, Clone, Default)]
struct Timeouts(Arc<Mutex<Vec<Duration>>>);

impl Connector for Timeouts {
    type Transport = NoTransport;

    fn endpoint(&self) -> String {
        "timeouts".to_string()
    }

    fn connect(&self, timeout: Duration) -> Result<NoTransport, ClientError> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(timeout);
        Err(ClientError::Unreachable {
            endpoint: "timeouts".to_string(),
        })
    }
}

#[test]
fn the_client_gets_only_what_is_left_of_the_deadline_after_the_wait() {
    const AGED: Duration = Duration::from_millis(600);
    let timeouts = Timeouts::default();
    let gated = Gated::new(timeouts.clone());
    let made = gated.places().make();
    let aged_until = Instant::now() + AGED;
    while Instant::now() < aged_until {
        std::thread::park_timeout(aged_until.saturating_duration_since(Instant::now()));
    }
    let _ = gated.status(made, false);
    let seen = timeouts.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert_eq!(seen.len(), 1);
    assert!(
        seen[0] <= Duration::from_millis(2_000) - AGED,
        "the connect got {:?} of a 2 s deadline whose call was made {AGED:?} earlier",
        seen[0]
    );
}

fn hex(byte: u8) -> String {
    format!("{byte:02x}").repeat(16)
}

fn listed(id: u8, proposed: bool, category: &str, first_summary: u8, count: u8) -> Listed {
    Listed {
        id: hex(id),
        proposed,
        category: category.to_string(),
        app: None,
        site: None,
        summaries: (first_summary..first_summary + count).map(hex).collect(),
    }
}

#[test]
fn the_list_holds_at_most_four_proposed_workflows_in_the_ruled_order() {
    let six = [
        listed(0x11, true, "tenant_rent", 0xe0, 2),
        listed(0x22, true, "shop_order", 0x30, 3),
        listed(0x33, false, "utility_bill_payment", 0x40, 9),
        listed(0x44, true, "tenant_rent", 0xb0, 2),
        listed(0x55, true, "email", 0x50, 5),
        listed(0x66, true, "utility_bill_payment", 0x60, 3),
    ];
    let ids: Vec<String> = proposals(&six).iter().map(|w| w.id.clone()).collect();
    assert_eq!(ids, [hex(0x55), hex(0x66), hex(0x22), hex(0x44)]);
}

/// A control whose relay hands out the answers that a test gave it, in order, and keeps the
/// bytes of each request.
#[derive(Default)]
struct Relayed {
    places: Arc<Places>,
    answers: Mutex<VecDeque<Result<Vec<u8>, ClientError>>>,
    requests: Mutex<Vec<Vec<u8>>>,
}

impl Relayed {
    fn answering(answers: Vec<Result<Vec<u8>, ClientError>>) -> Self {
        Relayed {
            answers: Mutex::new(answers.into()),
            ..Relayed::default()
        }
    }

    fn requests(&self) -> Vec<Vec<u8>> {
        self.requests.lock().expect("the requests").clone()
    }
}

impl Control for Relayed {
    fn places(&self) -> &Arc<Places> {
        &self.places
    }

    fn status(&self, _: Made, _: bool) -> Result<DaemonStatus, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn status_of_a_pass(
        &self,
        made: Made,
        claim: bool,
        hold: &crate::view_state::Hold,
    ) -> Result<DaemonStatus, ClientError> {
        self.status(made, claim && !hold.stands())
    }

    fn inventory(&self, _: Made) -> Result<InventoryReply, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn pause(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn resume(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn delete_all(&self, _: Made, _: Option<StoreId>) -> Result<DeleteOutcome, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn set_storage_cap(&self, _: Made, _: StorageCap) -> Result<CapSet, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn request_permission(&self, _: Made, _: Permission) -> Result<RequestOutcome, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn relay(&self, _: Made, request: &[u8]) -> Result<Vec<u8>, ClientError> {
        self.requests
            .lock()
            .expect("the requests")
            .push(request.to_vec());
        self.answers
            .lock()
            .expect("the answers")
            .pop_front()
            .unwrap_or(Err(ClientError::Unsupported))
    }

    #[cfg(any(target_os = "macos", windows))]
    fn owner_stream(&self, _: Made) -> Result<halvo_control::r2::OwnerStreamEnd, ClientError> {
        Err(ClientError::Unsupported)
    }

    #[cfg(any(target_os = "macos", windows))]
    fn run_mark(
        &self,
        _: Made,
        _: halvo_control::r2::RunEdge,
    ) -> Result<halvo_control::r2::RunMarked, ClientError> {
        Err(ClientError::Unsupported)
    }

    #[cfg(any(target_os = "macos", windows))]
    fn defer(&self, _: Made, _: i64) -> Result<Option<i64>, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn acknowledge_lost_permission(
        &self,
        _: Made,
        _: Permission,
    ) -> Result<DaemonStatus, ClientError> {
        Err(ClientError::Unsupported)
    }

    fn reset_settings(&self, _: Made) -> Result<DaemonStatus, ClientError> {
        Err(ClientError::Unsupported)
    }
}

fn owner_reply(key: &str, value: Value) -> Result<Vec<u8>, ClientError> {
    let mut reply = json!({"v": 1, "id": hex(0xaa)});
    reply[key] = value;
    Ok(serde_json::to_vec(&reply).expect("a reply serializes"))
}

fn page_item(id: u8, state: &str, category: &str, app: &str, summaries: &[u8]) -> Value {
    json!({
        "workflow": {
            "v": 1,
            "id": hex(id),
            "state": state,
            "category": category,
            "site": null,
            "app": app,
            "trigger": "schedule",
            "recurrence": {"count": 3, "period": 31, "first": 20705, "last": 20767},
            "confidence": 87,
            "agent": null,
        },
        "summaries": summaries.iter().copied().map(hex).collect::<Vec<_>>(),
        "hint": null,
    })
}

fn page(items: Vec<Value>, next: Option<u8>) -> Result<Vec<u8>, ClientError> {
    owner_reply("ok", json!({"workflows": items, "next": next.map(hex)}))
}

/// A stand-in of the system's function that names no app.
fn no_name(_: &str) -> Option<String> {
    None
}

fn daemon_refuses(code: ErrorCode) -> Result<Vec<u8>, ClientError> {
    Err(ClientError::Daemon(ErrorReply::new(code, "refused")))
}

/// The reply of a command as the page gets it.
fn as_the_page_gets_it<T: serde::Serialize, E: serde::Serialize>(reply: Result<T, E>) -> Value {
    match reply {
        Ok(value) => serde_json::to_value(value),
        Err(error) => serde_json::to_value(error),
    }
    .expect("a reply serializes")
}

#[test]
fn workflows_reads_every_page_and_lists_what_the_owner_holds() {
    let control = Relayed::answering(vec![
        page(
            vec![page_item(
                0x11,
                "proposed",
                "email",
                "com.example.mail",
                &[0x50, 0x51],
            )],
            Some(0x11),
        ),
        page(
            vec![page_item(
                0x22,
                "approved",
                "shop_order",
                "com.example.browser",
                &[0x60],
            )],
            None,
        ),
    ]);
    let reply = read(&control, &no_name);
    let afters: Vec<Option<WorkflowId>> = control
        .requests()
        .iter()
        .map(|bytes| match decode(bytes) {
            Ok(Decoded::InterfaceRead(request)) => match request.read() {
                Read::Workflows { after } => after.clone(),
                other => panic!("the request reads {other:?}"),
            },
            other => panic!("the request is no interface read: {other:?}"),
        })
        .collect();
    let next_of_the_first_page = WorkflowId::parse(&hex(0x11)).expect("a workflow id");
    assert_eq!(afters, [None, Some(next_of_the_first_page)]);
    let row = |id, category, count| Row {
        id: hex(id),
        category,
        app: None,
        site: None,
        count,
    };
    assert_eq!(
        reply,
        Ok(WorkflowsView::Running {
            proposals: vec![row(0x11, "email", 2)],
            stored: vec![row(0x11, "email", 2), row(0x22, "shop_order", 1)],
        })
    );
}

#[test]
fn each_answer_without_a_list_gives_its_state() {
    let cases = [
        (
            Err(ClientError::Unreachable {
                endpoint: "none".to_string(),
            }),
            json!({"state": "not_running"}),
        ),
        (
            Err(ClientError::Unsupported),
            json!({"state": "not_running"}),
        ),
        (
            daemon_refuses(ErrorCode::UnsupportedOp),
            json!({"state": "not_running"}),
        ),
        (
            daemon_refuses(ErrorCode::UnsupportedVersion),
            json!({"state": "not_running"}),
        ),
        (
            daemon_refuses(ErrorCode::StoreUnavailable),
            json!({"state": "store_unavailable"}),
        ),
        (
            daemon_refuses(ErrorCode::Busy),
            json!({"state": "deleting"}),
        ),
        (
            Ok(b"no reply of the owner".to_vec()),
            json!({"error": "broken_reply"}),
        ),
    ];
    for (answer, reply) in cases {
        let case = format!("{answer:?}");
        let control = Relayed::answering(vec![answer]);
        assert_eq!(
            as_the_page_gets_it(read(&control, &no_name)),
            reply,
            "{case}"
        );
        assert_eq!(control.requests().len(), 1, "{case}");
    }
}

#[test]
fn a_decision_sends_one_request_with_the_workflow_id_alone() {
    let workflow = hex(0x33);
    for decision in [Decision::Approve, Decision::Dismiss, Decision::Delete] {
        let control = Relayed::answering(vec![owner_reply("ok", Value::Null)]);
        assert_eq!(decide(&control, decision, &workflow), Ok(()), "{decision}");
        let requests = control.requests();
        assert_eq!(requests.len(), 1, "{decision}");
        let request: Value = serde_json::from_slice(&requests[0]).expect("a request is JSON");
        assert_eq!(request["request"], json!("interface_decision"));
        assert_eq!(
            request["body"],
            json!({"decision": decision.as_str(), "workflow": workflow}),
        );
    }
    let gone = Relayed::answering(vec![owner_reply("error", json!("no_workflow"))]);
    assert_eq!(decide(&gone, Decision::Approve, &workflow), Ok(()));
    let held = Relayed::answering(vec![owner_reply("error", json!("state"))]);
    assert_eq!(
        as_the_page_gets_it(decide(&held, Decision::Delete, &workflow)),
        json!({"error": "in_use"}),
    );
    for decision in [Decision::Approve, Decision::Dismiss] {
        let no_longer_proposed = Relayed::answering(vec![owner_reply("error", json!("state"))]);
        assert_eq!(
            decide(&no_longer_proposed, decision, &workflow),
            Ok(()),
            "{decision}"
        );
    }
    let no_daemon = Err(ClientError::Unreachable {
        endpoint: "none".to_string(),
    });
    for answer in [no_daemon, daemon_refuses(ErrorCode::UnsupportedOp)] {
        let case = format!("{answer:?}");
        let control = Relayed::answering(vec![answer]);
        assert_eq!(
            as_the_page_gets_it(decide(&control, Decision::Approve, &workflow)),
            json!({"error": "not_running"}),
            "{case}"
        );
    }
}

#[test]
fn a_row_names_the_app_that_the_system_names_and_no_app_id_and_no_day() {
    let (named, unnamed) = ("com.apple.Safari", "com.example.unnamed");
    let control = Relayed::answering(vec![page(
        vec![
            page_item(0x44, "proposed", "ride_order", named, &[0x70]),
            page_item(0x55, "proposed", "email", unnamed, &[0x71]),
        ],
        None,
    )]);
    let system = |app: &str| (app == named).then(|| "Safari".to_string());
    let reply = as_the_page_gets_it(read(&control, &system));
    assert!(!reply.to_string().contains(named), "{reply}");
    assert!(!reply.to_string().contains(unnamed), "{reply}");
    let rows: Vec<&Value> = ["proposals", "stored"]
        .iter()
        .flat_map(|list| reply[*list].as_array().expect("a list of rows"))
        .collect();
    assert_eq!(rows.len(), 4, "{reply}");
    for row in rows {
        let mut keys: Vec<&str> = row
            .as_object()
            .expect("a row is an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["app", "category", "count", "id", "site"]);
        let app = match row["id"].as_str() {
            Some(id) if id == hex(0x44) => json!("Safari"),
            Some(id) if id == hex(0x55) => Value::Null,
            _ => panic!("a row of no workflow of the page: {row}"),
        };
        assert_eq!(row["app"], app, "{reply}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn an_apps_names_are_read_only_where_it_lies_under_applications_or_system() {
    let read = |place: &str| super::workflows::names_readable_at(std::path::Path::new(place));
    for place in [
        "/Applications/Safari.app",
        "/System/Applications/Mail.app",
        "/System/Volumes/Preboot/Cryptexes/App/System/Applications/Safari.app",
    ] {
        assert!(read(place), "{place} is not read");
    }
    for place in [
        "/Users/someone/Downloads/Tool.app",
        "/Users/someone/Desktop/Tool.app",
        "/Users/someone/Documents/Tool.app",
        "/Users/someone/Applications/Tool.app",
        "/Volumes/Disk/Tool.app",
        "/Applications/../Users/someone/Downloads/Tool.app",
        "/System/Volumes/Data/Users/someone/Downloads/Tool.app",
        "/System/Volumes/Data/Users/someone/Applications/Tool.app",
        "/System/Volumes/Data/Applications/Tool.app",
    ] {
        assert!(!read(place), "{place} is read");
    }
}

/// The daemon's end of the control channel as a stand-in: each connection keeps the bytes that
/// the client writes and reads `reply` as one frame.
#[derive(Clone)]
struct OtherEnd {
    written: Arc<Mutex<Vec<u8>>>,
    reply: Vec<u8>,
}

struct OtherEndStream {
    written: Arc<Mutex<Vec<u8>>>,
    reply: std::io::Cursor<Vec<u8>>,
}

impl std::io::Read for OtherEndStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.reply, buf)
    }
}

impl std::io::Write for OtherEndStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.written
            .lock()
            .expect("the written bytes")
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl halvo_control::client::Transport for OtherEndStream {
    fn set_timeout(&mut self, _: Duration) -> std::io::Result<()> {
        Ok(())
    }

    fn end_request(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Connector for OtherEnd {
    type Transport = OtherEndStream;

    fn endpoint(&self) -> String {
        "the other end".to_string()
    }

    fn connect(&self, _: Duration) -> Result<OtherEndStream, ClientError> {
        let mut framed = Vec::new();
        halvo_control::wire::write_frame(
            &mut framed,
            &self.reply,
            halvo_control::wire::REPLY_MAX_BYTES,
        )
        .expect("the reply fits a frame");
        Ok(OtherEndStream {
            written: Arc::clone(&self.written),
            reply: std::io::Cursor::new(framed),
        })
    }
}

#[test]
fn the_gated_control_relays_one_request() {
    use halvo_schema_core::request::{InterfaceRead, InterfaceRole};
    let request = InterfaceRead::workflows(
        &InterfaceRole::claim(),
        halvo_schema_core::RequestId::from_random([0x5a; 16]),
        None,
    )
    .to_bytes()
    .expect("an interface read serializes");
    let reply = page(Vec::new(), None).expect("a reply of the owner");
    let other_end = OtherEnd {
        written: Arc::default(),
        reply: halvo_control::r2::Relayed::encode(&reply).expect("a frame of relayed"),
    };
    let gated = Gated::new(other_end.clone());
    let made = gated.places().make();
    assert_eq!(
        gated.relay(made, &request).expect("the owner's reply"),
        reply
    );
    let mut sent = Vec::new();
    halvo_control::wire::write_frame(
        &mut sent,
        &halvo_control::r2::Relay::encode(&request).expect("a frame of relay"),
        halvo_control::wire::REQUEST_MAX_BYTES,
    )
    .expect("the request fits a frame");
    assert_eq!(*other_end.written.lock().expect("the written bytes"), sent);
    assert_eq!(gated.places().in_flight(), 0);

    let no_daemon = Gated::new(Timeouts::default());
    assert_eq!(
        as_the_page_gets_it(read(&no_daemon, &no_name)),
        json!({"state": "not_running"})
    );
}

/// U5.4w, sentence 2: on Windows as on a Mac, the gated control sends the request of a run mark's
/// edge over its connector and gives the reply `run_marked`.
#[cfg(any(target_os = "macos", windows))]
#[test]
fn the_gated_control_sends_the_run_mark_of_an_edge_and_gives_its_reply() {
    use halvo_control::r2::{RunEdge, RunMarked};
    let marked = RunMarked {
        edge: RunEdge::Start,
        stored: true,
    };
    let other_end = OtherEnd {
        written: Arc::default(),
        reply: marked.encode(),
    };
    let gated = Gated::new(other_end.clone());
    let made = gated.places().make();
    assert_eq!(
        gated.run_mark(made, RunEdge::Start).expect("the reply"),
        marked
    );
    let mut sent = Vec::new();
    halvo_control::wire::write_frame(
        &mut sent,
        &RunEdge::Start.request(),
        halvo_control::wire::REQUEST_MAX_BYTES,
    )
    .expect("the request fits a frame");
    assert_eq!(*other_end.written.lock().expect("the written bytes"), sent);
}
