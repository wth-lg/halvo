//! The product's `Ends` (unit U9.2; the workflows run's W-C24.13 items 2 and 8): `ClientEnds` gets each end of an
//! owner stream by the request `owner_stream`, through the call that its holder hands it. The call is made on the one
//! client that the app holds, so `ClientEnds` opens no connection of its own.
//!
//! The request is built on macOS and Windows alone, and so is this module.
#![cfg(any(target_os = "macos", windows))]

use halvo_control::client::ClientError;
use halvo_control::proto::ErrorCode;
use halvo_control::r2::OwnerStreamEnd;

use super::{End, Ends, Refusal};

/// The product's `Ends`: each end is what one call of the request `owner_stream` returns.
pub struct ClientEnds<F> {
    call: F,
}

impl<F> ClientEnds<F>
where
    F: Fn() -> Result<OwnerStreamEnd, ClientError> + Send + Sync,
{
    /// `call` asks the daemon for one end of a new owner stream, as `Client::owner_stream` does on the app's client.
    pub fn new(call: F) -> ClientEnds<F> {
        ClientEnds { call }
    }
}

impl<F> Ends for ClientEnds<F>
where
    F: Fn() -> Result<OwnerStreamEnd, ClientError> + Send + Sync,
{
    /// Calls `call` once: its end is the link's `End`, and its error the refusal of the error's name.
    fn end(&self) -> Result<End, Refusal> {
        (self.call)().map(end_of).map_err(refusal)
    }
}

/// The link's `End` of the one descriptor of an end.
#[cfg(target_os = "macos")]
fn end_of(end: OwnerStreamEnd) -> End {
    End::new(std::os::fd::OwnedFd::from(end))
}

/// The link's `End` of the two handles of an end: the one that reads, then the one that writes.
#[cfg(windows)]
fn end_of(end: OwnerStreamEnd) -> End {
    use std::os::windows::io::OwnedHandle;
    let (reads, writes): (OwnedHandle, OwnedHandle) = end.into();
    End::new(reads, writes)
}

/// An error reply `busy` is `Busy`, also the busy of a daemon that is ending, and `store_unavailable` is
/// `StoreUnavailable`; every other error of the call is `Internal`.
fn refusal(error: ClientError) -> Refusal {
    match error {
        ClientError::Daemon(reply) | ClientError::ServiceEnding(reply) => match reply.code() {
            ErrorCode::Busy => Refusal::Busy,
            ErrorCode::StoreUnavailable => Refusal::StoreUnavailable,
            _ => Refusal::Internal,
        },
        _ => Refusal::Internal,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use halvo_control::proto::ErrorReply;

    use super::*;

    /// What `end` gives for an error of the call, and how often it made the call.
    fn refused(error: fn() -> ClientError) -> (Option<Refusal>, usize) {
        let calls = AtomicUsize::new(0);
        let ends = ClientEnds::new(|| {
            calls.fetch_add(1, Ordering::Relaxed);
            Err(error())
        });
        let refusal = ends.end().err();
        (refusal, calls.load(Ordering::Relaxed))
    }

    fn reply(code: ErrorCode) -> ClientError {
        ClientError::Daemon(ErrorReply::new(code, "a refusal of the test"))
    }

    /// A case: its name, the error of the call, and the refusal that `end` gives for it.
    type Case = (&'static str, fn() -> ClientError, Refusal);

    /// U9.2 ruling 2: `busy` gives `Busy`, `store_unavailable` gives `StoreUnavailable`, every other error `Internal`;
    /// each `end` makes the call once.
    #[test]
    fn a_refused_owner_stream_is_the_refusal_of_its_name() {
        let cases: [Case; 9] = [
            ("busy", || reply(ErrorCode::Busy), Refusal::Busy),
            (
                "busy of an ending daemon",
                || ClientError::ServiceEnding(ErrorReply::busy_of_an_ending_life()),
                Refusal::Busy,
            ),
            (
                "store_unavailable",
                || reply(ErrorCode::StoreUnavailable),
                Refusal::StoreUnavailable,
            ),
            ("internal", || reply(ErrorCode::Internal), Refusal::Internal),
            (
                "unsupported_version",
                || reply(ErrorCode::UnsupportedVersion),
                Refusal::Internal,
            ),
            (
                "invalid_argument",
                || reply(ErrorCode::InvalidArgument),
                Refusal::Internal,
            ),
            (
                "deadline",
                || ClientError::Deadline { seconds: 5 },
                Refusal::Internal,
            ),
            (
                "protocol",
                || ClientError::Protocol("a frame of the test".to_owned()),
                Refusal::Internal,
            ),
            (
                "io",
                || ClientError::Io(std::io::ErrorKind::BrokenPipe.into()),
                Refusal::Internal,
            ),
        ];
        for (name, error, want) in cases {
            assert_eq!(refused(error), (Some(want), 1), "{name}");
        }
    }
}
