use std::ffi::OsStr;
use std::fs::{self, File, Metadata};
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::at::Dir;
use crate::identity::{same_file, unchanged, FileId};
use crate::walk::{is_loop, Root, Walk};
use crate::{checkpoint_name, digest, removing_name, LockError, Locks, RunId, Sha256, FILE_MAX};

/// What a row of the list of checkpoints names (contracts.md A7.4): the folder that `prepare`
/// was given and the workbook's path as given to `prepare`, both absolute and with no parent
/// step (any other is refused as `not_file`), the run, and what the copy must hold.
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
    /// No file stood in the copy's place, or a directory on the way below the folder is
    /// absent: `gone`. An absent folder is never this answer but [`Undecided::Folder`].
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
    /// `link`: the folder itself, a directory on the way below it, or the copy itself is a
    /// link.
    Link,
    /// `not_file`: the copy is no regular file, a part of the way is no directory, the folder
    /// is no directory, the row's folder or path is relative or holds a parent step, or the
    /// row's path is not inside the folder (outside it, or the folder itself), which the call
    /// then never reaches.
    NotFile,
    /// `name`: the directory holds no entry whose own name is the built name byte for byte,
    /// though the system answered for it, as a file system that folds case does for a name in
    /// another case; or a part of the path holds a colon, which on Windows names a stream; or,
    /// between the move aside and the removal, the entry under the removing name stopped being
    /// the file that the call moved there, which then stays, and the call removes nothing.
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
    /// The folder is absent, as it is while the volume that holds it is not mounted: the copy
    /// may still lie in it, so the call answers neither `gone` nor `left`.
    Folder,
}

type Pass = Result<Removal, Undecided>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Leftover {
    Clear,
    Finished,
}

/// Removes the checkpoint of `row.run` beside `row.workbook`, inside `row.folder` alone
/// (contracts.md A11.3, A6.6; the coordinator's rulings CP-96 (e), CP-98 (3) and CP-124). It
/// builds the copy's name with [`checkpoint_name`] and never searches a folder for another; it
/// walks from the folder, which is no link itself (a directory above it may be one), to the
/// workbook's directory, takes the lock of that directory, and holds it from its first look at
/// the copy to the removal, so that it never overlaps a `prepare` or a `replace` of the sheet
/// executor.
/// Under the lock it walks the way again part by part, each directory opened without following
/// a link (on macOS by descriptors, each opened relative to the one before; on Windows by
/// handles that it holds open and shares with no deletion, so no directory of the way can be
/// moved or replaced until the call ends), and acts only on entries of the directory that it
/// opened, so a directory of the way that becomes a link is never followed out of the folder.
/// It removes the copy only when all of these hold: no directory on the way below the folder
/// is a link, and the copy is a regular file and no link; the directory holds an entry whose
/// own name is the built name byte for byte; and the copy's length and SHA-256, read under the
/// bound of a file, are the row's ([`Expect::Made`]) or the workbook's as it stands
/// ([`Expect::Planned`]). It then moves the copy aside under [`removing_name`], one name for
/// each checkpoint, checks that the moved entry is the file that it read with the same SHA-256,
/// and removes it; when that check misses or the removal fails, it puts the file back under its
/// own name. Right before the removal it reads the entry under the removing name again, and
/// removes it only while it is the file that the call moved there, compared by
/// [`crate::same_file`]; otherwise it answers `left` with the reason `name` and removes
/// nothing. A limit of A11.3 stays: a second party that, inside one call, gives the moved copy
/// another name in the folder between the move aside and the removal, a hard link at the
/// copy's own name among them, keeps that name and its bytes while the call answers `gone`,
/// because the call removes only the name that it moved the copy to. Before it looks at the
/// copy, it deals with what a call killed between the move
/// aside and the removal left under that name: a file that passes every check of the copy
/// goes, and so does a second name of the copy itself; any other entry stays, and when the
/// copy's own name then holds no entry, the answer is that entry's reason as `left`, so that no
/// answer is `gone` while the bytes lie in the folder. A copy that is absent counts as
/// removed, and so does a directory on the way that is absent; a folder that is absent, as
/// while its volume is not mounted, is [`Undecided::Folder`]. Every other miss leaves the file
/// and answers its reason, which is never a run's failure. It removes no checkpoint of a run
/// that has not ended: the caller calls it only for a run whose end is stored.
///
/// One call makes at most three passes (the coordinator's ruling CP-135 (6)): a pass in which
/// the system refused a step of the removal ([`Undecided::Remove`]) is made again, three in
/// all, and the answer of the last is the call's; a lock that it cannot take within the wait
/// ([`Undecided::Lock`]) and an absent folder ([`Undecided::Folder`]) are answered at once. The
/// caller writes no row for an [`Undecided`] answer and calls again at its next start.
pub fn remove(locks: &Locks, row: &Row<'_>) -> Result<Removal, Undecided> {
    let mut answer = pass(locks, row);
    for _ in 1..PASSES {
        if !matches!(answer, Err(Undecided::Remove(_))) {
            break;
        }
        answer = pass(locks, row);
    }
    answer
}

const PASSES: usize = 3;

fn pass(locks: &Locks, row: &Row<'_>) -> Pass {
    let (dir, last) = match place(row.folder, row.workbook) {
        Ok(place) => place,
        Err(answer) => return answer,
    };
    let Some(name) = checkpoint_name(Path::new(last), row.run) else {
        return Ok(Removal::Left(Left::NotFile));
    };
    let moved = removing_name(&name);
    let held = locks.hold(&dir).map_err(Undecided::Lock)?;
    let answer = match opened_dir(row, &dir) {
        Ok(at) => within(&at, &name, &moved, last, row.expect),
        Err(answer) => answer,
    };
    drop(held);
    answer
}

fn within(at: &Dir, name: &OsStr, moved: &OsStr, workbook: &OsStr, expect: Expect) -> Pass {
    let left = match leftover(at, name, moved, workbook, expect) {
        Ok(left) => left,
        Err(answer) => return answer,
    };
    match checked(at, name, workbook, expect) {
        Ok((file, seen, read)) => {
            drop(file);
            #[cfg(test)]
            crate::seam::reach(crate::seam::Moment::Checked);
            aside(at, name, moved, &seen, read)
        }
        Err(Removal::Absent) if left == Leftover::Finished => Ok(Removal::Removed),
        Err(answer) => Ok(answer),
    }
}

fn leftover(
    at: &Dir,
    name: &OsStr,
    moved: &OsStr,
    workbook: &OsStr,
    expect: Expect,
) -> Result<Leftover, Pass> {
    let found = match opened(at, moved) {
        Err(Removal::Absent) => return Ok(Leftover::Clear),
        Err(answer) => return stays(at, name, Ok(answer)),
        Ok((_, _, found)) => found,
    };
    match own_entry(at, moved) {
        Ok(true) => {}
        Ok(false) => return stays(at, name, Ok(Removal::Left(Left::Name))),
        Err(answer) => return stays(at, name, Ok(answer)),
    }
    if let Ok((_, _, copy)) = opened(at, name) {
        if same_file(&found, &copy) {
            let _ = at.unlink(moved);
            return Ok(Leftover::Clear);
        }
    }
    match checked(at, moved, workbook, expect) {
        Ok((file, seen, _)) if same_file(&found, &seen) => {
            drop(file);
            match at.unlink(moved) {
                Ok(()) => Ok(Leftover::Finished),
                Err(error) => stays(at, name, Err(Undecided::Remove(error.kind()))),
            }
        }
        Ok(_) => stays(at, name, Ok(Removal::Left(Left::Changed))),
        Err(Removal::Absent) => Ok(Leftover::Clear),
        Err(answer) => stays(at, name, Ok(answer)),
    }
}

fn stays(at: &Dir, name: &OsStr, answer: Pass) -> Result<Leftover, Pass> {
    match opened(at, name) {
        Err(Removal::Absent) => Err(answer),
        _ => Ok(Leftover::Clear),
    }
}

fn root(folder: &Path) -> Result<Root, Pass> {
    if !one_spelling(folder) {
        return Err(Ok(Removal::Left(Left::NotFile)));
    }
    let plain: PathBuf = folder.components().collect();
    if fs::symlink_metadata(&plain).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(Ok(Removal::Left(Left::Link)));
    }
    Root::new(&plain).map_err(|walk| match walk {
        Walk::Io(io::ErrorKind::NotFound) => Err(Undecided::Folder),
        walk => Ok(walked(walk)),
    })
}

fn place<'a>(folder: &Path, workbook: &'a Path) -> Result<(PathBuf, &'a OsStr), Pass> {
    let root = root(folder)?;
    if !one_spelling(workbook) {
        return Err(Ok(Removal::Left(Left::NotFile)));
    }
    root.parent(workbook).map_err(|walk| Ok(walked(walk)))
}

fn one_spelling(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
}

/// The workbook's directory walked again under the lock, part by part, and checked to be the
/// directory whose lock the call holds.
fn opened_dir(row: &Row<'_>, dir: &Path) -> Result<Dir, Pass> {
    match place(row.folder, row.workbook) {
        Ok((again, _)) if again == dir => {}
        Ok(_) => return Err(Ok(Removal::Left(Left::Link))),
        Err(answer) => return Err(answer),
    }
    let root = root(row.folder)?;
    let names = root.names(row.workbook).map_err(|walk| Ok(walked(walk)))?;
    let (_, parts) = names.split_last().ok_or(Ok(Removal::Left(Left::NotFile)))?;
    let below = |error: io::Error| match error.kind() {
        io::ErrorKind::NotFound => Removal::Absent,
        io::ErrorKind::NotADirectory => Removal::Left(Left::Link),
        _ if is_loop(&error) => Removal::Left(Left::Link),
        _ => Removal::Left(Left::Unreadable),
    };
    let folder = Dir::walk(root.dir(), &[]).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => Err(Undecided::Folder),
        _ => Ok(below(error)),
    })?;
    let at = folder.below(parts).map_err(|error| Ok(below(error)))?;
    let opened = at.id().map_err(|_| Ok(Removal::Left(Left::Unreadable)))?;
    let named = FileId::at(dir).map_err(|_| Ok(Removal::Left(Left::Link)))?;
    if !same_file(&opened, &named) {
        return Err(Ok(Removal::Left(Left::Link)));
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

fn aside(
    at: &Dir,
    name: &OsStr,
    moved: &OsStr,
    seen: &FileId,
    held: (Sha256, u64),
) -> Result<Removal, Undecided> {
    match at.rename_new(name, moved) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Removal::Absent),
        Err(error) => return Err(Undecided::Remove(error.kind())),
    }
    let still = opened(at, moved).is_ok_and(|(file, _, now)| {
        same_file(seen, &now) && digest::read(&file, FILE_MAX).is_ok_and(|read| read == held)
    });
    if !still {
        put_back(at, moved, name);
        return Ok(Removal::Left(Left::Changed));
    }
    #[cfg(test)]
    crate::seam::reach(crate::seam::Moment::Aside);
    if !opened(at, moved).is_ok_and(|(_, _, now)| same_file(seen, &now)) {
        return Ok(Removal::Left(Left::Name));
    }
    match at.unlink(moved) {
        Ok(()) => Ok(Removal::Removed),
        Err(error) => {
            put_back(at, moved, name);
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
        let dir = halvo_test_dir::make(&target.join("tmp").join("halvo-checkpoint-unit"), name)
            .expect("make the scratch directory");
        dir.canonicalize().expect("the canonical directory")
    }

    #[test]
    fn two_scratches_of_one_name_are_two_directories_and_the_second_is_empty() {
        let first = scratch("one-name");
        fs::write(first.join("mine"), b"first").expect("write into the first");
        let second = scratch("one-name");
        assert_ne!(first, second);
        assert_eq!(fs::read_dir(&second).expect("list").count(), 0);
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
        let answer = aside(&at, name, &removing_name(name), &seen, held);
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
        let moved = removing_name(name);
        assert_eq!(aside(&at, name, &moved, &seen, held), Ok(Removal::Removed));
        assert_eq!(
            fs::read(outside.join(name)).expect("the outside file stays"),
            bytes
        );
        assert!(
            !folder.join("sub-real").join(name).exists(),
            "the checkpoint is removed"
        );
    }

    use crate::seam::{self, Moment};
    use std::io::Write;
    use std::panic::{catch_unwind, AssertUnwindSafe};

    const COPY: &[u8] = b"the workbook as prepare read it";

    struct Case {
        folder: PathBuf,
        copy: PathBuf,
        locks: Locks,
        run: RunId,
    }

    impl Case {
        fn new(name: &str) -> Case {
            let base = scratch(name);
            let folder = base.join("folder");
            fs::create_dir_all(&folder).expect("make the folder");
            fs::write(folder.join("values.xlsx"), b"the workbook now").expect("the workbook");
            let run = RunId::parse("0123456789abcdef0123456789abcdef").expect("a run id");
            let name = checkpoint_name(Path::new("values.xlsx"), &run).expect("a name");
            let copy = folder.join(name);
            fs::write(&copy, COPY).expect("the checkpoint");
            fs::create_dir_all(base.join("data")).expect("make the data directory");
            let wait = std::time::Duration::from_secs(5);
            let locks = Locks::open(&base.join("data"), wait).expect("the locks open");
            Case {
                folder,
                copy,
                locks,
                run,
            }
        }

        fn remove(&self) -> Pass {
            let workbook = self.folder.join("values.xlsx");
            let row = Row {
                folder: &self.folder,
                workbook: &workbook,
                run: &self.run,
                expect: Expect::Made {
                    sha256: Sha256::of(COPY),
                    bytes: COPY.len() as u64,
                },
            };
            remove(&self.locks, &row)
        }

        fn moved(&self) -> PathBuf {
            let name = self.copy.file_name().expect("a name");
            self.folder.join(removing_name(name))
        }

        fn names(&self) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.folder)
                .expect("list")
                .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        }

        fn name(path: &Path) -> String {
            path.file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned()
        }
    }

    #[test]
    fn a_file_moved_into_the_checkpoints_place_after_its_checks_stays_under_its_name() {
        for theirs in [b"a file of the person's own".to_vec(), COPY.to_vec()] {
            let case = Case::new("seam-swap");
            let mine = case.folder.join("mine.xlsx");
            fs::write(&mine, &theirs).expect("the person's file");
            let (from, to) = (mine.clone(), case.copy.clone());
            let got = seam::with(
                move |moment| {
                    if moment == Moment::Checked {
                        fs::rename(&from, &to).expect("move it into the checkpoint's place");
                    }
                },
                || case.remove(),
            );
            assert_eq!(got, Ok(Removal::Left(Left::Changed)));
            assert_eq!(fs::read(&case.copy).expect("the person's file"), theirs);
            let left = vec![Case::name(&case.copy), "values.xlsx".to_string()];
            assert_eq!(case.names(), left);
        }
    }

    #[test]
    fn a_checkpoint_written_in_place_after_its_checks_is_left() {
        let case = Case::new("seam-write");
        let path = case.copy.clone();
        let got = seam::with(
            move |moment| {
                if moment == Moment::Checked {
                    let mut file = fs::OpenOptions::new()
                        .write(true)
                        .open(&path)
                        .expect("open the checkpoint for writing");
                    file.write_all(b"X").expect("write into the checkpoint");
                }
            },
            || case.remove(),
        );
        assert_eq!(got, Ok(Removal::Left(Left::Changed)));
        let mut written = COPY.to_vec();
        written[0] = b'X';
        assert_eq!(fs::read(&case.copy).expect("the checkpoint"), written);
        let left = vec![Case::name(&case.copy), "values.xlsx".to_string()];
        assert_eq!(case.names(), left);
    }

    #[test]
    fn a_removal_whose_every_pass_finds_the_removing_name_is_undecided_after_the_third_pass() {
        let case = Case::new("seam-passes");
        let theirs = b"a file of the person's own under the removing name";
        fs::write(case.moved(), theirs).expect("a file at the removing name");
        let passes = std::rc::Rc::new(std::cell::Cell::new(0));
        let counted = passes.clone();
        let got = seam::with(
            move |moment| {
                if moment == Moment::Checked {
                    counted.set(counted.get() + 1);
                }
            },
            || case.remove(),
        );
        assert_eq!(got, Err(Undecided::Remove(io::ErrorKind::AlreadyExists)));
        assert_eq!(passes.get(), 3, "the passes of one call");
        assert_eq!(fs::read(case.moved()).expect("the person's file"), theirs);
        assert_eq!(fs::read(&case.copy).expect("the checkpoint"), COPY);
    }

    #[test]
    fn a_pass_killed_between_the_move_aside_and_the_removal_is_finished_by_the_next() {
        let case = Case::new("seam-kill");
        let killed = catch_unwind(AssertUnwindSafe(|| {
            seam::with(
                |moment| assert_ne!(moment, Moment::Aside, "the pass is killed"),
                || case.remove(),
            )
        }));
        assert!(killed.is_err(), "the hook stops the pass");
        let mut left = vec![Case::name(&case.moved()), "values.xlsx".to_string()];
        left.sort();
        assert_eq!(case.names(), left);
        assert_eq!(fs::read(case.moved()).expect("the moved copy"), COPY);
        assert_eq!(case.remove(), Ok(Removal::Removed));
        assert_eq!(case.names(), vec!["values.xlsx".to_string()]);
    }

    fn renamed_onto_the_removing_name(theirs: &[u8]) -> (Pass, Option<Vec<u8>>, Vec<String>) {
        let case = Case::new("seam-onto-removing");
        let staged = case.folder.join("staged.xlsx");
        fs::write(&staged, theirs).expect("a file of another party");
        let (from, to) = (staged, case.moved());
        let got = seam::with(
            move |moment| {
                if moment == Moment::Aside {
                    fs::rename(&from, &to).expect("put it under the removing name");
                }
            },
            || case.remove(),
        );
        (got, fs::read(case.moved()).ok(), case.names())
    }

    #[test]
    fn a_file_of_other_bytes_renamed_onto_the_removing_name_after_the_move_aside_stays() {
        let theirs = b"a file of another party";
        let (got, kept, names) = renamed_onto_the_removing_name(theirs);
        assert_eq!(got, Ok(Removal::Left(Left::Name)), "{names:?}");
        assert_eq!(kept.as_deref(), Some(&theirs[..]), "{names:?}");
    }

    #[test]
    fn a_file_of_the_rows_bytes_renamed_onto_the_removing_name_after_the_move_aside_stays() {
        let (got, kept, names) = renamed_onto_the_removing_name(COPY);
        assert_eq!(got, Ok(Removal::Left(Left::Name)), "{names:?}");
        assert_eq!(kept.as_deref(), Some(COPY), "{names:?}");
    }
}
