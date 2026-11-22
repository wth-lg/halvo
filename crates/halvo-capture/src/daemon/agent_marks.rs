//! The listing of the runs' marks for the floor's set: the folder
//! `agents/marks` of the data directory, where the agent client writes a mark
//! for each run, listed once before the store's owner and the sources start
//! and then once a second by one thread beside the owner. A decision reads the
//! set that the last listing left and never lists.

use std::io;
use std::path::Path;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use crate::floor::exclude::{CreationTime, Listing, Marked, Marks, ProcessId};

/// How often the thread lists the marks again.
pub(crate) const LISTING_EVERY: Duration = Duration::from_secs(1);

/// The creation time of a process as [`super::owner_pid::start_time_of`]
/// reads it from the process table, the clock of a mark's line.
struct SystemCreationTime;

impl CreationTime for SystemCreationTime {
    fn created_of(&self, pid: ProcessId) -> Option<u64> {
        super::owner_pid::start_time_of(pid.get())
    }
}

/// The processes that the marks under the folder `marks` name, each by the
/// pid and the creation time of a line that [`halvo_control::marks::read`]
/// reads in the mark under its file name. A folder that is not there names
/// none. A folder that cannot be listed, an entry that cannot be read and a
/// mark whose bytes cannot be read make the listing unread, unless the error
/// says that it is gone. An entry that is no file and a file that does not
/// read as a mark name nobody.
pub(crate) fn list(marks: &Path) -> Listing {
    let mut listing = Listing::default();
    let entries = match std::fs::read_dir(marks) {
        Ok(entries) => entries,
        Err(error) => {
            listing.unread = error.kind() != io::ErrorKind::NotFound;
            return listing;
        }
    };
    for entry in entries {
        let read = entry.and_then(|entry| {
            if !entry.file_type()?.is_file() {
                return Ok(None);
            }
            let bytes = std::fs::read(entry.path())?;
            Ok(Some((entry.file_name(), bytes)))
        });
        let (name, bytes) = match read {
            Ok(Some(found)) => found,
            Ok(None) => continue,
            Err(error) => {
                listing.unread |= error.kind() != io::ErrorKind::NotFound;
                continue;
            }
        };
        let lines = name
            .to_str()
            .zip(std::str::from_utf8(&bytes).ok())
            .and_then(|(run, text)| halvo_control::marks::read(run, text));
        let Some(lines) = lines else {
            continue;
        };
        for line in [lines.runner, lines.browser].into_iter().flatten() {
            listing.named.push(Marked {
                pid: ProcessId::new(line.pid.get()),
                created: line.created.get(),
            });
        }
    }
    listing
}

/// The set of the marks under one data directory and the thread that lists
/// them into it, which ends once this value drops.
pub(crate) struct MarkListing {
    marks: Arc<Marks>,
    _stop: mpsc::Sender<()>,
}

impl MarkListing {
    /// Lists the marks under the data directory `dir` into a new set at once,
    /// and then every [`LISTING_EVERY`] on a thread of its own.
    pub(crate) fn start(dir: &Path) -> io::Result<MarkListing> {
        let folder = dir.join("agents").join("marks");
        let marks = Arc::new(Marks::new(Box::new(SystemCreationTime)));
        marks.take(list(&folder));
        let (stop, stopped) = mpsc::channel::<()>();
        let set = Arc::clone(&marks);
        std::thread::Builder::new()
            .name("halvo-capture-marks".to_owned())
            .spawn(move || {
                while stopped.recv_timeout(LISTING_EVERY) == Err(mpsc::RecvTimeoutError::Timeout) {
                    set.take(list(&folder));
                }
            })?;
        Ok(MarkListing { marks, _stop: stop })
    }

    /// The set that this listing fills.
    pub(crate) fn marks(&self) -> Arc<Marks> {
        Arc::clone(&self.marks)
    }
}
