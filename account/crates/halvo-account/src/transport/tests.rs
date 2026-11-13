//! The two limits of a call. The tests build the client through `Client::with_limits`, the
//! builder that `Client::new` calls, with short limits of their own, so that none waits 30 s.
//! No test sleeps: each waits on a socket or on a channel.
//! Each call runs on a thread of its own, and a test waits for it at most `CEILING`: a copy of
//! the client without a limit fails within that time.

use super::{Client, Limits};
use crate::flow::{run, Keep, NoRandom, NotOpened, Opener, Random, Reach};
use crate::grammar::{parse, Build};
use crate::origin::{Backend, PageUrl, Pages};
use crate::outcome::{Failure, Outcome};
use crate::sign_in::SignIn;
use halvo_secrets::{Delete, Read, Secret, Write};
use std::ffi::OsString;
use std::io::{ErrorKind, Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

/// The longest that a test waits for one call.
const CEILING: Duration = Duration::from_secs(45);

/// An address that no one answers on this Mac: 127.0.0.2 stands on no interface here, so a
/// connect to it gets neither an accept nor a refusal (evidence/verifier-20/i-connect-probe.txt
/// of the run). On Windows the whole of 127.0.0.0/8 is loopback, and the hosted windows runner
/// refuses this connect after 2 s (run 43342878441: `ConnectionRefused`, os error 10061).
const SILENT: &str = "127.0.0.2:9";

/// A store that holds one sign-in, for `status`, which reads it and nothing more.
struct Holding(SignIn);

impl Keep for Holding {
    fn read(&self) -> Read {
        Read::Found(self.0.to_secret())
    }

    fn write(&self, _secret: &Secret) -> Write {
        unreachable!("status writes nothing")
    }

    fn delete(&self) -> Delete {
        unreachable!("status deletes nothing")
    }
}

impl Opener for Holding {
    fn open(&self, _page: &PageUrl) -> Result<(), NotOpened> {
        unreachable!("status opens nothing")
    }
}

impl Random for Holding {
    fn fill(&self, _bytes: &mut [u8; 48]) -> Result<(), NoRandom> {
        unreachable!("status makes no sign-in")
    }
}

/// What one run of `status` came to: its exit code, its line, whether it failed with no
/// answer, and how long the call took.
struct Ran {
    code: u8,
    line: String,
    no_answer: bool,
    took: Duration,
}

/// Runs `status` with a stored sign-in, through a client of `limits` for the backend
/// `origin`, on a thread of its own. Answers `None` when the run did not end within
/// `CEILING`.
fn status_through(origin: &str, limits: Limits) -> Option<Ran> {
    let backend = Backend::from_address(origin).expect("a backend by address");
    let (tell, ended) = mpsc::channel();
    thread::spawn(move || {
        let client = Client::with_limits(backend, limits).expect("a client");
        let world = Holding(SignIn::from_random(&[7; 48]));
        let args = [OsString::from("status")];
        let invocation = parse(&args, Build::Debug).expect("status");
        let started = Instant::now();
        let outcome = run(
            &invocation,
            &Reach {
                keep: &world,
                transport: &client,
                opener: &world,
                random: &world,
                pages: &Pages::Loopback,
            },
        );
        let _ = tell.send(Ran {
            code: outcome.code(),
            line: outcome.line(),
            no_answer: matches!(outcome, Outcome::Failed(Failure::NoAnswer)),
            took: started.elapsed(),
        });
    });
    ended.recv_timeout(CEILING).ok()
}

/// A loopback stand-in that accepts one connection, reads the request and sends nothing. It
/// ends on its own when the client goes away, and at the latest when `hold` has passed since
/// its start; then it sets `closed` and closes the connection.
struct Silent {
    origin: String,
    request: mpsc::Receiver<Vec<u8>>,
    closed: Arc<AtomicBool>,
}

fn silent_stand_in(hold: Duration) -> Silent {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
    let origin = format!("http://{}", listener.local_addr().expect("an address"));
    let closed = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&closed);
    let (tell, request) = mpsc::channel();
    thread::spawn(move || {
        let started = Instant::now();
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut seen = Vec::new();
        let mut told = false;
        let mut chunk = [0; 1024];
        loop {
            let left = hold.saturating_sub(started.elapsed());
            if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
                break;
            }
            match stream.read(&mut chunk) {
                Ok(0) => return,
                Ok(read) => {
                    seen.extend_from_slice(&chunk[..read]);
                    if !told && seen.windows(4).any(|end| end == b"\r\n\r\n") {
                        let _ = tell.send(seen.clone());
                        told = true;
                    }
                }
                Err(error)
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    break
                }
                Err(_) => return,
            }
        }
        flag.store(true, Ordering::SeqCst);
        drop(stream);
    });
    Silent {
        origin,
        request,
        closed,
    }
}

#[test]
fn the_client_of_the_product_waits_10_s_to_connect_and_30_s_for_the_whole_call() {
    let backend = Backend::from_address("http://127.0.0.1:9").expect("a backend");
    let client = Client::new(backend).expect("a client");
    assert_eq!(
        client.limits,
        Limits {
            connect: Duration::from_secs(10),
            call: Duration::from_secs(30),
        }
    );
}

/// The call ends after its own limit and before the connect value, which is longer: a builder
/// that hands the call limit to the connect setting and the connect limit to the whole call
/// would end this call after 6 s, and fails here.
#[test]
fn a_call_whose_answer_is_held_fails_after_the_call_limit() {
    let short = Limits {
        connect: Duration::from_secs(6),
        call: Duration::from_secs(1),
    };
    let stand_in = silent_stand_in(Duration::from_secs(10));
    let ran = status_through(&stand_in.origin, short).expect("the call ended within the ceiling");
    let closed = stand_in.closed.load(Ordering::SeqCst);
    let request = stand_in
        .request
        .recv_timeout(Duration::from_secs(5))
        .expect("the stand-in read a request");
    assert!(
        request.starts_with(b"GET /v1/subscription HTTP/1.1\r\n"),
        "{}",
        String::from_utf8_lossy(&request)
    );
    assert!(
        !closed,
        "the call ended only when the stand-in closed, after {:?}",
        ran.took
    );
    assert_eq!(
        (ran.code, ran.line.as_str(), ran.no_answer),
        (1, "error=failed-call", true)
    );
    assert!(
        ran.took >= short.call,
        "the call ended before its limit, after {:?}",
        ran.took
    );
    assert!(
        ran.took < short.connect,
        "the call ended at the connect value and not at its own limit, after {:?}",
        ran.took
    );
}

/// On macOS the control connect must stay silent. On the hosted windows runner it is refused
/// (see `SILENT`), so there the test prints the control's line and shows nothing about the
/// connect limit; the limit is shown on macOS alone.
#[test]
fn a_connect_that_is_not_answered_fails_after_the_connect_limit() {
    let address: SocketAddr = SILENT.parse().expect("an address");
    let started = Instant::now();
    let control = TcpStream::connect_timeout(&address, Duration::from_secs(3));
    let silent = matches!(&control, Err(error) if error.kind() == ErrorKind::TimedOut);
    let gave = match &control {
        Ok(_) => "an accepted connection".to_string(),
        Err(error) => format!("{:?} ({error})", error.kind()),
    };
    let line = format!(
        "halvo-account connect control to {SILENT}: {gave} after {:.1} s, silent={silent}",
        started.elapsed().as_secs_f64()
    );
    // Written to the descriptor and not through print!, so that the line stands in the log of
    // a run in which the test passes.
    let _ = writeln!(std::io::stderr(), "{line}");
    if cfg!(windows) {
        if !silent {
            return;
        }
    } else {
        assert!(silent, "the control was answered: {line}");
    }
    let short = Limits {
        connect: Duration::from_secs(1),
        call: Duration::from_secs(20),
    };
    let ran = status_through(&format!("http://{SILENT}"), short)
        .expect("the call ended within the ceiling");
    assert_eq!(
        (ran.code, ran.line.as_str(), ran.no_answer),
        (1, "error=failed-call", true)
    );
    assert!(
        ran.took >= short.connect,
        "the call ended before its connect limit, after {:?}",
        ran.took
    );
    assert!(
        ran.took < short.call / 2,
        "the call waited past its connect limit, {:?}",
        ran.took
    );
}
