//! The binary `halvo-analysis`, the analysis worker of the workflow run (branch B): one process for one batch. It
//! reads one batch of capture records through the client of halvo-seam, makes one call of halvo-identify's episode
//! step with stage A, sends the episodes to the owner of the derived store as analysis writes, writes one log line
//! and exits. The owner makes the summaries and the proposals. The worker opens no file and no connection.
//!
//! # How the worker is started
//!
//! ```text
//! halvo-analysis --read=<r>,<w> --owner=<r>,<w> [--store=<store id> --position=<seg>,<idx>]
//! ```
//!
//! - `r` and `w` are ends that the starter lets the worker inherit: descriptor numbers on macOS, handle values on
//!   Windows. The worker marks each end close-on-exec or not inheritable.
//! - `--read` is the read channel, a byte stream of the seam protocol: the worker writes its messages to `w` and
//!   reads the replies from `r`.
//! - `--owner` is the owner's worker channel, the pipe pair between the worker and the owner: the worker writes its
//!   requests to `w` and reads the replies from `r`, one frame for each.
//! - `--store` and `--position` name the committed position of the batch before, with the id of its store; they
//!   come together. Without them the worker reads from the zero position of whatever store the read channel
//!   serves.
//! - Before it reads its arguments the worker turns core files and crash dumps off (`sys::crash_reports_off`).
//!
//! # The batch
//!
//! The worker reads until the newest record, until the batch holds 16,384 records that a record follows, or until
//! the first record whose local day, by the system's time zone, is not that of the batch's first record, whichever
//! comes first. It follows the run marks in the order of place: a `start` opens an agent's run, an `end` closes it,
//! and a `start` while a run is open or an `end` while none is open changes nothing. A batch that reaches the
//! 16,384 records ends before its last pause, the last of its records before which no run is open and whose
//! instant lies more than 331 s after that of every record before it: the 300 s past which the episode step
//! continues no episode and the 31 s that it moves a late keystroke or click back, together. With no pause and a
//! run open after its last record it ends before that run's `start` mark, and with neither it keeps them all. A
//! batch that reaches the newest record while a run is open ends before that run's `start` mark too. The position
//! of the first record that it did not take is the batch's commit position, and the next start reads from that
//! record on, so a day above the bound takes several starts and holds back no later day.
//!
//! When that `start` mark is the batch's first record, the batch keeps no record: at the newest record its commit
//! position is the one that it started from, so each start reads the open run again, and at the bound it reads on
//! to the run's `end` mark, whose position is its commit position. A run that is open at the first record of
//! another local day ends with its batch, and the next start reads the rest of the run as the person's. So when a
//! run's `end` mark never comes, as when its agent died, nothing after its `start` mark is committed until a later
//! local day holds a record, and the person's own records of that day after the mark are never proposed.
//!
//! The episode step closes every episode at the end of its call: a batch that ends at the bound with no pause, or
//! before a run's `start` mark, can make two episodes of one.
//!
//! # The window that a batch carries
//!
//! A batch read from a committed position whose first records hold a key or a click in front of any title record
//! carries the window that was in front there: after the batch, the worker reads back in front of the committed
//! position, through the same read channel, for the last title record before it (`read::carry`), and hands it to the
//! episode step. The batch's first records belong to that window while, from the title record on, each record's `seq`
//! is the one before it plus one, no record is a `foreground_app` or a `run_mark` record, and each holds the title
//! record's app; the first record that breaks this ends the carry, and the records from it to the batch's first title
//! record are dropped. The title record makes no step and adds nothing of its own to an episode. Where the read back
//! finds no such title record, at most `RECORDS_MAX` records back, or a read back is refused or fails, the batch
//! carries nothing: a keystroke or click in front of its first title record, also one that the store placed after
//! the batch's first record and that the episode step orders before it, has no window and is dropped, at this cut
//! as at the cut of a day. The worker keeps and writes nothing of the read back.
//!
//! The episode step gets the offset of local time from UTC at the batch's first record. The worker draws one
//! summary id from the system's random bytes for each episode that the step returns.
//!
//! It sends the episodes as one batch (`send::Sender::batch`): one or more writes of at most 65,500 bytes, the last
//! alone with the commit position. A worker that ends before that write leaves nothing in the store. A batch that
//! took no record and whose commit position is the position that it started from sends nothing.
//!
//! # The log
//!
//! The worker writes one line on standard error and nothing on standard output. After a committed batch:
//!
//! ```text
//! batch end=<newest|defer|day|records> run_open=<yes|no> start_first=<yes|no> records=<n> episodes=<n>
//! ```
//!
//! `end` says why the batch ended; this worker takes no request to defer and never writes `defer`. `run_open` and
//! `start_first` are the episode step's answers on an agent's run. `records` counts the records that the batch
//! keeps and `episodes` the episodes that it sent. After any other end the line is
//! `halvo-analysis: <what>: <name>`, where the name is a fixed word, the kind of an input or output error, or an
//! error name of a reply. No line holds a title, a site, an app id or the text of an error. A line that cannot be
//! written is dropped and changes no exit code.
//!
//! # How the worker ends
//!
//! Each end has its exit code (`exit`). A panic writes the line `halvo-analysis: panic`, with no text of the panic,
//! and ends the worker with the code 101 of the Rust runtime.

/// Writes one line on standard error, and drops a line that cannot be written.
macro_rules! log {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

mod read;
mod send;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as sys;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as sys;

use std::ffi::OsString;
use std::fs::File;

use halvo_identify::{episode_step, Call, Carry, StageA};
use halvo_schema_core::ids::{Position, StoreId, SummaryId};
use halvo_seam::client::{Client, StreamLink};

use send::{SendFault, Sender};

/// The exit codes of the worker.
mod exit {
    /// The owner acknowledged the commit of the batch, or the batch held nothing to commit.
    pub const COMMITTED: i32 = 0;
    /// The arguments are not of the form of the start, or an end that they name is not open.
    pub const USAGE: i32 = 64;
    /// The start call failed: core files and crash dumps are not off.
    pub const START: i32 = 70;
    /// The system gave no random bytes for an id.
    pub const RANDOM: i32 = 71;
    /// A fault of the read channel: a read or a write failed, the channel ended, a reply broke the protocol, or the
    /// server refused the cursor.
    pub const READ: i32 = 74;
    /// A fault or a refusal of the owner: a read or a write of its channel failed, the channel ended, a reply was
    /// no reply to the write sent, or the owner refused a write.
    pub const OWNER: i32 = 76;
}

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    std::panic::set_hook(Box::new(|_| log!("halvo-analysis: panic")));
    if let Err(e) = sys::crash_reports_off() {
        log!("halvo-analysis: start: {:?}", e.kind());
        return exit::START;
    }
    let args = match Args::parse(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(what) => {
            log!("halvo-analysis: usage: {what}");
            return exit::USAGE;
        }
    };
    let Ok(ends) = Ends::adopt(&args) else {
        log!("halvo-analysis: usage: end-not-open");
        return exit::USAGE;
    };
    let (read_in, read_out) = ends.read;
    let mut client = Client::new(StreamLink::new(read_in, read_out));
    let utc_offset = |secs: i64| sys::utc_offset(secs).unwrap_or(0);
    let taken = match read::take(&mut client, args.from, &utc_offset) {
        Ok(taken) => taken,
        Err(fault) => {
            log!("halvo-analysis: read: {fault}");
            return exit::READ;
        }
    };
    let mut dry = false;
    let mut ids = || {
        let bytes = random().unwrap_or_else(|| {
            dry = true;
            [0; 16]
        });
        SummaryId::from_random(bytes)
    };
    let carried = read::carry(&mut client, &taken);
    let call = Call {
        entries: &taken.entries,
        expiry: &taken.expiry,
        utc_offset_s: taken.utc_offset_s,
        carry: carried.as_ref().map(|carried| Carry {
            title: &carried.title,
            until: carried.until,
        }),
    };
    let outcome = episode_step(&call, &StageA, &mut ids);
    if dry {
        log!("halvo-analysis: random: summary-id");
        return exit::RANDOM;
    }
    let records = taken.entries.len();
    let sent = if records == 0 && taken.commit == taken.from {
        0
    } else {
        let (owner_in, owner_out) = ends.owner;
        let mut draw = random;
        let mut sender = Sender {
            reader: owner_in,
            writer: owner_out,
            store: taken.store,
            random: &mut draw,
        };
        match sender.batch(outcome.episodes, taken.commit) {
            Ok(sent) => sent,
            Err(SendFault::Random) => {
                log!("halvo-analysis: random: request-id");
                return exit::RANDOM;
            }
            Err(fault) => {
                log!("halvo-analysis: owner: {fault}");
                return exit::OWNER;
            }
        }
    };
    let yes_no = |yes: bool| if yes { "yes" } else { "no" };
    log!(
        "batch end={} run_open={} start_first={} records={records} episodes={sent}",
        taken.end.name(),
        yes_no(outcome.run_open),
        yes_no(outcome.start_first),
    );
    exit::COMMITTED
}

/// Sixteen random bytes of the system, or none where it gives none.
fn random() -> Option<[u8; 16]> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes)
}

/// The arguments of the start.
struct Args {
    read: (u64, u64),
    owner: (u64, u64),
    from: Option<(StoreId, Position)>,
}

/// The highest part of a position (C5.3): 2^63 - 1.
const POSITION_MAX: u64 = i64::MAX as u64;

impl Args {
    /// Reads the arguments, or names the argument at fault with a fixed word that holds no value of it.
    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Args, &'static str> {
        let mut read = None;
        let mut owner = None;
        let mut store = None;
        let mut position = None;
        for arg in args {
            let arg = arg.into_string().map_err(|_| "not-utf-8")?;
            let (name, value) = arg.split_once('=').ok_or("not-name=value")?;
            match name {
                "--read" => once(&mut read, pair(value).ok_or("--read")?)?,
                "--owner" => once(&mut owner, pair(value).ok_or("--owner")?)?,
                "--store" => once(&mut store, StoreId::parse(value).map_err(|_| "--store")?)?,
                "--position" => {
                    let (seg, idx) = pair(value)
                        .filter(|(seg, idx)| *seg <= POSITION_MAX && *idx <= POSITION_MAX)
                        .ok_or("--position")?;
                    once(&mut position, Position { seg, idx })?;
                }
                _ => return Err("unknown-argument"),
            }
        }
        let read: (u64, u64) = read.ok_or("--read-missing")?;
        let owner: (u64, u64) = owner.ok_or("--owner-missing")?;
        let mut ends = [read.0, read.1, owner.0, owner.1];
        ends.sort_unstable();
        if ends.windows(2).any(|two| two[0] == two[1]) {
            return Err("two-ends-are-one");
        }
        let from = match (store, position) {
            (Some(store), Some(position)) => Some((store, position)),
            (None, None) => None,
            _ => return Err("--store-and---position-come-together"),
        };
        Ok(Args { read, owner, from })
    }
}

fn once<T>(slot: &mut Option<T>, value: T) -> Result<(), &'static str> {
    match slot.replace(value) {
        Some(_) => Err("argument-twice"),
        None => Ok(()),
    }
}

/// The two numbers of `<first>,<second>`.
fn pair(value: &str) -> Option<(u64, u64)> {
    let (first, second) = value.split_once(',')?;
    Some((first.parse().ok()?, second.parse().ok()?))
}

/// The ends of the two channels, taken from the start: for each, the end that the worker reads and the end that it
/// writes.
struct Ends {
    read: (File, File),
    owner: (File, File),
}

impl Ends {
    fn adopt(args: &Args) -> std::io::Result<Ends> {
        Ok(Ends {
            read: (sys::adopt(args.read.0)?, sys::adopt(args.read.1)?),
            owner: (sys::adopt(args.owner.0)?, sys::adopt(args.owner.1)?),
        })
    }
}
