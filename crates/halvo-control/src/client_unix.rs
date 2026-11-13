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
    expected_pid: Option<u32>,
}

impl UnixConnector {
    pub fn for_data_dir(
        data_dir: &DataDir,
        account: &dyn AccountLookup,
    ) -> Result<Self, ClientError> {
        let socket = crate::paths::socket_path(data_dir, account)
            .map_err(|e| ClientError::Protocol(e.to_string()))?;
        Ok(Self::at(socket))
    }

    pub(crate) fn at(socket: PathBuf) -> Self {
        Self {
            socket,
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

    fn connect(&self, _timeout: Duration) -> Result<UnixStream, ClientError> {
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
    fn a_status_and_a_stop_cross_a_real_socket_of_this_user() {
        let (_base, dir) = short_data_dir();
        let server =
            ScriptedUnixServer::start(&dir, ScriptedDaemon::new(sample_status(Platform::Macos)))
                .unwrap();
        let client = Client::new(UnixConnector::at(dir.control_socket()));
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
        let client = Client::new(UnixConnector::at(dir.control_socket()).expecting_pid(mine));
        assert!(client.status().is_ok());
        let other = Client::new(UnixConnector::at(dir.control_socket()).expecting_pid(mine + 1));
        assert!(matches!(
            other.status(),
            Err(ClientError::PeerRejected(
                PeerRejection::NotTheExpectedServer
            ))
        ));
    }

    #[test]
    fn a_socket_directory_that_is_not_private_is_refused_before_a_connect() {
        use std::os::unix::fs::PermissionsExt;
        let (_base, dir) = short_data_dir();
        let server =
            ScriptedUnixServer::start(&dir, ScriptedDaemon::new(sample_status(Platform::Macos)))
                .unwrap();
        std::fs::set_permissions(dir.run_dir(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let client = Client::new(UnixConnector::at(dir.control_socket()));
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
        let client = Client::new(UnixConnector::at(dir.control_socket()));
        assert!(matches!(
            client.status(),
            Err(ClientError::PeerRejected(PeerRejection::NotASocket))
        ));
    }

    #[test]
    fn no_socket_or_no_listener_is_unreachable_and_names_the_socket() {
        let (_base, dir) = short_data_dir();
        let client = Client::new(UnixConnector::at(dir.control_socket()));
        match client.status() {
            Err(ClientError::Unreachable { endpoint }) => {
                assert_eq!(endpoint, dir.control_socket().to_str().unwrap())
            }
            other => panic!("{other:?}"),
        }
        crate::testing::make_private_dir(&dir.run_dir()).unwrap();
        drop(std::os::unix::net::UnixListener::bind(dir.control_socket()).unwrap());
        assert!(matches!(
            client.status(),
            Err(ClientError::Unreachable { .. })
        ));
    }

    #[test]
    fn a_reply_that_never_comes_ends_at_the_deadline_on_a_real_socket() {
        let (_base, dir) = short_data_dir();
        let mut daemon = ScriptedDaemon::new(sample_status(Platform::Macos));
        daemon.set_silent(true);
        let _server = ScriptedUnixServer::start(&dir, daemon).unwrap();
        let mut deadlines = Deadlines::V1;
        deadlines.read = Duration::from_millis(300);
        let client = Client::with_deadlines(UnixConnector::at(dir.control_socket()), deadlines);
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
}
