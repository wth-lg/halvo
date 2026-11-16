//! The one maker of the scratch directories of tests. A crate takes it as a dev-dependency alone.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(test)]
mod tests;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// Longest name that [`make`] takes, in bytes.
pub const NAME_MAX: usize = 64;

/// Makes a new, empty directory `<name>-<process id>-<n>` under `base`, which is made first with
/// its parents when it is missing, and returns its path. `n` comes from one counter of the
/// process. The directory is made by `create_dir`, which fails when the name stands; a name that
/// stands is passed over for the next number. So two calls never get one directory, in one
/// process or in two that share a process id, whatever the clock reads.
///
/// Fails with `InvalidInput` when `name` is empty, longer than [`NAME_MAX`] bytes, or holds a
/// byte other than an ASCII letter, an ASCII digit, `-` and `_`, and with the error of the file
/// system when making `base` or the directory fails for another reason than a standing name.
pub fn make(base: &Path, name: &str) -> io::Result<PathBuf> {
    make_with(base, name, &NEXT)
}

fn make_with(base: &Path, name: &str, next: &AtomicU64) -> io::Result<PathBuf> {
    let fits = !name.is_empty()
        && name.len() <= NAME_MAX
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !fits {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a scratch name is 1 to 64 bytes of ASCII letters, digits, - and _",
        ));
    }
    std::fs::create_dir_all(base)?;
    let pid = std::process::id();
    loop {
        let n = next.fetch_add(1, Ordering::Relaxed);
        let dir = base.join(format!("{name}-{pid}-{n}"));
        match std::fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
}
