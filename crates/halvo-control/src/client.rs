use std::io::{ErrorKind, Read, Write};
use std::time::{Duration, Instant};

use crate::model::{StorageCap, StoreId};
use crate::onboarding::{Permission, RequestOutcome};
use crate::proto::{
    decode_reply, encode_request, AlreadyDeleted, CapSet, DaemonStatus, Deleted, ErrorReply,
    InventoryReply, Reply, Request, Stopped,
};
use crate::wire::{read_frame, write_frame, FrameError, REPLY_MAX_BYTES, REQUEST_MAX_BYTES};

/// A connected stream to the daemon that carries one request and its reply.
pub trait Transport: Read + Write {
    /// Bounds the next read or write.
    fn set_timeout(&mut self, timeout: Duration) -> std::io::Result<()>;

    /// Ends the request, so that the server reads the frame and then the end of the stream.
    fn end_request(&mut self) -> std::io::Result<()>;
}

/// Makes the connection and runs the transport's peer checks before the request is sent.
pub trait Connector {
    type Transport: Transport;

    /// The socket path or pipe name, as `ctl` names it when no daemon answers.
    fn endpoint(&self) -> String;

    fn connect(&self, timeout: Duration) -> Result<Self::Transport, ClientError>;
}

/// Why a peer check refused the daemon's end of the channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PeerRejection {
    #[error("the socket's directory is not a directory with mode 0700")]
    SocketDirNotPrivate,
    #[error("the socket's directory belongs to another user")]
    SocketDirOfAnotherUser,
    #[error("the control endpoint is not a socket")]
    NotASocket,
    #[error("the socket belongs to another user")]
    SocketOfAnotherUser,
    #[error("the server runs as another user")]
    ServerOfAnotherUser,
    #[error("the server is not the process that was started")]
    NotTheExpectedServer,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("the control channel is not available on this system yet")]
    Unsupported,
    #[error("capture service not running at {endpoint}")]
    Unreachable { endpoint: String },
    /// No connection could be made while the instance lock of the data directory stayed
    /// held, up to the request's deadline (ruling PL-8 part (2)(b)).
    #[error("the data directory is in use and no connection could be made at {endpoint}; capture may be running")]
    DataDirInUse { endpoint: String },
    #[error("the control endpoint was refused: {0}")]
    PeerRejected(PeerRejection),
    #[error("no reply within {seconds} s")]
    Deadline { seconds: u64 },
    #[error("{}: {}", .0.code.as_str(), .0.message)]
    Daemon(ErrorReply),
    #[error("the reply could not be read: {0}")]
    Protocol(String),
    /// A reply of a protocol version other than 1.
    #[error("the reply could not be read: {}", crate::proto::ReplyError::Version(*version))]
    ReplyOfAnotherVersion { version: u64 },
    /// A reply that holds a text outside a closed list of protocol v1 where it reads a value of
    /// that list; the error names the list.
    #[error("the reply could not be read: {}", crate::proto::ReplyError::UnknownValue(*.0))]
    ValueUnknownToV1(crate::model::UnknownSpelling),
    /// A reply whose status breaks a rule of the status type; the text is the rule's.
    #[error("the capture service reported a state that does not hold together: {}", .0.rule())]
    StateDoesNotHoldTogether(crate::proto::StatusError),
    #[error("the daemon answered {kind} to this request")]
    UnexpectedReply { kind: &'static str },
    #[error("the control channel failed: {0}")]
    Io(std::io::Error),
}

/// How long the client waits for each request, from the connect to the end of the reply. After
/// a whole reply frame the client waits at most [`crate::REPLY_END_WAIT_MS`] inside this
/// deadline for the end of the stream; the server's reader, `wire::read_one_frame`, requires the
/// end of the stream after a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deadlines {
    pub read: Duration,
    pub pause: Duration,
    pub resume: Duration,
    pub stop: Duration,
    pub delete_all: Duration,
    pub set_storage_cap: Duration,
    pub permission: Duration,
    pub acknowledge: Duration,
}

impl Deadlines {
    /// The deadlines of contracts.md C2.1. `permission_asked` waits as `request_permission`
    /// does, and `acknowledge_lost_permission`, a write of the state file, as `pause` does.
    pub const V1: Deadlines = Deadlines {
        read: Duration::from_millis(2_000),
        pause: Duration::from_millis(10_000),
        resume: Duration::from_millis(20_000),
        stop: Duration::from_millis(crate::STOP_WAIT_MS),
        delete_all: Duration::from_millis(120_000),
        set_storage_cap: Duration::from_millis(5_000),
        permission: Duration::from_millis(2_000),
        acknowledge: Duration::from_millis(10_000),
    };

    pub fn of(&self, request: &Request) -> Duration {
        match request {
            Request::Status { .. } | Request::Inventory => self.read,
            Request::Pause => self.pause,
            Request::Resume => self.resume,
            Request::Stop => self.stop,
            Request::DeleteAll { .. } => self.delete_all,
            Request::SetStorageCap { .. } => self.set_storage_cap,
            Request::RequestPermission { .. } | Request::PermissionAsked { .. } => self.permission,
            Request::AcknowledgeLostPermission { .. } => self.acknowledge,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteOutcome {
    Deleted(Deleted),
    AlreadyDeleted(AlreadyDeleted),
    /// The delete removed nothing; the caller reads the status and asks the person again.
    Refused,
}

/// Whether an `already_deleted` can answer a delete armed with `armed`: only one that carried
/// an id, and only when the reply names another id as the current store.
pub fn already_deleted_answers(armed: Option<StoreId>, already: &AlreadyDeleted) -> bool {
    armed.is_some_and(|armed| armed != already.current)
}

/// The client of protocol v1. Each call makes one connection, sends one request and reads
/// one reply inside the request's deadline.
#[derive(Debug, Clone)]
pub struct Client<C> {
    connector: C,
    deadlines: Deadlines,
}

struct Timed<'a, T> {
    transport: &'a mut T,
    deadline: Instant,
}

fn left(deadline: Instant) -> std::io::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(ErrorKind::TimedOut.into())
    } else {
        Ok(left)
    }
}

impl<T: Transport> Timed<'_, T> {
    fn end_request(&mut self) -> std::io::Result<()> {
        self.transport.set_timeout(left(self.deadline)?)?;
        self.transport.end_request()
    }
}

impl<T: Transport> Read for Timed<'_, T> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.transport.set_timeout(left(self.deadline)?)?;
        self.transport.read(buf)
    }
}

impl<T: Transport> Write for Timed<'_, T> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.transport.set_timeout(left(self.deadline)?)?;
        self.transport.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.transport.set_timeout(left(self.deadline)?)?;
        self.transport.flush()
    }
}

fn is_timeout(error: &std::io::Error) -> bool {
    matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock)
}

/// The client's reader of a reply. It reads one frame inside the request's deadline, then
/// waits at most [`crate::REPLY_END_WAIT_MS`], and never past that deadline, for what follows:
/// a byte is [`FrameError::Trailing`], and the end of the stream, the end of the wait and any
/// error of the transport give the frame.
fn read_reply<T: Transport>(timed: &mut Timed<'_, T>) -> Result<Vec<u8>, FrameError> {
    let frame = read_frame(timed, REPLY_MAX_BYTES)?;
    let Ok(time_left) = left(timed.deadline) else {
        return Ok(frame);
    };
    let mut wait = time_left.min(Duration::from_millis(crate::REPLY_END_WAIT_MS));
    let wait_until = Instant::now() + wait;
    let mut probe = [0u8; 1];
    loop {
        if timed.transport.set_timeout(wait).is_err() {
            return Ok(frame);
        }
        match timed.transport.read(&mut probe) {
            Ok(0) => return Ok(frame),
            Ok(_) => return Err(FrameError::Trailing),
            Err(e) if e.kind() == ErrorKind::Interrupted => match left(wait_until) {
                Ok(rest) => wait = rest,
                Err(_) => return Ok(frame),
            },
            Err(_) => return Ok(frame),
        }
    }
}

impl<C: Connector> Client<C> {
    pub fn new(connector: C) -> Self {
        Self::with_deadlines(connector, Deadlines::V1)
    }

    pub fn with_deadlines(connector: C, deadlines: Deadlines) -> Self {
        Self {
            connector,
            deadlines,
        }
    }

    pub fn connector(&self) -> &C {
        &self.connector
    }

    pub(crate) fn connect(&self, deadline: Instant) -> Result<C::Transport, ClientError> {
        let timeout = left(deadline).map_err(|_| ClientError::Deadline { seconds: 0 })?;
        self.connector.connect(timeout)
    }

    /// Sends `request` and returns the daemon's reply, an error reply included.
    pub fn send(&self, request: &Request) -> Result<Reply, ClientError> {
        let limit = self.deadlines.of(request);
        let deadline = Instant::now() + limit;
        let timed_out = || ClientError::Deadline {
            seconds: limit.as_secs(),
        };
        let frame_error = |error: FrameError| match error {
            FrameError::Io(e) if is_timeout(&e) => timed_out(),
            FrameError::Io(e) => ClientError::Io(e),
            other => ClientError::Protocol(other.to_string()),
        };
        let mut transport = match self.connect(deadline) {
            Err(ClientError::Deadline { .. }) => return Err(timed_out()),
            other => other?,
        };
        let mut timed = Timed {
            transport: &mut transport,
            deadline,
        };
        write_frame(&mut timed, &encode_request(request), REQUEST_MAX_BYTES)
            .map_err(frame_error)?;
        timed.end_request().map_err(|e| {
            if is_timeout(&e) {
                timed_out()
            } else {
                ClientError::Io(e)
            }
        })?;
        let frame = read_reply(&mut timed).map_err(frame_error)?;
        let reply = decode_reply(&frame).map_err(|e| match e {
            crate::proto::ReplyError::Status(broken) => {
                ClientError::StateDoesNotHoldTogether(broken)
            }
            crate::proto::ReplyError::Version(version) => {
                ClientError::ReplyOfAnotherVersion { version }
            }
            crate::proto::ReplyError::UnknownValue(outside) => {
                ClientError::ValueUnknownToV1(outside)
            }
            other => ClientError::Protocol(other.to_string()),
        })?;
        #[cfg(feature = "r2")]
        let reply = crate::r2::decode_sections(reply)?;
        Ok(reply)
    }

    fn call(&self, request: Request) -> Result<Reply, ClientError> {
        match self.send(&request)? {
            Reply::Error(error) => Err(ClientError::Daemon(error)),
            reply => Ok(reply),
        }
    }

    fn status_reply(&self, request: Request) -> Result<DaemonStatus, ClientError> {
        let changes_capture = matches!(request, Request::Pause | Request::Resume);
        match (changes_capture, self.call(request)?) {
            (false, Reply::Status(status)) | (true, Reply::CaptureChanged(status)) => Ok(status),
            (_, other) => Err(ClientError::UnexpectedReply { kind: other.kind() }),
        }
    }

    /// Reads the status and renews nothing.
    pub fn status(&self) -> Result<DaemonStatus, ClientError> {
        self.status_reply(Request::Status { indicator: false })
    }

    /// Reads the status and renews the indicator lease.
    pub fn status_claiming_the_indicator(&self) -> Result<DaemonStatus, ClientError> {
        self.status_reply(Request::Status { indicator: true })
    }

    pub fn inventory(&self) -> Result<InventoryReply, ClientError> {
        match self.call(Request::Inventory)? {
            Reply::Inventory(inventory) => Ok(inventory),
            other => Err(ClientError::UnexpectedReply { kind: other.kind() }),
        }
    }

    pub fn pause(&self) -> Result<DaemonStatus, ClientError> {
        self.status_reply(Request::Pause)
    }

    pub fn resume(&self) -> Result<DaemonStatus, ClientError> {
        self.status_reply(Request::Resume)
    }

    pub fn stop(&self) -> Result<Stopped, ClientError> {
        match self.call(Request::Stop)? {
            Reply::Stopped(stopped) => Ok(stopped),
            other => Err(ClientError::UnexpectedReply { kind: other.kind() }),
        }
    }

    /// Deletes the store whose id the caller armed the delete with, or, with `None`, a
    /// store whose status shows no id. The client reads no status for it.
    pub fn delete_all(&self, armed: Option<StoreId>) -> Result<DeleteOutcome, ClientError> {
        match self.call(Request::DeleteAll { store: armed })? {
            Reply::Deleted(deleted) => Ok(DeleteOutcome::Deleted(deleted)),
            Reply::AlreadyDeleted(already) if already_deleted_answers(armed, &already) => {
                Ok(DeleteOutcome::AlreadyDeleted(already))
            }
            Reply::DeleteRefused => Ok(DeleteOutcome::Refused),
            other => Err(ClientError::UnexpectedReply { kind: other.kind() }),
        }
    }

    pub fn set_storage_cap(&self, cap: StorageCap) -> Result<CapSet, ClientError> {
        match self.call(Request::SetStorageCap { bytes: cap })? {
            Reply::CapSet(set) => Ok(set),
            other => Err(ClientError::UnexpectedReply { kind: other.kind() }),
        }
    }

    pub fn request_permission(
        &self,
        permission: Permission,
    ) -> Result<RequestOutcome, ClientError> {
        match self.call(Request::RequestPermission { permission })? {
            Reply::PermissionRequested(requested) => Ok(requested.outcome),
            other => Err(ClientError::UnexpectedReply { kind: other.kind() }),
        }
    }

    pub fn permission_asked(&self, permission: Permission) -> Result<DaemonStatus, ClientError> {
        self.status_reply(Request::PermissionAsked { permission })
    }

    pub fn acknowledge_lost_permission(
        &self,
        permission: Permission,
    ) -> Result<DaemonStatus, ClientError> {
        self.status_reply(Request::AcknowledgeLostPermission { permission })
    }
}

/// The connector of a system that has no control channel yet: every call answers
/// [`ClientError::Unsupported`] (contracts.md C15.8).
#[derive(Debug, Clone)]
pub struct UnsupportedConnector;

/// A transport that no connector of this crate makes.
#[derive(Debug)]
pub enum NoTransport {}

impl Read for NoTransport {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        match *self {}
    }
}

impl Write for NoTransport {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        match *self {}
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match *self {}
    }
}

impl Transport for NoTransport {
    fn set_timeout(&mut self, _: Duration) -> std::io::Result<()> {
        match *self {}
    }

    fn end_request(&mut self) -> std::io::Result<()> {
        match *self {}
    }
}

impl Connector for UnsupportedConnector {
    type Transport = NoTransport;

    fn endpoint(&self) -> String {
        String::new()
    }

    fn connect(&self, _: Duration) -> Result<NoTransport, ClientError> {
        Err(ClientError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
pub type SystemConnector = crate::client_unix::UnixConnector;

#[cfg(not(target_os = "macos"))]
pub type SystemConnector = UnsupportedConnector;

/// The connector to the daemon of `data_dir` on this system.
#[cfg(target_os = "macos")]
pub fn system_connector(data_dir: &crate::paths::DataDir) -> Result<SystemConnector, ClientError> {
    crate::client_unix::UnixConnector::for_data_dir(data_dir, &crate::paths::SystemAccount)
}

/// The connector to the daemon of `data_dir` on this system.
#[cfg(not(target_os = "macos"))]
pub fn system_connector(data_dir: &crate::paths::DataDir) -> Result<SystemConnector, ClientError> {
    let _ = data_dir;
    Ok(UnsupportedConnector)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{StorageCap, StoreId};
    use crate::onboarding::{GrantState, Permission, RequestOutcome};
    use crate::proto::{
        DaemonStatus, ErrorCode, PermissionForm, Platform, Reply, Section, SectionRow,
        StoreErrorKind, StoreState,
    };
    use crate::testing::{
        sample_inventory, sample_status, store_id, ScriptedConnector, ScriptedDaemon,
        FIXTURE_STORE_ID,
    };

    fn client(daemon: ScriptedDaemon) -> (Client<ScriptedConnector>, ScriptedConnector) {
        let connector = ScriptedConnector::new(daemon);
        (Client::new(connector.clone()), connector)
    }

    fn with_store(
        state: StoreState,
        cause: Option<StoreErrorKind>,
        id: Option<StoreId>,
    ) -> DaemonStatus {
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.store = state;
        fields.store_error = cause;
        fields.store_id = id;
        DaemonStatus::new(fields).unwrap()
    }

    fn daemon_error(result: Result<impl std::fmt::Debug, ClientError>) -> ErrorCode {
        match result {
            Err(ClientError::Daemon(reply)) => reply.code,
            other => panic!("{other:?}"),
        }
    }

    fn request_frame(request: &Request) -> Vec<u8> {
        let mut frame = Vec::new();
        crate::wire::write_frame(
            &mut frame,
            &encode_request(request),
            crate::wire::REQUEST_MAX_BYTES,
        )
        .unwrap();
        frame
    }

    #[test]
    fn a_silent_scripted_daemon_answers_nothing_whatever_arrives_and_still_records_a_request() {
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        daemon.set_silent(true);
        assert_eq!(daemon.serve(b"not one request frame"), None);
        assert_eq!(daemon.serve(&request_frame(&Request::Pause)), None);
        assert_eq!(daemon.requests(), [Request::Pause]);
        assert_eq!(
            daemon.status().fields().capture,
            crate::proto::Capture::Paused,
            "the handler acted although its reply never came"
        );
        daemon.set_raw_reply(vec![0, 0, 0, 1, b'x']);
        assert_eq!(daemon.serve(&request_frame(&Request::Stop)), None);
        assert_eq!(daemon.requests(), [Request::Pause, Request::Stop]);
    }

    #[test]
    fn while_a_mutation_is_in_flight_the_other_form_is_unsupported_a_served_mutation_busy_and_a_read_answered(
    ) {
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        daemon.set_mutation_in_flight(true);
        let answer = |daemon: &mut ScriptedDaemon, request: &Request| {
            let frame = daemon.serve(&request_frame(request)).unwrap();
            let body =
                crate::wire::read_one_frame(&mut &frame[..], crate::wire::REPLY_MAX_BYTES).unwrap();
            crate::proto::decode_reply(&body).unwrap()
        };
        let asked = Request::PermissionAsked {
            permission: Permission::Accessibility,
        };
        assert_eq!(
            answer(&mut daemon, &asked).code(),
            Some(ErrorCode::UnsupportedOp)
        );
        let requested = Request::RequestPermission {
            permission: Permission::Accessibility,
        };
        assert_eq!(
            answer(&mut daemon, &requested).code(),
            Some(ErrorCode::Busy)
        );
        assert_eq!(
            answer(&mut daemon, &Request::Pause).code(),
            Some(ErrorCode::Busy)
        );
        assert!(matches!(
            answer(&mut daemon, &Request::Status { indicator: false }),
            Reply::Status(_)
        ));
        daemon.set_form(PermissionForm::B);
        assert_eq!(
            answer(&mut daemon, &requested).code(),
            Some(ErrorCode::UnsupportedOp)
        );
        assert_eq!(answer(&mut daemon, &asked).code(), Some(ErrorCode::Busy));
    }

    #[derive(Debug, Clone, PartialEq)]
    enum Event {
        Timeout(Duration),
        Write(usize),
        Flush,
        End,
        Read,
    }

    #[derive(Clone)]
    struct Trickle {
        events: std::sync::Arc<std::sync::Mutex<Vec<Event>>>,
        pause: Duration,
    }

    impl Trickle {
        fn new(pause: Duration) -> Self {
            Self {
                events: std::sync::Arc::default(),
                pause,
            }
        }

        fn events(&self) -> Vec<Event> {
            self.events.lock().unwrap().clone()
        }

        fn push(&self, event: Event) {
            self.events.lock().unwrap().push(event);
        }
    }

    impl Read for Trickle {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            self.push(Event::Read);
            Ok(0)
        }
    }

    impl Write for Trickle {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            std::thread::sleep(self.pause);
            let n = buf.len().min(1);
            self.push(Event::Write(n));
            Ok(n)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.push(Event::Flush);
            Ok(())
        }
    }

    impl Transport for Trickle {
        fn set_timeout(&mut self, timeout: Duration) -> std::io::Result<()> {
            self.push(Event::Timeout(timeout));
            Ok(())
        }

        fn end_request(&mut self) -> std::io::Result<()> {
            self.push(Event::End);
            Ok(())
        }
    }

    impl Connector for Trickle {
        type Transport = Trickle;

        fn endpoint(&self) -> String {
            "trickle".into()
        }

        fn connect(&self, _: Duration) -> Result<Trickle, ClientError> {
            Ok(self.clone())
        }
    }

    #[test]
    fn each_write_call_and_the_end_of_the_request_get_the_time_that_is_left() {
        let trickle = Trickle::new(Duration::ZERO);
        let client = Client::new(trickle.clone());
        let _ = client.send(&Request::Pause);
        let events = trickle.events();
        let writes = events
            .iter()
            .filter(|e| matches!(e, Event::Write(_)))
            .count();
        assert_eq!(writes, encode_request(&Request::Pause).len() + 4);
        let mut last = Duration::MAX;
        for (at, event) in events.iter().enumerate() {
            if matches!(
                event,
                Event::Write(_) | Event::Flush | Event::End | Event::Read
            ) {
                let Some(Event::Timeout(given)) = at.checked_sub(1).map(|i| events[i].clone())
                else {
                    panic!("no timeout before event {at}: {event:?} in {events:?}");
                };
                assert!(given <= last, "{given:?} after {last:?}");
                assert!(given <= Deadlines::V1.pause);
                last = given;
            }
        }
    }

    #[test]
    fn a_deadline_that_passes_between_two_writes_ends_the_request_and_reads_no_reply() {
        let trickle = Trickle::new(Duration::from_millis(5));
        let mut deadlines = Deadlines::V1;
        deadlines.pause = Duration::from_millis(60);
        let client = Client::with_deadlines(trickle.clone(), deadlines);
        match client.send(&Request::Pause) {
            Err(ClientError::Deadline { seconds }) => assert_eq!(seconds, 0),
            other => panic!("{other:?}"),
        }
        let events = trickle.events();
        assert!(!events.contains(&Event::Read), "{events:?}");
        assert!(!events.contains(&Event::End), "{events:?}");
        let writes = events
            .iter()
            .filter(|e| matches!(e, Event::Write(_)))
            .count();
        assert!(
            writes < encode_request(&Request::Pause).len() + 4,
            "{writes}"
        );
    }

    /// What a [`Held`] transport does once its bytes are read.
    #[derive(Debug, Clone, Copy)]
    enum Then {
        /// Every read times out, as a socket whose peer keeps the connection.
        Silence,
        /// One more byte, then the end.
        Byte,
        /// An error of the transport that is no timeout.
        Fails,
        /// The end of the stream.
        End,
        /// One read that a signal interrupts, then every read times out.
        Interrupted,
        /// Every set_timeout fails, and a read would give one more byte.
        TimeoutFails,
        /// The read that gives the frame's last byte returns only after the time left that the
        /// client set before it has passed; then every read times out.
        LastByteAtTheDeadline,
    }

    /// A transport that gives `bytes` and then does as `then` says, and records each timeout
    /// that it is asked for.
    #[derive(Clone)]
    struct Held {
        bytes: Vec<u8>,
        at: std::sync::Arc<std::sync::Mutex<usize>>,
        then: Then,
        timeouts: std::sync::Arc<std::sync::Mutex<Vec<Duration>>>,
        after_frame: std::sync::Arc<std::sync::Mutex<AfterFrame>>,
    }

    /// What the client asked of a [`Held`] transport once the frame's bytes were read.
    #[derive(Debug, Default, Clone, PartialEq, Eq)]
    struct AfterFrame {
        timeouts: Vec<Duration>,
        reads: usize,
    }

    impl Held {
        fn new(bytes: Vec<u8>, then: Then) -> Self {
            Self {
                bytes,
                at: std::sync::Arc::default(),
                then,
                timeouts: std::sync::Arc::default(),
                after_frame: std::sync::Arc::default(),
            }
        }

        fn last_timeout(&self) -> Duration {
            *self.timeouts.lock().unwrap().last().unwrap()
        }

        fn after_frame(&self) -> AfterFrame {
            self.after_frame.lock().unwrap().clone()
        }
    }

    impl Read for Held {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            let mut at = self.at.lock().unwrap();
            if *at < self.bytes.len() {
                let n = buf.len().min(self.bytes.len() - *at);
                if matches!(self.then, Then::LastByteAtTheDeadline) && *at + n == self.bytes.len() {
                    std::thread::sleep(self.last_timeout() + Duration::from_millis(1));
                }
                buf[..n].copy_from_slice(&self.bytes[*at..*at + n]);
                *at += n;
                return Ok(n);
            }
            self.after_frame.lock().unwrap().reads += 1;
            match self.then {
                Then::Silence | Then::LastByteAtTheDeadline => Err(ErrorKind::WouldBlock.into()),
                Then::Interrupted if *at == self.bytes.len() => {
                    *at += 1;
                    Err(ErrorKind::Interrupted.into())
                }
                Then::Interrupted => Err(ErrorKind::WouldBlock.into()),
                Then::Byte | Then::TimeoutFails if *at == self.bytes.len() && !buf.is_empty() => {
                    *at += 1;
                    buf[0] = b'x';
                    Ok(1)
                }
                Then::Byte | Then::TimeoutFails | Then::End => Ok(0),
                Then::Fails => Err(ErrorKind::ConnectionReset.into()),
            }
        }
    }

    impl Write for Held {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Transport for Held {
        fn set_timeout(&mut self, timeout: Duration) -> std::io::Result<()> {
            self.timeouts.lock().unwrap().push(timeout);
            if *self.at.lock().unwrap() >= self.bytes.len() {
                self.after_frame.lock().unwrap().timeouts.push(timeout);
                if matches!(self.then, Then::TimeoutFails) {
                    return Err(ErrorKind::InvalidInput.into());
                }
            }
            Ok(())
        }

        fn end_request(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl Connector for Held {
        type Transport = Held;

        fn endpoint(&self) -> String {
            "held".into()
        }

        fn connect(&self, _: Duration) -> Result<Held, ClientError> {
            Ok(self.clone())
        }
    }

    fn status_frame() -> Vec<u8> {
        framed(&crate::proto::encode_reply(&Reply::Status(sample_status(
            Platform::Macos,
        ))))
    }

    #[test]
    fn the_wait_after_a_whole_reply_frame_is_250_ms() {
        assert_eq!(crate::REPLY_END_WAIT_MS, 250);
    }

    #[test]
    fn a_reply_frame_and_then_silence_is_the_reply_after_a_wait_of_the_named_constant() {
        let held = Held::new(status_frame(), Then::Silence);
        let client = Client::new(held.clone());
        assert_eq!(client.status().unwrap(), sample_status(Platform::Macos));
        assert_eq!(
            held.last_timeout(),
            Duration::from_millis(crate::REPLY_END_WAIT_MS)
        );
    }

    #[test]
    fn the_wait_after_a_reply_frame_never_passes_the_request_s_deadline() {
        let held = Held::new(status_frame(), Then::Silence);
        let mut deadlines = Deadlines::V1;
        deadlines.read = Duration::from_millis(100);
        let client = Client::with_deadlines(held.clone(), deadlines);
        match client.status() {
            Ok(status) => assert_eq!(status, sample_status(Platform::Macos)),
            Err(ClientError::Deadline { .. }) => {}
            other => panic!("{other:?}"),
        }
        for wait in held.after_frame().timeouts {
            assert!(
                wait <= Duration::from_millis(100),
                "after the reply frame the client asked its transport to wait {wait:?}, past the \
                 request's deadline of 100 ms in all. The test holds the wait that the client \
                 asks and no elapsed time: a thread that is held up makes that wait shorter or \
                 the answer Deadline, so a slow machine cannot fail it"
            );
        }
    }

    #[test]
    fn an_interrupted_read_in_the_wait_after_a_reply_frame_goes_on_with_the_time_that_is_left() {
        let held = Held::new(status_frame(), Then::Interrupted);
        let client = Client::new(held.clone());
        assert_eq!(client.status().unwrap(), sample_status(Platform::Macos));
        let asked = held.after_frame().timeouts;
        assert!(
            !asked.is_empty(),
            "the client asked for no wait after the frame"
        );
        for wait in &asked {
            assert!(
                *wait <= Duration::from_millis(crate::REPLY_END_WAIT_MS),
                "the client asked for a wait of {wait:?} after the frame, more than \
                 REPLY_END_WAIT_MS; asked in order: {asked:?}. The test holds the waits that the \
                 client asks and no elapsed time, so a slow machine only makes them shorter"
            );
        }
        for pair in asked.windows(2) {
            assert!(
                pair[1] <= pair[0],
                "after the interrupted read the client asked for {:?}, more than the {:?} before \
                 it: the wait goes on with the time that is left",
                pair[1],
                pair[0]
            );
        }
    }

    #[test]
    fn a_set_timeout_that_fails_after_a_reply_frame_ends_the_wait_and_reads_nothing_more() {
        let held = Held::new(status_frame(), Then::TimeoutFails);
        let client = Client::new(held.clone());
        assert_eq!(client.status().unwrap(), sample_status(Platform::Macos));
        assert_eq!(
            held.after_frame().reads,
            0,
            "the client read on after its set_timeout failed"
        );
    }

    #[test]
    fn a_whole_reply_frame_whose_last_byte_comes_as_the_deadline_passes_is_the_reply() {
        let held = Held::new(status_frame(), Then::LastByteAtTheDeadline);
        let mut deadlines = Deadlines::V1;
        deadlines.read = Duration::from_secs(10);
        let client = Client::with_deadlines(held.clone(), deadlines);
        let reply = client.status();
        assert_eq!(
            reply.as_ref().ok(),
            Some(&sample_status(Platform::Macos)),
            "{reply:?}: the transport gives the frame's last byte only after the time that was \
             left when the client asked for it has passed, so the deadline has passed when the \
             frame is whole and the order needs no clock of the test; the request has 10 s so \
             that a thread held up for seconds before that read cannot end it first"
        );
        assert!(held.after_frame().timeouts.is_empty());
    }

    #[test]
    fn a_byte_inside_the_wait_after_a_reply_frame_is_trailing() {
        let client = Client::new(Held::new(status_frame(), Then::Byte));
        match client.status() {
            Err(ClientError::Protocol(text)) => assert!(text.contains("bytes follow the frame")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_error_of_the_transport_after_a_reply_frame_is_the_reply() {
        let client = Client::new(Held::new(status_frame(), Then::Fails));
        assert_eq!(client.status().unwrap(), sample_status(Platform::Macos));
    }

    #[test]
    fn the_end_of_the_stream_after_a_reply_frame_is_the_reply() {
        let client = Client::new(Held::new(status_frame(), Then::End));
        assert_eq!(client.status().unwrap(), sample_status(Platform::Macos));
    }

    #[test]
    fn half_a_reply_frame_held_open_is_the_deadline_and_half_a_frame_then_the_end_is_truncated() {
        let mut half = status_frame();
        half.truncate(half.len() / 2);
        let held = Client::new(Held::new(half.clone(), Then::Silence));
        assert!(matches!(held.status(), Err(ClientError::Deadline { .. })));
        let ended = Client::new(Held::new(half, Then::End));
        match ended.status() {
            Err(ClientError::Protocol(text)) => assert!(text.contains("ended inside a frame")),
            other => panic!("{other:?}"),
        }
        let prefix = Client::new(Held::new(vec![0, 0], Then::Silence));
        assert!(matches!(prefix.status(), Err(ClientError::Deadline { .. })));
    }

    #[test]
    fn the_plain_read_sends_no_claim_and_the_claim_method_sends_one() {
        let (client, connector) = client(ScriptedDaemon::new(sample_status(Platform::Macos)));
        client.status().unwrap();
        client.status_claiming_the_indicator().unwrap();
        client.inventory().unwrap();
        client.status().unwrap();
        let claims: Vec<bool> = connector
            .requests()
            .iter()
            .map(Request::claims_the_indicator)
            .collect();
        assert_eq!(claims, [false, true, false, false]);
        assert_eq!(
            connector.requests()[0],
            Request::Status { indicator: false }
        );
        assert_eq!(connector.requests()[2], Request::Inventory);
    }

    #[test]
    fn t18_a_a_delete_with_the_id_of_an_unavailable_store_is_accepted() {
        for cause in [
            StoreErrorKind::Io,
            StoreErrorKind::NoSpace,
            StoreErrorKind::CustodyLocked,
            StoreErrorKind::RootDenied,
        ] {
            let status = with_store(StoreState::Unavailable, Some(cause), Some(store_id(5)));
            let (client, _) = client(ScriptedDaemon::new(status));
            match client.delete_all(Some(store_id(5))).unwrap() {
                DeleteOutcome::Deleted(deleted) => {
                    assert_ne!(deleted.store, Some(store_id(5)), "{cause:?}")
                }
                other => panic!("{cause:?}: {other:?}"),
            }
        }
    }

    #[test]
    fn t18_b_dash_is_accepted_where_status_shows_no_id_and_refused_beside_a_shown_id() {
        let no_id = with_store(StoreState::Sealed, Some(StoreErrorKind::KeyFile), None);
        let (client_a, _) = client(ScriptedDaemon::new(no_id));
        assert!(matches!(
            client_a.delete_all(None).unwrap(),
            DeleteOutcome::Deleted(_)
        ));
        let (client_b, connector) = client(ScriptedDaemon::new(sample_status(Platform::Macos)));
        assert_eq!(client_b.delete_all(None).unwrap(), DeleteOutcome::Refused);
        assert_eq!(
            connector.status().fields().store_id,
            Some(FIXTURE_STORE_ID),
            "a refused delete changed the store"
        );
    }

    /// The data directory that ctl is given in these tests: an absolute path on each system.
    fn ctl_data_dir() -> crate::paths::DataDir {
        crate::paths::DataDir::from_flag(if cfg!(windows) {
            r"C:\halvo-data"
        } else {
            "/halvo-data"
        })
        .unwrap()
    }

    const DELETE_REFUSED_STDOUT: &str = "delete_refused\n";
    const DELETE_REFUSED_STDERR: &str = "halvo-capture: this request deleted nothing: the store id in the request is not the id that the status shows, or one of the two is missing; an earlier delete may already have removed that store; read the status and ask again\n";

    fn ctl_delete(
        armed: Option<StoreId>,
        reply: Result<Reply, ClientError>,
    ) -> (String, String, i32) {
        let printed = crate::ctl_text::print(
            &crate::ctl_text::Verb::DeleteAll(armed),
            reply,
            &ctl_data_dir(),
        );
        (printed.stdout, printed.stderr, printed.status)
    }

    fn refused_by_ctl() -> (String, String, i32) {
        (
            DELETE_REFUSED_STDOUT.to_string(),
            DELETE_REFUSED_STDERR.to_string(),
            1,
        )
    }

    #[test]
    fn d7_an_armed_id_against_a_status_that_shows_no_id_is_delete_refused_with_exit_1_and_deletes_nothing(
    ) {
        for status in [
            with_store(StoreState::Unavailable, Some(StoreErrorKind::Io), None),
            with_store(StoreState::Sealed, Some(StoreErrorKind::KeyFile), None),
        ] {
            let (client, connector) = client(ScriptedDaemon::new(status.clone()));
            let armed = Some(store_id(5));
            let reply = client.send(&Request::DeleteAll { store: armed });
            assert_eq!(reply.as_ref().unwrap().kind(), "delete_refused");
            assert_eq!(ctl_delete(armed, reply), refused_by_ctl());
            assert_eq!(connector.status(), status);
        }
    }

    #[test]
    fn d10_a_second_dash_after_a_dash_delete_whose_reply_was_lost_is_delete_refused_with_exit_1() {
        let status = with_store(StoreState::Unavailable, Some(StoreErrorKind::Io), None);
        let (client, connector) = client(ScriptedDaemon::new(status));
        let lost = client.send(&Request::DeleteAll { store: None }).unwrap();
        assert_eq!(lost.kind(), "deleted");
        let fresh = connector.status().fields().store_id;
        assert!(fresh.is_some());
        let again = client.send(&Request::DeleteAll { store: None });
        assert_eq!(again.as_ref().unwrap().kind(), "delete_refused");
        assert_eq!(ctl_delete(None, again), refused_by_ctl());
        assert_eq!(connector.status().fields().store_id, fresh);
    }

    fn already_deleted_frame(current: StoreId) -> Vec<u8> {
        let body =
            format!(r#"{{"v":1,"body":{{"kind":"already_deleted","current":"{current}"}}}}"#);
        framed(body.as_bytes())
    }

    #[test]
    fn c3_an_already_deleted_that_names_the_armed_id_or_answers_no_id_is_an_error_with_exit_1() {
        for armed in [Some(FIXTURE_STORE_ID), None] {
            let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
            daemon.set_raw_reply(already_deleted_frame(FIXTURE_STORE_ID));
            let (client, _) = client(daemon);
            assert!(client.delete_all(armed).is_err(), "{armed:?}");
            let reply = client.send(&Request::DeleteAll { store: armed });
            assert_eq!(reply.as_ref().unwrap().kind(), "already_deleted");
            let (_, stderr, status) = ctl_delete(armed, reply);
            assert_eq!(status, 1, "{armed:?}");
            assert!(stderr.starts_with("halvo-capture: "), "{stderr:?}");
        }
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        daemon.set_raw_reply(already_deleted_frame(FIXTURE_STORE_ID));
        let (client, _) = client(daemon);
        assert!(client.delete_all(Some(store_id(7))).is_ok());
    }

    #[test]
    fn no_id_on_an_open_or_no_release_custody_store_that_shows_no_id_is_delete_refused() {
        for state in [StoreState::Open, StoreState::NoReleaseCustody] {
            let status = with_store(state, None, None);
            let (client, connector) = client(ScriptedDaemon::new(status.clone()));
            let reply = client.send(&Request::DeleteAll { store: None }).unwrap();
            assert_eq!(reply.kind(), "delete_refused", "{state:?}");
            assert_eq!(connector.status(), status);
        }
    }

    #[test]
    fn no_id_on_a_sealed_key_file_or_an_unavailable_io_store_that_shows_no_id_is_accepted() {
        for status in [
            with_store(StoreState::Sealed, Some(StoreErrorKind::KeyFile), None),
            with_store(StoreState::Unavailable, Some(StoreErrorKind::Io), None),
        ] {
            let (client, _) = client(ScriptedDaemon::new(status));
            let reply = client.send(&Request::DeleteAll { store: None }).unwrap();
            assert_eq!(reply.kind(), "deleted");
        }
    }

    #[test]
    fn an_already_deleted_frame_with_a_null_id_does_not_decode() {
        assert!(
            decode_reply(br#"{"v":1,"body":{"kind":"already_deleted","current":null}}"#).is_err()
        );
    }

    #[test]
    fn delete_refused_decodes_from_an_empty_body_and_ctl_prints_its_two_lines_with_exit_1() {
        let reply = decode_reply(br#"{"v":1,"body":{"kind":"delete_refused"}}"#).unwrap();
        assert_eq!(reply.kind(), "delete_refused");
        assert_eq!(
            crate::proto::encode_reply(&reply),
            br#"{"v":1,"body":{"kind":"delete_refused"}}"#
        );
        assert_eq!(
            ctl_delete(Some(store_id(5)), Ok(reply.clone())),
            refused_by_ctl()
        );
        assert_eq!(ctl_delete(None, Ok(reply)), refused_by_ctl());
    }

    /// A reply frame of `kind` whose status has the fields of the sample after `edit`, with no
    /// check of the status rule.
    fn unchecked_status_frame(
        kind: &str,
        edit: impl FnOnce(&mut crate::proto::StatusFields),
    ) -> Vec<u8> {
        let mut fields = sample_status(Platform::Macos).into_fields();
        edit(&mut fields);
        let mut body = serde_json::to_value(&fields).unwrap();
        body.as_object_mut()
            .unwrap()
            .insert("kind".into(), serde_json::Value::from(kind));
        let frame = serde_json::json!({"v": 1, "body": body});
        framed(frame.to_string().as_bytes())
    }

    fn paused_beside_running(f: &mut crate::proto::StatusFields) {
        f.capture = crate::proto::Capture::Paused;
        stop_all(f);
    }

    fn faulted_beside_a_lapsed_lease(f: &mut crate::proto::StatusFields) {
        f.capture = crate::proto::Capture::Faulted;
        f.capture_reason = Some(crate::proto::CaptureReason::NoSource);
        f.lease = crate::proto::Lease::Lapsed;
        stop_all(f);
    }

    const NOT_TOGETHER: &str = "the capture service reported a state that does not hold together: ";

    #[test]
    fn a_reply_whose_status_breaks_a_rule_of_the_type_is_its_own_error_through_status_and_pause() {
        for (edit, rule) in [
            (
                paused_beside_running as fn(&mut crate::proto::StatusFields),
                "paused has desired paused",
            ),
            (
                faulted_beside_a_lapsed_lease,
                "faulted has desired running and a fresh lease",
            ),
        ] {
            let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
            daemon.set_raw_reply(unchecked_status_frame("status", edit));
            let (read, _) = client(daemon);
            let error = read.status().unwrap_err();
            assert_eq!(error.to_string(), format!("{NOT_TOGETHER}{rule}"));
            let printed = crate::ctl_text::print(
                &crate::ctl_text::Verb::Status { indicator: false },
                Err(error),
                &ctl_data_dir(),
            );
            assert_eq!(
                (printed.stdout.as_str(), printed.stderr, printed.status),
                ("", format!("halvo-capture: {NOT_TOGETHER}{rule}\n"), 1)
            );

            let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
            daemon.set_raw_reply(unchecked_status_frame("capture_changed", edit));
            let (paused, _) = client(daemon);
            assert_eq!(
                paused.pause().unwrap_err().to_string(),
                format!("{NOT_TOGETHER}{rule}")
            );
        }
    }

    #[test]
    fn a_cap_set_whose_status_breaks_a_rule_of_the_type_is_its_own_error() {
        let mut fields = sample_status(Platform::Macos).into_fields();
        paused_beside_running(&mut fields);
        let body = serde_json::json!({
            "kind": "cap_set",
            "evicting": false,
            "status": serde_json::to_value(&fields).unwrap(),
        });
        let frame = serde_json::json!({"v": 1, "body": body});
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        daemon.set_raw_reply(framed(frame.to_string().as_bytes()));
        let (client, _) = client(daemon);
        assert_eq!(
            client
                .set_storage_cap(StorageCap::MIN)
                .unwrap_err()
                .to_string(),
            format!("{NOT_TOGETHER}paused has desired paused")
        );
    }

    #[test]
    fn a_reply_frame_that_cannot_be_read_stays_a_protocol_error() {
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        daemon.set_raw_reply(framed(br#"{"v":1,"body":{"kind":"status","pid":1}}"#));
        let (client, _) = client(daemon);
        assert!(matches!(client.status(), Err(ClientError::Protocol(_))));
    }

    fn ctl_line_for_status(error: ClientError) -> (String, String, i32) {
        let printed = crate::ctl_text::print(
            &crate::ctl_text::Verb::Status { indicator: false },
            Err(error),
            &ctl_data_dir(),
        );
        (printed.stdout, printed.stderr, printed.status)
    }

    #[test]
    fn a_reply_of_another_version_is_its_own_error_and_ctl_prints_the_line_that_it_printed_before()
    {
        for (body, version) in [
            (r#"{"v":2,"body":{"kind":"status"}}"#, 2),
            (r#"{"v":0,"body":{}}"#, 0),
            (r#"{"v":18446744073709551615}"#, u64::MAX),
        ] {
            let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
            daemon.set_raw_reply(framed(body.as_bytes()));
            let (client, _) = client(daemon);
            let error = client.status().unwrap_err();
            assert!(
                matches!(error, ClientError::ReplyOfAnotherVersion { version: v } if v == version),
                "{body}: {error:?}"
            );
            assert_eq!(
                ctl_line_for_status(error),
                (
                    String::new(),
                    format!("halvo-capture: the reply could not be read: the reply is of protocol version {version}, not 1\n"),
                    1
                ),
                "{body}"
            );
        }
    }

    #[test]
    fn a_reply_with_a_value_that_v1_does_not_know_is_its_own_error_and_ctl_prints_the_line_that_it_printed_before(
    ) {
        let status = |pointer: &str| {
            let mut json = serde_json::to_value(Reply::Status(sample_status(Platform::Macos)))
                .expect("the sample status encodes");
            *json.pointer_mut(pointer).expect("a field of the status") =
                serde_json::Value::from("not_in_v1");
            serde_json::json!({ "v": 1, "body": json }).to_string()
        };
        for (body, list) in [
            (status("/capture"), "capture state"),
            (status("/store"), "store state"),
            (status("/sources/0/state"), "source state"),
            (status("/kinds/1/name"), "kind"),
            (status("/custody"), "custody"),
            (
                r#"{"v":1,"body":{"kind":"error","code":"not_in_v1","message":"x"}}"#.to_string(),
                "error code",
            ),
        ] {
            let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
            daemon.set_raw_reply(framed(body.as_bytes()));
            let (client, _) = client(daemon);
            let error = client.status().unwrap_err();
            assert!(
                matches!(&error, ClientError::ValueUnknownToV1(outside) if outside.to_string() == format!("not a spelling of a {list}")),
                "{list}: {error:?}"
            );
            assert_eq!(
                ctl_line_for_status(error),
                (
                    String::new(),
                    format!("halvo-capture: the reply could not be read: the reply does not fit protocol version 1: not a spelling of a {list}\n"),
                    1
                ),
                "{list}"
            );
        }
    }

    #[test]
    fn a_broken_reply_stays_a_protocol_error_beside_the_two_typed_errors() {
        let mut malformed_version =
            serde_json::to_value(Reply::Status(sample_status(Platform::Macos))).unwrap();
        malformed_version["daemon_version"] = serde_json::Value::from("1.2");
        for body in [
            "{".to_string(),
            r#"{"v":1,"body":{"kind":"status","pid":1}}"#.to_string(),
            r#"{"v":1,"body":{"kind":"not_in_v1"}}"#.to_string(),
            r#"{"v":1,"body":{"kind":"error","code":7,"message":"x"}}"#.to_string(),
            r#"{"v":"2","body":{"kind":"status"}}"#.to_string(),
            serde_json::json!({ "v": 1, "body": malformed_version }).to_string(),
        ] {
            let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
            daemon.set_raw_reply(framed(body.as_bytes()));
            let (client, _) = client(daemon);
            let error = client.status().unwrap_err();
            assert!(
                matches!(error, ClientError::Protocol(_)),
                "{body}: {error:?}"
            );
        }
    }

    #[test]
    fn e14_the_scripted_store_shows_obstructed_and_takes_a_delete_with_the_shown_id() {
        let mut json = serde_json::to_value(sample_status(Platform::Macos).into_fields()).unwrap();
        json["store"] = serde_json::Value::from("unavailable");
        json["store_error"] = serde_json::Value::from("obstructed");
        json["store_retry_in_s"] = serde_json::Value::from(1);
        let obstructed: DaemonStatus = serde_json::from_value(json).unwrap();
        let (client, _) = client(ScriptedDaemon::new(obstructed.clone()));
        assert_eq!(client.status().unwrap(), obstructed);
        assert!(matches!(
            client.delete_all(Some(FIXTURE_STORE_ID)).unwrap(),
            DeleteOutcome::Deleted(_)
        ));
    }

    #[test]
    fn t18_c_a_deleted_reply_with_no_new_store_reaches_the_caller() {
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        daemon.set_fresh_store(None);
        let (client, connector) = client(daemon);
        match client.delete_all(Some(FIXTURE_STORE_ID)).unwrap() {
            DeleteOutcome::Deleted(deleted) => assert_eq!(deleted.store, None),
            other => panic!("{other:?}"),
        }
        assert_eq!(connector.status().fields().store_id, None);
    }

    #[test]
    fn t18_e_a_locked_or_deleting_store_answers_busy_and_deletes_nothing() {
        for status in [
            with_store(
                StoreState::Unavailable,
                Some(StoreErrorKind::Locked),
                Some(store_id(5)),
            ),
            with_store(StoreState::Deleting, None, Some(store_id(5))),
        ] {
            let (client, connector) = client(ScriptedDaemon::new(status.clone()));
            assert_eq!(
                daemon_error(client.delete_all(Some(store_id(5)))),
                ErrorCode::Busy
            );
            assert_eq!(connector.status(), status);
        }
    }

    #[test]
    fn a_delete_with_no_id_while_the_store_opens_with_no_id_answers_busy_and_deletes_nothing() {
        let opening = with_store(StoreState::Opening, None, None);
        let (client, connector) = client(ScriptedDaemon::new(opening.clone()));
        assert_eq!(daemon_error(client.delete_all(None)), ErrorCode::Busy);
        assert_eq!(connector.status(), opening);
        assert_eq!(
            client.delete_all(Some(store_id(5))).unwrap(),
            DeleteOutcome::Refused
        );
        assert_eq!(connector.status(), opening);
    }

    #[test]
    fn t18_f_after_a_delete_the_old_id_is_already_deleted_and_the_new_store_stays() {
        let (client, connector) = client(ScriptedDaemon::new(sample_status(Platform::Macos)));
        let armed = Some(FIXTURE_STORE_ID);
        let DeleteOutcome::Deleted(first) = client.delete_all(armed).unwrap() else {
            panic!("the first delete was not accepted");
        };
        let fresh = first.store.expect("a fresh store");
        assert_ne!(fresh, FIXTURE_STORE_ID);
        for _ in 0..2 {
            assert_eq!(
                client.delete_all(armed).unwrap(),
                DeleteOutcome::AlreadyDeleted(crate::proto::AlreadyDeleted { current: fresh })
            );
            assert_eq!(connector.status().fields().store_id, Some(fresh));
        }
        let deletes = connector
            .requests()
            .iter()
            .filter(|r| matches!(r, Request::DeleteAll { store } if *store == armed))
            .count();
        assert_eq!(deletes, 3, "the client read the id itself");
    }

    #[test]
    fn a_stop_reports_its_flush_and_its_loss() {
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        daemon.set_stop_reply(crate::proto::Stopped {
            flushed: false,
            events_lost: 3,
        });
        let (client, connector) = client(daemon);
        let stopped = client.stop().unwrap();
        assert!(!stopped.flushed);
        assert_eq!(stopped.events_lost, 3);
        assert_eq!(connector.requests(), [Request::Stop]);
    }

    #[test]
    fn a_mutation_while_another_is_in_flight_is_busy_and_a_read_is_answered() {
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        daemon.set_mutation_in_flight(true);
        let (client, connector) = client(daemon);
        assert_eq!(daemon_error(client.pause()), ErrorCode::Busy);
        assert_eq!(daemon_error(client.resume()), ErrorCode::Busy);
        assert_eq!(daemon_error(client.stop()), ErrorCode::Busy);
        assert_eq!(
            daemon_error(client.delete_all(Some(FIXTURE_STORE_ID))),
            ErrorCode::Busy
        );
        assert_eq!(
            daemon_error(client.set_storage_cap(StorageCap::MIN)),
            ErrorCode::Busy
        );
        assert!(client.status().is_ok());
        assert!(client.inventory().is_ok());
        assert_eq!(connector.status().fields().store_id, Some(FIXTURE_STORE_ID));
    }

    #[test]
    fn pause_and_resume_answer_the_status_after_they_settle() {
        let (client, _) = client(ScriptedDaemon::new(sample_status(Platform::Macos)));
        let paused = client.pause().unwrap();
        assert_eq!(paused.fields().capture, crate::proto::Capture::Paused);
        assert_eq!(paused.fields().desired, crate::model::CaptureState::Paused);
        let running = client.resume().unwrap();
        assert_eq!(running.fields().capture, crate::proto::Capture::Running);
        assert_eq!(
            running.fields().desired,
            crate::model::CaptureState::Running
        );
    }

    #[test]
    fn set_cap_answers_the_cap_and_whether_it_evicts() {
        let (client, _) = client(ScriptedDaemon::new(sample_status(Platform::Macos)));
        let set = client.set_storage_cap(StorageCap::MIN).unwrap();
        assert_eq!(
            set.status.fields().cap_bytes,
            crate::proto::Cap::Known(StorageCap::MIN)
        );
        assert!(!set.evicting);
    }

    #[test]
    fn form_a_serves_the_request_and_refuses_the_record() {
        let (client, _) = client(ScriptedDaemon::new(sample_status(Platform::Macos)));
        assert_eq!(
            client
                .request_permission(Permission::Accessibility)
                .unwrap(),
            RequestOutcome::AlreadyGranted
        );
        assert_eq!(
            daemon_error(client.permission_asked(Permission::Accessibility)),
            ErrorCode::UnsupportedOp
        );
    }

    #[test]
    fn form_b_records_the_ask_and_refuses_the_request() {
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        daemon.set_form(PermissionForm::B);
        let (client, _) = client(daemon);
        assert_eq!(
            daemon_error(client.request_permission(Permission::Accessibility)),
            ErrorCode::UnsupportedOp
        );
        assert!(client.permission_asked(Permission::Accessibility).is_ok());
    }

    fn lost_screen_status() -> DaemonStatus {
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.permissions[0].grant = GrantState::NotGranted;
        fields.onboarding = crate::testing::onboarding_of(Platform::Macos, &fields.permissions);
        DaemonStatus::new(fields).unwrap()
    }

    #[test]
    fn a_request_for_a_missing_permission_is_requested_and_recorded_as_asked() {
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.permissions[1].grant = GrantState::NotDetermined;
        fields.permissions[1].asked = false;
        fields.onboarding = crate::testing::onboarding_of(Platform::Macos, &fields.permissions);
        let (client, connector) = client(ScriptedDaemon::new(DaemonStatus::new(fields).unwrap()));
        assert_eq!(
            client
                .request_permission(Permission::Accessibility)
                .unwrap(),
            RequestOutcome::Requested
        );
        assert!(connector.status().fields().permissions[1].asked);
    }

    #[test]
    fn an_acknowledge_clears_the_lost_notice_and_leaves_the_permission_not_granted() {
        let status = lost_screen_status();
        let before = &status.fields().onboarding.permissions[0];
        assert!(before.lost);
        let (client, _) = client(ScriptedDaemon::new(status));
        let after = client
            .acknowledge_lost_permission(Permission::ScreenAndSystemAudioRecording)
            .unwrap();
        let row = &after.fields().onboarding.permissions[0];
        assert_eq!(row.permission, Permission::ScreenAndSystemAudioRecording);
        assert!(!row.lost);
        assert_eq!(row.grant, GrantState::NotGranted);
        assert!(!after.fields().permissions[0].once_granted);
        assert!(after.fields().onboarding.lost_grants.is_empty());
    }

    #[test]
    fn a_reply_section_that_no_one_knows_reaches_the_caller_unchanged() {
        let x = Section {
            name: "x".parse().unwrap(),
            rows: vec![
                SectionRow {
                    k: "a".into(),
                    v: "1".into(),
                },
                SectionRow {
                    k: "b".into(),
                    v: "2".into(),
                },
            ],
        };
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        daemon.set_inventory(sample_inventory(vec![x.clone()]));
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.ext = vec![x.clone()];
        daemon.set_status(DaemonStatus::new(fields).unwrap());
        let (client, _) = client(daemon);
        assert_eq!(client.inventory().unwrap().ext(), std::slice::from_ref(&x));
        assert_eq!(client.status().unwrap().fields().ext, [x]);
    }

    #[test]
    fn each_request_waits_for_its_own_deadline_and_a_reply_that_never_comes_is_a_deadline() {
        let cases: [(Request, u64); 10] = [
            (Request::Status { indicator: false }, 2_000),
            (Request::Inventory, 2_000),
            (Request::Pause, 10_000),
            (Request::Resume, 20_000),
            (Request::Stop, crate::STOP_WAIT_MS),
            (Request::DeleteAll { store: None }, 120_000),
            (
                Request::SetStorageCap {
                    bytes: StorageCap::MIN,
                },
                5_000,
            ),
            (
                Request::RequestPermission {
                    permission: Permission::Accessibility,
                },
                2_000,
            ),
            (
                Request::PermissionAsked {
                    permission: Permission::Accessibility,
                },
                2_000,
            ),
            (
                Request::AcknowledgeLostPermission {
                    permission: Permission::Accessibility,
                },
                10_000,
            ),
        ];
        for (request, ms) in cases {
            assert_eq!(Deadlines::V1.of(&request), Duration::from_millis(ms));
            let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
            daemon.set_silent(true);
            let (client, connector) = client(daemon);
            match client.send(&request) {
                Err(ClientError::Deadline { seconds }) => assert_eq!(seconds, ms / 1_000),
                other => panic!("{}: {other:?}", request.op()),
            }
            let waited = connector.longest_timeout().expect("a timeout was set");
            assert!(
                waited <= Duration::from_millis(ms) && waited > Duration::from_millis(ms - 1_000),
                "{}: {waited:?}",
                request.op()
            );
        }
    }

    #[test]
    fn the_stop_wait_is_20_seconds_and_the_lease_three_missed_polls_plus_one_second() {
        assert_eq!(crate::POLL_MS, 3_000);
        assert_eq!(crate::INDICATOR_LEASE_MS, 10_000);
        assert_eq!(crate::INDICATOR_LEASE_MS, 3 * crate::POLL_MS + 1_000);
        assert_eq!(crate::STOP_WAIT_MS, 20_000);
        assert_eq!(crate::SESSION_END_BUDGET_MS, 3_000);
    }

    fn bound(key: &str) -> u64 {
        let text = include_str!("../../../docs/baseline/r1-gate-bounds.txt");
        let lines: Vec<&str> = text
            .lines()
            .filter_map(|line| line.strip_prefix(key)?.strip_prefix('='))
            .collect();
        assert_eq!(lines.len(), 1, "{key}");
        lines[0].parse().unwrap()
    }

    #[test]
    fn the_bounds_keys_of_the_lease_and_the_stop_are_the_constants() {
        assert_eq!(bound("lease_bound_s") * 1_000, crate::INDICATOR_LEASE_MS);
        assert_eq!(bound("stop_deadline_s") * 1_000, crate::STOP_WAIT_MS);
    }

    #[test]
    fn the_exit_statuses_have_one_meaning_each() {
        use crate::exit::*;
        let all = [
            CLEAN,
            ERROR,
            TIMED_RUN_INTERRUPTED,
            TIMED_RUN_SOURCE_FAILED,
            RESTART,
            SOURCE_NOT_STOPPED,
            USAGE,
            UNREACHABLE,
            BUSY,
        ];
        assert_eq!(all, [0, 1, 2, 3, 4, 5, 64, 69, 75]);
        assert_eq!(FINAL_FLUSH_FAILED, ERROR);
    }

    #[test]
    fn a_reply_over_the_limit_is_a_protocol_error_and_never_a_panic() {
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        let mut frame = (crate::wire::REPLY_MAX_BYTES as u32 + 1)
            .to_be_bytes()
            .to_vec();
        frame.extend(std::iter::repeat_n(b' ', 16));
        daemon.set_raw_reply(frame);
        let (client, _) = client(daemon);
        assert!(matches!(client.status(), Err(ClientError::Protocol(_))));
    }

    #[test]
    fn a_server_whose_reply_would_pass_the_limit_answers_internal() {
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        let sections: Vec<Section> = (0..2_000)
            .map(|i| Section {
                name: format!("s{i}").parse().unwrap(),
                rows: vec![SectionRow {
                    k: "k".repeat(64),
                    v: "v".repeat(64),
                }],
            })
            .collect();
        daemon.set_inventory(sample_inventory(sections));
        let (client, _) = client(daemon);
        assert_eq!(daemon_error(client.inventory()), ErrorCode::Internal);
    }

    fn scripted(edit: impl FnOnce(&mut crate::proto::StatusFields)) -> ScriptedDaemon {
        let mut fields = sample_status(Platform::Macos).into_fields();
        edit(&mut fields);
        ScriptedDaemon::new(DaemonStatus::new(fields).unwrap())
    }

    fn stop_all(f: &mut crate::proto::StatusFields) {
        for line in &mut f.sources {
            if line.state != crate::proto::SourceState::Off {
                line.state = crate::proto::SourceState::Stopped { by_user: false };
            }
        }
    }

    fn lapsed(f: &mut crate::proto::StatusFields) {
        f.lease = crate::proto::Lease::Lapsed;
        f.capture = crate::proto::Capture::NoIndicator;
        stop_all(f);
    }

    fn paused(f: &mut crate::proto::StatusFields) {
        f.desired = crate::model::CaptureState::Paused;
        f.capture = crate::proto::Capture::Paused;
        stop_all(f);
    }

    fn input_lost(f: &mut crate::proto::StatusFields) {
        f.sources[2].state = crate::proto::SourceState::Failed {
            reason: crate::proto::Reason::Permission,
            error: None,
        };
    }

    fn running_sources(status: &DaemonStatus) -> usize {
        status
            .fields()
            .sources
            .iter()
            .filter(|s| s.state == crate::proto::SourceState::Running)
            .count()
    }

    #[test]
    fn d1_a_resume_under_a_lapsed_lease_reads_no_indicator_with_no_source_running() {
        let (client, _) = client(scripted(lapsed));
        let after = client.resume().unwrap();
        assert_eq!(after.fields().capture, crate::proto::Capture::NoIndicator);
        assert_eq!(running_sources(&after), 0);
    }

    #[test]
    fn d2_a_pause_then_a_resume_under_a_lapsed_lease_read_no_indicator() {
        let (client, _) = client(scripted(lapsed));
        client.pause().unwrap();
        let resumed = client.resume().unwrap();
        assert_eq!(resumed.fields().capture, crate::proto::Capture::NoIndicator);
        assert_eq!(running_sources(&resumed), 0);
    }

    #[test]
    fn d3_a_resume_of_a_partial_capture_answers_partial_and_the_next_request() {
        let (client, connector) = client(scripted(|f| {
            input_lost(f);
            f.capture = crate::proto::Capture::Partial;
        }));
        let after = client.resume().unwrap();
        assert_eq!(after.fields().capture, crate::proto::Capture::Partial);
        assert!(client.status().is_ok());
        assert_eq!(connector.requests().len(), 2);
    }

    #[test]
    fn d4_a_claim_under_no_indicator_with_a_failed_source_answers_a_status_that_holds_together() {
        let (client, _) = client(scripted(|f| {
            lapsed(f);
            input_lost(f);
        }));
        let claimed = client.status_claiming_the_indicator().unwrap();
        assert_eq!(claimed.fields().lease, crate::proto::Lease::Fresh);
        assert_eq!(claimed.fields().capture, crate::proto::Capture::Partial);
        assert!(DaemonStatus::new(claimed.into_fields()).is_ok());
        assert!(client.status().is_ok());
    }

    #[test]
    fn d5_a_resume_of_a_pause_with_a_failed_source_answers_a_status_that_holds_together() {
        let (client, _) = client(scripted(|f| {
            paused(f);
            input_lost(f);
        }));
        let resumed = client.resume().unwrap();
        assert_eq!(resumed.fields().capture, crate::proto::Capture::Partial);
        assert!(DaemonStatus::new(resumed.into_fields()).is_ok());
        assert!(client.status().is_ok());
    }

    fn capture(status: &DaemonStatus) -> crate::proto::Capture {
        status.fields().capture
    }

    fn pausing(f: &mut crate::proto::StatusFields) {
        f.capture = crate::proto::Capture::Pausing;
        f.capture_reason = None;
        stop_all(f);
        f.sources[2].state = crate::proto::SourceState::Failed {
            reason: crate::proto::Reason::Timeout,
            error: None,
        };
    }

    #[test]
    fn once_capture_reads_pausing_a_claim_a_pause_and_a_resume_read_pausing_and_start_no_source() {
        let (client, _) = client(scripted(|f| {
            f.lease = crate::proto::Lease::Lapsed;
            pausing(f);
        }));
        let claimed = client.status_claiming_the_indicator().unwrap();
        assert_eq!(capture(&claimed), crate::proto::Capture::Pausing);
        assert_eq!(running_sources(&claimed), 0);
        let resumed = client.resume().unwrap();
        assert_eq!(capture(&resumed), crate::proto::Capture::Pausing);
        assert_eq!(running_sources(&resumed), 0);
        let paused = client.pause().unwrap();
        assert_eq!(capture(&paused), crate::proto::Capture::Pausing);
        let stored = client.status().unwrap();
        assert_eq!(capture(&stored), crate::proto::Capture::Pausing);
        assert_eq!(running_sources(&stored), 0);
    }

    #[test]
    fn a_second_pause_while_capture_reads_pausing_prints_pausing_the_c2_5_line_and_exit_1() {
        let (client, _) = client(scripted(|f| {
            f.desired = crate::model::CaptureState::Paused;
            pausing(f);
        }));
        let reply = client.pause().unwrap();
        assert_eq!(capture(&reply), crate::proto::Capture::Pausing);
        let printed = crate::ctl_text::print(
            &crate::ctl_text::Verb::Pause,
            Ok(Reply::CaptureChanged(reply)),
            &ctl_data_dir(),
        );
        assert_eq!(printed.stdout, "capture=pausing\n");
        assert_eq!(
            printed.stderr,
            "halvo-capture: capture did not stop in time; the capture service is ending\n"
        );
        assert_eq!(printed.status, 1);
    }

    fn held(f: &mut crate::proto::StatusFields) {
        lapsed(f);
        f.capture = crate::proto::Capture::AwaitingResume;
    }

    #[test]
    fn a_claim_on_awaiting_resume_renews_the_lease_starts_no_source_and_keeps_the_hold() {
        let (client, _) = client(scripted(held));
        let claimed = client.status_claiming_the_indicator().unwrap();
        assert_eq!(claimed.fields().lease, crate::proto::Lease::Fresh);
        assert_eq!(capture(&claimed), crate::proto::Capture::AwaitingResume);
        assert_eq!(running_sources(&claimed), 0);
        assert_eq!(
            capture(&client.status().unwrap()),
            crate::proto::Capture::AwaitingResume
        );
        let resumed = client.resume().unwrap();
        assert_eq!(capture(&resumed), crate::proto::Capture::Running);
        assert_eq!(resumed, sample_status(Platform::Macos));
    }

    #[test]
    fn the_hold_reads_paused_under_a_pause_and_the_resume_that_ends_the_pause_clears_it() {
        let (client, _) = client(scripted(held));
        let paused = client.pause().unwrap();
        assert_eq!(capture(&paused), crate::proto::Capture::Paused);
        let resumed = client.resume().unwrap();
        assert_eq!(capture(&resumed), crate::proto::Capture::NoIndicator);
        assert_eq!(running_sources(&resumed), 0);
        assert_eq!(
            capture(&client.status().unwrap()),
            crate::proto::Capture::NoIndicator
        );
        let claimed = client.status_claiming_the_indicator().unwrap();
        assert_eq!(capture(&claimed), crate::proto::Capture::Running);
    }

    #[test]
    fn a_resume_restarts_a_stream_the_person_stopped_and_a_claim_does_not() {
        let user_stopped = crate::proto::SourceState::Stopped { by_user: true };
        let (client, _) = client(scripted(|f| {
            f.enabled_sources
                .insert(1, crate::proto::SourceName::Screen);
            lapsed(f);
            f.sources[1].state = user_stopped.clone();
        }));
        let claimed = client.status_claiming_the_indicator().unwrap();
        assert_eq!(claimed.fields().sources[1].state, user_stopped);
        assert_eq!(capture(&claimed), crate::proto::Capture::Partial);
        let resumed = client.resume().unwrap();
        assert_eq!(
            resumed.fields().sources[1].state,
            crate::proto::SourceState::Running
        );
        assert_eq!(capture(&resumed), crate::proto::Capture::Running);
    }

    fn no_source_can_run(f: &mut crate::proto::StatusFields) {
        for line in &mut f.sources {
            if line.state != crate::proto::SourceState::Off {
                line.state = crate::proto::SourceState::Failed {
                    reason: crate::proto::Reason::Permission,
                    error: None,
                };
            }
        }
    }

    #[test]
    fn a_resume_of_a_faulted_capture_keeps_the_reason_that_stood() {
        let (client, _) = client(scripted(|f| {
            no_source_can_run(f);
            f.capture = crate::proto::Capture::Faulted;
            f.capture_reason = Some(crate::proto::CaptureReason::Permission);
        }));
        let resumed = client.resume().unwrap();
        assert_eq!(capture(&resumed), crate::proto::Capture::Faulted);
        assert_eq!(
            resumed.fields().capture_reason,
            Some(crate::proto::CaptureReason::Permission)
        );
    }

    #[test]
    fn a_claim_that_finds_no_source_able_to_run_and_no_reason_reads_faulted_no_source() {
        let (client, _) = client(scripted(|f| {
            lapsed(f);
            no_source_can_run(f);
        }));
        let claimed = client.status_claiming_the_indicator().unwrap();
        assert_eq!(capture(&claimed), crate::proto::Capture::Faulted);
        assert_eq!(
            claimed.fields().capture_reason,
            Some(crate::proto::CaptureReason::NoSource)
        );
    }

    #[test]
    fn with_every_source_off_a_claim_and_a_resume_read_running_as_built() {
        let (client, _) = client(scripted(|f| {
            f.enabled_sources.clear();
            for line in &mut f.sources {
                line.state = crate::proto::SourceState::Off;
            }
            f.lease = crate::proto::Lease::Lapsed;
            f.capture = crate::proto::Capture::NoIndicator;
        }));
        let claimed = client.status_claiming_the_indicator().unwrap();
        assert_eq!(capture(&claimed), crate::proto::Capture::Running);
        assert_eq!(claimed.fields().capture_reason, None);
        let resumed = client.resume().unwrap();
        assert_eq!(capture(&resumed), crate::proto::Capture::Running);
    }

    #[test]
    fn in_the_scripted_daemon_a_claim_on_a_paused_status_with_a_lapsed_lease_renews_the_lease() {
        let (client, _) = client(scripted(|f| {
            paused(f);
            f.lease = crate::proto::Lease::Lapsed;
        }));
        let claimed = client.status_claiming_the_indicator().unwrap();
        assert_eq!(claimed.fields().lease, crate::proto::Lease::Fresh);
        assert_eq!(claimed.fields().capture, crate::proto::Capture::Paused);
        assert_eq!(running_sources(&claimed), 0);
    }

    #[test]
    fn in_the_scripted_daemon_a_claim_renews_a_lapsed_lease_and_a_plain_read_does_not() {
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.capture = crate::proto::Capture::NoIndicator;
        fields.lease = crate::proto::Lease::Lapsed;
        for line in &mut fields.sources {
            if line.state == crate::proto::SourceState::Running {
                line.state = crate::proto::SourceState::Stopped { by_user: false };
            }
        }
        let (client, _) = client(ScriptedDaemon::new(DaemonStatus::new(fields).unwrap()));
        let plain = client.status().unwrap();
        assert_eq!(plain.fields().lease, crate::proto::Lease::Lapsed);
        assert!(client.inventory().is_ok());
        assert_eq!(
            client.status().unwrap().fields().lease,
            crate::proto::Lease::Lapsed
        );
        let claimed = client.status_claiming_the_indicator().unwrap();
        assert_eq!(claimed.fields().lease, crate::proto::Lease::Fresh);
        assert_eq!(claimed.fields().capture, crate::proto::Capture::Running);
        assert_eq!(claimed, sample_status(Platform::Macos));
    }

    #[test]
    fn pause_and_resume_take_capture_changed_alone_and_the_reads_take_status_alone() {
        let status = || sample_status(Platform::Macos);
        let with = |reply: Reply| {
            let mut daemon = ScriptedDaemon::new(status());
            daemon.set_raw_reply(framed(&crate::proto::encode_reply(&reply)));
            client(daemon).0
        };
        let plain = with(Reply::Status(status()));
        assert!(matches!(
            plain.pause(),
            Err(ClientError::UnexpectedReply { kind: "status" })
        ));
        assert!(matches!(
            plain.resume(),
            Err(ClientError::UnexpectedReply { kind: "status" })
        ));
        let changed = with(Reply::CaptureChanged(status()));
        assert!(matches!(
            changed.status(),
            Err(ClientError::UnexpectedReply {
                kind: "capture_changed"
            })
        ));
        assert!(matches!(
            changed.status_claiming_the_indicator(),
            Err(ClientError::UnexpectedReply { .. })
        ));
        assert!(matches!(
            changed.acknowledge_lost_permission(Permission::Accessibility),
            Err(ClientError::UnexpectedReply { .. })
        ));
        assert!(matches!(
            changed.permission_asked(Permission::Accessibility),
            Err(ClientError::UnexpectedReply { .. })
        ));
        assert!(changed.pause().is_ok());
        assert!(plain.status().is_ok());
    }

    #[test]
    fn a_reply_of_the_wrong_kind_is_an_error_of_the_client() {
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        daemon.set_raw_reply(framed(&crate::proto::encode_reply(&Reply::Stopped(
            crate::proto::Stopped {
                flushed: true,
                events_lost: 0,
            },
        ))));
        let (client, _) = client(daemon);
        assert!(matches!(
            client.status(),
            Err(ClientError::UnexpectedReply { kind: "stopped" })
        ));
    }

    fn framed(body: &[u8]) -> Vec<u8> {
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn the_reserved_names_an_unknown_op_and_other_versions_reach_the_server_and_are_refused() {
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        let cases = [
            (
                r#"{"v":1,"body":{"op":"run_start"}}"#,
                ErrorCode::UnsupportedOp,
            ),
            (
                r#"{"v":1,"body":{"op":"run_end","at":"x"}}"#,
                ErrorCode::UnsupportedOp,
            ),
            (r#"{"v":1,"body":{"op":"relay"}}"#, ErrorCode::UnsupportedOp),
            (
                r#"{"v":0,"body":{"op":"status","indicator":false}}"#,
                ErrorCode::UnsupportedVersion,
            ),
            (
                r#"{"v":2,"body":{"op":"status","indicator":false}}"#,
                ErrorCode::UnsupportedVersion,
            ),
            (
                r#"{"v":1,"body":{"op":"pause","x":1}}"#,
                ErrorCode::InvalidArgument,
            ),
        ];
        for (text, code) in cases {
            let reply = daemon.answer_frame(text.as_bytes()).expect("an answer");
            let reply = crate::proto::decode_reply(&reply).unwrap();
            assert_eq!(reply.code(), Some(code), "{text}");
        }
        assert!(
            daemon.requests().is_empty(),
            "a refused request reached a handler"
        );
    }

    #[test]
    fn the_client_ends_its_request_so_the_server_reads_one_frame_and_its_end() {
        let (client, connector) = client(ScriptedDaemon::new(sample_status(Platform::Macos)));
        client.status().unwrap();
        assert_eq!(connector.ended_requests(), 1);
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn on_this_system_the_client_has_no_channel_yet_and_says_unsupported() {
        let dir = crate::paths::DataDir::from_flag(if cfg!(windows) {
            r"C:\halvo-data"
        } else {
            "/halvo-data"
        })
        .unwrap();
        let client = Client::new(system_connector(&dir).unwrap());
        assert!(matches!(client.status(), Err(ClientError::Unsupported)));
        assert!(matches!(client.stop(), Err(ClientError::Unsupported)));
    }
}
