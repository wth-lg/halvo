//! `agent-gate configs` makes one approved workflow per seed category on the gate host, sends `create_agent` for
//! each and prints one line per category and the count (A14.5). Exit 0 when every line is `stored=yes
//! state=approved agent=set result=valid`, 1 when a line is not, and 2 for a refusal, which prints one line
//! `REFUSED <cause>: <detail>` on standard error and nothing on standard output; the cause of a refusal is `usage`.
//!
//! `agent-gate privacy --data-dir DIR` plays the delete of a workflow on a new derived store under `DIR` and prints
//! one line `privacy check=<name> result=<ok|miss|not-run>` for `agent_deleted`, `agent_scrub` and
//! `agent_delete_all`, in that order (A14.4; `halvo_agent_gate::privacy`). Exit 0 when no line is `miss`, 1 when one
//! is, and 2 for a refusal: `usage`, or `store` when the play cannot run (the store does not open under `DIR`, or it
//! refuses a step of the play). The store stays under `DIR`.
//!
//! `agent-gate task fill --mode replay --seed N [--link host] [--browser PATH]` plays the task `fill` with the runner
//! through the gate host and prints the one line of A14.5 with `store=memory` (`halvo_agent_gate::task`). With
//! `--link product --workflow ID [--data-dir DIR]` in place of `--link host` it plays the task through the product's
//! owner over U9.2's link, with no gate host, and the line holds `store=owner`. Exit 0 when the line has
//! `marks=start,end`, `fixture=ok` and `result=completed`, 1 when it has not, and 2 for a refusal: `usage`, for every
//! other form of A14.5 and for a play that cannot run, such as an owner that gives no reply over the product link, or
//! `fixture`. Over the product link the runner stamps its days with the offset of this system's clock from UTC at the
//! start of the play (`clock::utc_offset`), read by the same calls of the system by which the product's owner counts
//! its local day: `tzset` and `localtime_r` on macOS, `SystemTimeToTzSpecificLocalTime` with the zone in force on
//! Windows.

use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use halvo_agent_gate::configs::configs;
use halvo_agent_gate::privacy::{self, Outcome};
use halvo_agent_gate::{task, GateHost};

const USAGE: &str = "agent-gate configs | agent-gate privacy --data-dir DIR | agent-gate task fill --mode replay --seed N [--link host | --link product --workflow ID [--data-dir DIR]] [--browser PATH]";

fn refuse(cause: &str, detail: &str) -> ExitCode {
    eprintln!("REFUSED {cause}: {detail}");
    ExitCode::from(2)
}

fn write_out(text: &str, pass: bool) -> ExitCode {
    let mut out = std::io::stdout().lock();
    if out.write_all(text.as_bytes()).is_err() || out.flush().is_err() {
        return ExitCode::from(1);
    }
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn run_configs() -> ExitCode {
    let mut host = GateHost::new();
    let report = configs(&mut host);
    write_out(&report.render(), report.pass())
}

fn run_privacy(data_dir: &Path) -> ExitCode {
    let played = privacy::process_key().and_then(|key| privacy::play(data_dir, &key));
    match played {
        Ok(delete) => {
            let outcomes = privacy::outcomes(&delete);
            let pass = outcomes.iter().all(|(_, o)| *o != Outcome::Miss);
            write_out(&privacy::render(&outcomes), pass)
        }
        Err(detail) => refuse("store", &detail),
    }
}

fn run_task(args: &[OsString]) -> ExitCode {
    match task::parse(args).and_then(|fill| task::play(&fill, clock::utc_offset_now())) {
        Ok(line) => write_out(&line.render(), line.pass()),
        Err(refused) => refuse(refused.cause.as_str(), &refused.detail),
    }
}

/// The offset of this system's clock from UTC, read as the product's owner reads its local calendar (crates/halvo-owner,
/// `local_date` in src/unix.rs and src/windows.rs), with the same form as halvo-analysis's `utc_offset`.
mod clock {
    use std::time::{SystemTime, UNIX_EPOCH};

    /// The offset now, in seconds east of UTC; 0 where the system gives no local time, as the owner then counts the UTC
    /// day.
    pub(crate) fn utc_offset_now() -> i32 {
        let secs = match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(after) => i64::try_from(after.as_secs()).unwrap_or(i64::MAX),
            Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
        };
        utc_offset(secs).unwrap_or(0)
    }

    #[cfg(unix)]
    extern "C" {
        fn tzset();
    }

    /// The offset of local time from UTC, in seconds, at a second since 1970-01-01, by the system's time zone.
    #[cfg(unix)]
    pub(crate) fn utc_offset(secs: i64) -> Option<i32> {
        // SAFETY: tzset reads the environment and the time zone files and sets the C library's state.
        unsafe { tzset() };
        let at = libc::time_t::try_from(secs).ok()?;
        // SAFETY: an all-zero tm is a valid value of the type, whose one pointer is then null.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        // SAFETY: localtime_r reads `at` and writes `tm` alone, both alive through the call.
        if unsafe { libc::localtime_r(&at, &mut tm) }.is_null() {
            return None;
        }
        i32::try_from(tm.tm_gmtoff).ok()
    }

    /// The seconds from 1601-01-01 to 1970-01-01.
    #[cfg(windows)]
    const FILETIME_EPOCH_S: i64 = 11_644_473_600;

    /// The offset of local time from UTC, in seconds, at a second since 1970-01-01, by the time zone that the system
    /// has in force.
    #[cfg(windows)]
    pub(crate) fn utc_offset(secs: i64) -> Option<i32> {
        use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
        use windows_sys::Win32::System::Time::{
            FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime,
        };
        let ticks = secs
            .checked_add(FILETIME_EPOCH_S)?
            .checked_mul(10_000_000)?;
        let ticks = u64::try_from(ticks).ok()?;
        let at = FILETIME {
            dwLowDateTime: (ticks & 0xffff_ffff) as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        // SAFETY: an all-zero SYSTEMTIME is a valid value of the type.
        let mut utc: SYSTEMTIME = unsafe { std::mem::zeroed() };
        // SAFETY: as above.
        let mut local: SYSTEMTIME = unsafe { std::mem::zeroed() };
        // SAFETY: the call reads `at` and writes `utc`, both alive through the call.
        if unsafe { FileTimeToSystemTime(&at, &mut utc) } == 0 {
            return None;
        }
        // SAFETY: a null zone asks for the zone in force; the call reads `utc` and writes `local`, both alive through
        // it.
        if unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) } == 0 {
            return None;
        }
        let local_secs = days_from_civil(
            i64::from(local.wYear),
            i64::from(local.wMonth),
            i64::from(local.wDay),
        ) * 86_400
            + i64::from(local.wHour) * 3_600
            + i64::from(local.wMinute) * 60
            + i64::from(local.wSecond);
        i32::try_from(local_secs - secs).ok()
    }

    /// The days from 1970-01-01 to a date of the proleptic Gregorian calendar.
    #[cfg(windows)]
    fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
        let year = if month <= 2 { year - 1 } else { year };
        let era = year.div_euclid(400);
        let year_of_era = year - era * 400;
        let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }

    /// A system with neither: no local time, so the offset of the UTC day.
    #[cfg(not(any(unix, windows)))]
    pub(crate) fn utc_offset(_secs: i64) -> Option<i32> {
        None
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [command] if command == "configs" => run_configs(),
        [command, flag, dir] if command == "privacy" && flag == "--data-dir" => {
            run_privacy(Path::new(dir))
        }
        [command, rest @ ..] if command == "task" => run_task(rest),
        _ => refuse("usage", USAGE),
    }
}
