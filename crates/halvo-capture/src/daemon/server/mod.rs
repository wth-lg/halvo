//! The control server of a service run (contracts.md C2.1, C3.2 step 6). It
//! serves protocol v1, one request for each connection, to peers of the
//! daemon's own user alone, and hands each request that it admits to the
//! service run's loop, which answers it between two turns of its main loop.

#[cfg(target_os = "macos")]
pub mod unix;
#[cfg(windows)]
pub mod windows;

use std::io::{ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use halvo_control::model::{CaptureState, Modality, UtcSecond};
use halvo_control::onboarding::{
    self, AssessInput, GrantState, Grants, OnboardingState, Permission, PermissionSet,
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
        #[cfg(target_os = "macos")]
        let facts = RunFacts {
            rss_peak_bytes: unix::rss_peak_bytes(),
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
    matches!(
        request,
        Request::Status { .. }
            | Request::Inventory
            | Request::Pause
            | Request::Resume
            | Request::Stop
            | Request::DeleteAll { .. }
            | Request::SetStorageCap { .. }
    )
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
    /// Whether the hold of a record in the run marker stands (ruling PL-3
    /// part (5)): capture reads `awaiting_resume` while desired is running.
    pub held: bool,
}

/// The status of a service run on a Mac (contracts.md C2.1, C2.3), built from
/// `parts` and `facts`; a status whose fields do not hold together is the
/// error of its rule.
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
    let permissions: Vec<PermissionLine> = Permission::R1_MACOS
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
    let onboarding = onboarding_of(&permissions, parts.switches);
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
        store_bytes: snapshot.and_then(|snapshot| snapshot.disk_bytes),
        store_logical_bytes: None,
        cap_bytes: match parts.cap {
            StateCap::Known(cap) => proto::Cap::Known(cap),
            StateCap::OutOfRange(_) | StateCap::Unread => proto::Cap::Unknown,
        },
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
        secure_input_keystrokes: Some(figures.secure_input_keystrokes > 0),
        typing: TYPING,
        keys_withheld: figures.keys_withheld,
        title_read_errors: figures.title_read_errors,
        last_flush_at: snapshot.and_then(|snapshot| second_of(snapshot.last_flush_at)),
        expiry: Health::Healthy,
        expiry_error: None,
        last_expiry_at: snapshot.and_then(|snapshot| second_of(snapshot.last_expiry_at)),
        last_eviction_at: snapshot.and_then(|snapshot| second_of(snapshot.last_eviction_at)),
        last_stop: facts.last_stop,
        last_stop_newest_capture: None,
        last_stop_kind: None,
        last_stop_lost: LostRecord::NONE,
        last_stop_sources: Vec::new(),
        state_file: match parts.health {
            StateHealth::Ok => proto::StateFile::Ok,
            StateHealth::Corrupt => proto::StateFile::Corrupt,
            StateHealth::NewerVersion(_) => proto::StateFile::NewerVersion,
            StateHealth::WriteFailed(_) => proto::StateFile::WriteFailed,
        },
        platform: Platform::Macos,
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
        password_guard: board.password_guard,
        password_guard_bound_ms: (board.password_guard != PasswordGuard::None)
            .then_some(MAC_GUARD_BOUND_MS),
        screen_filter_excludes_app: None,
        custody: Custody::RootFile,
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
        | Slot::Empty { .. } => None,
    };
    (
        parts.slot.state(),
        id.map(|id| halvo_control::model::StoreId::from_bytes(*id.as_bytes())),
        parts.slot.cause(),
        parts.slot.retry_in().map(|left| left.as_secs()),
    )
}

fn onboarding_of(lines: &[PermissionLine], switches: Switches) -> OnboardingState {
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
    onboarding::assess(&AssessInput {
        required: &Permission::R1_MACOS,
        now,
        granted_at_capture_start: set(|line| line.at_start == GrantState::Granted),
        asked: set(|line| line.asked),
        once_granted: set(|line| line.once_granted),
        macos_major: None,
        settings_entry_name: "Halvo",
        screen_activity: switches.is_on(Switch::ScreenActivity),
    })
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
}
