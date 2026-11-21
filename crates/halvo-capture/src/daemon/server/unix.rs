//! The control channel of a service run on a Mac: the Unix socket at
//! `socket_path` of the data directory, under run/ of mode 0700 (contracts.md
//! C3.2 step 6, C3.5).

use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use halvo_control::paths::{DataDir, SystemAccount};

use super::{Admitting, Asked};

const ACCEPT_POLL: Duration = Duration::from_millis(10);

/// A bound control channel: the socket, the thread that accepts on it, and
/// the requests that its connections hand to the service run's loop. Its drop
/// stops the accepting thread and unlinks the socket.
pub struct Listening {
    path: PathBuf,
    asked: Receiver<Asked>,
    admitting: Arc<Admitting>,
    closed: Arc<AtomicBool>,
    accepter: Option<JoinHandle<()>>,
}

impl Listening {
    /// Makes run/ of `dir` with mode 0700, unlinks a socket that an earlier
    /// life left at the control socket's path, binds the socket there and
    /// starts accepting. The caller holds the instance lock of `dir`.
    pub fn bind(dir: &DataDir) -> io::Result<Listening> {
        let run = dir.run_dir();
        make_private_dir(&run)?;
        let path = halvo_control::paths::socket_path(dir, &SystemAccount)
            .map_err(|error| io::Error::other(error.to_string()))?;
        if let Some(parent) = path.parent().filter(|parent| *parent != run) {
            make_private_dir(parent)?;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        let own = effective_uid();
        let admitting = Arc::new(Admitting::default());
        let closed = Arc::new(AtomicBool::new(false));
        let (hand, asked) = mpsc::channel();
        let accepter = std::thread::Builder::new()
            .name("halvo-control-server".to_owned())
            .spawn({
                let admitting = admitting.clone();
                let closed = closed.clone();
                move || accept(&listener, own, &admitting, &closed, &hand)
            })?;
        Ok(Listening {
            path,
            asked,
            admitting,
            closed,
            accepter: Some(accepter),
        })
    }

    /// The next request that a connection handed over, if one waits.
    pub fn next(&self) -> Option<Asked> {
        self.asked.try_recv().ok()
    }

    /// What the connections share with the loop.
    pub fn admitting(&self) -> &Admitting {
        &self.admitting
    }

    /// The path of the socket.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(accepter) = self.accepter.take() {
            let _ = accepter.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

fn make_private_dir(path: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
}

fn accept(
    listener: &UnixListener,
    own: u32,
    admitting: &Arc<Admitting>,
    closed: &AtomicBool,
    hand: &Sender<Asked>,
) {
    while !closed.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let admitting = admitting.clone();
                let hand = hand.clone();
                let _ = std::thread::Builder::new()
                    .name("halvo-control-connection".to_owned())
                    .spawn(move || serve_one(stream, own, &admitting, &hand));
            }
            Err(_) => std::thread::sleep(ACCEPT_POLL),
        }
    }
}

fn serve_one(mut stream: UnixStream, own: u32, admitting: &Arc<Admitting>, hand: &Sender<Asked>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(super::READ_DEADLINE));
    let _ = stream.set_write_timeout(Some(super::REPLY_WRITE_DEADLINE));
    let peer = peer_uid(&stream).ok();
    let written = super::serve_connection(&mut stream, peer, own, admitting, hand);
    let _ = stream.shutdown(std::net::Shutdown::Write);
    drop(stream);
    if let Some(written) = written {
        let _ = written.send(());
    }
}

fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    if unsafe { libc::getpeereid(stream.as_raw_fd(), &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(uid)
}

fn effective_uid() -> u32 {
    unsafe { libc::geteuid() }
}

/// The peak resident size of this process in bytes, as getrusage reports it
/// on a Mac; 0 when the call fails.
pub fn rss_peak_bytes() -> u64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return 0;
    }
    let usage = unsafe { usage.assume_init() };
    u64::try_from(usage.ru_maxrss).unwrap_or(0)
}
