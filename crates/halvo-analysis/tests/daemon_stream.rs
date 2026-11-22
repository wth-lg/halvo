//! The tests of the line of the daemon's control server that hands a request of protocol v2 to the worker slot's
//! form (halvo-capture, daemon::server and daemon::r2::service::serve). Each opens the slot over a new data
//! directory that holds a capture store, with the directory of the built owner as the place of the programs, as
//! tests/daemon_children.rs does, and writes the figures of a read into the cell, so that an owner could start. A
//! thread then serves one end of a socket pair with the server's `serve_connection`, and the test asks on the other
//! end. No test ticks the driver, so no worker starts. The process has one form, so one lock keeps the tests apart.
//! The control server is built on macOS alone, and so are these tests.

#![cfg(target_os = "macos")]

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use halvo_capture::daemon::custody_policy::Custody;
use halvo_capture::daemon::port::{DataDirStore, Opened, StoreHost};
use halvo_capture::daemon::r2::daemon_caller::cell;
use halvo_capture::daemon::r2::service;
use halvo_capture::daemon::r2::when::{Reading, Readings};
use halvo_capture::daemon::server::{
    serve_connection, Admitting, READ_DEADLINE, REPLY_WRITE_DEADLINE,
};
use halvo_control::client::{Client, ClientError, Connector, Transport};
use halvo_control::proto::{decode_reply, ErrorCode, Reply};
use halvo_control::r2::OwnerStreamEnd;
use halvo_control::wire::{read_frame, write_frame, REPLY_MAX_BYTES, REQUEST_MAX_BYTES};
use halvo_schema_core::ids::{Position, StoreId};

/// The longest wait of a test's own read for a reply that the server owes at once.
const WAIT: Duration = Duration::from_secs(30);

/// The lock that keeps the tests of this file apart.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// A new directory under the target's tmp directory, removed when the value is dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let parent = Path::new(env!("CARGO_TARGET_TMPDIR"));
        Scratch(halvo_test_dir::make(parent, name).unwrap())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.0) {
            eprintln!(
                "the scratch directory {} was not removed: {e}",
                self.0.display()
            );
        }
    }
}

/// A writer of lines: the test reads what was written.
#[derive(Clone, Default)]
struct Lines(Arc<Mutex<Vec<u8>>>);

impl Lines {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl Write for Lines {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The readings of a computer on mains, not in low-power mode, idle for 10 min. No test ticks the driver, so it
/// reads none of them.
struct Allowed;

impl Readings for Allowed {
    fn read(&mut self) -> Reading {
        Reading {
            idle: Some(Duration::from_secs(600)),
            mains: Some(true),
            low_power: Some(false),
        }
    }
}

/// The owner's binary beside the worker's, built with the cargo that built this test, in the same target directory
/// and profile, as `owner_binary` of tests/daemon_children.rs builds it.
fn owner_binary() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let worker = Path::new(env!("CARGO_BIN_EXE_halvo-analysis"));
        let profile_dir = worker.parent().unwrap();
        let mut cmd = Command::new(env!("CARGO"));
        cmd.args(["build", "--locked", "-p", "halvo-owner"])
            .args(["--bin", "halvo-owner", "--target-dir"])
            .arg(profile_dir.parent().unwrap())
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .stdin(Stdio::null());
        match profile_dir.file_name().and_then(|name| name.to_str()) {
            Some("debug") => {}
            Some("release") => {
                cmd.arg("--release");
            }
            other => {
                panic!("the worker's binary lies in the directory {other:?}, of no known profile")
            }
        }
        let built = cmd.output().unwrap();
        assert!(
            built.status.success(),
            "cargo did not build the owner's binary: {}",
            String::from_utf8_lossy(&built.stderr)
        );
        let owner = profile_dir.join("halvo-owner");
        assert!(owner.is_file(), "no file {}", owner.display());
        owner
    })
}

/// Writes the figures of one read from ordinal 0 of a capture store into the cell, as the cursor does: with them
/// the owner slot starts an owner at the first call that needs one.
fn write_read() {
    let store = StoreId::parse("0123456789abcdef0123456789abcdef").unwrap();
    cell().keep(store, Position { seg: 1, idx: 0 }, Some(1));
}

/// The user id of this process, the daemon's own in these tests.
fn own_user() -> u32 {
    // SAFETY: the call reads the effective user id of this process and touches no memory.
    unsafe { libc::geteuid() }
}

/// A data directory that holds a capture store, with the writer of the slot's lines. The slot is stopped when the
/// rig is dropped, before its directory is removed, and the lock of the tests is let go last.
struct Rig {
    lines: Lines,
    scratch: Scratch,
    _one: MutexGuard<'static, ()>,
}

impl Drop for Rig {
    /// Stops the slot, so that the next test finds no owner; a test that fails prints what the slot wrote.
    fn drop(&mut self) {
        service::stop();
        if std::thread::panicking() {
            eprintln!("the slot wrote: {:?}", self.lines.text());
        }
    }
}

impl Rig {
    /// Makes a capture store in a new data directory under the root file's custody and drops it, as `Rig::new` of
    /// tests/daemon_children.rs does.
    fn new(name: &str) -> Rig {
        let one = ONE_AT_A_TIME.lock().unwrap_or_else(PoisonError::into_inner);
        let scratch = Scratch::new(name);
        let host = DataDirStore::at(&scratch.0);
        let Opened::ReadWrite { store, .. } = host.create(Custody::RootFile) else {
            panic!("no capture store was made in {}", scratch.0.display());
        };
        drop(store);
        Rig {
            lines: Lines::default(),
            scratch,
            _one: one,
        }
    }

    /// The open of the store in the service's run, with the directory of the built programs, and then the figures
    /// of a read in the cell.
    fn open_with_a_read(&self) {
        service::open(
            &self.scratch.0,
            Ok(owner_binary().parent().unwrap().to_path_buf()),
            Allowed,
            Box::new(self.lines.clone()),
        );
        write_read();
    }

    /// Whether derived/owner.pid stands: the slot writes it at each start of an owner and removes it at the owner's
    /// end.
    fn owner_pid_stands(&self) -> bool {
        self.scratch.0.join("derived").join("owner.pid").exists()
    }
}

/// A connector that gives the client call the client's end of the test's control connection, once.
struct Once(Mutex<Option<UnixStream>>);

impl Connector for Once {
    type Transport = UnixStream;

    fn endpoint(&self) -> String {
        "the test's control connection".to_owned()
    }

    fn connect(&self, _: Duration) -> Result<UnixStream, ClientError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .take()
            .expect("the client call connects once"))
    }
}

/// One connection of the control server: a thread serves one end of a new socket pair with `serve_connection`, as
/// the server's accepting thread does, for a peer of the user id `peer` in a daemon of this process's user, while
/// `call` asks on the other end. No request of these tests is one that the server hands to the service's loop.
fn connection<T>(peer: u32, call: impl FnOnce(UnixStream) -> T) -> T {
    let (client, mut server) = UnixStream::pair().unwrap();
    server.set_read_timeout(Some(READ_DEADLINE)).unwrap();
    server
        .set_write_timeout(Some(REPLY_WRITE_DEADLINE))
        .unwrap();
    let admitting = Arc::new(Admitting::default());
    let (hand, handed) = mpsc::channel();
    let answer = std::thread::scope(|scope| {
        let serving = scope
            .spawn(|| serve_connection(&mut server, Some(peer), own_user(), &admitting, &hand));
        let answer = call(client);
        assert!(
            serving.join().unwrap().is_none(),
            "the server waits for the loop's answer"
        );
        answer
    });
    assert!(
        handed.try_recv().is_err(),
        "a request reached the service's loop"
    );
    answer
}

/// The client's call `owner_stream` on one connection whose peer has the user id `peer`.
fn owner_stream(peer: u32) -> Result<OwnerStreamEnd, ClientError> {
    connection(peer, |client| {
        Client::new(Once(Mutex::new(Some(client)))).owner_stream()
    })
}

/// The code of the error reply that refused the client's call.
fn refusal(answer: Result<OwnerStreamEnd, ClientError>) -> ErrorCode {
    match answer {
        Err(ClientError::Daemon(error)) => error.code(),
        other => panic!("the client's call was not refused by an error reply: {other:?}"),
    }
}

/// Rule 1: after the open and a read in the cell, the client's call answers an end of a stream, and
/// derived/owner.pid stands: the exchange started the owner.
#[test]
fn a_stream_request_gets_an_end() {
    let rig = Rig::new("stream-end");
    rig.open_with_a_read();
    let end = owner_stream(own_user());
    let _end = end.unwrap_or_else(|error| {
        panic!(
            "the client's call answered no end: {error:?}; the slot wrote: {:?}",
            rig.lines.text()
        )
    });
    assert!(rig.owner_pid_stands(), "no derived/owner.pid stands");
}

/// Rule 2: a request of protocol v2 whose op the module does not serve is answered as v1 answers it,
/// `unsupported_version`, and no owner starts for it.
#[test]
fn an_unknown_op_is_answered_as_v1_answers() {
    let rig = Rig::new("stream-unknown-op");
    rig.open_with_a_read();
    let reply = connection(own_user(), |mut client| {
        client.set_timeout(WAIT).unwrap();
        write_frame(
            &mut client,
            br#"{"v":2,"body":{"op":"other"}}"#,
            REQUEST_MAX_BYTES,
        )
        .unwrap();
        let frame = read_frame(&mut client, REPLY_MAX_BYTES).unwrap();
        decode_reply(&frame).unwrap()
    });
    assert!(matches!(reply, Reply::Error(_)), "the reply is {reply:?}");
    assert_eq!(reply.code(), Some(ErrorCode::UnsupportedVersion));
    assert!(!rig.owner_pid_stands(), "an owner started");
}

/// Rule 3: with no slot in the form, after the stop, the client gets `store_unavailable`; while the mark of a
/// delete stands it gets `busy`; no owner runs after either.
#[test]
fn a_stream_request_is_refused_with_no_slot_and_during_a_delete() {
    let rig = Rig::new("stream-refused");
    rig.open_with_a_read();
    service::stop();
    assert_eq!(
        refusal(owner_stream(own_user())),
        ErrorCode::StoreUnavailable,
        "after the stop"
    );
    assert!(!rig.owner_pid_stands(), "an owner started after the stop");

    rig.open_with_a_read();
    service::delete();
    assert_eq!(
        refusal(owner_stream(own_user())),
        ErrorCode::Busy,
        "after the delete"
    );
    assert!(!rig.owner_pid_stands(), "an owner started after the delete");
}

/// Rule 4: the call stands behind the server's peer check: a peer of another user gets `peer_rejected`, and no
/// owner starts for it.
#[test]
fn a_peer_of_another_user_gets_no_end() {
    let rig = Rig::new("stream-other-user");
    rig.open_with_a_read();
    assert_eq!(
        refusal(owner_stream(own_user() + 1)),
        ErrorCode::PeerRejected
    );
    assert!(!rig.owner_pid_stands(), "an owner started");
}
