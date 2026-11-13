use std::ffi::OsStr;
use std::fs::{File, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::at::Dir;
use crate::identity::{same_file, unchanged, FileId};
use crate::walk::{is_loop, Root, Walk};
use crate::{checkpoint_name, digest, removing_name, LockError, Locks, RunId, Sha256, FILE_MAX};

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
    /// Every check held, and the system refused a step of the removal with an error of this
    /// kind: the move of the copy aside to its removing name (`AlreadyExists` only when an
    /// entry stands at that name; `PermissionDenied` when an application holds the copy open
    /// or the directory cannot be written; on Windows a sharing violation while another
    /// process holds a directory of the way open for deletion), or the removal of the moved
    /// copy, after which the copy is put back under its own name.
    Remove(io::ErrorKind),
}

/// Removes the checkpoint of `row.run` beside `row.workbook`, inside `row.folder` alone
/// (contracts.md A11.3, A6.6; the coordinator's rulings CP-96 (e), CP-98 (3) and CP-124). It
/// builds the copy's name with [`checkpoint_name`] and never searches a folder for another; it
/// walks from the folder, which may itself be reached through a link, to the workbook's
/// directory, takes the lock of that directory, and holds it from its first look at the copy
/// to the removal, so that it never overlaps a `prepare` or a `replace` of the sheet executor.
/// Under the lock it walks the way again part by part, each directory opened without following
/// a link (on macOS by descriptors, each opened relative to the one before; on Windows by
/// handles that it holds open and shares with no deletion, so no directory of the way can be
/// moved or replaced until the call ends), and acts only on entries of the directory that it
/// opened, so a directory of the way that becomes a link is never followed out of the folder.
/// It removes the copy only when all of these hold: no directory on the way below the folder
/// is a link, and the copy is a regular file and no link; the directory holds an entry whose
/// own name is the built name byte for byte; and the copy's length and SHA-256, read under the
/// bound of a file, are the row's ([`Expect::Made`]) or the workbook's as it stands
/// ([`Expect::Planned`]). It then moves the copy aside under [`removing_name`], checks that the
/// moved entry is the file that it read with the same SHA-256, and removes it; when that check
/// misses or the removal fails, it puts the file back under its own name. A copy that is
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
    let answer = match opened_dir(row, &dir) {
        Ok(at) => match checked(&at, &name, last, row.expect) {
            Ok((file, seen, read)) => {
                drop(file);
                aside(&at, &name, &seen, read)
            }
            Err(answer) => Ok(answer),
        },
        Err(answer) => Ok(answer),
    };
    drop(held);
    answer
}

fn place<'a>(folder: &Path, workbook: &'a Path) -> Result<(PathBuf, &'a OsStr), Removal> {
    let root = Root::new(folder).map_err(walked)?;
    root.parent(workbook).map_err(walked)
}

/// The workbook's directory walked again under the lock, part by part, and checked to be the
/// directory whose lock the call holds.
fn opened_dir(row: &Row<'_>, dir: &Path) -> Result<Dir, Removal> {
    match place(row.folder, row.workbook) {
        Ok((again, _)) if again == dir => {}
        Ok(_) => return Err(Removal::Left(Left::Link)),
        Err(answer) => return Err(answer),
    }
    let root = Root::new(row.folder).map_err(walked)?;
    let names = root.names(row.workbook).map_err(walked)?;
    let (_, parts) = names.split_last().ok_or(Removal::Left(Left::NotFile))?;
    let at = Dir::walk(root.dir(), parts).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => Removal::Absent,
        io::ErrorKind::NotADirectory => Removal::Left(Left::Link),
        _ if is_loop(&error) => Removal::Left(Left::Link),
        _ => Removal::Left(Left::Unreadable),
    })?;
    let opened = at.id().map_err(|_| Removal::Left(Left::Unreadable))?;
    let named = FileId::at(dir).map_err(|_| Removal::Left(Left::Link))?;
    if !same_file(&opened, &named) {
        return Err(Removal::Left(Left::Link));
    }
    Ok(at)
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

fn checked(
    at: &Dir,
    name: &OsStr,
    workbook: &OsStr,
    expect: Expect,
) -> Result<(File, FileId, (Sha256, u64)), Removal> {
    let (file, seen, id) = opened(at, name)?;
    if !own_entry(at, name)? {
        return Err(Removal::Left(Left::Name));
    }
    let (sha256, bytes) = match expect {
        Expect::Made { sha256, bytes } => (sha256, bytes),
        Expect::Planned => {
            let (book, _, _) = opened(at, workbook).map_err(|_| Removal::Left(Left::Changed))?;
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
    let (_, now, now_id) = opened(at, name).map_err(|_| Removal::Left(Left::Changed))?;
    if !same_file(&id, &now_id) || !unchanged(&seen, &now) {
        return Err(Removal::Left(Left::Changed));
    }
    Ok((file, id, (sha256, bytes)))
}

static ASIDE: AtomicU64 = AtomicU64::new(0);

fn aside(at: &Dir, name: &OsStr, seen: &FileId, held: (Sha256, u64)) -> Result<Removal, Undecided> {
    let count = ASIDE.fetch_add(1, Ordering::Relaxed) & 0xffff_ffff;
    let own = (u64::from(std::process::id()) << 32) | count;
    let moved = removing_name(name, own);
    match at.rename_new(name, &moved) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Removal::Absent),
        Err(error) => return Err(Undecided::Remove(error.kind())),
    }
    let still = opened(at, &moved).is_ok_and(|(file, _, now)| {
        same_file(seen, &now) && digest::read(&file, FILE_MAX).is_ok_and(|read| read == held)
    });
    if !still {
        put_back(at, &moved, name);
        return Ok(Removal::Left(Left::Changed));
    }
    match at.unlink(&moved) {
        Ok(()) => Ok(Removal::Removed),
        Err(error) => {
            put_back(at, &moved, name);
            Err(Undecided::Remove(error.kind()))
        }
    }
}

/// Puts the moved file back under its own name: a second name and then the removal of the
/// moved one, or, where the system makes no second name, a rename that replaces no entry.
fn put_back(at: &Dir, moved: &OsStr, name: &OsStr) {
    if at.link(moved, name).is_ok() {
        let _ = at.unlink(moved);
    } else {
        let _ = at.rename_new(moved, name);
    }
}

fn opened(at: &Dir, name: &OsStr) -> Result<(File, Metadata, FileId), Removal> {
    let (file, seen) = at
        .open(name)
        .map_err(|error| match (error.kind(), at.look(name)) {
            (io::ErrorKind::NotFound, _) => Removal::Absent,
            _ if is_loop(&error) => Removal::Left(Left::Link),
            (_, Some(meta)) if meta.file_type().is_symlink() => Removal::Left(Left::Link),
            (_, Some(meta)) if !meta.is_file() => Removal::Left(Left::NotFile),
            _ => Removal::Left(Left::Unreadable),
        })?;
    if seen.file_type().is_symlink() {
        return Err(Removal::Left(Left::Link));
    }
    if !seen.is_file() {
        return Err(Removal::Left(Left::NotFile));
    }
    let id = FileId::of(&file).map_err(|_| Removal::Left(Left::Unreadable))?;
    Ok((file, seen, id))
}

fn own_entry(at: &Dir, name: &OsStr) -> Result<bool, Removal> {
    let names = at.names().map_err(|_| Removal::Left(Left::Unreadable))?;
    Ok(names.iter().any(|entry| entry == name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let exe = std::env::current_exe().expect("the test binary");
        let target = exe.ancestors().nth(3).expect("the target directory");
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = target
            .join("tmp")
            .join("halvo-checkpoint-unit")
            .join(format!("{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("make the scratch directory");
        dir.canonicalize().expect("the canonical directory")
    }

    /// The review's patch aside-no-same-file: a file with the same bytes that took the copy's
    /// place after the checks is not the checked file; `aside` puts it back and leaves it.
    #[test]
    fn aside_removes_only_the_checked_file_and_a_same_bytes_swap_stays() {
        let dir = scratch("aside-swap");
        let bytes = b"the workbook as prepare read it";
        let name = OsStr::new("values.halvo-checkpoint-x.xlsx");
        fs::write(dir.join(name), bytes).expect("the checked copy");
        let at = Dir::walk(&dir, &[]).expect("the directory");
        let (file, _, seen) = opened(&at, name).expect("opened");
        drop(file);
        fs::write(dir.join("person.xlsx"), bytes).expect("a file of the same bytes");
        fs::rename(dir.join("person.xlsx"), dir.join(name)).expect("swap it in");
        let held = (Sha256::of(bytes), bytes.len() as u64);
        let answer = aside(&at, name, &seen, held);
        assert_eq!(answer, Ok(Removal::Left(Left::Changed)));
        assert_eq!(
            fs::read(dir.join(name)).expect("the swapped file stays"),
            bytes
        );
        let names: Vec<_> = fs::read_dir(&dir).expect("list").flatten().collect();
        assert_eq!(names.len(), 1, "no file under a removing name");
    }

    /// B2 of CP-124 in a forced order: the workbook's directory becomes a link to a directory
    /// outside the folder after the call opened it; every step acts on the directory that it
    /// opened, and the file outside with the copy's name and bytes stays.
    #[cfg(unix)]
    #[test]
    fn a_directory_that_becomes_a_link_after_the_walk_is_never_followed() {
        let base = scratch("forced-flip");
        let folder = base.join("folder");
        let outside = base.join("outside");
        fs::create_dir_all(folder.join("sub")).expect("the sub directory");
        fs::create_dir_all(&outside).expect("the outside directory");
        let bytes = b"the workbook as prepare read it";
        let name = OsStr::new("values.halvo-checkpoint-x.xlsx");
        fs::write(folder.join("sub").join(name), bytes).expect("the checkpoint");
        fs::write(outside.join(name), bytes).expect("the person's copy outside");
        let at = Dir::walk(&folder, &[OsStr::new("sub")]).expect("the directory");
        fs::rename(folder.join("sub"), folder.join("sub-real")).expect("move the directory");
        std::os::unix::fs::symlink(&outside, folder.join("sub")).expect("the link");
        let (file, _, seen) = opened(&at, name).expect("opened");
        drop(file);
        let held = (Sha256::of(bytes), bytes.len() as u64);
        assert_eq!(aside(&at, name, &seen, held), Ok(Removal::Removed));
        assert_eq!(
            fs::read(outside.join(name)).expect("the outside file stays"),
            bytes
        );
        assert!(
            !folder.join("sub-real").join(name).exists(),
            "the checkpoint is removed"
        );
    }
}
