//! The lifeline of contracts.md C14 item 2, its unix form: standard input is
//! the read end of a pipe whose write end the app alone holds, and the parent
//! that started the run is the one that the start reads.

use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::FileTypeExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use super::ParentWatch;

/// Why a lifeline did not start.
#[derive(Debug)]
pub enum Refused {
    /// Standard input is no pipe.
    NotAPipe,
    /// The parent was gone before the start.
    ParentGone,
    /// The watcher thread did not start.
    Watcher(std::io::Error),
}

/// The lifeline of a run started with `--lifeline`: a watcher thread reads
/// standard input until its end of file or an error, and every read of
/// [`ParentWatch::ended`] compares the parent's pid with the one of the
/// start, for a write end that leaked into another process.
pub struct Lifeline {
    closed: Arc<AtomicBool>,
    parent: u32,
}

impl Lifeline {
    /// Starts the watch: standard input must be a pipe, and a parent that is
    /// gone already (the process was handed to launchd, pid 1) refuses it.
    pub fn start() -> Result<Lifeline, Refused> {
        if !input_is_a_pipe() {
            return Err(Refused::NotAPipe);
        }
        let parent = std::os::unix::process::parent_id();
        if parent == 1 {
            return Err(Refused::ParentGone);
        }
        let closed = Arc::new(AtomicBool::new(false));
        let watched = closed.clone();
        std::thread::Builder::new()
            .name("halvo-lifeline".to_owned())
            .spawn(move || {
                let mut input = std::io::stdin().lock();
                let mut bytes = [0_u8; 64];
                loop {
                    match input.read(&mut bytes) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(error) if error.kind() == ErrorKind::Interrupted => {}
                        Err(_) => break,
                    }
                }
                watched.store(true, Ordering::SeqCst);
            })
            .map_err(Refused::Watcher)?;
        Ok(Lifeline { closed, parent })
    }

    /// The parent's pid as the start read it: the exclusion root of the run.
    pub fn parent(&self) -> u32 {
        self.parent
    }
}

impl ParentWatch for Lifeline {
    fn ended(&self) -> bool {
        self.closed.load(Ordering::SeqCst) || std::os::unix::process::parent_id() != self.parent
    }
}

fn input_is_a_pipe() -> bool {
    use std::os::fd::AsFd;
    std::io::stdin()
        .as_fd()
        .try_clone_to_owned()
        .map(std::fs::File::from)
        .and_then(|input| input.metadata())
        .is_ok_and(|meta| meta.file_type().is_fifo())
}

/// The lifeline of the mode `--service-run --lifeline`, or the status that
/// ends the mode before its store opens: 64 for a standard input that is no
/// pipe, 0 for a parent that is gone already and 1 for a watcher that did
/// not start, each but the second with its line on `err`.
pub fn start_or_status(err: &mut dyn Write) -> Result<Lifeline, i32> {
    match Lifeline::start() {
        Ok(lifeline) => Ok(lifeline),
        Err(Refused::NotAPipe) => {
            let _ = writeln!(
                err,
                "halvo-capture: --lifeline needs a pipe on standard input"
            );
            Err(halvo_control::exit::USAGE)
        }
        Err(Refused::ParentGone) => Err(halvo_control::exit::CLEAN),
        Err(Refused::Watcher(error)) => {
            let _ = writeln!(
                err,
                "halvo-capture: the lifeline's watcher did not start: {}",
                error.kind()
            );
            Err(halvo_control::exit::ERROR)
        }
    }
}
