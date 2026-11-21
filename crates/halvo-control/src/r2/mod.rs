//! The workflows run's part of the control crate, built with the feature `r2` alone (contracts.md C24.1 of that run):
//! the version step of protocol v2 and the server's one entry for a body of that version.
//!
//! Protocol v2 keeps the framing, the limits and the transport of v1. [`crate::proto::decode_request`] hands the body
//! of a `{"v":2,"body":{...}}` request to the server as [`crate::proto::Decoded::Unknown`], and the server passes it
//! to [`dispatch_unknown`]. The requests of this module: `owner_stream`, in owner_stream.rs. A further request joins
//! as one module file beside owner_stream.rs, one `mod` line below and one arm of the match in [`dispatch_unknown`].

use crate::client::ClientError;
use crate::proto::Reply;

#[cfg(any(target_os = "macos", windows))]
mod owner_stream;
#[cfg(target_os = "macos")]
mod owner_stream_macos;
#[cfg(windows)]
mod owner_stream_windows;
mod run_mark;

#[cfg(any(target_os = "macos", windows))]
pub use owner_stream::{
    CarriesEnds, Connection, OwnerChannel, OwnerStream, OwnerStreamEnd, OwnerStreamReady,
    ReadyError,
};
#[cfg(target_os = "macos")]
pub use owner_stream_macos::send_with_end;
pub use run_mark::{mark_request, RunEdge, RunMarked};

/// The protocol version of the bodies of this module.
pub const VERSION: u64 = 2;

/// Whether [`crate::proto::decode_request`] hands a body of `version` to the server as unknown: 2 alone. Any other
/// version but 1 stays `unsupported_version` (contracts.md C24.3 of the workflows run).
pub const fn takes_version(version: u64) -> bool {
    version == VERSION
}

/// The reply that `Client::send` decoded, unchanged: no section of this module is read from a reply yet.
pub fn decode_sections(reply: Reply) -> Result<Reply, ClientError> {
    Ok(reply)
}

/// The server's one call for a request that the v1 table does not hold. It gets the decoded request, the accepted
/// connection, borrowed for the length of the call, and the owner's channel, and returns whether it answered the
/// request on the connection itself. For a version other than 2, or an op that is no request of this module, it
/// writes nothing and returns false, and the server answers as v1 does. The server checks the peer before this call,
/// by its `PeerPolicy`; the request names no role.
#[cfg(any(target_os = "macos", windows))]
pub fn dispatch_unknown(
    request: &crate::proto::UnknownRequest,
    connection: Connection<'_>,
    owner: &mut dyn OwnerChannel,
) -> bool {
    if request.version() != VERSION {
        return false;
    }
    match request.op() {
        Some(OwnerStream::OP) => {
            owner_stream::serve(request, connection, owner);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use crate::proto::{decode_request, Decoded, RequestError};

    #[test]
    fn v2_is_unknown_and_v3_is_refused() {
        match decode_request(br#"{"v":2,"body":{"op":"owner_stream"}}"#) {
            Ok(Decoded::Unknown(request)) => {
                assert_eq!(request.version(), 2);
                assert_eq!(request.op(), Some("owner_stream"));
            }
            other => panic!("a version 2 request decodes as {other:?}"),
        }
        assert_eq!(
            decode_request(br#"{"v":3,"body":{"op":"owner_stream"}}"#),
            Err(RequestError::UnsupportedVersion { version: 3 })
        );
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn owner_stream_has_one_spelling() {
        use super::{OwnerStream, OwnerStreamReady};
        assert_eq!(
            OwnerStream.encode(),
            br#"{"v":2,"body":{"op":"owner_stream"}}"#
        );
        let null = br#"{"v":2,"body":{"kind":"owner_stream_ready","handles":null}}"#;
        assert_eq!(
            OwnerStreamReady::decode(null),
            Ok(OwnerStreamReady { handles: None })
        );
        assert_eq!(OwnerStreamReady { handles: None }.encode(), null);
        assert_eq!(
            OwnerStreamReady::decode(
                br#"{"v":2,"body":{"kind":"owner_stream_ready","handles":[3,4]}}"#
            ),
            Ok(OwnerStreamReady {
                handles: Some([3, 4])
            })
        );
        assert!(OwnerStreamReady::decode(
            br#"{"v":2,"body":{"kind":"owner_stream_ready","handles":[3,4,5]}}"#
        )
        .is_err());
    }
}
