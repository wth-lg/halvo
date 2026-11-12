use std::io::{Read, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use halvo_schema::executable::frame::FRAME_JSON_MAX;
use halvo_schema::executable::{FromRunner, ToRunner};

/// The time from the close of the link within which the runner and every process of its groups are gone (A6.1).
pub const LINK_END: Duration = Duration::from_secs(20);

/// How long after the close the reader waits for the run to end before it ends the process itself.
pub(crate) const LINK_GRACE: Duration = Duration::from_secs(15);

const _: () = assert!(LINK_GRACE.as_secs() < LINK_END.as_secs());

/// Why the run stops talking to its supervisor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Broken {
    /// The link closed: an end of file or an error on standard input, or an error on writing standard output.
    Closed,
    /// A frame that breaks the form of A6.2, named by its rule.
    Frame(&'static str),
}

/// What the reader has handed over and the run has not taken: at most one frame, and the notes of a cancel and of
/// the link's end (a close, or a frame refused by its rule, after which the reader reads no further).
#[derive(Default)]
struct Held {
    frame: Option<ToRunner>,
    cancelled: bool,
    end: Option<Broken>,
    reading: bool,
}

struct Shared {
    held: Mutex<Held>,
    changed: Condvar,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Held> {
        self.held
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn wait<'a>(&self, held: MutexGuard<'a, Held>) -> MutexGuard<'a, Held> {
        self.changed
            .wait(held)
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Reads one frame: a 4-byte big-endian length and that many bytes of JSON, at most 65,536.
fn read_frame(input: &mut dyn Read) -> Result<Vec<u8>, Option<&'static str>> {
    let mut head = [0u8; 4];
    input.read_exact(&mut head).map_err(|_| None)?;
    let len = u32::from_be_bytes(head) as usize;
    if len > FRAME_JSON_MAX {
        return Err(Some("frame_length"));
    }
    let mut body = vec![0u8; len];
    input.read_exact(&mut body).map_err(|_| None)?;
    Ok(body)
}

/// The bytes of one frame on the pipe.
pub(crate) fn encode(json: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(json.len() + 4);
    bytes.extend_from_slice(&(json.len() as u32).to_be_bytes());
    bytes.extend_from_slice(json.as_bytes());
    bytes
}

/// What one read of the input gives the reader.
enum Read1 {
    Cancel,
    Frame(Box<ToRunner>),
    End(Broken),
}

fn read_one(input: &mut dyn Read) -> Read1 {
    match read_frame(input) {
        Ok(bytes) => match ToRunner::parse(&bytes) {
            Ok(ToRunner::Cancel(_)) => Read1::Cancel,
            Ok(frame) => Read1::Frame(Box::new(frame)),
            Err(refusal) => Read1::End(Broken::Frame(refusal.rule())),
        },
        Err(Some(rule)) => Read1::End(Broken::Frame(rule)),
        Err(None) => Read1::End(Broken::Closed),
    }
}

/// Ends the process with the status 1 `LINK_GRACE` from now, on a thread of its own, so that no lock and no write
/// of the run delays it.
fn end_after(grace: bool) {
    if grace {
        std::thread::spawn(|| {
            std::thread::sleep(LINK_GRACE);
            std::process::exit(1);
        });
    }
}

/// The reader: it reads standard input apart from the run's steps and never waits for the run. It holds at most one
/// frame that the run has not taken; any number of `cancel` frames leave one note; a second frame that is no
/// `cancel` while one waits is a frame out of turn, after which, as after a close or a refused frame, it reads no
/// further and the process ends `LINK_GRACE` later.
fn read_all(shared: &Shared, input: &mut dyn Read, grace: bool) {
    loop {
        {
            let mut held = shared.lock();
            held.reading = true;
            shared.changed.notify_all();
        }
        let read = read_one(input);
        if matches!(read, Read1::End(_)) {
            end_after(grace);
        }
        let mut held = shared.lock();
        held.reading = false;
        let last = match read {
            Read1::Cancel => {
                held.cancelled = true;
                false
            }
            Read1::Frame(frame) if held.frame.is_none() => {
                held.frame = Some(*frame);
                false
            }
            Read1::Frame(_) => {
                held.end.get_or_insert(Broken::Frame("out_of_turn"));
                true
            }
            Read1::End(broken) => {
                held.end.get_or_insert(broken);
                true
            }
        };
        let out_of_turn = last && held.end == Some(Broken::Frame("out_of_turn"));
        shared.changed.notify_all();
        drop(held);
        if out_of_turn {
            end_after(grace);
        }
        if last {
            return;
        }
    }
}

/// The runner's end of the link. A thread of its own reads standard input apart from the run's steps, so that a
/// close is seen while a step waits.
///
/// Before an effect row and before the request of an effect leaves, the run waits until the reader has handed
/// over everything that it read and stands in its next read, and it writes the row, or lets the request leave,
/// under the one lock under which the reader notes an end of file, so that no row follows that note. A propose or
/// begin row can still leave after an end of file that stands at the input but that the reader's read has not yet
/// returned; it is never followed by its effect, since no answer to its row can come through a closed input. The
/// supervisor drops what it reads from a runner after its close.
pub(crate) struct Link {
    out: Box<dyn Write>,
    shared: Arc<Shared>,
    broken: Option<Broken>,
}

impl Link {
    /// Starts the reader of `input`. With `end_after_grace`, the reader ends the process with the status 1 when
    /// the run has not ended `LINK_GRACE` after the close.
    pub(crate) fn start(
        input: Box<dyn Read + Send>,
        out: Box<dyn Write>,
        end_after_grace: bool,
    ) -> Link {
        let shared = Arc::new(Shared {
            held: Mutex::new(Held::default()),
            changed: Condvar::new(),
        });
        let reader = Arc::clone(&shared);
        std::thread::spawn(move || {
            let mut input = input;
            read_all(&reader, &mut *input, end_after_grace);
        });
        Link {
            out,
            shared,
            broken: None,
        }
    }

    fn fail(&mut self, broken: Broken) -> Broken {
        *self.broken.get_or_insert(broken)
    }

    /// What came while no request waited: the end of the link, or a frame out of turn.
    fn look(held: &mut Held) -> Result<(), Broken> {
        if let Some(end) = held.end {
            return Err(end);
        }
        if held.frame.take().is_some() {
            return Err(Broken::Frame("out_of_turn"));
        }
        Ok(())
    }

    /// Takes what came while no request waited: a close or a refused frame breaks the link, and a frame is a frame
    /// out of turn.
    pub(crate) fn poll(&mut self) -> Result<(), Broken> {
        if let Some(broken) = self.broken {
            return Err(broken);
        }
        let shared = Arc::clone(&self.shared);
        let looked = Self::look(&mut shared.lock());
        looked.map_err(|broken| self.fail(broken))
    }

    fn write(&mut self, frame: &FromRunner) -> Result<(), Broken> {
        let json = frame
            .to_json()
            .map_err(|_| self.fail(Broken::Frame("own_frame")))?;
        self.out
            .write_all(&encode(&json))
            .and_then(|()| self.out.flush())
            .map_err(|_| self.fail(Broken::Closed))
    }

    /// Writes one frame, unless the link has closed or broken, which it reads first from what came on it.
    pub(crate) fn send(&mut self, frame: &FromRunner) -> Result<(), Broken> {
        self.poll()?;
        self.write(frame)
    }

    /// Waits for the reply to the request that was written: the one frame that came, or the end of the link.
    fn reply(&mut self) -> Result<ToRunner, Broken> {
        let shared = Arc::clone(&self.shared);
        let mut held = shared.lock();
        loop {
            if let Some(frame) = held.frame.take() {
                return Ok(frame);
            }
            if let Some(end) = held.end {
                drop(held);
                return Err(self.fail(end));
            }
            held = shared.wait(held);
        }
    }

    /// Writes one request frame and waits for the next frame that is not `cancel`.
    pub(crate) fn ask(&mut self, frame: &FromRunner) -> Result<ToRunner, Broken> {
        self.send(frame)?;
        self.reply()
    }

    /// Waits until the reader has handed over everything that it read and stands in its next read, and gives the
    /// lock under which the reader notes the link's end; or the end, or a frame out of turn.
    fn settled(shared: &Shared) -> Result<MutexGuard<'_, Held>, Broken> {
        let mut held = shared.lock();
        loop {
            Self::look(&mut held)?;
            if held.reading {
                return Ok(held);
            }
            held = shared.wait(held);
        }
    }

    /// Does `act` once the reader stands in its next read with nothing more to hand over and no end noted, under the
    /// lock under which it would note one: the request of an effect leaves in it.
    pub(crate) fn settle<T>(&mut self, act: impl FnOnce() -> T) -> Result<T, Broken> {
        if let Some(broken) = self.broken {
            return Err(broken);
        }
        let shared = Arc::clone(&self.shared);
        let held = Self::settled(&shared);
        match held {
            Ok(held) => {
                let done = act();
                drop(held);
                Ok(done)
            }
            Err(broken) => Err(self.fail(broken)),
        }
    }

    /// Writes the request frame of an effect row as `settle` lets an effect act, and waits for its reply.
    pub(crate) fn ask_effect(&mut self, frame: &FromRunner) -> Result<ToRunner, Broken> {
        if let Some(broken) = self.broken {
            return Err(broken);
        }
        let shared = Arc::clone(&self.shared);
        let held = Self::settled(&shared);
        match held {
            Ok(held) => {
                let written = self.write(frame);
                drop(held);
                written?;
            }
            Err(broken) => return Err(self.fail(broken)),
        }
        self.reply()
    }

    /// Whether a `cancel` came.
    pub(crate) fn cancelled(&self) -> bool {
        self.shared.lock().cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use halvo_schema::executable::frame::{Cancel, Done, SecretReply};
    use halvo_schema::executable::Outcome;

    struct Out(Arc<Mutex<Vec<u8>>>);

    impl Write for Out {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn framed(frame: &ToRunner) -> Vec<u8> {
        encode(&frame.to_json().expect("a frame"))
    }

    fn done() -> FromRunner {
        FromRunner::Done(Done {
            outcome: Outcome::Completed,
        })
    }

    fn link_of(input: Vec<u8>) -> (Link, Arc<Mutex<Vec<u8>>>) {
        let out = Arc::new(Mutex::new(Vec::new()));
        let link = Link::start(
            Box::new(std::io::Cursor::new(input)),
            Box::new(Out(Arc::clone(&out))),
            false,
        );
        (link, out)
    }

    /// Waits until the reader has noted the link's end.
    fn ended(link: &Link) -> Broken {
        let mut held = link.shared.lock();
        loop {
            if let Some(end) = held.end {
                return end;
            }
            held = link.shared.wait(held);
        }
    }

    #[test]
    fn a_close_that_the_reader_has_taken_stops_the_next_frame() {
        let (mut link, out) = link_of(Vec::new());
        assert_eq!(ended(&link), Broken::Closed);
        assert_eq!(link.send(&done()), Err(Broken::Closed));
        assert!(out.lock().expect("lock").is_empty(), "nothing written");
    }

    #[test]
    fn ten_thousand_cancels_and_a_close_leave_one_note_no_frame_and_the_close() {
        let cancel = framed(&ToRunner::Cancel(Cancel {}));
        let input: Vec<u8> = std::iter::repeat_n(cancel, 10_000).flatten().collect();
        let (mut link, _out) = link_of(input);
        assert_eq!(link.ask(&done()), Err(Broken::Closed), "the close is seen");
        assert!(link.cancelled(), "the cancel is noted");
        assert!(
            link.shared.lock().frame.is_none(),
            "the link holds no frame"
        );
    }

    /// A link whose input is an OS pipe that the test writes, and its output.
    fn piped() -> (Link, std::io::PipeWriter, Arc<Mutex<Vec<u8>>>) {
        let (input, writer) = std::io::pipe().expect("a pipe");
        let out = Arc::new(Mutex::new(Vec::new()));
        let link = Link::start(Box::new(input), Box::new(Out(Arc::clone(&out))), false);
        (link, writer, out)
    }

    #[test]
    fn a_second_reply_before_the_first_is_taken_is_a_frame_out_of_turn() {
        let (mut link, mut writer, _out) = piped();
        link.send(&done()).expect("the request");
        let first = ToRunner::SecretReply(SecretReply::None);
        let mut replies = framed(&first);
        replies.extend(framed(&ToRunner::SecretReply(SecretReply::None)));
        replies.extend(framed(&ToRunner::Cancel(Cancel {})));
        writer.write_all(&replies).expect("write");
        assert_eq!(ended(&link), Broken::Frame("out_of_turn"));
        assert_eq!(link.reply(), Ok(first), "the first reply is given");
        assert_eq!(link.poll(), Err(Broken::Frame("out_of_turn")));
        assert!(!link.cancelled(), "the reader read no further");
    }

    #[test]
    fn after_a_reply_and_an_end_of_file_together_no_effect_acts_in_500_links() {
        let reply = framed(&ToRunner::SecretReply(SecretReply::None));
        let mut late_rows = 0;
        for n in 0..500 {
            let (mut link, mut writer, out) = piped();
            link.send(&done()).expect("the request");
            writer.write_all(&reply).expect("write");
            drop(writer);
            assert!(link.reply().is_ok(), "link {n}: the reply");
            let before = out.lock().expect("lock").len();
            assert_eq!(
                link.ask_effect(&done()),
                Err(Broken::Closed),
                "link {n}: the row that proposes or begins is not answered"
            );
            if out.lock().expect("lock").len() > before {
                late_rows += 1;
            }
            let mut acted = false;
            assert_eq!(
                link.settle(|| acted = true),
                Err(Broken::Closed),
                "link {n}: no effect is begun"
            );
            assert!(!acted, "link {n}: no replace is made");
        }
        eprintln!("links=500 propose_or_begin_rows_after_the_end_of_file={late_rows}");
    }

    #[test]
    fn a_frame_is_its_length_in_four_bytes_big_endian_then_its_json() {
        assert_eq!(encode("{}"), [0, 0, 0, 2, b'{', b'}']);
    }

    #[test]
    fn the_reader_takes_65536_bytes_and_refuses_one_more() {
        let mut at_max = (FRAME_JSON_MAX as u32).to_be_bytes().to_vec();
        at_max.extend(std::iter::repeat_n(b' ', FRAME_JSON_MAX));
        assert_eq!(
            read_frame(&mut at_max.as_slice()).map(|b| b.len()),
            Ok(FRAME_JSON_MAX)
        );
        let over = (FRAME_JSON_MAX as u32 + 1).to_be_bytes();
        assert_eq!(read_frame(&mut over.as_slice()), Err(Some("frame_length")));
    }

    #[test]
    fn an_end_of_file_inside_a_frame_is_a_close() {
        let cut = [0u8, 0, 0, 9, b'{'];
        assert_eq!(read_frame(&mut cut.as_slice()), Err(None));
        assert_eq!(read_frame(&mut [0u8, 0].as_slice()), Err(None));
    }
}
