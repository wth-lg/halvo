//! The port that a timed run drains into until a real store exists: it
//! acknowledges each batch with a rising ordinal and keeps nothing.

use std::io;
use std::path::PathBuf;
use std::time::SystemTime;

use super::port::{
    ByteBound, CaptureStore, MaintFault, MaintainStore, Ordinal, Pass, SegmentWrite, Usage,
    WriteFault, Written,
};

/// A port that keeps nothing, for the store directory `dir`.
#[derive(Debug)]
pub struct DiscardPort {
    dir: PathBuf,
    next: u64,
}

impl DiscardPort {
    pub fn new(dir: PathBuf) -> DiscardPort {
        DiscardPort { dir, next: 1 }
    }
}

impl MaintainStore for DiscardPort {
    fn expire_due(&mut self, _now: SystemTime, _max_segments: usize) -> Result<Pass, MaintFault> {
        Ok(Pass::default())
    }

    fn evict_to(&mut self, _bound: ByteBound, _max_segments: usize) -> Result<Pass, MaintFault> {
        Ok(Pass::default())
    }

    fn usage(&mut self) -> Result<Usage, MaintFault> {
        Ok(Usage::default())
    }
}

impl CaptureStore for DiscardPort {
    fn write_batch(&mut self, write: SegmentWrite<'_>) -> Result<Written, WriteFault> {
        if write.is_malformed() {
            return Err(WriteFault::Refused);
        }
        let ordinal = Ordinal::new(self.next);
        self.next += 1;
        Ok(Written {
            ordinal,
            logical_bytes: write.payload().len() as u64,
            disk_bytes: 0,
        })
    }

    fn volume_free_bytes(&self) -> io::Result<u64> {
        free_bytes(&self.dir)
    }
}

#[cfg(target_os = "macos")]
fn free_bytes(dir: &std::path::Path) -> io::Result<u64> {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(dir.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `path` is a NUL-terminated string that outlives the call, and
    // `stat` points to writable memory of the size of `libc::statvfs`.
    let status = unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) };
    if status != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `statvfs` returned 0, so it filled `stat`.
    let stat = unsafe { stat.assume_init() };
    Ok(u64::from(stat.f_bavail).saturating_mul(stat.f_frsize))
}

#[cfg(not(target_os = "macos"))]
fn free_bytes(_dir: &std::path::Path) -> io::Result<u64> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::daemon::model_store::write_one;
    use crate::daemon::port::tests::a_pass_keeps_to_its_chunk_and_lists_acknowledged_ordinals;

    #[test]
    fn the_discard_port_acknowledges_with_rising_ordinals_and_keeps_nothing() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut port = DiscardPort::new(dir);
        let mut acknowledged = Vec::new();
        for _ in 0..3 {
            let written = write_one(&mut port, b"{}\n").unwrap();
            assert_eq!(written.logical_bytes, 3);
            acknowledged.push(written.ordinal);
        }
        assert!(acknowledged.windows(2).all(|pair| pair[1] > pair[0]));
        assert_eq!(port.usage().unwrap(), Usage::default());
        a_pass_keeps_to_its_chunk_and_lists_acknowledged_ordinals(&mut port, &acknowledged);
        #[cfg(target_os = "macos")]
        assert!(port.volume_free_bytes().unwrap() > 0);
    }
}
