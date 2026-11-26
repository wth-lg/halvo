//! The control server of a service run (contracts.md C2.1, C3.2 step 6). It
//! serves protocol v1, one request for each connection, to peers of the
//! daemon's own user alone, and hands each request that it admits to the
//! service run's loop, which answers it between two turns of its main loop.

#[cfg(target_os = "macos")]
pub mod unix;
#[cfg(windows)]
pub mod windows;

#[cfg(target_os = "macos")]
pub use unix::Listening;
#[cfg(windows)]
pub use windows::Listening;

use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use halvo_control::model::{CaptureState, Modality, UtcSecond};
use halvo_control::onboarding::{
    self, AssessInput, GrantState, Grants, OnboardingState, Permission, PermissionSet,
    WalkbackAction, WalkbackStep,
};
use halvo_control::proto::{
    self, Admission, CaptureReason, Custody, DaemonStatus, DaemonVersion, Decoded, DrainError,
    ErrorCode, ErrorReply, Health, KindLine, LastStop, Life, LostRecord, OnOff, Parent,
    PasswordGuard, PermissionLine, Platform, Reply, Request, SourceLine, StatusError, StatusFields,
    StoreErrorKind, StoreState, SwitchLine, SwitchName, PERMISSION_FORM,
};
use halvo_control::wire::{self, REPLY_MAX_BYTES, REQUEST_MAX_BYTES};

use super::board::{Board, Capture as BoardCapture};
use super::slot::Slot;
use super::state::{Cap as StateCap, Desired, SharedStateFile, StateHealth};
use crate::floor::{Switch, Switches};

/// How long the server waits for each read of a request (contracts.md C2.1).
pub const READ_DEADLINE: Duration = Duration::from_secs(2);

/// How long the server may take to write a reply: REPLY_WRITE_DEADLINE_MS of
/// contracts.md C2.1.
pub const REPLY_WRITE_DEADLINE: Duration = Duration::from_millis(2_000);

/// The longest that a connection waits for the service run's loop to answer
/// the request that it handed over: the deadline of `delete_all`, the longest
/// deadline of a client of this build's requests.
pub const ANSWER_WAIT: Duration = halvo_control::client::Deadlines::V1.delete_all;

/// Whether the server serves a peer whose effective user id is `peer`, in a
/// daemon whose own effective user id is `own`: only when the two are one.
pub fn serves_peer(peer: u32, own: u32) -> bool {
    peer == own
}

/// What the connections of one server share with the service run's loop: the
/// mutation in flight and the life (contracts.md C2.1, ruling PL-17).
#[derive(Debug, Default)]
pub struct Admitting {
    in_flight: AtomicBool,
    life: Arc<EndingLife>,
    #[cfg(feature = "r2")]
    owner: std::sync::Mutex<Option<super::owner::OwnerHandle>>,
}

impl Admitting {
    /// The store owner that a mark request reaches: the owner of the open
    /// store, or `None` while no store is open for capture.
    #[cfg(feature = "r2")]
    pub fn hold_owner(&self, owner: Option<super::owner::OwnerHandle>) {
        *self
            .owner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = owner;
    }

    /// Marks the life as ending: from now on every mutation but a Pause and a
    /// Resume answers the busy of an ending life.
    pub fn end_the_life(&self) {
        self.life.end_the_life();
    }

    pub(crate) fn life(&self) -> Life {
        if self.life.ending.load(Ordering::SeqCst) {
            Life::Ending
        } else {
            Life::Going
        }
    }

    /// The life that the loop and the supervisor share with the connections.
    pub fn ending_life(&self) -> Arc<EndingLife> {
        self.life.clone()
    }
}

/// The life of a service run as its connections, its loop and its
/// supervisor share it (ruling PL-17): whether it is ending, and what a
/// connection answers from in a life that is ending, while the loop serves no
/// request: the state file through its one writer, the facts of the run and
/// the parts of the status as the supervisor last published them. There a
/// Pause writes desired paused first, where desired is not paused yet, and
/// answers `capture_changed` with the status as it stands; a Resume answers
/// the status as it stands and writes nothing; a claim and a Status answer
/// the status and start nothing.
#[derive(Default)]
pub struct EndingLife {
    ending: AtomicBool,
    state: Mutex<Option<SharedStateFile>>,
    facts: Mutex<Option<RunFacts>>,
    parts: Mutex<Option<StatusParts>>,
}

impl std::fmt::Debug for EndingLife {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EndingLife")
            .field("ending", &self.ending)
            .finish_non_exhaustive()
    }
}

impl EndingLife {
    /// Marks the life as ending.
    pub fn end_the_life(&self) {
        self.ending.store(true, Ordering::SeqCst);
    }

    /// The state file and the facts of the run, from which a connection of
    /// an ending life answers.
    pub fn serve_from(&self, state: Option<SharedStateFile>, facts: RunFacts) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = state;
        *self.facts.lock().unwrap_or_else(PoisonError::into_inner) = Some(facts);
    }

    /// The parts of the status as the supervisor publishes them.
    pub fn publish(&self, parts: StatusParts) {
        *self.parts.lock().unwrap_or_else(PoisonError::into_inner) = Some(parts);
    }

    /// The answer of an ending life to `request`, which the admission
    /// answered `admission`: none when the life is not ending, when the
    /// request is not a Pause, a Resume, a claim or a Status, or when the
    /// loop has not given the life its state file and its facts yet.
    pub fn answer(&self, admission: Admission, request: &Request) -> Option<Reply> {
        if !self.ending.load(Ordering::SeqCst) {
            return None;
        }
        let keeps_the_word = match (admission, request) {
            (Admission::PauseKeepingTheWord, _) => true,
            (Admission::StatusAsItStands | Admission::ClaimStartingNothing, _)
            | (Admission::Serve, Request::Status { .. }) => false,
            _ => return None,
        };
        let facts = (*self.facts.lock().unwrap_or_else(PoisonError::into_inner))?;
        let state = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()?;
        let mut parts = self
            .parts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()?;
        {
            let mut file = state.lock().unwrap_or_else(PoisonError::into_inner);
            if keeps_the_word && file.state().desired != Desired::Paused {
                let _ = file.update(|state| state.desired = Desired::Paused);
            }
            parts.health = file.health();
            if keeps_the_word || file.state().desired == Desired::Paused {
                parts.desired = Desired::Paused;
            }
        }
        #[cfg(any(target_os = "macos", windows))]
        let facts = RunFacts {
            rss_peak_bytes: rss_peak_bytes(),
            ..facts
        };
        Some(match (status_of(&parts, &facts), request) {
            (Err(broken), _) => error(ErrorCode::Internal, broken.rule()),
            (Ok(status), Request::Status { .. }) => Reply::Status(status),
            (Ok(status), _) => Reply::CaptureChanged(status),
        })
    }
}

/// A request that the server admitted, handed to the service run's loop.
pub struct Asked {
    pub request: Request,
    reply: Sender<Reply>,
    written: Receiver<()>,
    admitting: Arc<Admitting>,
}

impl Asked {
    /// Hands `reply` to the connection; a mutation that it answers is in
    /// flight no longer. The answer tells when the connection has written the
    /// reply and closed.
    pub fn answer(self, reply: Reply) -> Written {
        let _ = self.reply.send(reply);
        if self.request.is_mutation() {
            self.admitting.in_flight.store(false, Ordering::SeqCst);
        }
        Written(self.written)
    }
}

/// The end of a connection whose request the loop answered.
pub struct Written(Receiver<()>);

impl Written {
    /// Waits at most `bound` for the connection to write its reply, shut down
    /// its writing side and close; true when it did.
    pub fn wait(&self, bound: Duration) -> bool {
        self.0.recv_timeout(bound).is_ok()
    }
}

/// How the request of a connection ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestEnd {
    /// The frame, then the end of the stream: the client of a Unix socket
    /// shuts down its writing side.
    EndOfStream,
    /// The frame's length alone: a byte-mode pipe has no half-close
    /// (contracts.md C15.4).
    Length,
}

/// What the stream of a connection does with a request that the v1 table does
/// not hold and that is no run mark. The server asks it, with the feature
/// `r2`, before it answers such a request as v1 does.
pub trait ServesUnknown {
    /// Serves `request` on the stream itself, where the stream is one that a
    /// later protocol version's request is answered on: true when the request
    /// was answered there, false when the server is to answer it as v1 does.
    fn serve_unknown(&mut self, _request: &proto::UnknownRequest) -> bool {
        false
    }
}

/// Serves the one request of a connection on `stream`, whose peer has the
/// effective user id `peer` (`None` when it could not be read), in a daemon
/// whose own id is `own`. A peer of another user, or one whose id could not
/// be read, gets the error `peer_rejected` and no read of its request. Any
/// other peer is served by [`serve_request`], whose known request ends at the
/// end of its stream.
pub fn serve_connection<S: Read + Write + ServesUnknown>(
    stream: &mut S,
    peer: Option<u32>,
    own: u32,
    admitting: &Arc<Admitting>,
    hand: &Sender<Asked>,
) -> Option<Sender<()>> {
    if !peer.is_some_and(|peer| serves_peer(peer, own)) {
        write_reply(
            stream,
            &error(
                ErrorCode::PeerRejected,
                "the peer is not a process of the capture service's own user",
            ),
        );
        return None;
    }
    serve_request(stream, RequestEnd::EndOfStream, admitting, hand)
}

/// Serves the one request of a connection on `stream` whose peer was
/// admitted. A request that the v1 table does not know is answered at its
/// whole frame; a known one once it has ended as `ends` says. With the
/// feature `r2`, a request that the v1 table does not know and that is no run
/// mark goes to the stream first ([`ServesUnknown`]), with no wait for the
/// end of the stream before it, and is answered as v1 answers it only where
/// the stream did not answer it. An admitted request goes to the loop through
/// `hand` and its answer is written. The answer is the sender that the caller
/// signals once it has ended its writing side and closed the connection, when
/// the loop answered.
pub fn serve_request<S: Read + Write + ServesUnknown>(
    stream: &mut S,
    ends: RequestEnd,
    admitting: &Arc<Admitting>,
    hand: &Sender<Asked>,
) -> Option<Sender<()>> {
    let request = match read_request(stream, ends) {
        Got::Request(request) => request,
        Got::Answer(reply) => {
            write_reply(stream, &reply);
            return None;
        }
        #[cfg(feature = "r2")]
        Got::Mark(edge) => {
            if wire::write_frame(stream, &marked(edge, admitting), REPLY_MAX_BYTES).is_ok() {
                let _ = stream.flush();
            }
            return None;
        }
        #[cfg(feature = "r2")]
        Got::Unknown(unknown) => {
            if !stream.serve_unknown(&unknown) {
                write_reply(stream, &proto::unknown_answer(&unknown));
            }
            return None;
        }
    };
    let admission = match refusal(&request, admitting) {
        Ok(admission) => admission,
        Err(reply) => {
            write_reply(stream, &reply);
            return None;
        }
    };
    if let Some(reply) = admitting.life.answer(admission, &request) {
        write_reply(stream, &reply);
        if request.is_mutation() {
            admitting.in_flight.store(false, Ordering::SeqCst);
        }
        return None;
    }
    let (reply, answer) = mpsc::channel();
    let (written, written_rx) = mpsc::channel();
    let asked = Asked {
        request,
        reply,
        written: written_rx,
        admitting: admitting.clone(),
    };
    if hand.send(asked).is_err() {
        write_reply(
            stream,
            &error(ErrorCode::Internal, "the capture service is not serving"),
        );
        return None;
    }
    let reply = answer.recv_timeout(ANSWER_WAIT).ok()?;
    write_reply(stream, &reply);
    Some(written)
}

enum Got {
    Request(Request),
    Answer(Box<Reply>),
    #[cfg(feature = "r2")]
    Mark(halvo_control::r2::RunEdge),
    #[cfg(feature = "r2")]
    Unknown(proto::UnknownRequest),
}

#[cfg(feature = "r2")]
const MARK_WAIT: Duration = Duration::from_millis(1_500);

#[cfg(feature = "r2")]
fn marked(edge: halvo_control::r2::RunEdge, admitting: &Admitting) -> Vec<u8> {
    use super::owner::{Answer, Command};
    let owner = admitting
        .owner
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let stored = owner
        .and_then(|owner| owner.ask(Command::Mark(edge)).ok())
        .and_then(|pending| pending.wait_for(MARK_WAIT))
        .is_some_and(|answer| answer == Answer::Marked(true));
    halvo_control::r2::RunMarked { edge, stored }.encode()
}

fn read_request<S: Read>(stream: &mut S, ends: RequestEnd) -> Got {
    let Ok(body) = wire::read_frame(stream, REQUEST_MAX_BYTES) else {
        return Got::Answer(Box::new(error(
            ErrorCode::InvalidArgument,
            "the request is not one whole frame",
        )));
    };
    match proto::decode_request(&body) {
        Err(refused) => Got::Answer(Box::new(refused.answer())),
        Ok(Decoded::Unknown(unknown)) => {
            #[cfg(feature = "r2")]
            match halvo_control::r2::mark_request(&unknown) {
                Some(Ok(edge)) => Got::Mark(edge),
                Some(Err(refused)) => Got::Answer(Box::new(Reply::Error(refused))),
                None => Got::Unknown(unknown),
            }
            #[cfg(not(feature = "r2"))]
            Got::Answer(Box::new(proto::unknown_answer(&unknown)))
        }
        Ok(Decoded::Known(request)) if ends == RequestEnd::Length || at_the_end(stream) => {
            Got::Request(request)
        }
        Ok(Decoded::Known(_)) => Got::Answer(Box::new(error(
            ErrorCode::InvalidArgument,
            "the request is not one frame followed by the end of its stream",
        ))),
    }
}

fn at_the_end<S: Read>(stream: &mut S) -> bool {
    let mut probe = [0u8; 1];
    loop {
        match stream.read(&mut probe) {
            Ok(0) => return true,
            Ok(_) => return false,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
    }
}

fn served_by_this_build(request: &Request) -> bool {
    match request {
        Request::Status { .. }
        | Request::Inventory
        | Request::Pause
        | Request::Resume
        | Request::Stop
        | Request::DeleteAll { .. }
        | Request::SetStorageCap { .. }
        | Request::ResetSettings
        | Request::SetTyping { .. }
        | Request::SetSearch { .. } => true,
        Request::RequestPermission { .. } | Request::AcknowledgeLostPermission { .. } => {
            cfg!(target_os = "macos")
        }
        Request::PermissionAsked { .. } => false,
    }
}

fn refusal(request: &Request, admitting: &Admitting) -> Result<Admission, Box<Reply>> {
    if PERMISSION_FORM.serves(request) && !served_by_this_build(request) {
        return Err(Box::new(error(
            ErrorCode::UnsupportedOp,
            "this build of the capture service does not serve the op",
        )));
    }
    let mutation = request.is_mutation() && PERMISSION_FORM.serves(request);
    let claimed = mutation
        && admitting
            .in_flight
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok();
    let admission = proto::admission(
        request,
        PERMISSION_FORM,
        mutation && !claimed,
        admitting.life(),
    );
    let refused = match admission {
        Admission::OtherForm => Some(error(
            ErrorCode::UnsupportedOp,
            "this build of the capture service does not serve the op",
        )),
        Admission::InFlight => Some(error(
            ErrorCode::Busy,
            "another request that changes the capture service is in flight",
        )),
        Admission::Ending => Some(Reply::Error(ErrorReply::busy_of_an_ending_life())),
        Admission::PauseKeepingTheWord
        | Admission::StatusAsItStands
        | Admission::ClaimStartingNothing
        | Admission::Serve => None,
    };
    match refused {
        Some(reply) => {
            if claimed {
                admitting.in_flight.store(false, Ordering::SeqCst);
            }
            Err(Box::new(reply))
        }
        None => Ok(admission),
    }
}

fn error(code: ErrorCode, message: &str) -> Reply {
    Reply::Error(ErrorReply::new(code, message))
}

fn write_reply<S: Write>(stream: &mut S, reply: &Reply) {
    let encoded = proto::encode_reply(reply);
    let body = if encoded.len() > REPLY_MAX_BYTES {
        proto::encode_reply(&error(
            ErrorCode::Internal,
            "the reply is larger than a reply may be",
        ))
    } else {
        encoded
    };
    if wire::write_frame(stream, &body, REPLY_MAX_BYTES).is_ok() {
        let _ = stream.flush();
    }
}

/// The bound of the password guard that a Mac's status reports: no constant
/// of the capture crate holds the Mac's bound at this head, so the status
/// reports the value of halvo-control's sample status.
pub const MAC_GUARD_BOUND_MS: u64 = 30_000;

/// Whether this build records typing, as a Mac's status says it: the capture
/// crate's one constant `TYPING_ON_MAC` on a Mac, and nothing elsewhere.
#[cfg(target_os = "macos")]
pub const TYPING: Option<bool> = Some(crate::macos::input::TYPING_ON_MAC);

/// Whether this build records typing: no status of this system says it.
#[cfg(not(target_os = "macos"))]
pub const TYPING: Option<bool> = None;

/// The peak of this process's memory in bytes, as a status reports it: on a
/// Mac the peak resident size that getrusage reports, on Windows the peak
/// working set that GetProcessMemoryInfo reports; 0 when the call fails.
#[cfg(target_os = "macos")]
pub fn rss_peak_bytes() -> u64 {
    unix::rss_peak_bytes()
}

/// The peak of this process's memory in bytes, as a status reports it: on a
/// Mac the peak resident size that getrusage reports, on Windows the peak
/// working set that GetProcessMemoryInfo reports; 0 when the call fails.
#[cfg(windows)]
pub fn rss_peak_bytes() -> u64 {
    use ::windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use ::windows::Win32::System::Threading::GetCurrentProcess;
    let mut counters = PROCESS_MEMORY_COUNTERS::default();
    let size = u32::try_from(std::mem::size_of::<PROCESS_MEMORY_COUNTERS>()).unwrap_or(0);
    // SAFETY: GetCurrentProcess returns the pseudo handle of this process,
    // and the call writes at most `size` bytes into `counters`, which is one
    // PROCESS_MEMORY_COUNTERS that lives across the call.
    let read = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) };
    match read {
        Ok(()) => u64::try_from(counters.PeakWorkingSetSize).unwrap_or(u64::MAX),
        Err(_) => 0,
    }
}

/// What a status reads of the life beside the supervisor: its start, whether a
/// parent is watched, the reading of the run marker at its start and the peak
/// of its resident memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunFacts {
    pub started_at: UtcSecond,
    pub parent: Parent,
    pub last_stop: LastStop,
    pub rss_peak_bytes: u64,
}

/// What the supervisor holds that a status reads.
#[derive(Debug, Clone)]
pub struct StatusParts {
    pub board: Board,
    pub desired: Desired,
    pub lease: proto::Lease,
    pub slot: Slot,
    pub obstructed: Option<Duration>,
    pub health: StateHealth,
    pub cap: StateCap,
    pub switches: Switches,
    pub once_granted: Vec<Permission>,
    pub asked: Vec<Permission>,
    /// Whether the input source's events hold keys, so that it needs Input
    /// Monitoring; the walk names that permission only then.
    pub input_keys: bool,
    /// Whether the hold of a record in the run marker stands (ruling PL-3
    /// part (5)): capture reads `awaiting_resume` while desired is running.
    pub held: bool,
    /// A turn of typing to off that the state file did not take, which the
    /// status carries beside key_class off alone.
    pub typing_off_unsaved: Option<proto::TypingOffUnsaved>,
    /// Whether the search for repeated tasks is on, as the state file holds
    /// it in memory; none where the supervisor holds no state file.
    pub search: Option<bool>,
    /// Whether the floor left out events by doubt in a row since the last record built
    /// ([`crate::floor::exclude::FloorHandle::left_out_in_doubt`]).
    pub left_out_in_doubt: bool,
    /// What the floor tells of its drops by doubt
    /// ([`crate::floor::exclude::FloorHandle::doubt_told`]).
    pub doubt_told: crate::floor::exclude::DoubtTold,
}

/// The text of the list `name` in the line that a service writes for `status`, for a test
/// that reads a list as it stands on the wire; none where the line holds no such list.
#[cfg(test)]
pub(crate) fn written_list(status: &DaemonStatus, name: &str) -> Option<String> {
    let line = serde_json::to_string(status).expect("a status is written as JSON");
    let key = format!("\"{name}\":[");
    let start = line.find(&key)? + key.len() - 1;
    let mut depth = 0_usize;
    for (at, sign) in line[start..].char_indices() {
        match sign {
            '[' => depth += 1,
            ']' if depth == 1 => return Some(line[start..=start + at].to_string()),
            ']' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// The status of a service run (contracts.md C2.1, C2.3) on the system of
/// its board, built from `parts` and `facts`; a status whose fields do not
/// hold together is the error of its rule. On Windows it names the platform
/// windows, lists no permission, reports no secure-input count, the custody
/// dpapi and the password guard `none` while the keyboard source reads off
/// (contracts.md C10.2).
pub fn status_of(parts: &StatusParts, facts: &RunFacts) -> Result<DaemonStatus, StatusError> {
    let board = &parts.board;
    let snapshot = board.snapshot;
    let figures = board.figures;
    let desired = match parts.desired {
        Desired::Running => CaptureState::Running,
        Desired::Paused => CaptureState::Paused,
    };
    let lines = board.source_lines();
    let fresh = parts.lease == proto::Lease::Fresh;
    let (capture, capture_reason) = match capture_of(board, &lines, desired, fresh) {
        (capture, _)
            if parts.held
                && desired == CaptureState::Running
                && capture != proto::Capture::Pausing =>
        {
            (proto::Capture::AwaitingResume, None)
        }
        read => read,
    };
    let (store, store_id, store_error, store_retry_in_s) = store_of(parts);
    let failing = board.drain.failing || board.drain.hold;
    let enabled_sources = lines
        .iter()
        .filter(|line| line.state != proto::SourceState::Off)
        .map(|line| line.name)
        .collect();
    let platform = board.platform;
    let guard = guard_of(board, &lines);
    let permissions: Vec<PermissionLine> = required_of(platform)
        .iter()
        .map(|&name| PermissionLine {
            name,
            grant: board
                .permissions
                .iter()
                .find(|(permission, _)| *permission == name)
                .map_or(GrantState::NotDetermined, |(_, grant)| *grant),
            at_start: match &board.granted_at_start {
                None => GrantState::NotDetermined,
                Some(granted) if granted.contains(&name) => GrantState::Granted,
                Some(_) => GrantState::NotGranted,
            },
            asked: parts.asked.contains(&name),
            once_granted: parts.once_granted.contains(&name),
        })
        .collect();
    let onboarding = onboarding_of(
        platform,
        &permissions,
        &board.permissions,
        parts.switches,
        parts.input_keys,
        board.keys_restart_did_not_help,
    );
    DaemonStatus::new(StatusFields {
        daemon_version: env!("CARGO_PKG_VERSION").parse().unwrap_or(DaemonVersion {
            major: 0,
            minor: 0,
            patch: 0,
        }),
        pid: std::process::id(),
        started_at: facts.started_at,
        rss_peak_bytes: facts.rss_peak_bytes,
        parent: facts.parent,
        data_dir_resolved: None,
        desired,
        capture,
        capture_reason,
        lease: parts.lease,
        store,
        store_id,
        store_error,
        store_retry_in_s,
        store_delete_unfinished: store == StoreState::Deleting && parts.slot.delete_unfinished(),
        store_bytes: snapshot.and_then(|snapshot| snapshot.disk_bytes),
        store_logical_bytes: None,
        cap_bytes: match parts.cap {
            StateCap::Known(cap) => proto::Cap::Known(cap),
            StateCap::OutOfRange(_) | StateCap::Unread => proto::Cap::Unknown,
        },
        cap_not_set: parts.cap.not_set(),
        drain: if failing {
            Health::Failing
        } else {
            Health::Healthy
        },
        drain_error: failing.then_some(if board.drain.failing {
            DrainError::Io
        } else {
            DrainError::Rejected
        }),
        drain_since: None,
        events_pending: snapshot.map_or(0, |snapshot| snapshot.events_pending),
        events_lost: snapshot.map_or(0, |snapshot| snapshot.events_lost),
        dropped_frames: 0,
        pointer_coalesced: figures.pointer_coalesced,
        tap_disabled_timeout: figures.tap_disabled_timeout,
        tap_disabled_user_input: figures.tap_disabled_user_input,
        late_events: figures.late_events > 0,
        max_lag_ms: figures.max_lag_ms,
        secure_input_keystrokes: (platform == Platform::Macos)
            .then_some(figures.secure_input_keystrokes > 0),
        typing: TYPING,
        keys_withheld: figures.keys_withheld,
        typing_gate: figures.typing_gate,
        left_out_in_doubt: parts.left_out_in_doubt,
        left_out: left_out_of(parts),
        left_out_before: left_out_before_of(parts),
        title_read_errors: figures.title_read_errors,
        last_flush_at: snapshot.and_then(|snapshot| second_of(snapshot.last_flush_at)),
        expiry: Health::Healthy,
        expiry_error: None,
        last_expiry_at: snapshot.and_then(|snapshot| second_of(snapshot.last_expiry_at)),
        last_eviction_at: snapshot.and_then(|snapshot| second_of(snapshot.last_eviction_at)),
        last_stop: facts.last_stop,
        last_stop_newest_capture: None,
        last_stop_kind: board.last_life.as_ref().map(|life| life.kind),
        last_stop_lost: board
            .last_life
            .as_ref()
            .map_or(LostRecord::NONE, |life| life.lost.clone()),
        last_stop_sources: Vec::new(),
        state_file: match parts.health {
            StateHealth::Ok => proto::StateFile::Ok,
            StateHealth::Corrupt => proto::StateFile::Corrupt,
            StateHealth::NewerVersion(_) => proto::StateFile::NewerVersion,
            StateHealth::WriteFailed(_) => proto::StateFile::WriteFailed,
        },
        platform,
        enabled_sources,
        switches: SwitchName::ALL
            .iter()
            .map(|&name| SwitchLine {
                name,
                value: if parts.switches.is_on(switch_of(name)) {
                    OnOff::On
                } else {
                    OnOff::Off
                },
            })
            .collect(),
        typing_off_unsaved: parts
            .typing_off_unsaved
            .filter(|_| !parts.switches.is_on(switch_of(SwitchName::KeyClass))),
        search: parts
            .search
            .map(|on| if on { OnOff::On } else { OnOff::Off }),
        permissions,
        sources: lines,
        kinds: Modality::ALL
            .iter()
            .map(|&name| KindLine {
                name,
                dropped: 0,
                refused: 0,
            })
            .collect(),
        password_guard: guard,
        password_guard_bound_ms: (guard != PasswordGuard::None).then_some(MAC_GUARD_BOUND_MS),
        screen_filter_excludes_app: None,
        custody: match platform {
            Platform::Macos => Custody::RootFile,
            Platform::Windows => Custody::Dpapi,
        },
        onboarding,
        ext: Vec::new(),
    })
}

fn capture_of(
    board: &Board,
    lines: &[SourceLine],
    desired: CaptureState,
    fresh: bool,
) -> (proto::Capture, Option<CaptureReason>) {
    let faulted = |lines: &[SourceLine]| {
        let permission = lines.iter().any(|line| {
            matches!(
                line.state,
                proto::SourceState::Failed {
                    reason: proto::Reason::Permission,
                    ..
                }
            )
        });
        if permission {
            CaptureReason::Permission
        } else {
            CaptureReason::NoSource
        }
    };
    match board.capture {
        BoardCapture::Pausing => (proto::Capture::Pausing, None),
        BoardCapture::NoIndicator => (proto::Capture::NoIndicator, None),
        BoardCapture::Running => (proto::Capture::Running, None),
        BoardCapture::Partial => (proto::Capture::Partial, None),
        BoardCapture::Faulted => (proto::Capture::Faulted, Some(faulted(lines))),
        BoardCapture::Stopped if fresh && desired == CaptureState::Running => {
            (proto::Capture::Faulted, Some(CaptureReason::Drain))
        }
        BoardCapture::Stopped | BoardCapture::Starting if desired == CaptureState::Paused => {
            (proto::Capture::Paused, None)
        }
        BoardCapture::Stopped | BoardCapture::Starting => (proto::Capture::Starting, None),
    }
}

fn store_of(
    parts: &StatusParts,
) -> (
    StoreState,
    Option<halvo_control::model::StoreId>,
    Option<StoreErrorKind>,
    Option<u64>,
) {
    if let Some(left) = parts.obstructed {
        return (
            StoreState::Unavailable,
            None,
            Some(StoreErrorKind::Obstructed),
            Some(left.as_secs()),
        );
    }
    let id = match &parts.slot {
        Slot::Open { id } => Some(*id),
        Slot::Maintenance { id } | Slot::Sealed { id, .. } | Slot::Unavailable { id, .. } => *id,
        Slot::Opening
        | Slot::OlderFormat
        | Slot::NewerFormat
        | Slot::Deleting
        | Slot::DeleteUnfinished
        | Slot::Empty { .. } => None,
    };
    (
        parts.slot.state(),
        id.map(|id| halvo_control::model::StoreId::from_bytes(*id.as_bytes())),
        parts.slot.cause(),
        parts.slot.retry_in().map(|left| left.as_secs()),
    )
}

fn required_of(platform: Platform) -> &'static [Permission] {
    match platform {
        Platform::Macos => &Permission::R1_MACOS,
        Platform::Windows => &[],
    }
}

fn guard_of(board: &Board, lines: &[SourceLine]) -> PasswordGuard {
    let keyboard_off = lines.iter().any(|line| {
        line.name == proto::SourceName::Keyboard && line.state == proto::SourceState::Off
    });
    if board.platform == Platform::Windows && keyboard_off {
        PasswordGuard::None
    } else {
        board.password_guard
    }
}

fn onboarding_of(
    platform: Platform,
    lines: &[PermissionLine],
    answered: &[(Permission, GrantState)],
    switches: Switches,
    input_keys: bool,
    keys_restart_did_not_help: bool,
) -> OnboardingState {
    let set = |pick: fn(&PermissionLine) -> bool| -> PermissionSet {
        lines
            .iter()
            .filter(|line| pick(line))
            .map(|line| line.name)
            .collect()
    };
    let now = lines.iter().fold(Grants::default(), |grants, line| {
        grants.with(line.name, line.grant)
    });
    let mut state = onboarding::assess(&AssessInput {
        required: &required_of(platform)
            .iter()
            .copied()
            .filter(|permission| input_keys || *permission != Permission::InputMonitoring)
            .collect::<Vec<_>>(),
        now,
        granted_at_capture_start: set(|line| line.at_start == GrantState::Granted),
        asked: set(|line| line.asked),
        once_granted: lines
            .iter()
            .filter(|line| {
                line.once_granted
                    && answered
                        .iter()
                        .any(|(permission, _)| *permission == line.name)
            })
            .map(|line| line.name)
            .collect(),
        macos_major: None,
        settings_entry_name: "Halvo",
        screen_activity: switches.is_on(Switch::ScreenActivity),
    });
    if keys_restart_did_not_help {
        let permission = Permission::InputMonitoring;
        state.walkback.push(WalkbackStep {
            permission,
            action: WalkbackAction::RestartCapture,
            settings_url: permission.settings_url().to_string(),
            text: onboarding::KEYS_RESTART_DID_NOT_HELP.to_string(),
        });
    }
    state
}

fn switch_of(name: SwitchName) -> Switch {
    match name {
        SwitchName::TitleText => Switch::TitleText,
        SwitchName::KeyClass => Switch::KeyClass,
        SwitchName::PointerClass => Switch::PointerClass,
        SwitchName::ScreenActivity => Switch::ScreenActivity,
        SwitchName::SiteRead => Switch::SiteRead,
    }
}

/// What the status says is left out now. Every record, while the floor's row of drops by
/// doubt stands, since the row's first drop. And under a hold of secure input that the Mac's
/// input source has read for 30 seconds or more, since its first reading, the parts that
/// the floor drops under it and whose switch is on: typing only where the tap asked for the
/// key types, window titles and screen activity; no entry where none of them is on. And
/// typing, while the Mac's input source holds that keys were pressed through 30 seconds
/// and none of them reached its tap, since the first reading of those seconds.
fn left_out_of(parts: &StatusParts) -> Vec<proto::LeftOut> {
    let mut left_out = Vec::new();
    let doubt = parts.doubt_told.since.filter(|_| parts.left_out_in_doubt);
    if let Some(since) = doubt.and_then(utc_second) {
        left_out.push(proto::LeftOut {
            cause: proto::LeftOutCause::Doubt,
            what: vec![proto::LeftOutPart::Everything],
            since,
        });
    }
    let hold = parts.board.figures.secure_hold_since;
    if let Some(since) = hold.and_then(utc_second) {
        let on = |switch| parts.switches.is_on(switch);
        let what: Vec<proto::LeftOutPart> = [
            (
                parts.board.figures.secure_hold_keys && on(Switch::KeyClass),
                proto::LeftOutPart::Typing,
            ),
            (on(Switch::TitleText), proto::LeftOutPart::WindowTitles),
            (
                on(Switch::ScreenActivity),
                proto::LeftOutPart::ScreenActivity,
            ),
        ]
        .into_iter()
        .filter_map(|(left, part)| left.then_some(part))
        .collect();
        if !what.is_empty() {
            left_out.push(proto::LeftOut {
                cause: proto::LeftOutCause::SecureInput,
                what,
                since,
            });
        }
    }
    let no_key = parts.board.figures.no_key_since;
    if let Some(since) = no_key.and_then(utc_second) {
        left_out.push(proto::LeftOut {
            cause: proto::LeftOutCause::NoKeyArrives,
            what: vec![proto::LeftOutPart::Typing],
            since,
        });
    }
    left_out
}

/// For how long behind its end a status tells of the last hold of secure input that a Mac's
/// input source kept, counted by the system's clock.
const HOLD_TOLD_FOR_SECONDS: u64 = 24 * 60 * 60;

/// What the status says was left out before: the last row of drops by doubt that the floor
/// tells of behind its end, as every record from the row's first drop to its end; and the last
/// hold of secure input of a minute or more that a Mac's input source saw end, for
/// [`HOLD_TOLD_FOR_SECONDS`] behind its end by the system's clock, with the parts that it left
/// out, where it names one.
fn left_out_before_of(parts: &StatusParts) -> Vec<proto::LeftOutBefore> {
    let times = parts
        .doubt_told
        .before
        .and_then(|(from, until)| Some((utc_second(from)?, utc_second(until)?)));
    let doubt = times.map(|(from, until)| proto::LeftOutBefore {
        cause: proto::LeftOutCause::Doubt,
        what: vec![proto::LeftOutPart::Everything],
        from,
        until,
    });
    let hold = parts.board.figures.secure_hold_before.and_then(held_before);
    doubt.into_iter().chain(hold).collect()
}

/// The entry of a hold of secure input that ended: none where [`HOLD_TOLD_FOR_SECONDS`] have
/// passed since its end by the system's clock, where it names no part, or where a time does
/// not read.
fn held_before(hold: crate::source::HoldBefore) -> Option<proto::LeftOutBefore> {
    let now = crate::floor::exclude::unix_seconds_now();
    if now.saturating_sub(hold.until) >= HOLD_TOLD_FOR_SECONDS {
        return None;
    }
    let what: Vec<proto::LeftOutPart> = [
        (hold.typing, proto::LeftOutPart::Typing),
        (hold.window_titles, proto::LeftOutPart::WindowTitles),
        (hold.screen_activity, proto::LeftOutPart::ScreenActivity),
    ]
    .into_iter()
    .filter_map(|(left, part)| left.then_some(part))
    .collect();
    if what.is_empty() {
        return None;
    }
    Some(proto::LeftOutBefore {
        cause: proto::LeftOutCause::SecureInput,
        what,
        from: utc_second(hold.from)?,
        until: utc_second(hold.until)?,
    })
}

fn utc_second(unix_seconds: u64) -> Option<UtcSecond> {
    UtcSecond::from_unix_seconds(i64::try_from(unix_seconds).ok()?).ok()
}

fn second_of(at: Option<std::time::SystemTime>) -> Option<UtcSecond> {
    let seconds = at?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    UtcSecond::from_unix_seconds(i64::try_from(seconds).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Recorded {
        input: std::io::Cursor<Vec<u8>>,
        reads: usize,
        out: Vec<u8>,
    }

    impl Recorded {
        fn holding(request: &Request) -> Recorded {
            let mut input = Vec::new();
            wire::write_frame(
                &mut input,
                &proto::encode_request(request),
                REQUEST_MAX_BYTES,
            )
            .unwrap();
            Recorded {
                input: std::io::Cursor::new(input),
                reads: 0,
                out: Vec::new(),
            }
        }

        fn reply(&self) -> Reply {
            let frame = wire::read_frame(&mut &self.out[..], REPLY_MAX_BYTES).unwrap();
            proto::decode_reply(&frame).unwrap()
        }
    }

    impl Read for Recorded {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.reads += 1;
            self.input.read(buf)
        }
    }

    impl Write for Recorded {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.out.write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl ServesUnknown for Recorded {}

    #[test]
    fn behind_a_settings_reset_the_status_says_the_cap_is_not_set_and_no_other_cap_does() {
        use halvo_control::model::{StorageCap, UtcSecond};

        let platform = if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Macos
        };
        let shown = |cap: StateCap| {
            let parts = StatusParts {
                board: Board::new(platform),
                desired: Desired::Paused,
                lease: proto::Lease::Fresh,
                slot: Slot::Opening,
                obstructed: None,
                health: StateHealth::Ok,
                cap,
                switches: Switches::default(),
                once_granted: Vec::new(),
                asked: Vec::new(),
                input_keys: false,
                held: false,
                typing_off_unsaved: None,
                search: None,
                left_out_in_doubt: false,
                doubt_told: crate::floor::exclude::DoubtTold::default(),
            };
            let facts = RunFacts {
                started_at: UtcSecond::from_unix_seconds(1_800_000_000).unwrap(),
                parent: proto::Parent::None,
                last_stop: proto::LastStop::None,
                rss_peak_bytes: 0,
            };
            let status = status_of(&parts, &facts).unwrap();
            (status.fields().cap_bytes, status.fields().cap_not_set)
        };
        let reset = super::super::state::after_a_reset().storage_cap;
        assert_eq!(shown(reset), (proto::Cap::Unknown, true));
        assert_eq!(super::super::cap_bound(reset), None);
        for cap in [
            StateCap::OutOfRange(1),
            StateCap::OutOfRange(StorageCap::MIN.bytes() - 1),
            StateCap::OutOfRange(StorageCap::MAX.bytes() + 1),
            StateCap::OutOfRange(u64::MAX),
            StateCap::Unread,
        ] {
            assert_eq!(shown(cap), (proto::Cap::Unknown, false), "{cap:?}");
        }
        let set = StateCap::Known(StorageCap::MIN);
        assert_eq!(shown(set), (proto::Cap::Known(StorageCap::MIN), false));
        assert!(super::super::cap_bound(set).is_some());
    }

    #[test]
    fn the_server_refuses_a_peer_of_another_user() {
        let own = 501;
        assert!(serves_peer(own, own), "the daemon's own user is served");
        for other in [0, 502, u32::MAX] {
            assert!(!serves_peer(other, own), "the user {other} is served");
        }
        let admitting = Arc::new(Admitting::default());
        let (hand, handed) = mpsc::channel();
        for peer in [Some(502), Some(0), None] {
            let mut stream = Recorded::holding(&Request::Stop);
            let written = serve_connection(&mut stream, peer, own, &admitting, &hand);
            assert!(written.is_none(), "{peer:?}");
            assert_eq!(stream.reads, 0, "a refused connection was read: {peer:?}");
            assert_eq!(
                stream.reply().code(),
                Some(ErrorCode::PeerRejected),
                "{peer:?}"
            );
        }
        assert!(
            handed.try_recv().is_err(),
            "a refused request reached the loop"
        );
        admitting.in_flight.store(true, Ordering::SeqCst);
        let mut stream = Recorded::holding(&Request::Stop);
        assert!(serve_connection(&mut stream, Some(own), own, &admitting, &hand).is_none());
        assert!(stream.reads > 0, "the own user's request was not read");
        assert_eq!(stream.reply().code(), Some(ErrorCode::Busy));
    }

    #[cfg(windows)]
    #[test]
    fn the_status_of_a_windows_run_names_windows_and_holds_the_keyboard_off() {
        use halvo_control::model::{StorageCap, UtcSecond};
        use halvo_control::proto::{SourceName, SourceState};

        let mut switches = Switches::default();
        for switch in Switch::ALL {
            switches.set(switch, true);
        }
        let running = [
            SourceName::Foreground,
            SourceName::Pointer,
            SourceName::WindowTitle,
        ];
        let mut board = Board::new(Platform::Windows);
        for name in running {
            board.set_source(name, &crate::source::SourceState::Running, false);
        }
        board.enabled = running.to_vec();
        board.capture = BoardCapture::Running;
        let parts = StatusParts {
            board,
            desired: Desired::Running,
            lease: proto::Lease::Fresh,
            slot: Slot::Opening,
            obstructed: None,
            health: StateHealth::Ok,
            cap: StateCap::Known(StorageCap::DEFAULT),
            switches,
            once_granted: Vec::new(),
            asked: Vec::new(),
            input_keys: false,
            held: false,
            typing_off_unsaved: None,
            search: None,
            left_out_in_doubt: false,
            doubt_told: crate::floor::exclude::DoubtTold::default(),
        };
        let facts = RunFacts {
            started_at: UtcSecond::from_unix_seconds(1_800_000_000).unwrap(),
            parent: proto::Parent::None,
            last_stop: proto::LastStop::None,
            rss_peak_bytes: 0,
        };
        let status = status_of(&parts, &facts).unwrap();
        let fields = status.fields();
        assert_eq!(fields.platform, Platform::Windows);
        let names: Vec<SourceName> = fields.sources.iter().map(|line| line.name).collect();
        assert_eq!(names, SourceName::WINDOWS);
        for line in &fields.sources {
            let off = !running.contains(&line.name);
            assert_eq!(line.state == SourceState::Off, off, "{:?}", line.name);
        }
        assert_eq!(fields.password_guard, PasswordGuard::None);
        assert_eq!(fields.password_guard_bound_ms, None);
        assert!(!fields.keys_withheld);
        assert_eq!(fields.secure_input_keystrokes, None);
        assert_eq!(fields.typing, None);
        assert!(fields.permissions.is_empty());
        assert_eq!(fields.custody, Custody::Dpapi);

        let text = halvo_control::ctl_text::status_text(&status);
        for wanted in [
            "platform=windows",
            "source name=keyboard state=off",
            "source name=screen state=off",
            "source name=system_audio state=off",
            "keys_withheld=no",
        ] {
            assert!(
                text.lines().any(|line| line == wanted),
                "no line {wanted} in:\n{text}"
            );
        }
        let json = serde_json::to_value(&status).unwrap();
        for (key, value) in json.as_object().unwrap() {
            if key.contains("key") {
                assert!(!value.is_number(), "a count of keys: {key}={value}");
            }
        }
    }

    /// Starts the screen source of the service set once, on the computer that
    /// runs the test, then reads a second set whose first screen source is a
    /// stand-in that fails its begin. The board holds a running foreground
    /// line, as the board of a run does, for the foreground source that the
    /// test does not start.
    #[cfg(windows)]
    #[test]
    fn the_status_of_a_windows_run_reads_the_screen_running_only_while_it_runs() {
        use halvo_control::model::{StorageCap, UtcSecond};
        use halvo_control::proto::SourceName;

        use crate::daemon::sources::{starting, RunKind, RunningSources};
        use crate::floor::exclude::{FloorHandle, NoLineage};
        use crate::source::{CaptureError, CaptureSource, SourceContext, Started};
        use crate::windows::sources::ServiceSet;

        struct FailsItsBegin(CaptureError);

        impl CaptureSource for FailsItsBegin {
            fn begin(&mut self, _ctx: SourceContext) -> Result<Started, CaptureError> {
                Err(self.0.clone())
            }

            fn shutdown(&mut self) -> Result<(), CaptureError> {
                Ok(())
            }
        }

        let _serial = crate::windows::tests::serial();
        let switches = |on| {
            let mut switches = Switches::default();
            switches.set(Switch::ScreenActivity, on);
            switches
        };
        let asked = |on| {
            starting(Platform::Windows, RunKind::Service, switches(on))
                .into_iter()
                .filter(|name| *name == SourceName::Screen)
                .collect::<Vec<_>>()
        };
        let (sink, _writer) = crate::sink::channel(64);
        let ctx = |on| {
            SourceContext::with_floor(
                sink.clone(),
                FloorHandle::new(None, Arc::new(NoLineage), switches(on)),
            )
        };
        let line = |sources: &RunningSources, on| {
            let mut board = Board::new(Platform::Windows);
            for (name, state, unstopped) in sources.states() {
                board.set_source(name, state, unstopped);
            }
            board.set_source(
                SourceName::Foreground,
                &crate::source::SourceState::Running,
                false,
            );
            let mut wanted = vec![SourceName::Foreground];
            wanted.extend(asked(on));
            board.capture = board.capture_of(&wanted);
            board.enabled = wanted;
            let parts = StatusParts {
                board,
                desired: Desired::Running,
                lease: proto::Lease::Fresh,
                slot: Slot::Opening,
                obstructed: None,
                health: StateHealth::Ok,
                cap: StateCap::Known(StorageCap::DEFAULT),
                switches: switches(on),
                once_granted: Vec::new(),
                asked: Vec::new(),
                input_keys: false,
                held: false,
                typing_off_unsaved: None,
                search: None,
                left_out_in_doubt: false,
                doubt_told: crate::floor::exclude::DoubtTold::default(),
            };
            let facts = RunFacts {
                started_at: UtcSecond::from_unix_seconds(1_800_000_000).unwrap(),
                parent: proto::Parent::None,
                last_stop: proto::LastStop::None,
                rss_peak_bytes: 0,
            };
            let status = status_of(&parts, &facts).unwrap();
            let text = halvo_control::ctl_text::status_text(&status);
            let of = |start: &str| {
                text.lines()
                    .find(|line| line.starts_with(start))
                    .unwrap_or_default()
                    .to_string()
            };
            (of("source name=screen "), of("capture="))
        };
        let mut set = ServiceSet::new();
        let mut sources = RunningSources::new(Platform::Windows);
        let mut lines = Vec::new();
        let off = sources.start(&mut set, &ctx(false), &asked(false));
        sources.read_health();
        lines.push(line(&sources, false));
        let first = sources.start(&mut set, &ctx(true), &asked(true));
        sources.read_health();
        lines.push(line(&sources, true));
        let first_stop = sources.stop_one(SourceName::Screen);
        lines.push(line(&sources, true));
        let second = sources.start(&mut set, &ctx(true), &asked(true));
        lines.push(line(&sources, true));
        sources.read_health();
        lines.push(line(&sources, true));
        let second_stop = sources.stop_one(SourceName::Screen);
        lines.push(line(&sources, true));
        let error = CaptureError::Start("the screen capture could not start".to_string());
        let mut failing = ServiceSet::with_first_screen(Box::new(FailsItsBegin(error.clone())));
        let mut held = RunningSources::new(Platform::Windows);
        let failed_start = held.start(&mut failing, &ctx(true), &asked(true));
        held.read_health();
        let at_the_failed_start = line(&held, true);
        let restarted = held.start_one(&mut failing, &ctx(true), SourceName::Screen);
        held.read_health();
        let behind_the_restart = line(&held, true);
        assert!(off.is_empty(), "{off:?}");
        assert_eq!(first, [(SourceName::Screen, Ok(Started::Running))]);
        assert_eq!(first_stop, Some(Ok(())));
        assert_eq!(second, [(SourceName::Screen, Ok(Started::Off))]);
        assert_eq!(second_stop, Some(Ok(())));
        let read = |screen: &str, capture: &str| {
            (
                format!("source name=screen state={screen}"),
                format!("capture={capture}"),
            )
        };
        assert_eq!(
            lines,
            [
                read("off", "running"),
                read("running", "running"),
                read("stopped", "partial"),
                read("stopped", "partial"),
                read("stopped", "partial"),
                read("stopped", "partial"),
            ],
            "the screen and capture lines with the switch off, while the source runs, behind \
             its stop, at the start behind it, at the health read behind that start, behind \
             the second stop"
        );
        assert_eq!(failed_start, [(SourceName::Screen, Err(error.clone()))]);
        assert_eq!(restarted, Some(Err(error)));
        let failed = read("failed reason=os_error", "partial");
        assert_eq!(
            (at_the_failed_start, behind_the_restart),
            (failed.clone(), failed),
            "the screen and capture lines at a first start that failed and behind the \
             restart's make"
        );
    }

    #[test]
    fn the_status_says_left_out_in_doubt_from_the_third_drop_by_doubt_in_a_row_until_a_record_is_built(
    ) {
        use crate::event::{AppIdentity, EventClock, FloorEvent};
        use crate::floor::exclude::{
            DropReason, FloorHandle, Listing, Marks, NoCreationTime, NoLineage, PrivateWindow,
            ProcessId, Subject, Verdict,
        };
        use crate::floor::kind::{self, Recorded};
        use crate::secure_input::{FixedProbe, SecureInputReading};
        use halvo_control::model::{StorageCap, UtcSecond};

        fn reason<K: Recorded>(verdict: Verdict<K>) -> Option<DropReason> {
            match verdict {
                Verdict::Keep(_) => None,
                Verdict::Drop(reason) => Some(reason),
            }
        }

        let platform = if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Macos
        };
        let mut every_switch_on = Switches::default();
        for switch in Switch::ALL {
            every_switch_on.set(switch, true);
        }
        let marks = Arc::new(Marks::new(Box::new(NoCreationTime)));
        let floor = FloorHandle::new(None, Arc::new(NoLineage), every_switch_on)
            .with_marks(Arc::clone(&marks));
        let mut keys_off = every_switch_on;
        keys_off.set(Switch::KeyClass, false);
        let typing_off = floor.clone().with_switches(keys_off);
        let unread = || {
            marks.take(Listing {
                named: Vec::new(),
                unread: true,
            });
        };
        let read_again = || marks.take(Listing::default());
        let inactive = FixedProbe(SecureInputReading::Inactive);
        let active = FixedProbe(SecureInputReading::Active);
        let app =
            |id: &str| Subject::new(AppIdentity::bundle_id(id.to_string()), ProcessId::new(200));
        let terminal = || {
            let verdict = floor.decide::<kind::ForegroundApp>(
                app("com.apple.Terminal"),
                PrivateWindow::Unknown,
                &inactive,
            );
            match verdict {
                Verdict::Keep(keep) => {
                    let _ = FloorEvent::foreground_app(keep, None, EventClock::system());
                    None
                }
                Verdict::Drop(reason) => Some(reason),
            }
        };
        let halvo_s_own_window = || {
            reason(floor.decide::<kind::ForegroundApp>(
                app("com.halvo.desktop"),
                PrivateWindow::Unknown,
                &inactive,
            ))
        };
        let private_window = || {
            reason(floor.decide::<kind::WindowTitle>(
                app("com.apple.Safari"),
                PrivateWindow::Private,
                &inactive,
            ))
        };
        let password_field = || {
            reason(floor.decide::<kind::WindowTitle>(
                app("com.apple.Safari"),
                PrivateWindow::Unknown,
                &active,
            ))
        };
        let key_with_typing_off = || {
            reason(typing_off.decide::<kind::Keystroke>(
                app("com.apple.Terminal"),
                PrivateWindow::Unknown,
                &inactive,
            ))
        };
        let facts = RunFacts {
            started_at: UtcSecond::from_unix_seconds(1_800_000_000).unwrap(),
            parent: proto::Parent::None,
            last_stop: proto::LastStop::None,
            rss_peak_bytes: 0,
        };
        let shown = || {
            let parts = StatusParts {
                board: Board::new(platform),
                desired: Desired::Running,
                lease: proto::Lease::Fresh,
                slot: Slot::Opening,
                obstructed: None,
                health: StateHealth::Ok,
                cap: StateCap::Known(StorageCap::DEFAULT),
                switches: every_switch_on,
                once_granted: Vec::new(),
                asked: Vec::new(),
                input_keys: false,
                held: false,
                typing_off_unsaved: None,
                search: None,
                left_out_in_doubt: floor.left_out_in_doubt(),
                doubt_told: crate::floor::exclude::DoubtTold::default(),
            };
            let status = status_of(&parts, &facts).unwrap();
            serde_json::to_value(&status)
                .unwrap()
                .get("left_out_in_doubt")
                .cloned()
        };
        let doubt = Some(DropReason::UnknownLineage);
        let yes = Some(serde_json::Value::Bool(true));

        unread();
        assert_eq!([terminal(), terminal()], [doubt, doubt]);
        assert_eq!(shown(), None, "two drops by doubt in a row");
        assert_eq!(terminal(), doubt);
        assert_eq!(shown(), yes, "the third drop by doubt in a row");
        assert_eq!(halvo_s_own_window(), Some(DropReason::ListedApp));
        assert_eq!(
            shown(),
            yes,
            "a drop on purpose behind them: a person who reads the page with Halvo's own window \
             in front"
        );
        read_again();
        assert_eq!(terminal(), None);
        assert_eq!(shown(), None, "a record built");
        unread();
        assert_eq!(
            [
                terminal(),
                halvo_s_own_window(),
                terminal(),
                halvo_s_own_window()
            ],
            [
                doubt,
                Some(DropReason::ListedApp),
                doubt,
                Some(DropReason::ListedApp)
            ]
        );
        assert_eq!(
            shown(),
            None,
            "two more drops by doubt with drops on purpose between them"
        );
        read_again();
        assert_eq!(terminal(), None);
        for _ in 0..25 {
            assert_eq!(
                [
                    halvo_s_own_window(),
                    private_window(),
                    password_field(),
                    key_with_typing_off()
                ],
                [
                    Some(DropReason::ListedApp),
                    Some(DropReason::PrivateWindow),
                    Some(DropReason::SecureInput),
                    Some(DropReason::SwitchOff)
                ]
            );
        }
        assert_eq!(shown(), None, "a hundred drops on purpose alone");
    }
}
