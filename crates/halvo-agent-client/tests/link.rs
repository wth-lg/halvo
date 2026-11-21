mod common;

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use common::{ask_store, effect, id, Rig, Store};
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
}

/// What the test's ends saw: the calls of `Ends`, the requests that a far end read, and the memory owner that
/// answers them.
#[derive(Default)]
struct Seen {
    asked: AtomicUsize,
    read: AtomicUsize,
    owner: Store,
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
type FarWrites = Box<dyn Write + Send>;

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

/// U9.2 point 3: a refusal of `Ends` and the request after it inside the wait each cost one `store` and the second
/// asks nobody; after the wait an end that is dead when it arrives costs one `store`, and the next request takes a
/// new end and gets the owner's reply. No request but the last reaches the owner.
#[test]
fn a_dead_end_costs_one_error_and_the_next_request_takes_a_new_end() {
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
            answered(0x74, "store"),
            stored(0x75),
        ]
    );
    assert_eq!(seen.asked.load(Ordering::SeqCst), 3, "calls of Ends");
    assert_eq!(
        seen.read.load(Ordering::SeqCst),
        1,
        "requests that a far end read"
    );
    for n in [0x72, 0x73, 0x74] {
        assert_eq!(seen.owner.count(&id(n)), 0, "{n:#x} reached the owner");
    }
    assert_eq!(seen.owner.count(&id(0x75)), 1);
    rig.sup.kill(&run).unwrap();
}
