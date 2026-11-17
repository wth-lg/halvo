use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::client::{ClientError, Connector, PeerRejection, Transport};
use crate::paths::{AccountLookup, DataDir};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileKind {
    Directory,
    Socket,
    Link,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PathFacts {
    pub(crate) kind: FileKind,
    pub(crate) uid: u32,
    pub(crate) mode: u32,
}

pub(crate) fn read_facts(path: &Path) -> std::io::Result<PathFacts> {
    let meta = std::fs::symlink_metadata(path)?;
    let kind = meta.file_type();
    let kind = if kind.is_symlink() {
        FileKind::Link
    } else if kind.is_dir() {
        FileKind::Directory
    } else if kind.is_socket() {
        FileKind::Socket
    } else {
        FileKind::Other
    };
    Ok(PathFacts {
        kind,
        uid: meta.uid(),
        mode: meta.mode(),
    })
}

pub(crate) fn check_socket_dir(facts: &PathFacts, euid: u32) -> Result<(), PeerRejection> {
    if facts.kind != FileKind::Directory || facts.mode & 0o7777 != 0o700 {
        return Err(PeerRejection::SocketDirNotPrivate);
    }
    if facts.uid != euid {
        return Err(PeerRejection::SocketDirOfAnotherUser);
    }
    Ok(())
}

pub(crate) fn check_socket(facts: &PathFacts, euid: u32) -> Result<(), PeerRejection> {
    if facts.kind != FileKind::Socket {
        return Err(PeerRejection::NotASocket);
    }
    if facts.uid != euid {
        return Err(PeerRejection::SocketOfAnotherUser);
    }
    Ok(())
}

pub(crate) fn check_server(
    uid: u32,
    pid: u32,
    euid: u32,
    expected_pid: Option<u32>,
) -> Result<(), PeerRejection> {
    if uid != euid {
        return Err(PeerRejection::ServerOfAnotherUser);
    }
    if expected_pid.is_some_and(|expected| expected != pid) {
        return Err(PeerRejection::NotTheExpectedServer);
    }
    Ok(())
}

/// Before it sends, it requires the socket's
/// directory to be a directory of this user with mode 0700, the socket to be a socket of
/// this user, and the server to run as this user; with an expected pid, the server must
/// be that process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnixConnector {
    socket: PathBuf,
    data_dir: DataDir,
    expected_pid: Option<u32>,
}

impl UnixConnector {
    pub fn for_data_dir(
        data_dir: &DataDir,
        account: &dyn AccountLookup,
    ) -> Result<Self, ClientError> {
        let socket = crate::paths::socket_path(data_dir, account)
            .map_err(|e| ClientError::Protocol(e.to_string()))?;
        Ok(Self {
            socket,
            data_dir: data_dir.clone(),
            expected_pid: None,
        })
    }

    /// The connector to the control socket under `run/` of `data_dir`.
    #[cfg(test)]
    pub(crate) fn at(data_dir: &DataDir) -> Self {
        Self {
            socket: data_dir.control_socket(),
            data_dir: data_dir.clone(),
            expected_pid: None,
        }
    }

    /// Accepts only the server process `pid`.
    pub fn expecting_pid(mut self, pid: u32) -> Self {
        self.expected_pid = Some(pid);
        self
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// The data directory whose instance lock the connector reads.
    pub fn data_dir(&self) -> &DataDir {
        &self.data_dir
    }

    fn unreachable(&self) -> ClientError {
        ClientError::Unreachable {
            endpoint: self.endpoint(),
        }
    }
}

fn effective_uid() -> u32 {
    // SAFETY: geteuid reads the process's effective user id; it takes no pointer and cannot fail.
    unsafe { libc::geteuid() }
}

fn peer_of(stream: &UnixStream) -> std::io::Result<(u32, u32)> {
    let fd = stream.as_raw_fd();
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: `fd` is the open socket of `stream`, and the two out pointers are live locals.
    if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: `fd` is open; `pid` and `len` are live locals and `len` holds the size of `pid`.
    let got = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut len,
        )
    };
    if got != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let pid =
        u32::try_from(pid).map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
    Ok((uid, pid))
}

impl Transport for UnixStream {
    fn set_timeout(&mut self, timeout: Duration) -> std::io::Result<()> {
        let set = self
            .set_read_timeout(Some(timeout))
            .and_then(|()| self.set_write_timeout(Some(timeout)));
        match set {
            Err(e) if e.raw_os_error() == Some(libc::EINVAL) && self.peer_addr().is_err() => Ok(()),
            other => other,
        }
    }

    fn end_request(&mut self) -> std::io::Result<()> {
        self.shutdown(std::net::Shutdown::Write)
    }
}

impl Connector for UnixConnector {
    type Transport = UnixStream;

    fn endpoint(&self) -> String {
        self.socket.to_string_lossy().into_owned()
    }

    /// Tries the connect; on each path that finds no daemon, and on an error of the transport, it
    /// reads the instance lock of the data directory. A cause at the data folder that the read
    /// carries answers first (ruling CT-15), so a read of the socket's directory never hides it.
    /// Held: it waits a step and tries again inside `timeout`, and at its end answers that the
    /// directory is in use, whatever the connect's error (ruling CT-16 C2). Free, or no data
    /// directory: an error of the transport answers the cause that
    /// [`DataDir::is_provisioned`] reads at the folder, or else itself (ruling CT-16 C1), and a
    /// connect that found no daemon answers unreachable at once. Every error of the transport
    /// that it answers is marked [`BeforeTheRequest`].
    fn connect(&self, timeout: Duration) -> Result<UnixStream, ClientError> {
        let until = std::time::Instant::now() + timeout;
        loop {
            let missed = match self.connect_once() {
                Err(missed @ (ClientError::Unreachable { .. } | ClientError::Io(_))) => missed,
                other => return other,
            };
            let read = match crate::instance_lock::read(&self.data_dir) {
                Err(e) if crate::paths::entry_cause(&e).is_some() => {
                    return Err(ClientError::Io(e))
                }
                Err(e) => {
                    return Err(match missed {
                        ClientError::Io(missed) => before_the_request(missed),
                        _ => before_the_request(e),
                    })
                }
                Ok(read) => read,
            };
            match (read, missed) {
                (crate::instance_lock::LockRead::Held, _) => {}
                (_, ClientError::Io(missed)) => {
                    return Err(match self.data_dir.is_provisioned() {
                        Err(cause) => ClientError::Io(crate::paths::cause_error(cause)),
                        Ok(_) => before_the_request(missed),
                    })
                }
                (_, _) => return Err(self.unreachable()),
            }
            let left = until.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Err(ClientError::DataDirInUse {
                    endpoint: self.endpoint(),
                });
            }
            std::thread::sleep(left.min(Duration::from_millis(CONNECT_STEP_MS)));
        }
    }
}

/// The step between two connects while the instance lock is held.
pub const CONNECT_STEP_MS: u64 = 50;

/// The payload of an error of the connect, which came before any byte of a request was sent; it
/// reads as the error that it holds.
#[derive(Debug)]
pub(crate) struct BeforeTheRequest(std::io::Error);

impl std::fmt::Display for BeforeTheRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::error::Error for BeforeTheRequest {}

fn before_the_request(error: std::io::Error) -> ClientError {
    ClientError::Io(std::io::Error::new(error.kind(), BeforeTheRequest(error)))
}

impl UnixConnector {
    fn connect_once(&self) -> Result<UnixStream, ClientError> {
        let euid = effective_uid();
        let missing = |e: std::io::Error| match e.kind() {
            std::io::ErrorKind::NotFound => self.unreachable(),
            _ => ClientError::Io(e),
        };
        let dir = self.socket.parent().ok_or_else(|| self.unreachable())?;
        check_socket_dir(&read_facts(dir).map_err(missing)?, euid)
            .map_err(ClientError::PeerRejected)?;
        check_socket(&read_facts(&self.socket).map_err(missing)?, euid)
            .map_err(ClientError::PeerRejected)?;
        let stream = UnixStream::connect(&self.socket).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                self.unreachable()
            }
            _ => ClientError::Io(e),
        })?;
        let (uid, pid) = peer_of(&stream).map_err(ClientError::Io)?;
        check_server(uid, pid, euid, self.expected_pid).map_err(ClientError::PeerRejected)?;
        Ok(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{Client, ClientError, Deadlines, PeerRejection};
    use crate::paths::DataDir;
    use crate::proto::Platform;
    use crate::testing::{sample_status, ScriptedDaemon, ScriptedUnixServer};
    use std::time::{Duration, Instant};

    fn euid() -> u32 {
        // SAFETY: geteuid reads the process's effective user id; it takes no pointer and cannot fail.
        unsafe { libc::geteuid() }
    }

    fn facts(kind: FileKind, uid: u32, mode: u32) -> PathFacts {
        PathFacts { kind, uid, mode }
    }

    #[test]
    fn a_socket_directory_with_mode_0755_or_of_another_owner_or_a_link_is_refused() {
        let me = euid();
        assert_eq!(
            check_socket_dir(&facts(FileKind::Directory, me, 0o700), me),
            Ok(())
        );
        for (case, f) in [
            ("0755", facts(FileKind::Directory, me, 0o755)),
            ("0750", facts(FileKind::Directory, me, 0o750)),
            ("0701", facts(FileKind::Directory, me, 0o701)),
            (
                "0711 with the sticky bit",
                facts(FileKind::Directory, me, 0o1711),
            ),
            ("0700 sticky", facts(FileKind::Directory, me, 0o1700)),
            ("0700 setgid", facts(FileKind::Directory, me, 0o2700)),
            ("0700 setuid", facts(FileKind::Directory, me, 0o4700)),
            ("another owner", facts(FileKind::Directory, me + 1, 0o700)),
            ("root", facts(FileKind::Directory, 0, 0o700)),
            ("a link", facts(FileKind::Link, me, 0o700)),
            ("a file", facts(FileKind::Other, me, 0o700)),
        ] {
            if case == "root" && me == 0 {
                continue;
            }
            assert!(check_socket_dir(&f, me).is_err(), "{case}");
        }
        assert_eq!(
            check_socket_dir(&facts(FileKind::Directory, me, 0o755), me),
            Err(PeerRejection::SocketDirNotPrivate)
        );
        assert_eq!(
            check_socket_dir(&facts(FileKind::Directory, me + 1, 0o700), me),
            Err(PeerRejection::SocketDirOfAnotherUser)
        );
    }

    #[test]
    fn a_socket_of_another_owner_or_a_path_that_is_no_socket_is_refused() {
        let me = euid();
        assert_eq!(
            check_socket(&facts(FileKind::Socket, me, 0o755), me),
            Ok(())
        );
        assert_eq!(
            check_socket(&facts(FileKind::Socket, me + 1, 0o600), me),
            Err(PeerRejection::SocketOfAnotherUser)
        );
        for kind in [FileKind::Link, FileKind::Other, FileKind::Directory] {
            assert_eq!(
                check_socket(&facts(kind, me, 0o600), me),
                Err(PeerRejection::NotASocket)
            );
        }
    }

    #[test]
    fn a_server_of_another_user_or_another_pid_than_the_expected_one_is_refused() {
        let me = euid();
        assert_eq!(check_server(me, 10, me, None), Ok(()));
        assert_eq!(check_server(me, 10, me, Some(10)), Ok(()));
        assert_eq!(
            check_server(me + 1, 10, me, None),
            Err(PeerRejection::ServerOfAnotherUser)
        );
        assert_eq!(
            check_server(me, 11, me, Some(10)),
            Err(PeerRejection::NotTheExpectedServer)
        );
    }

    #[test]
    fn the_facts_of_a_link_are_the_link_s_own_and_not_its_target_s() {
        let base = tempfile::Builder::new().prefix("cu").tempdir().unwrap();
        let dir = base.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        let link = base.path().join("l");
        std::os::unix::fs::symlink(&dir, &link).unwrap();
        assert_eq!(read_facts(&link).unwrap().kind, FileKind::Link);
        assert_eq!(read_facts(&dir).unwrap().kind, FileKind::Directory);
        assert_eq!(read_facts(&dir).unwrap().uid, euid());
    }

    fn short_data_dir() -> (tempfile::TempDir, DataDir) {
        let base = tempfile::Builder::new().prefix("cu").tempdir().unwrap();
        let dir = DataDir::from_flag(base.path().join("h")).unwrap();
        assert!(
            dir.control_socket().as_os_str().len() < crate::paths::SOCKET_PATH_MAX_BYTES,
            "the target directory is too deep for a socket under run/"
        );
        (base, dir)
    }

    #[test]
    fn f2_the_scripted_unix_server_answers_a_resume_of_a_partial_capture_and_the_next_request() {
        let (_base, dir) = short_data_dir();
        let mut fields = sample_status(Platform::Macos).into_fields();
        fields.sources[2].state = crate::proto::SourceState::Failed {
            reason: crate::proto::Reason::Permission,
            error: None,
        };
        fields.capture = crate::proto::Capture::Partial;
        let server = ScriptedUnixServer::start(
            &dir,
            ScriptedDaemon::new(crate::proto::DaemonStatus::new(fields).unwrap()),
        )
        .unwrap();
        let client = Client::new(UnixConnector::at(&dir));
        assert!(client.resume().is_ok());
        assert!(client.status().is_ok());
        assert_eq!(server.requests().len(), 2);
    }

    #[test]
    fn a_status_and_a_stop_cross_a_real_socket_of_this_user() {
        let (_base, dir) = short_data_dir();
        let server =
            ScriptedUnixServer::start(&dir, ScriptedDaemon::new(sample_status(Platform::Macos)))
                .unwrap();
        let client = Client::new(UnixConnector::at(&dir));
        assert_eq!(client.status().unwrap(), sample_status(Platform::Macos));
        assert!(client.stop().unwrap().flushed);
        assert_eq!(server.requests().len(), 2);
    }

    #[test]
    fn the_expected_server_pid_is_checked_on_the_connection() {
        let (_base, dir) = short_data_dir();
        let _server =
            ScriptedUnixServer::start(&dir, ScriptedDaemon::new(sample_status(Platform::Macos)))
                .unwrap();
        let mine = std::process::id();
        let client = Client::new(UnixConnector::at(&dir).expecting_pid(mine));
        assert!(client.status().is_ok());
        let other = Client::new(UnixConnector::at(&dir).expecting_pid(mine + 1));
        assert!(matches!(
            other.status(),
            Err(ClientError::PeerRejected(
                PeerRejection::NotTheExpectedServer
            ))
        ));
    }

    #[test]
    fn a_peer_that_sends_its_whole_reply_and_keeps_the_connection_gives_the_reply_inside_the_deadline(
    ) {
        use std::io::{Read, Write};

        let (_base, dir) = short_data_dir();
        crate::testing::make_private_dir(&dir.run_dir()).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(dir.control_socket()).unwrap();
        let (release, held) = std::sync::mpsc::channel::<()>();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let _ = stream.read_to_end(&mut request);
            let body = crate::proto::encode_reply(&crate::proto::Reply::Status(sample_status(
                Platform::Macos,
            )));
            let mut frame = (body.len() as u32).to_be_bytes().to_vec();
            frame.extend_from_slice(&body);
            stream.write_all(&frame).unwrap();
            let _ = held.recv();
        });
        let reply = client_within(&dir, 10_000).status();
        let _ = release.send(());
        server.join().unwrap();
        assert_eq!(
            reply.as_ref().ok(),
            Some(&sample_status(Platform::Macos)),
            "{reply:?}: the peer keeps the connection until the client has answered, so a reply \
             is one that the client took without the end of the stream; the deadline is 10 s so \
             that a hosted runner that holds a thread for seconds cannot end the request first"
        );
    }

    const CHILD_DATA_DIR: &str = "HALVO_CONTROL_TEST_CHILD_DATA_DIR";

    #[test]
    #[ignore = "a child of the_pid_of_a_server_in_another_process_is_the_one_checked"]
    fn serve_a_scripted_daemon_until_standard_input_closes() {
        use std::io::Read;

        let Some(dir) = std::env::var_os(CHILD_DATA_DIR) else {
            return;
        };
        let dir = DataDir::from_flag(std::path::PathBuf::from(dir)).unwrap();
        let _server =
            ScriptedUnixServer::start(&dir, ScriptedDaemon::new(sample_status(Platform::Macos)))
                .unwrap();
        println!("server-ready");
        let (closed, wait) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = std::io::stdin().read_to_end(&mut Vec::new());
            let _ = closed.send(());
        });
        let _ = wait.recv_timeout(Duration::from_secs(120));
    }

    /// The server child: its standard input is closed and it is waited for, with a
    /// bound, and killed when the bound passes, on every path out of the test.
    struct ServerChild(Option<std::process::Child>);

    impl Drop for ServerChild {
        fn drop(&mut self) {
            let Some(mut child) = self.0.take() else {
                return;
            };
            drop(child.stdin.take());
            let until = Instant::now() + Duration::from_secs(10);
            while Instant::now() < until {
                if !matches!(child.try_wait(), Ok(None)) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = child.kill();
            let until = Instant::now() + Duration::from_secs(10);
            while Instant::now() < until && matches!(child.try_wait(), Ok(None)) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    #[test]
    fn the_pid_of_a_server_in_another_process_is_the_one_checked() {
        let (_base, dir) = short_data_dir();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "client_unix::tests::serve_a_scripted_daemon_until_standard_input_closes",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_DATA_DIR, dir.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let child_pid = child.id();
        let mut child = ServerChild(Some(child));
        let stdout = child.0.as_mut().unwrap().stdout.take().unwrap();
        let (ready, wait) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(stdout).lines() {
                match line {
                    Ok(line) if line.contains("server-ready") => {
                        let _ = ready.send(true);
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            let _ = ready.send(false);
        });
        assert_eq!(
            wait.recv_timeout(Duration::from_secs(60)),
            Ok(true),
            "the server child did not report its socket bound in 60 s"
        );
        assert_ne!(child_pid, std::process::id());

        let theirs = Client::new(UnixConnector::at(&dir).expecting_pid(child_pid));
        assert_eq!(theirs.status().unwrap(), sample_status(Platform::Macos));
        let mine = Client::new(UnixConnector::at(&dir).expecting_pid(std::process::id()));
        assert!(matches!(
            mine.status(),
            Err(ClientError::PeerRejected(
                PeerRejection::NotTheExpectedServer
            ))
        ));
        drop(child);
    }

    #[test]
    fn a_socket_directory_that_is_not_private_is_refused_before_a_connect() {
        use std::os::unix::fs::PermissionsExt;
        let (_base, dir) = short_data_dir();
        let server =
            ScriptedUnixServer::start(&dir, ScriptedDaemon::new(sample_status(Platform::Macos)))
                .unwrap();
        std::fs::set_permissions(dir.run_dir(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let client = Client::new(UnixConnector::at(&dir));
        let result = client.status();
        std::fs::set_permissions(dir.run_dir(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(matches!(
            result,
            Err(ClientError::PeerRejected(
                PeerRejection::SocketDirNotPrivate
            ))
        ));
        assert!(server.requests().is_empty(), "the client connected");
    }

    #[test]
    fn a_link_at_the_socket_path_is_refused() {
        let (_base, dir) = short_data_dir();
        let (_other_base, other) = short_data_dir();
        let _server =
            ScriptedUnixServer::start(&other, ScriptedDaemon::new(sample_status(Platform::Macos)))
                .unwrap();
        crate::testing::make_private_dir(&dir.run_dir()).unwrap();
        std::os::unix::fs::symlink(other.control_socket(), dir.control_socket()).unwrap();
        let client = Client::new(UnixConnector::at(&dir));
        assert!(matches!(
            client.status(),
            Err(ClientError::PeerRejected(PeerRejection::NotASocket))
        ));
    }

    #[test]
    fn no_socket_or_no_listener_is_unreachable_and_names_the_socket() {
        let (_base, dir) = short_data_dir();
        let client = client_within(&dir, 300);
        match client.status() {
            Err(ClientError::Unreachable { endpoint }) => {
                assert_eq!(endpoint, dir.control_socket().to_str().unwrap())
            }
            other => panic!("{other:?}"),
        }
        crate::testing::make_private_dir(dir.path()).unwrap();
        for case in [
            "no run/ directory",
            "no socket file",
            "a bound socket that does not listen",
        ] {
            match case {
                "no socket file" => crate::testing::make_private_dir(&dir.run_dir()).unwrap(),
                "a bound socket that does not listen" => {
                    drop(std::os::unix::net::UnixListener::bind(dir.control_socket()).unwrap())
                }
                _ => {}
            }
            assert!(
                matches!(client.status(), Err(ClientError::Unreachable { .. })),
                "{case}, the lock free"
            );
            let lock = held(&dir);
            in_use(client.status(), &dir, case);
            drop(lock);
        }
    }

    const IN_USE: &str = "the data directory is in use and no connection could be made at ";

    fn client_within(dir: &DataDir, ms: u64) -> Client<UnixConnector> {
        let mut deadlines = Deadlines::V1;
        deadlines.read = Duration::from_millis(ms);
        Client::with_deadlines(UnixConnector::at(dir), deadlines)
    }

    fn held(dir: &DataDir) -> crate::instance_lock::InstanceLock {
        crate::testing::make_private_dir(dir.path()).unwrap();
        crate::instance_lock::InstanceLock::try_take(dir)
            .unwrap()
            .expect("the lock is free before the test takes it")
    }

    fn in_use<T: std::fmt::Debug>(result: Result<T, ClientError>, dir: &DataDir, case: &str) {
        let error = result.unwrap_err();
        assert_eq!(
            error.to_string(),
            format!(
                "{IN_USE}{}; capture may be running",
                dir.control_socket().display()
            ),
            "{case}"
        );
    }

    /// A connector whose socket lies outside its data directory, as C3.5 allows when the
    /// socket goes under the user's temporary directory.
    fn connector_beside(data_dir: &Path, socket_dir: &DataDir) -> UnixConnector {
        UnixConnector {
            socket: socket_dir.control_socket(),
            data_dir: DataDir::from_flag(data_dir).unwrap(),
            expected_pid: None,
        }
    }

    #[test]
    fn a_fifo_at_the_data_directory_path_is_an_io_error_with_exit_1_at_once_through_the_connector()
    {
        use crate::instance_lock::planted;

        let (_base, socket_dir) = short_data_dir();
        crate::testing::make_private_dir(&socket_dir.run_dir()).unwrap();
        let fifo = socket_dir.path().join("planted");
        planted::mkfifo(&fifo);
        let connector = connector_beside(&fifo, &socket_dir);
        let answer = planted::answer_within(&fifo, &fifo, move |_| Client::new(connector).status());
        let Some(answer) = answer else {
            panic!(
                "the connect did not return within {:?} on a FIFO at the data directory's path",
                planted::BOUND
            );
        };
        assert!(matches!(answer, Err(ClientError::Io(_))), "{answer:?}");
        let printed = crate::ctl_text::print(
            &crate::ctl_text::Verb::Status { indicator: false },
            answer.map(crate::proto::Reply::Status),
            &socket_dir,
        );
        assert_eq!(printed.status, 1, "{printed:?}");
        assert!(!printed.stderr.contains("not running"), "{printed:?}");
    }

    #[test]
    fn an_open_error_of_the_lock_read_is_an_io_error_with_exit_1_through_the_connector() {
        use crate::instance_lock::planted;

        let (_base, socket_dir) = short_data_dir();
        crate::testing::make_private_dir(&socket_dir.run_dir()).unwrap();
        let closed_path = socket_dir.path().join("closed");
        let _closed = planted::Closed::make(&closed_path);
        let answer = Client::new(connector_beside(&closed_path, &socket_dir)).status();
        assert!(matches!(answer, Err(ClientError::Io(_))), "{answer:?}");
        let printed = crate::ctl_text::print(
            &crate::ctl_text::Verb::Status { indicator: false },
            answer.map(crate::proto::Reply::Status),
            &socket_dir,
        );
        assert_eq!(printed.status, 1, "{printed:?}");
        assert!(!printed.stderr.contains("not running"), "{printed:?}");
    }

    #[test]
    fn the_in_use_answer_comes_no_later_than_the_deadline_and_one_step() {
        let (_base, dir) = short_data_dir();
        let lock = held(&dir);
        let started = Instant::now();
        let result = client_within(&dir, 5_000).status();
        let took = started.elapsed();
        drop(lock);
        in_use(result, &dir, "a lock held through the whole deadline");
        assert!(
            took < Duration::from_secs(15),
            "the in-use answer came after {took:?}; the request's deadline is 5 s and one step \
             50 ms. The bound leaves 10 s past the deadline for a hosted runner that holds a \
             thread for seconds, and a wait of four deadlines, 20 s, passes it"
        );
    }

    #[test]
    fn no_data_directory_is_unreachable() {
        let (_base, dir) = short_data_dir();
        assert!(!dir.path().exists());
        assert!(matches!(
            client_within(&dir, 300).status(),
            Err(ClientError::Unreachable { .. })
        ));
    }

    #[test]
    fn f3_a_listener_whose_queue_is_full_is_in_use_under_a_held_lock_and_not_unreachable() {
        use std::os::unix::io::AsRawFd;

        let (_base, dir) = short_data_dir();
        crate::testing::make_private_dir(&dir.run_dir()).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(dir.control_socket()).unwrap();
        // SAFETY: the descriptor is the bound socket of `listener`; listen takes no pointer.
        assert_eq!(unsafe { libc::listen(listener.as_raw_fd(), 1) }, 0);
        let mut queued = Vec::new();
        let mut refused = false;
        for _ in 0..16 {
            match UnixStream::connect(dir.control_socket()) {
                Ok(stream) => queued.push(stream),
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                    refused = true;
                    break;
                }
                Err(e) => panic!("{e}"),
            }
        }
        assert!(
            refused,
            "no connect was refused after {} connections",
            queued.len()
        );
        println!(
            "f3: {} connections held before the first refusal",
            queued.len()
        );
        let client = client_within(&dir, 300);
        let lock = held(&dir);
        in_use(client.status(), &dir, "a full queue");
        drop(lock);
        assert!(matches!(
            client.status(),
            Err(ClientError::Unreachable { .. })
        ));
        drop(queued);
    }

    #[test]
    fn a_listener_that_appears_while_the_lock_is_held_gives_the_reply() {
        let (_base, dir) = short_data_dir();
        let lock = held(&dir);
        let starter = {
            let dir = dir.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                ScriptedUnixServer::start(&dir, ScriptedDaemon::new(sample_status(Platform::Macos)))
                    .unwrap()
            })
        };
        let reply = client_within(&dir, 5_000).status();
        let server = starter.join().unwrap();
        drop(lock);
        assert_eq!(reply.unwrap(), sample_status(Platform::Macos));
        drop(server);
    }

    #[test]
    fn a_lock_that_is_freed_during_the_wait_gives_unreachable() {
        let (_base, dir) = short_data_dir();
        let lock = held(&dir);
        let (calls, called) = std::sync::mpsc::channel();
        let (answers, answer) = std::sync::mpsc::channel();
        let started = Instant::now();
        let client = {
            let dir = dir.clone();
            std::thread::spawn(move || {
                calls.send(()).unwrap();
                let _ = answers.send(client_within(&dir, 15_000).status());
            })
        };
        called.recv().unwrap();
        assert!(
            answer.recv_timeout(Duration::from_millis(300)).is_err(),
            "the client answered while the lock was held and before it was freed"
        );
        drop(lock);
        let result = answer.recv().unwrap();
        let took = started.elapsed();
        client.join().unwrap();
        assert!(
            matches!(result, Err(ClientError::Unreachable { .. })),
            "{result:?}: the lock was freed while the client waited; its deadline is 15 s so \
             that a hosted runner that holds a thread for seconds still ends the wait by the \
             freed lock"
        );
        assert!(
            took >= Duration::from_millis(300),
            "{took:?}: the lock is freed only after 300 ms in which the client gave no answer, \
             counted from before the client's thread was made, so the answer comes later on any \
             machine"
        );
    }

    #[test]
    fn a_reply_that_never_comes_ends_at_the_deadline_on_a_real_socket() {
        let (_base, dir) = short_data_dir();
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        daemon.set_silent(true);
        let _server = ScriptedUnixServer::start(&dir, daemon).unwrap();
        let mut deadlines = Deadlines::V1;
        deadlines.read = Duration::from_millis(300);
        let client = Client::with_deadlines(UnixConnector::at(&dir), deadlines);
        let started = Instant::now();
        assert!(matches!(client.status(), Err(ClientError::Deadline { .. })));
        let took = started.elapsed();
        assert!(
            took >= Duration::from_millis(300) && took < Duration::from_secs(3),
            "{took:?}"
        );
    }

    #[test]
    fn the_socket_of_the_data_directory_comes_from_the_one_resolver() {
        let (_base, dir) = short_data_dir();
        let connector = UnixConnector::for_data_dir(&dir, &crate::paths::SystemAccount).unwrap();
        assert_eq!(connector.socket(), dir.control_socket());
    }

    /// Sets each path back to mode 0700 when it is dropped, on every path out of the test.
    struct ModeBack(Vec<PathBuf>);

    impl Drop for ModeBack {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            for path in &self.0 {
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
            }
        }
    }

    #[test]
    fn ct15_ctl_prints_the_cause_at_the_data_folder_and_no_raw_os_error_at_the_name_and_below_it() {
        use crate::paths::EntryAtDataDir;
        use std::os::unix::fs::{symlink, PermissionsExt};

        let mode = |path: &Path, bits: u32| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits)).unwrap()
        };
        let mut rows = 0;
        for base_kind in [
            "a file",
            "a link to a file",
            "a link loop",
            "a link that leads nowhere",
            "a link behind a closed parent",
            "a link to a folder of mode 0000",
            "a folder of mode 0000",
        ] {
            for below in [false, true] {
                let (_root, short) = short_data_dir();
                let root = short.path().parent().unwrap().to_path_buf();
                let name = root.join("n");
                let mut back = ModeBack(Vec::new());
                let cause = match base_kind {
                    "a file" => {
                        std::fs::write(&name, b"x").unwrap();
                        EntryAtDataDir::NotADirectory
                    }
                    "a link to a file" => {
                        std::fs::write(root.join("f"), b"x").unwrap();
                        symlink(root.join("f"), &name).unwrap();
                        EntryAtDataDir::NotADirectory
                    }
                    "a link loop" => {
                        symlink(&name, &name).unwrap();
                        EntryAtDataDir::LinkLeadsNowhere
                    }
                    "a link that leads nowhere" => {
                        symlink(root.join("unplugged"), &name).unwrap();
                        EntryAtDataDir::LinkLeadsNowhere
                    }
                    "a link behind a closed parent" => {
                        let closed = root.join("c");
                        std::fs::create_dir_all(closed.join("t")).unwrap();
                        symlink(closed.join("t"), &name).unwrap();
                        back.0.push(closed.clone());
                        mode(&closed, 0o000);
                        EntryAtDataDir::CannotBeRead
                    }
                    "a link to a folder of mode 0000" => {
                        let target = root.join("t");
                        std::fs::create_dir(&target).unwrap();
                        symlink(&target, &name).unwrap();
                        back.0.push(target.clone());
                        mode(&target, 0o000);
                        EntryAtDataDir::CannotBeRead
                    }
                    "a folder of mode 0000" => {
                        std::fs::create_dir(&name).unwrap();
                        back.0.push(name.clone());
                        mode(&name, 0o000);
                        EntryAtDataDir::CannotBeRead
                    }
                    _ => unreachable!(),
                };
                if below && base_kind == "a folder of mode 0000" {
                    continue;
                }
                let dir =
                    DataDir::from_flag(if below { name.join("halvo") } else { name }).unwrap();
                let answer = client_within(&dir, 300).status();
                let printed = crate::ctl_text::print(
                    &crate::ctl_text::Verb::Status { indicator: false },
                    answer.map(crate::proto::Reply::Status),
                    &dir,
                );
                let row = format!("{base_kind}, below: {below}");
                assert_eq!(
                    (printed.stderr.as_str(), printed.status),
                    (
                        format!("halvo-capture: {cause}: {}\n", dir.path().display()).as_str(),
                        1
                    ),
                    "{row}"
                );
                assert!(!printed.stderr.contains("os error"), "{row}");
                drop(back);
                rows += 1;
            }
        }
        assert_eq!(rows, 13);
    }

    #[test]
    fn ct15_ctl_reads_a_data_folder_whose_name_is_longer_than_255_bytes_as_cannot_be_read() {
        let (_root, short) = short_data_dir();
        let long = short.path().parent().unwrap().join("x".repeat(256));
        for path in [long.clone(), long.join("halvo")] {
            let dir = DataDir::from_flag(path).unwrap();
            for connector in [
                UnixConnector::at(&dir),
                UnixConnector::for_data_dir(&dir, &crate::paths::SystemAccount).unwrap(),
            ] {
                let mut deadlines = Deadlines::V1;
                deadlines.read = Duration::from_millis(300);
                let answer = Client::with_deadlines(connector, deadlines).status();
                let printed = crate::ctl_text::print(
                    &crate::ctl_text::Verb::Status { indicator: false },
                    answer.map(crate::proto::Reply::Status),
                    &dir,
                );
                assert_eq!(
                    (printed.stderr, printed.status),
                    (
                        format!(
                            "halvo-capture: the data directory or a folder on its path cannot be read: {}\n",
                            dir.path().display()
                        ),
                        1
                    )
                );
            }
        }
    }

    fn set_mode(path: &Path, bits: u32) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits)).unwrap();
    }

    #[test]
    fn ct16_c1_a_free_data_folder_that_opens_and_cannot_be_searched_prints_cannot_be_read_through_both_connectors(
    ) {
        for bits in [0o600, 0o400] {
            let (_base, dir) = short_data_dir();
            crate::testing::make_private_dir(dir.path()).unwrap();
            let _back = ModeBack(vec![dir.path().to_path_buf()]);
            set_mode(dir.path(), bits);
            for connector in [
                UnixConnector::at(&dir),
                UnixConnector::for_data_dir(&dir, &crate::paths::SystemAccount).unwrap(),
            ] {
                let mut deadlines = Deadlines::V1;
                deadlines.read = Duration::from_millis(300);
                let answer = Client::with_deadlines(connector, deadlines).status();
                let printed = crate::ctl_text::print(
                    &crate::ctl_text::Verb::Status { indicator: false },
                    answer.map(crate::proto::Reply::Status),
                    &dir,
                );
                assert!(
                    !printed.stderr.contains("os error"),
                    "{bits:o}: {printed:?}"
                );
                assert_eq!(
                    (printed.stderr, printed.status),
                    (
                        format!(
                            "halvo-capture: the data directory or a folder on its path cannot be read: {}\n",
                            dir.path().display()
                        ),
                        1
                    ),
                    "{bits:o}"
                );
            }
        }
    }

    #[test]
    fn ct16_c2_a_held_folder_that_cannot_be_searched_or_a_socket_that_refuses_the_connect_answers_in_use_after_the_deadline(
    ) {
        for bits in [0o600, 0o400] {
            let (_base, dir) = short_data_dir();
            let lock = held(&dir);
            let _back = ModeBack(vec![dir.path().to_path_buf()]);
            set_mode(dir.path(), bits);
            let started = Instant::now();
            let result = client_within(&dir, 300).status();
            assert!(started.elapsed() >= Duration::from_millis(300), "{bits:o}");
            in_use(result, &dir, &format!("held folder of mode {bits:o}"));
            drop(lock);
        }
        let (_base, dir) = short_data_dir();
        crate::testing::make_private_dir(&dir.run_dir()).unwrap();
        let _listener = std::os::unix::net::UnixListener::bind(dir.control_socket()).unwrap();
        let _back = ModeBack(vec![dir.control_socket()]);
        set_mode(&dir.control_socket(), 0o000);
        let lock = held(&dir);
        let started = Instant::now();
        let result = client_within(&dir, 300).status();
        assert!(started.elapsed() >= Duration::from_millis(300));
        in_use(result, &dir, "a socket of mode 0000 beside a held lock");
        drop(lock);
    }

    #[test]
    fn ct16_c3_a_delete_whose_channel_breaks_after_the_request_says_its_outcome_is_not_known() {
        use std::io::Read;

        let (_base, dir) = short_data_dir();
        crate::testing::make_private_dir(&dir.run_dir()).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(dir.control_socket()).unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let _ = stream.read_to_end(&mut request);
            request.len()
        });
        let armed = Some(crate::testing::FIXTURE_STORE_ID);
        let answer =
            client_within(&dir, 10_000).send(&crate::proto::Request::DeleteAll { store: armed });
        assert!(server.join().unwrap() > 0, "the request was sent");
        let printed =
            crate::ctl_text::print(&crate::ctl_text::Verb::DeleteAll(armed), answer, &dir);
        assert_eq!(printed.status, 1, "{printed:?}");
        assert!(
            printed.stderr.starts_with(
                "halvo-capture: whether this request deleted anything is not known; read the status; "
            ),
            "{printed:?}"
        );
        assert_eq!(printed.stderr.lines().count(), 1, "{printed:?}");
        for claim in ["deleted nothing", "nothing was deleted"] {
            assert!(!printed.stderr.contains(claim), "{printed:?}");
        }
    }
}
