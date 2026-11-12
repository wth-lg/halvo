use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::time::Duration;

use halvo_schema::executable::frame::FRAME_JSON_MAX;
use halvo_schema::executable::{FromRunner, ToRunner};

/// The time from the close of the link within which the runner and every process of its groups are gone (A6.1).
pub const LINK_END: Duration = Duration::from_secs(20);

/// How long after the close the reader waits for the run to end before it ends the process itself.
pub(crate) const LINK_GRACE: Duration = Duration::from_secs(15);

/// Why the run stops talking to its supervisor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Broken {
    /// The link closed: an end of file or an error on standard input, or an error on writing standard output.
    Closed,
    /// A frame that breaks the form of A6.2, named by its rule.
    Frame(&'static str),
}

enum Event {
    Frame(Box<ToRunner>),
    Closed,
    Bad(&'static str),
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

/// The runner's end of the link. A thread of its own reads standard input apart from the run's steps, so that
/// a close is seen while a step waits.
pub(crate) struct Link {
    out: Box<dyn Write>,
    events: Receiver<Event>,
    closed: Arc<AtomicBool>,
    cancelled: bool,
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
        let (tx, events) = mpsc::channel();
        let closed = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&closed);
        std::thread::spawn(move || {
            let mut input = input;
            loop {
                let event = match read_frame(&mut *input) {
                    Ok(bytes) => match ToRunner::parse(&bytes) {
                        Ok(frame) => Event::Frame(Box::new(frame)),
                        Err(refusal) => Event::Bad(refusal.rule()),
                    },
                    Err(Some(rule)) => Event::Bad(rule),
                    Err(None) => Event::Closed,
                };
                let last = !matches!(event, Event::Frame(_));
                if last {
                    seen.store(true, Ordering::SeqCst);
                }
                if tx.send(event).is_err() || last {
                    break;
                }
            }
            if end_after_grace {
                std::thread::sleep(LINK_GRACE);
                std::process::exit(1);
            }
        });
        Link {
            out,
            events,
            closed,
            cancelled: false,
            broken: None,
        }
    }

    fn take(&mut self, event: Event) -> Result<Option<ToRunner>, Broken> {
        match event {
            Event::Frame(frame) => match *frame {
                ToRunner::Cancel(_) => {
                    self.cancelled = true;
                    Ok(None)
                }
                frame => Ok(Some(frame)),
            },
            Event::Closed => Err(self.fail(Broken::Closed)),
            Event::Bad(rule) => Err(self.fail(Broken::Frame(rule))),
        }
    }

    fn fail(&mut self, broken: Broken) -> Broken {
        self.closed.store(true, Ordering::SeqCst);
        *self.broken.get_or_insert(broken)
    }

    /// Writes one frame.
    pub(crate) fn send(&mut self, frame: &FromRunner) -> Result<(), Broken> {
        if let Some(broken) = self.broken {
            return Err(broken);
        }
        let json = frame
            .to_json()
            .map_err(|_| self.fail(Broken::Frame("own_frame")))?;
        self.out
            .write_all(&encode(&json))
            .and_then(|()| self.out.flush())
            .map_err(|_| self.fail(Broken::Closed))
    }

    /// Writes one request frame and waits for the next frame that is not `cancel`.
    pub(crate) fn ask(&mut self, frame: &FromRunner) -> Result<ToRunner, Broken> {
        self.send(frame)?;
        loop {
            let event = self.events.recv().unwrap_or(Event::Closed);
            if let Some(reply) = self.take(event)? {
                return Ok(reply);
            }
        }
    }

    /// Takes the frames that came while no request waited: a `cancel` is noted, a close or a bad frame breaks the
    /// link, and any other frame is a frame out of turn.
    pub(crate) fn poll(&mut self) -> Result<(), Broken> {
        if let Some(broken) = self.broken {
            return Err(broken);
        }
        loop {
            match self.events.try_recv() {
                Ok(event) => {
                    if self.take(event)?.is_some() {
                        return Err(self.fail(Broken::Frame("out_of_turn")));
                    }
                }
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Err(self.fail(Broken::Closed)),
            }
        }
    }

    /// Whether a `cancel` came.
    pub(crate) fn cancelled(&self) -> bool {
        self.cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
