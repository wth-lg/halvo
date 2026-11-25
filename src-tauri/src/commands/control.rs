use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[cfg(any(target_os = "macos", windows))]
use halvo_control::client::Transport;
use halvo_control::client::{
    Client, ClientError, Connector, Deadlines, DeleteOutcome, NoTransport, SystemConnector,
};
use halvo_control::model::{StorageCap, StoreId};
use halvo_control::onboarding::{Permission, RequestOutcome};
use halvo_control::proto::{CapSet, DaemonStatus, InventoryReply, Request};
#[cfg(any(target_os = "macos", windows))]
use halvo_control::r2::{OwnerStreamEnd, RunEdge, RunMarked};
#[cfg(any(target_os = "macos", windows))]
use halvo_control::wire::{read_frame, write_frame, REPLY_MAX_BYTES, REQUEST_MAX_BYTES};

use crate::view_state::Hold;

pub const MAX_CALLS_IN_FLIGHT: usize = 4;

/// How long a relayed request waits for a place: the 5 s that the client call of the request
/// `relay` then waits from its own connect. The control crate holds its own 5 s as
/// `CLIENT_DEADLINE` in crates/halvo-control/src/r2/owner_stream.rs, a module that it does not
/// export.
const RELAY_LIMIT: Duration = Duration::from_secs(5);

#[cfg(test)]
pub const REPLY_BYTES_IN_FLIGHT: usize = MAX_CALLS_IN_FLIGHT * halvo_control::wire::REPLY_MAX_BYTES;

#[derive(Debug, Default)]
struct Line {
    next: u64,
    waiting: VecDeque<u64>,
    in_flight: usize,
}

#[derive(Debug, Default)]
pub struct Places {
    line: Mutex<Line>,
    turn: Condvar,
}

#[derive(Debug)]
pub struct Made {
    places: Arc<Places>,
    number: u64,
    deadline_from: Instant,
    entered: bool,
}

#[derive(Debug)]
pub struct Place {
    places: Arc<Places>,
}

impl Places {
    fn line(&self) -> std::sync::MutexGuard<'_, Line> {
        self.line.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn make(self: &Arc<Self>) -> Made {
        let mut line = self.line();
        let number = line.next;
        line.next += 1;
        line.waiting.push_back(number);
        Made {
            places: Arc::clone(self),
            number,
            deadline_from: Instant::now(),
            entered: false,
        }
    }

    pub fn next_number(&self) -> u64 {
        self.line().next
    }

    #[cfg(test)]
    pub fn in_flight(&self) -> usize {
        self.line().in_flight
    }

    #[cfg(test)]
    pub fn waiting(&self) -> usize {
        self.line().waiting.len()
    }

    fn leave(&self, number: u64) {
        let mut line = self.line();
        line.waiting.retain(|n| *n != number);
        drop(line);
        self.turn.notify_all();
    }

    fn give_back(&self) {
        let mut line = self.line();
        line.in_flight -= 1;
        drop(line);
        self.turn.notify_all();
    }
}

impl Made {
    pub fn number(&self) -> u64 {
        self.number
    }

    fn enter(mut self, until: Instant) -> Option<Place> {
        let places = Arc::clone(&self.places);
        let mut line = places.line();
        loop {
            if line.waiting.front() == Some(&self.number) && line.in_flight < MAX_CALLS_IN_FLIGHT {
                line.waiting.pop_front();
                line.in_flight += 1;
                self.entered = true;
                drop(line);
                places.turn.notify_all();
                return Some(Place { places });
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            line = places
                .turn
                .wait_timeout(line, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

impl Drop for Made {
    fn drop(&mut self) {
        if !self.entered {
            self.places.leave(self.number);
        }
    }
}

impl Drop for Place {
    fn drop(&mut self) {
        self.places.give_back();
    }
}

pub trait Control: Send + Sync {
    fn places(&self) -> &Arc<Places>;
    fn status(&self, made: Made, claim: bool) -> Result<DaemonStatus, ClientError>;
    /// The Status of a pass of the poller: it claims the indicator where `claim` holds and the
    /// app's hold does not stand, read once the call has its place, so that no claim leaves
    /// after a set of the hold that came while the call waited for it (contracts.md C14
    /// item 4).
    fn status_of_a_pass(
        &self,
        made: Made,
        claim: bool,
        hold: &Hold,
    ) -> Result<DaemonStatus, ClientError>;
    fn inventory(&self, made: Made) -> Result<InventoryReply, ClientError>;
    fn pause(&self, made: Made) -> Result<DaemonStatus, ClientError>;
    fn resume(&self, made: Made) -> Result<DaemonStatus, ClientError>;
    fn delete_all(&self, made: Made, armed: Option<StoreId>) -> Result<DeleteOutcome, ClientError>;
    fn set_storage_cap(&self, made: Made, cap: StorageCap) -> Result<CapSet, ClientError>;
    fn request_permission(
        &self,
        made: Made,
        permission: Permission,
    ) -> Result<RequestOutcome, ClientError>;

    /// Relays one serialized request of the role `interface` to the owner of the derived store
    /// and returns the owner's reply as it wrote it.
    fn relay(&self, made: Made, request: &[u8]) -> Result<Vec<u8>, ClientError>;

    /// Sends one acknowledge_lost_permission for `permission` and answers the status that comes
    /// back.
    fn acknowledge_lost_permission(
        &self,
        made: Made,
        permission: Permission,
    ) -> Result<DaemonStatus, ClientError>;

    /// Sends one reset_settings and answers the status that comes back: the settings after the
    /// reset, before the daemon's life ends and starts again.
    fn reset_settings(&self, made: Made) -> Result<DaemonStatus, ClientError>;

    /// Sends one set_typing, which turns the switch of keys alone, and answers the status that
    /// comes back.
    fn set_typing(&self, made: Made, on: bool) -> Result<DaemonStatus, ClientError>;

    /// Sends one set_search, which turns the search for repeated tasks, and answers the status
    /// that comes back. A control that reaches no service of its own answers
    /// `ClientError::Unsupported`.
    fn set_search(&self, _made: Made, _on: bool) -> Result<DaemonStatus, ClientError> {
        Err(ClientError::Unsupported)
    }

    /// Asks the daemon for one end of a new owner stream (the workflows run's W-C24.13), on which the coordinator's
    /// requests reach the owner of the derived store.
    #[cfg(any(target_os = "macos", windows))]
    fn owner_stream(&self, made: Made) -> Result<OwnerStreamEnd, ClientError>;

    /// Sends one run mark, `run_start` or `run_end` (the workflows run's C24.12), and gives the daemon's reply.
    #[cfg(any(target_os = "macos", windows))]
    fn run_mark(&self, made: Made, edge: RunEdge) -> Result<RunMarked, ClientError>;

    /// Asks the daemon to hold the search for repeated tasks off until `until`, in nanoseconds since 1970 (the
    /// workflows run's C24.5), and gives the deadline in force, none where none is.
    #[cfg(any(target_os = "macos", windows))]
    fn defer(&self, made: Made, until: i64) -> Result<Option<i64>, ClientError>;
}

/// How long a stream call or a mark waits for its place in the line and for the daemon, from the call on.
#[cfg(any(target_os = "macos", windows))]
const CALL_DEADLINE: Duration = Duration::from_secs(5);

/// The call of a new owner stream on the system's connector of this account.
#[cfg(any(target_os = "macos", windows))]
#[derive(Clone)]
struct Streams(Arc<dyn Fn() -> Result<OwnerStreamEnd, ClientError> + Send + Sync>);

#[cfg(any(target_os = "macos", windows))]
impl std::fmt::Debug for Streams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Streams")
    }
}

fn the_same_for_each_request(left: Duration) -> Deadlines {
    Deadlines {
        read: left,
        pause: left,
        resume: left,
        stop: left,
        delete_all: left,
        set_storage_cap: left,
        permission: left,
        acknowledge: left,
    }
}

#[derive(Debug)]
pub struct Gated<C> {
    connector: C,
    places: Arc<Places>,
    #[cfg(any(target_os = "macos", windows))]
    streams: Option<Streams>,
}

impl<C: Connector + Clone> Gated<C> {
    pub fn new(connector: C) -> Self {
        Gated {
            connector,
            places: Arc::default(),
            #[cfg(any(target_os = "macos", windows))]
            streams: None,
        }
    }

    /// The control of `connector` whose `owner_stream` asks the daemon for an end of a new owner stream; every
    /// other call is that of `new`.
    #[cfg(any(target_os = "macos", windows))]
    pub fn streaming(connector: C) -> Self
    where
        C: Send + Sync + 'static,
        C::Transport: halvo_control::r2::CarriesEnds,
    {
        let call = connector.clone();
        Gated {
            streams: Some(Streams(Arc::new(move || {
                Client::new(call.clone()).owner_stream()
            }))),
            ..Gated::new(connector)
        }
    }

    fn run<T>(
        &self,
        made: Made,
        request: &Request,
        call: impl FnOnce(&Client<C>) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        self.within(made, Deadlines::V1.of(request), call)
    }

    /// Runs `call` in a place of the line, which it enters within `limit` of the moment at which
    /// the call was made.
    fn within<T>(
        &self,
        made: Made,
        limit: Duration,
        call: impl FnOnce(&Client<C>) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        let until = made.deadline_from + limit;
        let late = || ClientError::Deadline {
            seconds: limit.as_secs(),
        };
        let place = made.enter(until).ok_or_else(late)?;
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(late());
        }
        let answer = call(&Client::with_deadlines(
            self.connector.clone(),
            the_same_for_each_request(left),
        ));
        drop(place);
        match answer {
            Err(ClientError::Deadline { .. }) => Err(late()),
            other => other,
        }
    }
}

impl<C: Connector + Clone + Send + Sync> Control for Gated<C> {
    fn places(&self) -> &Arc<Places> {
        &self.places
    }

    fn status(&self, made: Made, claim: bool) -> Result<DaemonStatus, ClientError> {
        let request = Request::Status { indicator: claim };
        self.run(made, &request, |client| {
            if claim {
                client.status_claiming_the_indicator()
            } else {
                client.status()
            }
        })
    }

    fn status_of_a_pass(
        &self,
        made: Made,
        claim: bool,
        hold: &Hold,
    ) -> Result<DaemonStatus, ClientError> {
        let request = Request::Status { indicator: claim };
        self.run(made, &request, |client| {
            if claim && !hold.stands() {
                client.status_claiming_the_indicator()
            } else {
                client.status()
            }
        })
    }

    fn inventory(&self, made: Made) -> Result<InventoryReply, ClientError> {
        self.run(made, &Request::Inventory, Client::inventory)
    }

    fn pause(&self, made: Made) -> Result<DaemonStatus, ClientError> {
        self.run(made, &Request::Pause, Client::pause)
    }

    fn resume(&self, made: Made) -> Result<DaemonStatus, ClientError> {
        self.run(made, &Request::Resume, Client::resume)
    }

    fn delete_all(&self, made: Made, armed: Option<StoreId>) -> Result<DeleteOutcome, ClientError> {
        self.run(made, &Request::DeleteAll { store: armed }, |client| {
            client.delete_all(armed)
        })
    }

    fn set_storage_cap(&self, made: Made, cap: StorageCap) -> Result<CapSet, ClientError> {
        self.run(made, &Request::SetStorageCap { bytes: cap }, |client| {
            client.set_storage_cap(cap)
        })
    }

    fn request_permission(
        &self,
        made: Made,
        permission: Permission,
    ) -> Result<RequestOutcome, ClientError> {
        self.run(made, &Request::RequestPermission { permission }, |client| {
            client.request_permission(permission)
        })
    }

    fn acknowledge_lost_permission(
        &self,
        made: Made,
        permission: Permission,
    ) -> Result<DaemonStatus, ClientError> {
        self.run(
            made,
            &Request::AcknowledgeLostPermission { permission },
            |client| client.acknowledge_lost_permission(permission),
        )
    }

    fn reset_settings(&self, made: Made) -> Result<DaemonStatus, ClientError> {
        self.run(made, &Request::ResetSettings, Client::reset_settings)
    }

    fn set_typing(&self, made: Made, on: bool) -> Result<DaemonStatus, ClientError> {
        self.run(made, &Request::SetTyping { on }, |client| {
            client.set_typing(on)
        })
    }

    fn set_search(&self, made: Made, on: bool) -> Result<DaemonStatus, ClientError> {
        self.run(made, &Request::SetSearch { on }, |client| {
            client.set_search(on)
        })
    }

    /// The stream call of `streaming`, in its place in the line; a control made by `new` answers
    /// `ClientError::Unsupported`.
    #[cfg(any(target_os = "macos", windows))]
    fn owner_stream(&self, made: Made) -> Result<OwnerStreamEnd, ClientError> {
        let Some(Streams(call)) = &self.streams else {
            return Err(ClientError::Unsupported);
        };
        let until = made.deadline_from + CALL_DEADLINE;
        let place = made.enter(until).ok_or(ClientError::Deadline {
            seconds: CALL_DEADLINE.as_secs(),
        })?;
        let end = call();
        drop(place);
        end
    }

    /// The request of the edge, `{"v":1,"body":{"op":"run_start"}}` or `run_end`, in its place in the line: the
    /// reply `run_marked`, or the daemon's error reply.
    #[cfg(any(target_os = "macos", windows))]
    fn run_mark(&self, made: Made, edge: RunEdge) -> Result<RunMarked, ClientError> {
        let until = made.deadline_from + CALL_DEADLINE;
        let late = || ClientError::Deadline {
            seconds: CALL_DEADLINE.as_secs(),
        };
        let place = made.enter(until).ok_or_else(late)?;
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(late());
        }
        let frame = |e: halvo_control::wire::FrameError| ClientError::Protocol(e.to_string());
        let mut transport = self.connector.connect(left)?;
        transport.set_timeout(left).map_err(ClientError::Io)?;
        write_frame(&mut transport, &edge.request(), REQUEST_MAX_BYTES).map_err(frame)?;
        transport.end_request().map_err(ClientError::Io)?;
        let reply = read_frame(&mut transport, REPLY_MAX_BYTES).map_err(frame)?;
        drop(place);
        if let Some(marked) = RunMarked::decode(&reply) {
            return Ok(marked);
        }
        match halvo_control::proto::decode_reply(&reply) {
            Ok(halvo_control::proto::Reply::Error(error)) => Err(ClientError::Daemon(error)),
            _ => Err(ClientError::Protocol(
                "the reply is no run_marked".to_owned(),
            )),
        }
    }

    /// The request `defer` of `until` by `Client::defer`, once the call has its place in the line, for which it waits
    /// as long as a mark does; the client's call then waits up to 5 s for the daemon. It gives the deadline in force
    /// that the daemon names, or the error of the call.
    #[cfg(any(target_os = "macos", windows))]
    fn defer(&self, made: Made, until: i64) -> Result<Option<i64>, ClientError> {
        let by = made.deadline_from + CALL_DEADLINE;
        let late = ClientError::Deadline {
            seconds: CALL_DEADLINE.as_secs(),
        };
        let place = made.enter(by).ok_or(late)?;
        let answer = Client::new(self.connector.clone()).defer(until);
        drop(place);
        answer
    }

    fn relay(&self, made: Made, request: &[u8]) -> Result<Vec<u8>, ClientError> {
        self.within(made, RELAY_LIMIT, |client| client.relay(request))
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Unresolved;

impl Connector for Unresolved {
    type Transport = NoTransport;

    fn endpoint(&self) -> String {
        String::new()
    }

    fn connect(&self, _: Duration) -> Result<NoTransport, ClientError> {
        Err(ClientError::Io(std::io::Error::other(
            "the control endpoint of this account could not be resolved",
        )))
    }
}

/// The pid of the app's own daemon child of the moment, none while it has none: the connector
/// of `control_of_child` expects that process as the server at each connect. Beside it, what the
/// app's control read in its answers, for the supervisor of the child.
#[derive(Debug, Default)]
pub struct ChildPid {
    pid: AtomicU32,
    #[cfg(any(target_os = "macos", windows, test))]
    read: Mutex<Read>,
}

#[cfg(any(target_os = "macos", windows, test))]
#[derive(Debug, Default)]
struct Read {
    answers: u64,
    no_daemon: bool,
    sight: Option<(u32, Sight)>,
    deleting: Vec<u32>,
    answered_as_own: bool,
}

/// What an answer of the app's own child showed of that child (contracts.md C14 item 4).
#[cfg(any(target_os = "macos", windows, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sight {
    /// A status that reads capture=pausing beside a source that carries the mark `unstopped`,
    /// a source that a stop did not end.
    SourceNotStopped,
    /// A status of a daemon_version other than the app's, or a reply of another protocol
    /// version: a reply of another version, a value unknown to v1, or the error
    /// unsupported_version; never unsupported_op, which a daemon of the app's own version gives
    /// for an op that its build does not serve.
    AnotherVersion,
    /// The request's deadline passed with no reply, while no delete_all of the app was in
    /// flight against that child; the passed deadline of the delete_all itself is one.
    NoReply,
}

impl ChildPid {
    pub fn get(&self) -> Option<u32> {
        match self.pid.load(Ordering::SeqCst) {
            0 => None,
            pid => Some(pid),
        }
    }

    /// Names `pid` as the app's child of the moment, 0 for none; the child so named has not
    /// answered as the app's own yet.
    #[cfg(any(target_os = "macos", windows, test))]
    pub fn set(&self, pid: u32) {
        let mut read = self.read();
        read.answered_as_own = false;
        self.pid.store(pid, Ordering::SeqCst);
    }

    /// Whether the child of the moment has answered as the app's own: a status of its own pid
    /// came from it since `set` named it. Such a child has passed the instance lock of
    /// contracts.md C3.2 step 3.
    #[cfg(any(target_os = "macos", windows, test))]
    pub fn answered_as_own(&self) -> bool {
        self.read().answered_as_own
    }

    #[cfg(any(target_os = "macos", windows, test))]
    fn read(&self) -> std::sync::MutexGuard<'_, Read> {
        self.read.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// How many answers the app's control has taken, not counting any answer of a request that
    /// named a child of the app which had ended by the time the answer came: such an answer, the
    /// late answer of that child among them, tells nothing of a service that holds the directory.
    #[cfg(any(target_os = "macos", windows, test))]
    pub fn answers(&self) -> u64 {
        self.read().answers
    }

    /// Whether an answer that `answers` counts came after the first `answers` ones and the newest
    /// such answer reads that no daemon answers.
    #[cfg(any(target_os = "macos", windows, test))]
    pub fn no_daemon_after(&self, answers: u64) -> bool {
        let read = self.read();
        read.answers > answers && read.no_daemon
    }

    /// Takes what the answers showed of the child `child`; a sight of another child is dropped.
    #[cfg(any(target_os = "macos", windows, test))]
    pub fn take_sight(&self, child: u32) -> Option<Sight> {
        match self.read().sight.take() {
            Some((of, sight)) if of == child => Some(sight),
            _ => None,
        }
    }

    #[cfg(any(target_os = "macos", windows, test))]
    fn delete_began(&self, child: Option<u32>) {
        let Some(child) = child else {
            return;
        };
        let mut read = self.read();
        read.deleting.push(child);
        if read.sight == Some((child, Sight::NoReply)) {
            read.sight = None;
        }
    }

    #[cfg(any(target_os = "macos", windows, test))]
    fn delete_ended(&self, child: Option<u32>) {
        let Some(child) = child else {
            return;
        };
        let mut read = self.read();
        if let Some(at) = read.deleting.iter().position(|of| *of == child) {
            read.deleting.remove(at);
        }
    }

    #[cfg(any(target_os = "macos", windows, test))]
    fn took(
        &self,
        before: Option<u32>,
        error: Option<&ClientError>,
        status: Option<&DaemonStatus>,
    ) {
        let mut read = self.read();
        if before.is_some_and(|child| self.get() != Some(child)) {
            return;
        }
        read.answers += 1;
        read.no_daemon = matches!(error, Some(ClientError::Unreachable { .. }));
        let Some(child) = before else {
            return;
        };
        if status.is_some_and(|status| status.fields().pid == child) {
            read.answered_as_own = true;
        }
        let sight = match (status, error) {
            (Some(status), _) if status.fields().pid != child => None,
            (Some(status), _) if source_not_stopped(status) => Some(Sight::SourceNotStopped),
            (Some(status), _) if another_daemon_version(status) => Some(Sight::AnotherVersion),
            (_, Some(ClientError::Deadline { .. })) if !read.deleting.contains(&child) => {
                Some(Sight::NoReply)
            }
            (_, Some(error)) if of_another_protocol(error) => Some(Sight::AnotherVersion),
            _ => None,
        };
        let waiting = read
            .sight
            .filter(|(of, _)| *of == child)
            .map(|(_, kind)| kind);
        let kept = match (sight, waiting) {
            (Some(sight), None) => Some(sight),
            (Some(Sight::SourceNotStopped), Some(other)) if other != Sight::SourceNotStopped => {
                Some(Sight::SourceNotStopped)
            }
            _ => None,
        };
        if let Some(sight) = kept {
            read.sight = Some((child, sight));
        }
    }
}

#[cfg(any(target_os = "macos", windows, test))]
struct Deleting<'a> {
    pid: &'a ChildPid,
    child: Option<u32>,
}

#[cfg(any(target_os = "macos", windows, test))]
impl<'a> Deleting<'a> {
    fn began(pid: &'a ChildPid, child: Option<u32>) -> Self {
        pid.delete_began(child);
        Deleting { pid, child }
    }
}

#[cfg(any(target_os = "macos", windows, test))]
impl Drop for Deleting<'_> {
    fn drop(&mut self) {
        self.pid.delete_ended(self.child);
    }
}

#[cfg(any(target_os = "macos", windows, test))]
fn source_not_stopped(status: &DaemonStatus) -> bool {
    let fields = status.fields();
    fields.capture == halvo_control::proto::Capture::Pausing
        && fields.sources.iter().any(|line| line.unstopped)
}

#[cfg(any(target_os = "macos", windows, test))]
fn of_another_protocol(error: &ClientError) -> bool {
    match error {
        ClientError::ReplyOfAnotherVersion { .. } | ClientError::ValueUnknownToV1(_) => true,
        ClientError::Daemon(reply) => {
            reply.code() == halvo_control::proto::ErrorCode::UnsupportedVersion
        }
        _ => false,
    }
}

#[cfg(any(target_os = "macos", windows, test))]
fn another_daemon_version(status: &DaemonStatus) -> bool {
    crate::app::VERSION
        .parse::<halvo_control::proto::DaemonVersion>()
        .ok()
        != Some(status.fields().daemon_version)
}

/// The app's control of its child: each answer of the control inside tells `pid` what it read,
/// and what it showed of the child that `pid` named while the request ran. It hands every
/// method of `Control` on to the control inside; a builder who adds a method to `Control` adds
/// its forward here, and adds that method, with an answer of its own, to the stand-in
/// `EachItsOwn` and to the last row of the test
/// `another_daemon_version_restarts_the_own_child_once_and_a_child_seen_ending_is_killed`.
#[cfg(any(target_os = "macos", windows, test))]
pub struct Watching<C> {
    inner: C,
    pid: Arc<ChildPid>,
}

#[cfg(any(target_os = "macos", windows, test))]
impl<C> Watching<C> {
    pub fn new(inner: C, pid: Arc<ChildPid>) -> Self {
        Watching { inner, pid }
    }

    fn took<T>(
        &self,
        before: Option<u32>,
        answer: Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        self.pid.took(before, answer.as_ref().err(), None);
        answer
    }

    fn took_status(
        &self,
        before: Option<u32>,
        answer: Result<DaemonStatus, ClientError>,
    ) -> Result<DaemonStatus, ClientError> {
        self.pid
            .took(before, answer.as_ref().err(), answer.as_ref().ok());
        answer
    }
}

#[cfg(any(target_os = "macos", windows, test))]
impl<C: Control> Control for Watching<C> {
    fn places(&self) -> &Arc<Places> {
        self.inner.places()
    }

    fn status(&self, made: Made, claim: bool) -> Result<DaemonStatus, ClientError> {
        let before = self.pid.get();
        self.took_status(before, self.inner.status(made, claim))
    }

    fn status_of_a_pass(
        &self,
        made: Made,
        claim: bool,
        hold: &Hold,
    ) -> Result<DaemonStatus, ClientError> {
        let before = self.pid.get();
        self.took_status(before, self.inner.status_of_a_pass(made, claim, hold))
    }

    fn inventory(&self, made: Made) -> Result<InventoryReply, ClientError> {
        let before = self.pid.get();
        self.took(before, self.inner.inventory(made))
    }

    fn pause(&self, made: Made) -> Result<DaemonStatus, ClientError> {
        let before = self.pid.get();
        self.took_status(before, self.inner.pause(made))
    }

    fn resume(&self, made: Made) -> Result<DaemonStatus, ClientError> {
        let before = self.pid.get();
        self.took_status(before, self.inner.resume(made))
    }

    fn delete_all(&self, made: Made, armed: Option<StoreId>) -> Result<DeleteOutcome, ClientError> {
        let before = self.pid.get();
        let deleting = Deleting::began(&self.pid, before);
        let answer = self.inner.delete_all(made, armed);
        drop(deleting);
        self.took(before, answer)
    }

    fn set_storage_cap(&self, made: Made, cap: StorageCap) -> Result<CapSet, ClientError> {
        let before = self.pid.get();
        self.took(before, self.inner.set_storage_cap(made, cap))
    }

    fn request_permission(
        &self,
        made: Made,
        permission: Permission,
    ) -> Result<RequestOutcome, ClientError> {
        let before = self.pid.get();
        self.took(before, self.inner.request_permission(made, permission))
    }

    fn relay(&self, made: Made, request: &[u8]) -> Result<Vec<u8>, ClientError> {
        self.inner.relay(made, request)
    }

    fn acknowledge_lost_permission(
        &self,
        made: Made,
        permission: Permission,
    ) -> Result<DaemonStatus, ClientError> {
        let before = self.pid.get();
        self.took_status(
            before,
            self.inner.acknowledge_lost_permission(made, permission),
        )
    }

    fn reset_settings(&self, made: Made) -> Result<DaemonStatus, ClientError> {
        let before = self.pid.get();
        self.took_status(before, self.inner.reset_settings(made))
    }

    fn set_typing(&self, made: Made, on: bool) -> Result<DaemonStatus, ClientError> {
        let before = self.pid.get();
        self.took_status(before, self.inner.set_typing(made, on))
    }

    fn set_search(&self, made: Made, on: bool) -> Result<DaemonStatus, ClientError> {
        let before = self.pid.get();
        self.took_status(before, self.inner.set_search(made, on))
    }

    #[cfg(any(target_os = "macos", windows))]
    fn owner_stream(&self, made: Made) -> Result<OwnerStreamEnd, ClientError> {
        self.inner.owner_stream(made)
    }

    #[cfg(any(target_os = "macos", windows))]
    fn run_mark(&self, made: Made, edge: RunEdge) -> Result<RunMarked, ClientError> {
        self.inner.run_mark(made, edge)
    }

    #[cfg(any(target_os = "macos", windows))]
    fn defer(&self, made: Made, until: i64) -> Result<Option<i64>, ClientError> {
        self.inner.defer(made, until)
    }
}

/// The system's connector, which expects the server to be the child of `pid` while it holds one.
#[derive(Clone)]
pub struct ToChild {
    connector: SystemConnector,
    pid: Arc<ChildPid>,
}

impl Connector for ToChild {
    type Transport = <SystemConnector as Connector>::Transport;

    fn endpoint(&self) -> String {
        self.connector.endpoint()
    }

    fn connect(&self, timeout: Duration) -> Result<Self::Transport, ClientError> {
        match self.pid.get() {
            Some(pid) => expecting(self.connector.clone(), pid).connect(timeout),
            None => self.connector.connect(timeout),
        }
    }
}

/// The app's control of the system's connector: on macOS and on Windows one that also asks for
/// owner streams.
#[cfg(any(target_os = "macos", windows))]
fn streaming(connector: ToChild) -> Gated<ToChild> {
    Gated::streaming(connector)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn streaming(connector: ToChild) -> Gated<ToChild> {
    Gated::new(connector)
}

#[cfg(any(target_os = "macos", windows))]
fn expecting(connector: SystemConnector, pid: u32) -> SystemConnector {
    connector.expecting_pid(pid)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn expecting(connector: SystemConnector, pid: u32) -> SystemConnector {
    let _ = pid;
    connector
}

pub fn control_of<E>(
    resolve: impl FnOnce() -> Result<halvo_control::paths::DataDir, E>,
) -> (Arc<dyn Control>, Option<halvo_control::paths::DataDir>) {
    control_of_child(resolve, Arc::default())
}

/// The app's control of the daemon of the data directory that `resolve` gives, which expects
/// the server to be the child that `pid` names while it names one.
pub fn control_of_child<E>(
    resolve: impl FnOnce() -> Result<halvo_control::paths::DataDir, E>,
    pid: Arc<ChildPid>,
) -> (Arc<dyn Control>, Option<halvo_control::paths::DataDir>) {
    let Ok(dir) = resolve() else {
        return (Arc::new(Gated::new(Unresolved)), None);
    };
    match halvo_control::client::system_connector(&dir) {
        Ok(connector) => {
            let control = streaming(ToChild {
                connector,
                pid: Arc::clone(&pid),
            });
            (watched(control, pid), Some(dir))
        }
        Err(_) => (Arc::new(Gated::new(Unresolved)), Some(dir)),
    }
}

/// `control` inside the wrapper `Watching`, which reads its answers for `pid`.
#[cfg(any(target_os = "macos", windows, test))]
pub fn watched<C: Control + 'static>(control: C, pid: Arc<ChildPid>) -> Arc<dyn Control> {
    Arc::new(Watching::new(control, pid))
}

/// `control` alone: no child of the app lives on this system.
#[cfg(not(any(target_os = "macos", windows, test)))]
pub fn watched<C: Control + 'static>(control: C, pid: Arc<ChildPid>) -> Arc<dyn Control> {
    let _ = pid;
    Arc::new(control)
}
