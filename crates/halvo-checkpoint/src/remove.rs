use std::ffi::OsStr;
use std::fs::{self, File, Metadata};
use std::io;
use std::path::{Path, PathBuf};

use crate::walk::{is_loop, no_follow, same_file, Root, Walk};
use crate::{checkpoint_name, digest, LockError, Locks, RunId, Sha256, FILE_MAX};

/// What a row of the list of checkpoints names (contracts.md A7.4): the folder that `prepare`
/// was given, the workbook's path as given to `prepare` (absolute, or relative to the folder),
/// the run, and what the copy must hold.
pub struct Row<'a> {
    /// The row's `folder`; the call works inside it alone.
    pub folder: &'a Path,
    /// The row's `workbook`.
    pub workbook: &'a Path,
    /// The row's `run`, whose checkpoint the call removes.
    pub run: &'a RunId,
    /// What the copy must hold to be removed.
    pub expect: Expect,
}

/// What the copy must hold to be removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expect {
    /// A row in the state `made`: the copy's SHA-256 and length as the row holds them.
    Made {
        /// The row's `sha256`.
        sha256: Sha256,
        /// The row's `bytes`.
        bytes: u64,
    },
    /// A row in the state `planned`, which holds no SHA-256: the copy must be byte for byte the
    /// workbook as it stands at the call, read under the lock (contracts.md A6.6, CP-98 (4)).
    Planned,
}

/// What [`remove`] did, in the terms of the list's row: [`Removal::state`] is the row's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Removal {
    /// Every check held and the copy is removed: `gone`.
    Removed,
    /// No file stood in the copy's place, or the folder or a directory on the way is absent:
    /// `gone`.
    Absent,
    /// A check missed and the file is left: `left` with the reason.
    Left(Left),
}

impl Removal {
    /// The row's state: `gone` or `left`.
    pub fn state(self) -> &'static str {
        match self {
            Removal::Removed | Removal::Absent => "gone",
            Removal::Left(_) => "left",
        }
    }
}

/// Why [`remove`] left a file, with the row's closed list of reasons (contracts.md A7.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Left {
    /// `changed`: the copy's length or SHA-256 is not the row's, or for [`Expect::Planned`]
    /// it is not the workbook as it stands (also when the workbook is absent or unreadable),
    /// or another file took its place while the call read it.
    Changed,
    /// `link`: a directory on the way below the folder, or the copy itself, is a link.
    Link,
    /// `not_file`: the copy is no regular file, a part of the way is no directory, the folder
    /// is no directory, or the row's path is not inside the folder (outside it, with a parent
    /// step, or the folder itself), which the call then never reaches.
    NotFile,
    /// `name`: the directory holds no entry whose own name is the built name byte for byte,
    /// though the system answered for it, as a file system that folds case does for a name in
    /// another case; or a part of the path holds a colon, which on Windows names a stream.
    Name,
    /// `unreadable`: the copy or its directory cannot be read, or the row's length is over the
    /// bound of a file ([`FILE_MAX`]), which the call never reads past.
    Unreadable,
}

impl Left {
    /// The reason as the row writes it.
    pub fn name(self) -> &'static str {
        match self {
            Left::Changed => "changed",
            Left::Link => "link",
            Left::NotFile => "not_file",
            Left::Name => "name",
            Left::Unreadable => "unreadable",
        }
    }
}

/// Why [`remove`] decided nothing: it left the file and the row's state as they are, and a later
/// call may try again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Undecided {
    /// The lock of the workbook's directory could not be taken.
    Lock(LockError),
    /// Every check held, and the system refused the removal with an error of this kind (an
    /// application holds the copy open, the directory cannot be written).
    Remove(io::ErrorKind),
}

/// Removes the checkpoint of `row.run` beside `row.workbook`, inside `row.folder` alone
/// (contracts.md A11.3, A6.6; the coordinator's rulings CP-96 (e) and CP-98 (3)). It builds the
/// copy's name with [`checkpoint_name`] and never searches a folder for another; it walks from
/// the folder, which may itself be reached through a link, to the workbook's directory, and
/// holds the lock of that directory from its first look at the copy to the removal, so that it
/// never overlaps a `prepare` or a `replace` of the sheet executor. It removes the copy only
/// when all of these hold: no directory on the way below the folder is a link, and the copy is a
/// regular file and no link; the directory holds an entry whose own name is the built name byte
/// for byte; and the copy's length and SHA-256, read under the bound of a file, are the row's
/// ([`Expect::Made`]) or the workbook's as it stands ([`Expect::Planned`]). A copy that is
/// absent counts as removed. Every other miss leaves the file and answers its reason, which is
/// never a run's failure. It removes no checkpoint of a run that has not ended: the caller
/// calls it only for a run whose end is stored.
pub fn remove(locks: &Locks, row: &Row<'_>) -> Result<Removal, Undecided> {
    let (dir, last) = match place(row.folder, row.workbook) {
        Ok(place) => place,
        Err(answer) => return Ok(answer),
    };
    let Some(name) = checkpoint_name(Path::new(last), row.run) else {
        return Ok(Removal::Left(Left::NotFile));
    };
    let held = locks.hold(&dir).map_err(Undecided::Lock)?;
    let answer = match checked(&dir, &name, last, row.expect) {
        Ok(file) => {
            drop(file);
            match fs::remove_file(dir.join(&name)) {
                Ok(()) => Ok(Removal::Removed),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Removal::Absent),
                Err(error) => Err(Undecided::Remove(error.kind())),
            }
        }
        Err(answer) => Ok(answer),
    };
    drop(held);
    answer
}

/// The walked directory of the workbook and the workbook's file name.
fn place<'a>(folder: &Path, workbook: &'a Path) -> Result<(PathBuf, &'a OsStr), Removal> {
    let root = Root::new(folder).map_err(walked)?;
    root.parent(workbook).map_err(walked)
}

fn walked(walk: Walk) -> Removal {
    match walk {
        Walk::Io(io::ErrorKind::NotFound) => Removal::Absent,
        Walk::Io(_) => Removal::Left(Left::Unreadable),
        Walk::Link => Removal::Left(Left::Link),
        Walk::Name => Removal::Left(Left::Name),
        Walk::Folder | Walk::Outside | Walk::Directory | Walk::NotFile => {
            Removal::Left(Left::NotFile)
        }
    }
}

/// The checks of [`remove`] on the entry `name` of `dir`, made under the lock; the open copy
/// when every one holds.
fn checked(dir: &Path, name: &OsStr, workbook: &OsStr, expect: Expect) -> Result<File, Removal> {
    let path = dir.join(name);
    let (file, seen) = opened(&path)?;
    if !own_entry(dir, name)? {
        return Err(Removal::Left(Left::Name));
    }
    let (sha256, bytes) = match expect {
        Expect::Made { sha256, bytes } => (sha256, bytes),
        Expect::Planned => {
            let (book, _) =
                opened(&dir.join(workbook)).map_err(|_| Removal::Left(Left::Changed))?;
            let (sha256, bytes) =
                digest::read(&book, FILE_MAX).map_err(|_| Removal::Left(Left::Changed))?;
            if bytes > FILE_MAX {
                return Err(Removal::Left(Left::Changed));
            }
            (sha256, bytes)
        }
    };
    if seen.len() != bytes {
        return Err(Removal::Left(Left::Changed));
    }
    if bytes > FILE_MAX {
        return Err(Removal::Left(Left::Unreadable));
    }
    let (held, count) =
        digest::read(&file, FILE_MAX).map_err(|_| Removal::Left(Left::Unreadable))?;
    if count != bytes || held != sha256 {
        return Err(Removal::Left(Left::Changed));
    }
    let now = fs::symlink_metadata(&path).map_err(|_| Removal::Left(Left::Changed))?;
    if !same_file(&seen, &now) {
        return Err(Removal::Left(Left::Changed));
    }
    Ok(file)
}

/// The regular file at `path`, opened without following a link, with what the open file says
/// of itself.
fn opened(path: &Path) -> Result<(File, Metadata), Removal> {
    let named = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(Removal::Absent),
        Err(_) => return Err(Removal::Left(Left::Unreadable)),
    };
    if named.file_type().is_symlink() {
        return Err(Removal::Left(Left::Link));
    }
    if !named.is_file() {
        return Err(Removal::Left(Left::NotFile));
    }
    let file = no_follow(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => Removal::Absent,
        _ if is_loop(&error) => Removal::Left(Left::Link),
        _ => Removal::Left(Left::Unreadable),
    })?;
    let seen = file
        .metadata()
        .map_err(|_| Removal::Left(Left::Unreadable))?;
    if seen.file_type().is_symlink() {
        return Err(Removal::Left(Left::Link));
    }
    if !seen.is_file() {
        return Err(Removal::Left(Left::NotFile));
    }
    if !same_file(&named, &seen) {
        return Err(Removal::Left(Left::Changed));
    }
    Ok((file, seen))
}

/// Whether the directory `dir` holds an entry whose own name is `name` byte for byte.
fn own_entry(dir: &Path, name: &OsStr) -> Result<bool, Removal> {
    let entries = fs::read_dir(dir).map_err(|_| Removal::Left(Left::Unreadable))?;
    for entry in entries {
        let entry = entry.map_err(|_| Removal::Left(Left::Unreadable))?;
        if entry.file_name() == name {
            return Ok(true);
        }
    }
    Ok(false)
}
