mod common;

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use common::{ask_store, effect, id, wait_until, Rig, Store};
use halvo_agent_client::link::{End, Ends, Refusal, StreamLink};
use halvo_agent_client::OwnerLink;
use halvo_seam::frame::{read_frame, write_frame, CLIENT_FRAME_MAX, SERVER_FRAME_MAX};

const SHORT: Duration = Duration::from_secs(10);

/// What the far end of a new stream does.
#[derive(Clone, Copy)]
enum Far {
    /// Reads one request, writes the first half of the frame of the memory owner's reply to it, and closes.
    Half,
    /// Is closed before the link gets the stream's other end.
    Dead,
    /// Answers each request from the memory owner until the stream ends.
    Serve,
    /// Answers one request from the memory owner and leaves, as an owner that exits while the link is idle: it ends
    /// the stream for the link, makes the file `Seen::left`, and counts the bytes that it reads after that until
    /// the link closes the stream.
    Leaves,
}

/// What the test's ends saw: the calls of `Ends`, the requests that a far end read, and the memory owner that
/// answers them; for a far end that leaves, the file that it makes when it has left and the bytes that it read after.
#[derive(Default)]
struct Seen {
    asked: AtomicUsize,
    read: AtomicUsize,
    owner: Store,
    left: OnceLock<PathBuf>,
    late: Mutex<Option<usize>>,
}

/// The test's `Ends`: each call takes the next planned answer, and for a far end makes a new stream and starts the
/// far end. With no answer left it refuses `internal`.
struct Planned {
    plan: Mutex<VecDeque<Result<Far, Refusal>>>,
    seen: Arc<Seen>,
}

impl Planned {
    fn new(plan: Vec<Result<Far, Refusal>>) -> (Planned, Arc<Seen>) {
        let seen = Arc::new(Seen::default());
        let ends = Planned {
            plan: Mutex::new(plan.into()),
            seen: seen.clone(),
        };
        (ends, seen)
    }
}

impl Ends for Planned {
    fn end(&self) -> Result<End, Refusal> {
        self.seen.asked.fetch_add(1, Ordering::SeqCst);
        let far = self
            .plan
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(Refusal::Internal))?;
        let (end, reads, writes) = stream();
        far_end(far, reads, writes, self.seen.clone());
        Ok(end)
    }
}

type FarReads = Box<dyn Read + Send>;
type FarWrites = Box<dyn FarWrite>;

/// The far end's half that writes. `leave` ends the stream for the link and leaves the far end's half that reads
/// open.
trait FarWrite: Write + Send {
    fn leave(self: Box<Self>);
}

#[cfg(unix)]
impl FarWrite for std::os::unix::net::UnixStream {
    fn leave(self: Box<Self>) {
        self.shutdown(std::net::Shutdown::Write).unwrap();
    }
}

#[cfg(windows)]
impl FarWrite for std::io::PipeWriter {
    fn leave(self: Box<Self>) {
        drop(self);
    }
}

/// A new stream: the link's end, and the far end as the half that reads and the half that writes.
#[cfg(unix)]
fn stream() -> (End, FarReads, FarWrites) {
    let (near, far) = std::os::unix::net::UnixStream::pair().unwrap();
    (
        End::new(near.into()),
        Box::new(far.try_clone().unwrap()),
        Box::new(far),
    )
}

/// A new stream of two pipes: the link's end, and the far end as the half that reads and the half that writes.
#[cfg(windows)]
fn stream() -> (End, FarReads, FarWrites) {
    let (near_reads, far_writes) = std::io::pipe().unwrap();
    let (far_reads, near_writes) = std::io::pipe().unwrap();
    (
        End::new(near_reads.into(), near_writes.into()),
        Box::new(far_reads),
        Box::new(far_writes),
    )
}

/// Does what `far` says with the far end of a stream; a far end that reads does so in a thread of its own.
fn far_end(far: Far, mut reads: FarReads, mut writes: FarWrites, seen: Arc<Seen>) {
    if let Far::Dead = far {
        return;
    }
    thread::spawn(move || {
        while let Ok(Some(request)) = read_frame(&mut reads, CLIENT_FRAME_MAX) {
            seen.read.fetch_add(1, Ordering::SeqCst);
            let Ok(reply) = seen.owner.request(&request) else {
                return;
            };
            if let Far::Half = far {
                let mut frame = u32::try_from(reply.len()).unwrap().to_be_bytes().to_vec();
                frame.extend_from_slice(&reply);
                let _ = writes.write_all(&frame[..frame.len() / 2]);
                return;
            }
            if write_frame(&mut writes, &reply, SERVER_FRAME_MAX).is_err() {
                return;
            }
            if let Far::Leaves = far {
                writes.leave();
                std::fs::write(seen.left.get().unwrap(), b"").unwrap();
                let mut late = Vec::new();
                let _ = reads.read_to_end(&mut late);
                *seen.late.lock().unwrap() = Some(late.len());
                return;
            }
        }
    });
}

fn answered(n: u8, error: &str) -> String {
    format!(
        r#"{{"t":"store_reply","reply":{{"v":1,"id":"{}","error":"{error}"}}}}"#,
        id(n)
    )
}

fn stored(n: u8) -> String {
    format!(
        r#"{{"t":"store_reply","reply":{{"v":1,"id":"{}","ok":{{"stored":1}}}}}}"#,
        id(n)
    )
}

/// U9.2 point 2: the far end reads the request, writes half the frame of its reply and closes. The link returns an
/// error and sends the request on no new end, and the library answers the runner `store`.
#[test]
fn a_stream_that_ends_inside_a_reply_is_answered_store() {
    let (ends, seen) = Planned::new(vec![Ok(Far::Half)]);
    let rig = Rig::linked("link-half", Arc::new(StreamLink::new(ends)));
    let script = rig.script(&[
        "hello",
        &ask_store(&id(0x70), &effect(&id(0x71), 1, "held")),
        "log asked",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("asked", SHORT), "{}", script.log());
    assert_eq!(script.replies(), vec![answered(0x70, "store")]);
    assert_eq!(seen.read.load(Ordering::SeqCst), 1, "the far end read");
    assert_eq!(seen.asked.load(Ordering::SeqCst), 1, "calls of Ends");
    assert_eq!(seen.owner.count(&id(0x70)), 1);
    rig.sup.kill(&run).unwrap();
}

/// U9.2 point 3, turned by U9.9: a refusal of `Ends` and the request after it inside the wait each cost one `store`
/// and the second asks nobody; after the wait an end that is dead when it arrives costs nothing: the link finds it
/// ended before the send, asks `Ends` for a new end and sends the request once, on that end, as it sends the next
/// request. The two requests in the wait reach no owner.
#[test]
fn a_refused_end_costs_its_wait_and_a_dead_end_costs_nothing() {
    const WAIT: Duration = Duration::from_millis(700);
    let (ends, seen) = Planned::new(vec![Err(Refusal::Busy), Ok(Far::Dead), Ok(Far::Serve)]);
    let rig = Rig::linked(
        "link-dead",
        Arc::new(StreamLink::with_ask_again(ends, WAIT)),
    );
    let script = rig.script(&[
        "hello",
        &ask_store(&id(0x72), &effect(&id(0x76), 1, "held")),
        &ask_store(&id(0x73), &effect(&id(0x77), 1, "held")),
        "sleep 1000",
        &ask_store(&id(0x74), &effect(&id(0x78), 1, "held")),
        &ask_store(&id(0x75), &effect(&id(0x79), 1, "held")),
        "log asked",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("asked", SHORT), "{}", script.log());
    assert_eq!(
        script.replies(),
        vec![
            answered(0x72, "store"),
            answered(0x73, "store"),
            stored(0x74),
            stored(0x75),
        ]
    );
    assert_eq!(seen.asked.load(Ordering::SeqCst), 3, "calls of Ends");
    assert_eq!(
        seen.read.load(Ordering::SeqCst),
        2,
        "requests that a far end read"
    );
    for n in [0x72, 0x73] {
        assert_eq!(seen.owner.count(&id(n)), 0, "{n:#x} reached the owner");
    }
    for n in [0x74, 0x75] {
        assert_eq!(seen.owner.count(&id(n)), 1, "{n:#x} at the owner");
    }
    rig.sup.kill(&run).unwrap();
}

/// U9.9: the far end answers one request and leaves while the link is idle, as an owner that exits after its idle
/// time. The next request asks `Ends` for a new end, is sent once on it, and the runner gets the owner's reply; the
/// far end that left read no byte of that request.
#[test]
fn a_stream_that_ended_while_idle_costs_the_next_request_nothing() {
    let (ends, seen) = Planned::new(vec![Ok(Far::Leaves), Ok(Far::Serve)]);
    let rig = Rig::linked("link-left", Arc::new(StreamLink::new(ends)));
    let left = rig.dir.join("left");
    seen.left.set(left.clone()).unwrap();
    let script = rig.script(&[
        "hello",
        &ask_store(&id(0x7a), &effect(&id(0x7c), 1, "held")),
        &format!("wait-for {}", left.display()),
        &ask_store(&id(0x7b), &effect(&id(0x7d), 1, "held")),
        "log asked",
        "idle",
    ]);
    let spec = rig.spec(&script);
    let run = spec.run.clone();
    rig.sup.start(spec).unwrap();
    assert!(script.wait_log("asked", SHORT), "{}", script.log());
    assert!(
        script.log().contains("waited found=yes"),
        "{}",
        script.log()
    );
    assert_eq!(script.replies(), vec![stored(0x7a), stored(0x7b)]);
    assert_eq!(seen.asked.load(Ordering::SeqCst), 2, "calls of Ends");
    assert_eq!(
        seen.read.load(Ordering::SeqCst),
        2,
        "requests that a far end read"
    );
    for n in [0x7a, 0x7b] {
        assert_eq!(seen.owner.count(&id(n)), 1, "{n:#x} at the owner");
    }
    assert!(wait_until(SHORT, || seen.late.lock().unwrap().is_some()));
    assert_eq!(
        *seen.late.lock().unwrap(),
        Some(0),
        "bytes that the far end read after it left"
    );
    rig.sup.kill(&run).unwrap();
}
