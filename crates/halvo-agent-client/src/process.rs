//! What the process table records of a process: its creation time in microseconds since the Unix epoch, read on
//! macOS by `proc_pidinfo` and on Windows by `GetProcessTimes`, as the trial's `procs.rs` reads it, and on macOS
//! its process group.

use std::num::{NonZeroU32, NonZeroU64};

use halvo_schema::executable::text::CreationTime;

use crate::sys;

/// The creation time of the running process `pid`, or none when it is gone, has exited or cannot be read.
pub fn creation_time(pid: u32) -> Option<CreationTime> {
    #[cfg(target_os = "macos")]
    {
        match sys::info(pid) {
            sys::Read::Found(i) if !i.zombie => {
                NonZeroU64::new(i.started_us).map(CreationTime::from_micros)
            }
            _ => None,
        }
    }
    #[cfg(windows)]
    {
        sys::creation_time(pid)
            .and_then(NonZeroU64::new)
            .map(CreationTime::from_micros)
    }
}

/// Whether the process `pid` runs and has the creation time `created`.
pub fn is_running(pid: u32, created: CreationTime) -> bool {
    creation_time(pid) == Some(created)
}

/// The process group of the running process `pid` on macOS; none on Windows, which has no process groups.
pub fn process_group(pid: u32) -> Option<NonZeroU32> {
    #[cfg(target_os = "macos")]
    {
        match sys::info(pid) {
            sys::Read::Found(i) if !i.zombie => NonZeroU32::new(i.pgid),
            _ => None,
        }
    }
    #[cfg(windows)]
    {
        let _ = pid;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_own_process_has_a_creation_time_in_the_past_that_stays_the_same() {
        let me = std::process::id();
        let first = creation_time(me).expect("the own process is in the table");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros();
        assert!(u128::from(first.micros().get()) <= now);
        assert!(u128::from(first.micros().get()) + 3_600_000_000 > now);
        assert_eq!(creation_time(me), Some(first));
        assert!(is_running(me, first));
        let other = CreationTime::from_micros(first.micros().checked_add(1).unwrap());
        assert!(!is_running(me, other));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_layout_of_the_bsd_info_reads_the_own_pid_parent_and_group() {
        let me = std::process::id();
        let sys::Read::Found(i) = sys::info(me) else {
            panic!("the own process is in the table");
        };
        assert_eq!(i.pid, me);
        assert_eq!(i.ppid, std::os::unix::process::parent_id());
        assert!(!i.zombie);
        let child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let sys::Read::Found(c) = sys::info(child.id()) else {
            panic!("the child is in the table");
        };
        assert_eq!(c.ppid, me);
        assert_eq!(c.pgid, i.pgid);
        assert!(c.started_us >= i.started_us);
        let mut child = child;
        child.kill().unwrap();
        child.wait().unwrap();
    }
}
