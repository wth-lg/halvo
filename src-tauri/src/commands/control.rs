use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use halvo_control::client::{
    Client, ClientError, Connector, Deadlines, DeleteOutcome, NoTransport,
};
use halvo_control::model::{StorageCap, StoreId};
use halvo_control::onboarding::{Permission, RequestOutcome};
use halvo_control::proto::{CapSet, DaemonStatus, InventoryReply, Request};

/// How many calls of the client are in flight at a time, whoever sends them.
pub const PLACES: usize = 4;

/// The bytes that the replies of the calls in flight can hold at most: one reply of at most
/// `wire::REPLY_MAX_BYTES` for each place (docs/baseline/ledger.d/app-2.txt).
#[cfg(test)]
pub const REPLY_BYTES_IN_FLIGHT: usize = PLACES * halvo_control::wire::REPLY_MAX_BYTES;

#[derive(Debug, Default)]
struct Line {
    next: u64,
    waiting: VecDeque<u64>,
    in_flight: usize,
}

/// The places of the client's calls: a call waits for a free place in the order in which the
/// calls were made.
#[derive(Debug, Default)]
pub struct Places {
    line: Mutex<Line>,
    turn: Condvar,
}

/// A call as it was made: its number in the order of the calls and the instant at which its
/// deadline starts. Dropped before it enters, it leaves the line.
#[derive(Debug)]
pub struct Made {
    places: Arc<Places>,
    number: u64,
    at: Instant,
    entered: bool,
}

/// A place that a call holds until it is dropped.
#[derive(Debug)]
pub struct Place {
    places: Arc<Places>,
}

impl Places {
    fn line(&self) -> std::sync::MutexGuard<'_, Line> {
        self.line.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Makes a call: it takes the next number and joins the line now.
    pub fn make(self: &Arc<Self>) -> Made {
        let mut line = self.line();
        let number = line.next;
        line.next += 1;
        line.waiting.push_back(number);
        Made {
            places: Arc::clone(self),
            number,
            at: Instant::now(),
            entered: false,
        }
    }

    /// The number that the next call made will take.
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

    /// Waits for a place until `until` and takes it when this call is first in the line.
    fn enter(mut self, until: Instant) -> Option<Place> {
        let places = Arc::clone(&self.places);
        let mut line = places.line();
        loop {
            if line.waiting.front() == Some(&self.number) && line.in_flight < PLACES {
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

/// The calls that the app makes of the capture service. Each call carries the moment it was
/// made, from which its deadline runs.
pub trait Control: Send + Sync {
    fn places(&self) -> &Arc<Places>;
    fn status(&self, made: Made, claim: bool) -> Result<DaemonStatus, ClientError>;
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
}

/// The deadlines of one call: `left` for each request.
fn every(left: Duration) -> Deadlines {
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

/// The client of protocol v1 behind the places.
#[derive(Debug)]
pub struct Gated<C> {
    connector: C,
    places: Arc<Places>,
}

impl<C: Connector + Clone> Gated<C> {
    pub fn new(connector: C) -> Self {
        Gated {
            connector,
            places: Arc::default(),
        }
    }

    fn run<T>(
        &self,
        made: Made,
        request: &Request,
        call: impl FnOnce(&Client<C>) -> Result<T, ClientError>,
    ) -> Result<T, ClientError> {
        let limit = Deadlines::V1.of(request);
        let until = made.at + limit;
        let late = || ClientError::Deadline {
            seconds: limit.as_secs(),
        };
        let place = made.enter(until).ok_or_else(late)?;
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(late());
        }
        let answer = call(&Client::with_deadlines(self.connector.clone(), every(left)));
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
}

/// The connector of an app whose data directory or control endpoint could not be resolved:
/// every call fails as a channel that failed.
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

/// The control of the data directory that `resolve` gives, or of none when it fails. It
/// cannot fail itself.
pub fn control_of<E>(
    resolve: impl FnOnce() -> Result<halvo_control::paths::DataDir, E>,
) -> (Arc<dyn Control>, Option<halvo_control::paths::DataDir>) {
    let Ok(dir) = resolve() else {
        return (Arc::new(Gated::new(Unresolved)), None);
    };
    match halvo_control::client::system_connector(&dir) {
        Ok(connector) => (Arc::new(Gated::new(connector)), Some(dir)),
        Err(_) => (Arc::new(Gated::new(Unresolved)), Some(dir)),
    }
}
