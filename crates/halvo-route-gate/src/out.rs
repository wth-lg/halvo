//! The `--out` file of `generate` and `derive`: always a new file, never one that stands at the path.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};

/// Why `write_new` put nothing at its path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refused {
    /// Something stands at the path, before the call or by the time of the link.
    Exists,
    /// The bytes could not be written, or could not be put at the path.
    Unwritable,
}

/// Writes `text` to a new file at `path`. The bytes go to a new file of another name in the
/// directory of `path` and are synced; `before_link` is then called once, and a hard link puts the
/// file at `path`. A file that stands at `path` is never written over or removed: the call returns
/// `Exists`. Whatever the outcome, the call removes the other name before it returns.
pub fn write_new(path: &str, text: &str, before_link: impl FnOnce()) -> Result<(), Refused> {
    if std::fs::symlink_metadata(path).is_ok() {
        return Err(Refused::Exists);
    }
    let Some((part, mut file)) = new_part(path) else {
        return Err(Refused::Unwritable);
    };
    let written = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    let placed = written.and_then(|()| {
        before_link();
        std::fs::hard_link(&part, path)
    });
    let _ = std::fs::remove_file(&part);
    match placed {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Err(Refused::Exists),
        Err(_) => Err(Refused::Unwritable),
    }
}

fn new_part(path: &str) -> Option<(String, File)> {
    for n in 0..1_000u32 {
        let part = format!("{path}.{}-{n}.part", std::process::id());
        match OpenOptions::new().write(true).create_new(true).open(&part) {
            Ok(file) => return Some((part, file)),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(_) => return None,
        }
    }
    None
}
