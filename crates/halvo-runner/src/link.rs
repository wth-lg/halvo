use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
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
}

struct Shared {
    held: Mutex<Held>,
    changed: Condvar,
    /// The mark that the reader stands in its read. The reader sets it under the lock as it enters a read, with a
    /// wake-up, and clears it without the lock as the first thing after the read returns, so that the run's thread
    /// never delays it.
    reading: AtomicBool,
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

/// What the reader calls to end the process later: once after an end of file, and once after a frame out of
/// turn. The process gives `start_grace`; a test gives its own.
pub(crate) type EndLater = Box<dyn FnMut() + Send>;

/// Ends the process with the status 1 `LINK_GRACE` from now, on a thread of its own, so that no lock and no write
/// of the run delays it.
pub(crate) fn start_grace() {
    std::thread::spawn(|| {
        std::thread::sleep(LINK_GRACE);
        std::process::exit(1);
    });
}

/// The reader: it reads standard input apart from the run's steps and never waits for the run. It holds at most one
/// frame that the run has not taken; any number of `cancel` frames leave one note; a second frame that is no
/// `cancel` while one waits is a frame out of turn, after which, as after a close or a refused frame, it reads no
/// further and the process ends `LINK_GRACE` later. The first thing that it does when a read returns is to clear
/// the mark that it stands in its read, before it calls `end_later` and before it takes the lock to note what it
/// read.
fn read_all(shared: &Shared, input: &mut dyn Read, end_later: &mut dyn FnMut()) {
    loop {
        let held = shared.lock();
        shared.reading.store(true, Ordering::SeqCst);
        shared.changed.notify_all();
        drop(held);
        let read = read_one(input);
        shared.reading.store(false, Ordering::SeqCst);
        if matches!(read, Read1::End(_)) {
            end_later();
        }
        let mut held = shared.lock();
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
            end_later();
        }
        if last {
            return;
        }
    }
}

/// The runner's end of the link. A thread of its own reads standard input apart from the run's steps, so that a
/// close is seen while a step waits.
///
/// Before an effect row, and before the replace that follows an answered begin row, the run waits until the
/// reader has handed over everything that it read and stands in its next read, and it writes the row, or lets the
/// effect act, under the one lock under which the reader notes what it read, and only while the reader's mark says
/// that it stands in its read. A propose or begin row, or the replace after an answered begin row, can go while
/// the system has not yet given the end of file to the reader's read; the reader marks the end as the first thing
/// after that read returns, and nothing of an effect follows the mark. The supervisor drops what it reads from a
/// runner after its close.
pub(crate) struct Link {
    out: Box<dyn Write + Send>,
    shared: Arc<Shared>,
    broken: Option<Broken>,
}

impl Link {
    /// Starts the reader of `input`, which calls `end_later` after the link's end; the process gives
    /// `start_grace`, which ends it with the status 1 when the run has not ended `LINK_GRACE` after the close.
    pub(crate) fn start(
        input: Box<dyn Read + Send>,
        out: Box<dyn Write + Send>,
        end_later: EndLater,
    ) -> Link {
        let shared = Arc::new(Shared {
            held: Mutex::new(Held::default()),
            changed: Condvar::new(),
            reading: AtomicBool::new(false),
        });
        let reader = Arc::clone(&shared);
        std::thread::spawn(move || {
            let mut input = input;
            let mut end_later = end_later;
            read_all(&reader, &mut *input, &mut *end_later);
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

    /// Waits until the reader has handed over everything that it read and its mark says that it stands in its next
    /// read, and gives the lock under which the reader notes the link's end; or the end, or a frame out of turn.
    fn settled(shared: &Shared) -> Result<MutexGuard<'_, Held>, Broken> {
        let mut held = shared.lock();
        loop {
            Self::look(&mut held)?;
            if shared.reading.load(Ordering::SeqCst) {
                return Ok(held);
            }
            held = shared.wait(held);
        }
    }

    /// Does `act` once the reader stands in its next read with nothing more to hand over and no end noted, under the
    /// lock under which it would note one: the request of an effect leaves in it. A `cancel` that the link holds
    /// then lets nothing act, and the answer is none (A6.2, CP-135 (4), reading 1).
    pub(crate) fn settle<T>(&mut self, act: impl FnOnce() -> T) -> Result<Option<T>, Broken> {
        if let Some(broken) = self.broken {
            return Err(broken);
        }
        let shared = Arc::clone(&self.shared);
        let held = Self::settled(&shared);
        match held {
            Ok(held) if held.cancelled => Ok(None),
            Ok(held) => {
                let done = act();
                drop(held);
                Ok(Some(done))
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
    use std::sync::atomic::AtomicUsize;
    use std::sync::{mpsc, OnceLock};
    use std::time::Instant;

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

    /// A later end that does nothing.
    fn none() -> EndLater {
        Box::new(|| {})
    }

    /// A later end that tells the test when the reader has got an end of file and holds the reader there, between
    /// the return of its read and its note, until the test lets it go.
    fn holding() -> (EndLater, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (got, heard) = mpsc::channel();
        let (go, gone) = mpsc::channel::<()>();
        let end: EndLater = Box::new(move || {
            let _ = got.send(());
            let _ = gone.recv();
        });
        (end, heard, go)
    }

    /// Whether `seen` holds within `bound`, asked every 5 ms.
    fn within(bound: Duration, seen: impl Fn() -> bool) -> bool {
        let until = Instant::now() + bound;
        while Instant::now() < until {
            if seen() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        seen()
    }

    fn link_of(input: Vec<u8>) -> (Link, Arc<Mutex<Vec<u8>>>) {
        let out = Arc::new(Mutex::new(Vec::new()));
        let link = Link::start(
            Box::new(std::io::Cursor::new(input)),
            Box::new(Out(Arc::clone(&out))),
            none(),
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
        let link = Link::start(Box::new(input), Box::new(Out(Arc::clone(&out))), none());
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
    fn a_frame_that_came_while_no_request_waited_is_out_of_turn_at_the_next_send() {
        let (mut link, mut writer, out) = piped();
        let frame = framed(&ToRunner::SecretReply(SecretReply::None));
        writer.write_all(&frame).expect("write");
        {
            let mut held = link.shared.lock();
            while held.frame.is_none() {
                held = link.shared.wait(held);
            }
        }
        assert_eq!(link.send(&done()), Err(Broken::Frame("out_of_turn")));
        assert!(out.lock().expect("lock").is_empty(), "nothing written");
        drop(writer);
    }

    #[test]
    fn a_close_that_the_reader_has_taken_stops_an_effect_row_and_an_effect() {
        let (mut link, out) = link_of(Vec::new());
        assert_eq!(ended(&link), Broken::Closed);
        assert_eq!(link.ask_effect(&done()), Err(Broken::Closed));
        assert!(out.lock().expect("lock").is_empty(), "no row written");
        let (mut link, _out) = link_of(Vec::new());
        assert_eq!(ended(&link), Broken::Closed);
        let mut acted = false;
        assert_eq!(link.settle(|| acted = true), Err(Broken::Closed));
        assert!(!acted, "no effect");
    }

    /// An input whose one read tells the test that the reader stands in it, and ends the file when the test lets
    /// it.
    struct Gate {
        inside: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
    }

    impl Read for Gate {
        fn read(&mut self, _bytes: &mut [u8]) -> std::io::Result<usize> {
            let _ = self.inside.send(());
            let _ = self.release.recv();
            Ok(0)
        }
    }

    /// A gated input, the receiver that hears the reader stand in its read, and the sender that ends the file.
    fn gated() -> (Gate, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (inside, heard) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let gate = Gate {
            inside,
            release: released,
        };
        (gate, heard, release)
    }

    /// Ends the gated file and tells whether the reader notes the end within half a second; a lock that the caller
    /// holds keeps it from noting.
    fn end_and_look(release: &mpsc::Sender<()>, shared: &Shared) -> bool {
        release.send(()).expect("the reader waits in its read");
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until {
            if let Ok(held) = shared.held.try_lock() {
                if held.end.is_some() {
                    return true;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// A writer that, as the row is written, ends the gated file and records whether the reader noted the end.
    struct Noting {
        release: mpsc::Sender<()>,
        shared: Arc<OnceLock<Arc<Shared>>>,
        noted: Arc<Mutex<Option<bool>>>,
    }

    impl Write for Noting {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let shared = self.shared.get().expect("the link's state");
            let noted = end_and_look(&self.release, shared);
            *self.noted.lock().expect("lock") = Some(noted);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn an_end_of_file_taken_while_an_effect_row_is_written_is_noted_after_the_row() {
        let (gate, heard, release) = gated();
        let cell = Arc::new(OnceLock::new());
        let noted = Arc::new(Mutex::new(None));
        let out = Noting {
            release,
            shared: Arc::clone(&cell),
            noted: Arc::clone(&noted),
        };
        let mut link = Link::start(Box::new(gate), Box::new(out), none());
        assert!(cell.set(Arc::clone(&link.shared)).is_ok(), "set once");
        heard.recv().expect("the reader stands in its read");
        assert_eq!(link.ask_effect(&done()), Err(Broken::Closed));
        assert_eq!(
            *noted.lock().expect("lock"),
            Some(false),
            "the row is written, and the end of file is noted after it"
        );
    }

    #[test]
    fn an_end_of_file_taken_while_an_effect_acts_is_noted_after_the_effect() {
        let (gate, heard, release) = gated();
        let out = Arc::new(Mutex::new(Vec::new()));
        let mut link = Link::start(Box::new(gate), Box::new(Out(out)), none());
        heard.recv().expect("the reader stands in its read");
        let shared = Arc::clone(&link.shared);
        assert_eq!(
            link.settle(|| end_and_look(&release, &shared)),
            Ok(Some(false)),
            "the effect acts, and the end of file is noted after it"
        );
        assert_eq!(ended(&link), Broken::Closed);
    }

    #[test]
    fn while_the_reader_holds_an_end_of_file_no_effect_row_leaves_and_no_effect_acts() {
        let reply = framed(&ToRunner::SecretReply(SecretReply::None));
        let (end, got, go) = holding();
        let out = Arc::new(Mutex::new(Vec::new()));
        let input = std::io::Cursor::new(reply.clone());
        let mut link = Link::start(Box::new(input), Box::new(Out(Arc::clone(&out))), end);
        assert!(link.reply().is_ok(), "the reply");
        got.recv().expect("the reader got the end of file");
        std::thread::scope(|s| {
            let asked = s.spawn(|| link.ask_effect(&done()));
            let written = within(Duration::from_millis(500), || {
                !out.lock().expect("lock").is_empty()
            });
            go.send(()).expect("the reader goes on");
            assert_eq!(asked.join().expect("the row's thread"), Err(Broken::Closed));
            assert!(!written, "no row while the reader holds the end of file");
        });
        assert!(out.lock().expect("lock").is_empty(), "no row");

        let (end, got, go) = holding();
        let input = std::io::Cursor::new(reply);
        let quiet = Box::new(Out(Arc::new(Mutex::new(Vec::new()))));
        let mut link = Link::start(Box::new(input), quiet, end);
        assert!(link.reply().is_ok(), "the reply");
        got.recv().expect("the reader got the end of file");
        let acted = AtomicBool::new(false);
        std::thread::scope(|s| {
            let settled = s.spawn(|| link.settle(|| acted.store(true, Ordering::SeqCst)));
            let early = within(Duration::from_millis(500), || acted.load(Ordering::SeqCst));
            go.send(()).expect("the reader goes on");
            assert_eq!(
                settled.join().expect("the effect's thread"),
                Err(Broken::Closed)
            );
            assert!(!early, "no effect while the reader holds the end of file");
        });
        assert!(!acted.load(Ordering::SeqCst), "no effect");
    }

    /// After a reply and an end of file written together, the row that proposes or begins an effect is not
    /// answered in any of 500 links; in 500 links of its own the first call after them is `settle`. It prints the
    /// count of rows that left and of acts that ran, with no bound on either: both can still go while the system
    /// has not given the end of file to the reader's read. The test that holds `settle` is
    /// `while_the_reader_holds_an_end_of_file_no_effect_row_leaves_and_no_effect_acts`.
    #[test]
    fn after_a_reply_and_an_end_of_file_no_row_is_answered_and_rows_and_acts_are_counted() {
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
        }
        let mut acts = 0;
        for n in 0..500 {
            let (mut link, mut writer, _out) = piped();
            link.send(&done()).expect("the request");
            writer.write_all(&reply).expect("write");
            drop(writer);
            assert!(link.reply().is_ok(), "link {n}: the reply");
            match link.settle(|| ()) {
                Ok(_) => acts += 1,
                Err(broken) => assert_eq!(broken, Broken::Closed, "link {n}: the close"),
            }
        }
        eprintln!(
            "links=500 propose_or_begin_rows_after_the_end_of_file={late_rows} acts_after_the_end_of_file={acts}"
        );
    }

    /// A later end that counts its calls.
    fn counting(calls: &Arc<AtomicUsize>) -> EndLater {
        let calls = Arc::clone(calls);
        Box::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
        })
    }

    fn quiet() -> Box<Out> {
        Box::new(Out(Arc::new(Mutex::new(Vec::new()))))
    }

    #[test]
    fn an_end_of_file_calls_the_later_end_once() {
        let calls = Arc::new(AtomicUsize::new(0));
        let input = std::io::Cursor::new(Vec::new());
        let link = Link::start(Box::new(input), quiet(), counting(&calls));
        assert_eq!(ended(&link), Broken::Closed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_frame_out_of_turn_calls_the_later_end_once() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (input, mut writer) = std::io::pipe().expect("a pipe");
        let link = Link::start(Box::new(input), quiet(), counting(&calls));
        let mut replies = framed(&ToRunner::SecretReply(SecretReply::None));
        replies.extend(framed(&ToRunner::SecretReply(SecretReply::None)));
        writer.write_all(&replies).expect("write");
        assert_eq!(ended(&link), Broken::Frame("out_of_turn"));
        assert!(
            within(Duration::from_secs(5), || calls.load(Ordering::SeqCst) > 0),
            "the later end is called"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1, "once");
        drop(writer);
    }

    #[test]
    fn an_end_of_file_calls_the_later_end_while_the_run_holds_the_lock() {
        let (gate, heard, release) = gated();
        let (called, call) = mpsc::channel();
        let end: EndLater = Box::new(move || {
            let _ = called.send(());
        });
        let link = Link::start(Box::new(gate), quiet(), end);
        heard.recv().expect("the reader stands in its read");
        let held = link.shared.lock();
        release.send(()).expect("the end of file");
        let early = call.recv_timeout(Duration::from_secs(5));
        drop(held);
        assert_eq!(
            early,
            Ok(()),
            "the later end is called while the lock is held"
        );
        assert_eq!(ended(&link), Broken::Closed);
    }

    #[test]
    fn a_run_that_waits_before_the_reader_stands_in_its_read_is_woken_when_it_does() {
        let shared = Arc::new(Shared {
            held: Mutex::new(Held::default()),
            changed: Condvar::new(),
            reading: AtomicBool::new(false),
        });
        let (settled, waited) = mpsc::channel();
        let waiter = Arc::clone(&shared);
        std::thread::spawn(move || {
            let got = Link::settled(&waiter).is_ok();
            let _ = settled.send(got);
        });
        assert!(
            waited.recv_timeout(Duration::from_millis(200)).is_err(),
            "the run's thread waits while no reader stands in its read"
        );
        let (gate, heard, release) = gated();
        let reader = Arc::clone(&shared);
        std::thread::spawn(move || {
            let mut gate = gate;
            read_all(&reader, &mut gate, &mut || {});
        });
        heard.recv().expect("the reader stands in its read");
        assert_eq!(
            waited.recv_timeout(Duration::from_secs(5)),
            Ok(true),
            "the run's thread gets the settled link once the reader stands in its read"
        );
        drop(release);
    }

    /// CP-135 (4), reading 1, A6.2: a `cancel` that the link holds when the reader stands in its next read lets
    /// nothing act under the lock: no request is let through and no `replace` is made.
    #[test]
    fn a_cancel_that_the_link_holds_lets_no_effect_act() {
        let (mut link, mut writer, _out) = piped();
        writer
            .write_all(&framed(&ToRunner::Cancel(Cancel {})))
            .expect("write");
        assert!(
            within(Duration::from_secs(5), || link.cancelled()),
            "the cancel is noted"
        );
        let acted = AtomicBool::new(false);
        let _ = link.settle(|| acted.store(true, Ordering::SeqCst));
        assert!(
            !acted.load(Ordering::SeqCst),
            "no effect acts after a cancel"
        );
        drop(writer);
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
