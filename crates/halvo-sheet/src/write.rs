use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use halvo_checkpoint::{
    checkpoint_name, name_units, names_fit, open_regular, own_name, regular, same_entry, LockError,
    Locks, Mark, Marked, Root, RunId, Sha256, NAME_MAX,
};

use crate::book::{self, guarded};
use crate::carry;
use crate::error::{Error, Kept, PathRefusal, Refusal};
use crate::parts;
use crate::patch;
use crate::range::Range;
use crate::same;
use crate::source::Value;
use crate::xml;

/// What [`crate::Folder::prepare`] made beside the workbook and what
/// [`crate::Folder::replace`] needs: the checkpoint `<name>.halvo-checkpoint-<run id><ext>`, a
/// copy of the workbook as it was, which all calls of the run share; and a prepared copy of
/// this call's own, `<name>.halvo-prepared-<run id>-<part of its own>-<digest><ext>`, the
/// workbook with the new cells, which the check of `prepare` read; `<digest>` is the first 16
/// hex digits of the copy's SHA-256.
///
/// It holds the workbook as `prepare` read it and the prepared copy in memory until it is
/// dropped, and the copy's mark in `agents/locks/` (a shared lock of the system on a file of
/// Halvo's own, no handle on a file of the person's folder), so that the sweep of another
/// `prepare` leaves the copy. Dropping one that no `replace` put in place removes its prepared
/// copy when no link stands on the way to it and it still holds the bytes that `prepare`
/// checked, also when the access list that it carried forbids that on macOS. It never
/// removes the checkpoint: once a run's checkpoint is made, no call of this crate removes it;
/// `remove` of the crate halvo-checkpoint removes it once the run has ended.
pub struct Prepared {
    pub(crate) root: Root,
    pub(crate) workbook: PathBuf,
    pub(crate) checkpoint: PathBuf,
    pub(crate) copy: PathBuf,
    pub(crate) original: Vec<u8>,
    pub(crate) changed: Vec<u8>,
    pub(crate) sha256: Sha256,
    pub(crate) unmarked: usize,
    pub(crate) _mark: Mark,
    pub(crate) answer: OnceLock<Result<(), Error>>,
}

impl fmt::Debug for Prepared {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Prepared")
            .field("answer", &self.answer.get())
            .finish_non_exhaustive()
    }
}

impl Prepared {
    /// The workbook that `replace` replaces.
    pub fn workbook(&self) -> &Path {
        &self.workbook
    }

    /// The checkpoint: a copy of the workbook as `prepare` found it, which stays beside it
    /// after a `replace`.
    pub fn checkpoint(&self) -> &Path {
        &self.checkpoint
    }

    /// The checkpoint's file name, `<name>.halvo-checkpoint-<run id><ext>`, as the crate
    /// halvo-checkpoint builds it; the runner writes it into the list's row (contracts.md A7.4).
    pub fn checkpoint_name(&self) -> &OsStr {
        self.checkpoint.file_name().unwrap_or_default()
    }

    /// The SHA-256 of the checkpoint, which is the workbook as `prepare` read it.
    pub fn checkpoint_sha256(&self) -> Sha256 {
        self.sha256
    }

    /// The length of the checkpoint in bytes.
    pub fn checkpoint_bytes(&self) -> u64 {
        self.original.len() as u64
    }

    /// This call's prepared copy, which `replace` renames into the workbook's place.
    pub fn copy(&self) -> &Path {
        &self.copy
    }

    /// How many entries beside the workbook the sweep of this call left because they have no
    /// mark in `agents/locks/`: each has a prepared name of this workbook byte for byte, is a
    /// regular file and no link, and holds bytes whose digest is the one that its name says.
    /// Such an entry was made by a call of another data directory, or by a call that made no
    /// mark; the sweep never removes it.
    pub fn unmarked(&self) -> usize {
        self.unmarked
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if self.answer.get() == Some(&Ok(())) {
            return;
        }
        let Ok(copy) = self.root.file(&self.copy) else {
            return;
        };
        if let Ok(file) = open_regular(&copy) {
            if same_bytes(&file, &self.changed) {
                let _ = remove_made(&file, &copy);
            }
        }
    }
}

fn beside(workbook: &Path, tag: &str) -> PathBuf {
    workbook.with_file_name(own_name(workbook, tag).unwrap_or_default())
}

/// Whether the person cannot write the file: read-only, or on Unix no write bit for its owner.
fn read_only(meta: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o200 == 0 {
            return true;
        }
    }
    meta.permissions().readonly()
}

pub(crate) fn range_cells(range: Range) -> u64 {
    u64::from(range.rows()) * u64::from(range.cols())
}

pub(crate) fn directory(workbook: &Path) -> &Path {
    workbook.parent().unwrap_or(workbook)
}

pub(crate) fn prepare(
    root: &Root,
    locks: &Locks,
    workbook: PathBuf,
    run: &RunId,
    sheet: &str,
    range: Range,
    values: &[Vec<Value>],
) -> Result<Prepared, Error> {
    let shape = values.len() == range.rows() as usize
        && values.iter().all(|row| row.len() == range.cols() as usize);
    if !shape {
        return Err(Error::Shape);
    }
    let writable = |c: char| {
        !(c < '\u{20}' && !matches!(c, '\t' | '\n' | '\r') || c == '\u{fffe}' || c == '\u{ffff}')
    };
    let texts_ok = values.iter().flatten().all(|value| match value {
        Value::Number(x) => x.is_finite(),
        Value::Text(text) => text.chars().all(writable) && text.encode_utf16().count() <= 32_767,
    });
    if !texts_ok {
        return Err(Error::Value);
    }
    if read_only(&fs::metadata(&workbook)?) {
        return Err(Error::ReadOnly);
    }
    if fs::metadata(&workbook)?.len() > parts::FILE_MAX {
        return Err(parts::size());
    }
    let original = parts::package_of(&open_regular(&workbook)?)?;
    let entries = parts::entries(&original)?;
    let inventory = parts::inventory(&entries)?;
    let refused = inventory.refused();
    if !refused.is_empty() {
        return Err(Error::Workbook(Refusal::Kinds(refused)));
    }
    let part = parts::part_of(&entries, sheet)?;
    let book_part = parts::workbook_name(&entries)?;
    let source = patch::text(entries.get(&part).ok_or_else(book::unreadable)?)?;
    if !xml::fits(xml::extent(source)?, Some(range), parts::CELLS_MAX) {
        return Err(parts::size());
    }
    let changed = patch::write(&original, &entries, &part, range, values)?;
    match parts::within_bounds(&changed) {
        Err(error) if error == parts::size() => return Err(Error::FillSize),
        other => other?,
    }
    let changed = spoiled(changed);
    drop(entries);
    book::readable(&original, sheet)?;
    let checkpoint = workbook.with_file_name(checkpoint_name(&workbook, run).unwrap_or_default());
    let held = locks.hold(directory(&workbook)).map_err(Error::Lock)?;
    if root.file(&workbook)? != workbook {
        return Err(Error::Path(PathRefusal::Link));
    }
    let book = open_regular(&workbook)?;
    if !same_bytes(&book, &original) {
        return Err(Error::Io(io::ErrorKind::Interrupted));
    }
    let meta = book.metadata()?;
    let made = Made {
        book: &book,
        workbook: &workbook,
        permissions: meta.permissions(),
    };
    let unmarked = sweep(&workbook, locks);
    keep_checkpoint(&checkpoint, &original, &made)?;
    let (copy, mark, mut file) = write_copy(&made, run, &changed, locks)?;
    drop(held);
    drop(changed);
    let checked = read_back(&mut file).map_err(Error::from).and_then(|disk| {
        check(&original, &disk, (&part, &book_part), sheet, range, values)?;
        Ok(disk)
    });
    match checked {
        Ok(disk) => Ok(Prepared {
            root: root.clone(),
            workbook,
            checkpoint,
            copy,
            sha256: Sha256::of(&original),
            original,
            changed: disk,
            unmarked,
            _mark: mark,
            answer: OnceLock::new(),
        }),
        Err(error) => {
            let _ = remove_made(&file, &copy);
            drop(file);
            drop(mark);
            Err(error)
        }
    }
}

/// Whether every name that `prepare` builds for `workbook`, and that the removal of the
/// checkpoint of `run` builds, holds at most [`NAME_MAX`] units as the crate counts them: the checkpoint,
/// the name that the removal moves it to, and the prepared copy with the longest part of its
/// own.
pub(crate) fn names_fit_for(workbook: &Path, run: &RunId) -> bool {
    let copy = prepared_name(workbook, run, &own_part(u64::MAX), &Sha256::of(&[]));
    names_fit(workbook, run) && name_units(copy.file_name().unwrap_or_default()) <= NAME_MAX
}

fn read_back(file: &mut File) -> io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(0))?;
    let mut bytes = Vec::new();
    file.take(parts::FILE_MAX + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Whether the file that `file` has open, read from where the handle stands, holds `bytes` and
/// nothing more, read in chunks under the bound of a file.
fn same_bytes(file: &File, bytes: &[u8]) -> bool {
    let mut reader = file.take(parts::FILE_MAX + 1);
    let mut chunk = vec![0u8; 1 << 16];
    let mut at = 0;
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return at == bytes.len(),
            Ok(count) => {
                if bytes.get(at..at + count) != Some(&chunk[..count]) {
                    return false;
                }
                at += count;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return false,
        }
    }
}

/// The workbook that `prepare` read, through the handle that read it, and the permissions that
/// each file that `prepare` makes gets.
struct Made<'a> {
    book: &'a File,
    workbook: &'a Path,
    permissions: Permissions,
}

pub(crate) fn replace(prepared: &Prepared) -> Result<(), Error> {
    let holds = |path: &Path, bytes: &[u8]| {
        regular(path).is_ok() && fs::read(path).is_ok_and(|read| read == bytes)
    };
    if !holds(&prepared.workbook, &prepared.original) {
        return Err(Error::NotReplaced(Kept::Workbook));
    }
    if !holds(&prepared.checkpoint, &prepared.original) {
        return Err(Error::NotReplaced(Kept::Checkpoint));
    }
    if !holds(&prepared.copy, &prepared.changed) {
        return Err(Error::NotReplaced(Kept::Copy));
    }
    fs::rename(&prepared.copy, &prepared.workbook)
        .map_err(|error| Error::NotReplaced(Kept::Rename(error.kind())))
}

/// Makes the file `path`, which must not exist, writes `bytes` into it and gives it the
/// workbook's permissions and what the workbook holds besides its bytes, each through the
/// handle that made it, and returns that handle. When a step fails it removes the file that it
/// made.
fn create(path: &Path, bytes: &[u8], made: &Made<'_>) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.access_mode(carry::MADE_ACCESS);
    }
    let mut file = options.open(path)?;
    let filled = fill(&mut file, bytes)
        .and_then(|()| file.set_permissions(made.permissions.clone()))
        .and_then(|()| carry::carry(made.book, made.workbook, &file, path));
    match filled {
        Ok(()) => Ok(file),
        Err(error) => {
            let _ = remove_made(&file, path);
            Err(error)
        }
    }
}

/// Removes the file at `path` when it is the file that `made` has open, a regular file and no
/// link; when the access list that the file carried forbids the removal on macOS, it empties
/// that list through `made` and tries once more. `NotFound` when another entry stands there.
pub(crate) fn remove_made(made: &File, path: &Path) -> io::Result<()> {
    let ours = || {
        if same_entry(made, path) {
            Ok(())
        } else {
            Err(io::Error::from(io::ErrorKind::NotFound))
        }
    };
    ours()?;
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            carry::uncarry(made)?;
            ours()?;
            fs::remove_file(path)
        }
        other => other,
    }
}

fn fill(file: &mut File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()
}

/// Makes the run's checkpoint, or takes one of the run that is already there and equal to the
/// workbook without writing it again.
fn keep_checkpoint(checkpoint: &Path, original: &[u8], made: &Made<'_>) -> Result<(), Error> {
    match create(checkpoint, original, made) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            regular(checkpoint).map_err(|_| Error::Checkpoint)?;
            if fs::read(checkpoint)? == original {
                Ok(())
            } else {
                Err(Error::Checkpoint)
            }
        }
        Err(error) => Err(error.into()),
    }
}

static CALLS: AtomicU64 = AtomicU64::new(0);

const PREPARED: &str = "prepared-";

fn prepared_name(workbook: &Path, run: &RunId, own: &str, sha256: &Sha256) -> PathBuf {
    let digest = sha256.hex();
    beside(
        workbook,
        &format!("{PREPARED}{}-{own}-{}", run.as_str(), &digest[..16]),
    )
}

fn prepared_digest(workbook: &Path, name: &OsStr) -> Option<String> {
    let template = own_name(workbook, PREPARED)?;
    let template = template.as_encoded_bytes();
    let stem_end = template.len() - workbook.extension().map_or(0, |ext| ext.len() + 1);
    let (head, tail) = template.split_at(stem_end);
    let name = name.as_encoded_bytes();
    let middle = name.strip_prefix(head)?.strip_suffix(tail)?;
    let hex = |part: &[u8]| {
        part.iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    };
    let fits = middle.len() == 32 + 1 + 16 + 1 + 16
        && middle[32] == b'-'
        && middle[49] == b'-'
        && hex(&middle[..32])
        && hex(&middle[33..49])
        && hex(&middle[50..]);
    fits.then(|| String::from_utf8_lossy(&middle[50..]).into_owned())
}

/// Removes the prepared copies of `workbook` that a killed call left, and answers how many it
/// left because they have no mark ([`Prepared::unmarked`]). It removes an entry only when all
/// of these hold: its own name is a prepared name of this workbook byte for byte; it is a
/// regular file and no link; `agents/locks/` holds a mark for this directory and this name that
/// no live call holds; and its bytes have the digest that its name says. The copy goes first,
/// then its mark; a mark whose copy stays, stays too.
fn sweep(workbook: &Path, locks: &Locks) -> usize {
    let dir = directory(workbook);
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut unmarked = 0;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(digest) = prepared_digest(workbook, &name) else {
            continue;
        };
        let path = dir.join(&name);
        match locks.marked(dir, &name) {
            Ok(Marked::Left(mark)) => match own_digest(&path, &digest) {
                Some(file) if remove_made(&file, &path).is_ok() => {
                    drop(file);
                    drop(mark);
                }
                _ => mark.keep(),
            },
            Ok(Marked::Unmarked) => {
                if own_digest(&path, &digest).is_some() {
                    unmarked += 1;
                }
            }
            Ok(Marked::Live) | Err(_) => {}
        }
    }
    unmarked
}

/// The entry at `path` opened without following a link, when it is a regular file whose bytes
/// have the digest `digest` that its name says.
fn own_digest(path: &Path, digest: &str) -> Option<File> {
    let file = open_regular(path).ok()?;
    let own = Sha256::of_reader(&file, parts::FILE_MAX)
        .is_ok_and(|(held, count)| count <= parts::FILE_MAX && held.hex().starts_with(digest));
    own.then_some(file)
}

fn own_part(call: u64) -> String {
    format!("{:08x}{:08x}", std::process::id(), call & 0xffff_ffff)
}

/// Writes the prepared copy under a name that no other call has: the run id, the process and
/// a count of the calls of this process, the count taken again when a file of that name
/// stands. It makes the copy's mark first and then the copy; when the copy cannot be made, the
/// mark goes.
fn write_copy(
    made: &Made<'_>,
    run: &RunId,
    bytes: &[u8],
    locks: &Locks,
) -> Result<(PathBuf, Mark, File), Error> {
    let sha256 = Sha256::of(bytes);
    let dir = directory(made.workbook);
    for _ in 0..64 {
        let own = own_part(CALLS.fetch_add(1, Ordering::Relaxed));
        let copy = prepared_name(made.workbook, run, &own, &sha256);
        let mark = match locks.mark(dir, copy.file_name().unwrap_or_default()) {
            Ok(mark) => mark,
            Err(LockError::Busy) => continue,
            Err(error) => return Err(Error::Lock(error)),
        };
        match create(&copy, bytes, made) {
            Ok(file) => return Ok((copy, mark, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                drop(mark);
                continue;
            }
            Err(error) => {
                drop(mark);
                return Err(error.into());
            }
        }
    }
    Err(Error::Io(io::ErrorKind::AlreadyExists))
}

fn check(
    original: &[u8],
    disk: &[u8],
    written: (&str, &str),
    sheet: &str,
    range: Range,
    values: &[Vec<Value>],
) -> Result<(), Error> {
    if !same::package(original, disk, written, range) {
        return Err(Error::Sheet);
    }
    if guarded(|| book::holds(disk, sheet, range, values)) != Ok(true) {
        return Err(Error::Sheet);
    }
    Ok(())
}

#[cfg(test)]
pub(crate) type Seam = Box<dyn Fn(&str, Vec<u8>) -> Vec<u8>>;

#[cfg(test)]
thread_local! {
    /// A change that a unit test makes to each entry of the prepared copy before it is written,
    /// so that the check meets a copy that the writer would not make.
    pub(crate) static SPOIL: std::cell::RefCell<Option<Seam>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn spoiled(bytes: Vec<u8>) -> Vec<u8> {
    SPOIL.with(|seam| match seam.borrow().as_ref() {
        None => bytes,
        Some(change) => {
            let all: Vec<(String, Vec<u8>)> = crate::testing::ordered(&bytes)
                .into_iter()
                .map(|(name, content)| {
                    let content = change(&name, content);
                    (name, content)
                })
                .collect();
            crate::testing::zipped_in_order(&all)
        }
    })
}

#[cfg(not(test))]
fn spoiled(bytes: Vec<u8>) -> Vec<u8> {
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus_file, scratch};
    use crate::{Cause, Folder};

    const SHEET: &str = "xl/worksheets/sheet1.xml";
    const RUN: &str = "0123456789abcdef0123456789abcdef";

    /// CP-134 H1: the bound of the names holds both markers. For each length of the workbook's
    /// name around the bound, `names_fit_for` answers as the longest name that the two crates
    /// build: the checkpoint, the name that `remove` moves it to, and the prepared copy with the
    /// longest part of its own. A marker that grows while the bound does not read its name makes
    /// the two answers differ.
    #[test]
    fn the_bound_of_the_names_holds_the_checkpoint_the_removing_and_the_prepared_marker() {
        let run = RunId::parse(RUN).expect("a run id");
        let mut flips = Vec::new();
        for units in 168..=180usize {
            let workbook = PathBuf::from(format!("dir/{}.xlsx", "a".repeat(units - 5)));
            let checkpoint = checkpoint_name(&workbook, &run).expect("a checkpoint name");
            let removing = halvo_checkpoint::removing_name(&checkpoint, u64::MAX);
            let prepared = prepared_name(&workbook, &run, &own_part(u64::MAX), &Sha256::of(&[]));
            let longest = [
                name_units(&checkpoint),
                name_units(&removing),
                name_units(prepared.file_name().expect("a name")),
            ]
            .into_iter()
            .max()
            .unwrap_or_default();
            let fits = names_fit_for(&workbook, &run);
            assert_eq!(
                fits,
                longest <= NAME_MAX,
                "{units} units: longest {longest}"
            );
            if !fits {
                flips.push(units);
            }
        }
        assert_eq!(flips.first(), Some(&174), "the first length refused");
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("list")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn edit_part(part: &'static str, from: &'static str, to: String) -> Seam {
        Box::new(move |name, content| {
            if name != part {
                return content;
            }
            let text = String::from_utf8(content).expect("utf-8");
            assert_eq!(text.matches(from).count(), 1, "{part} holds {from} once");
            text.replacen(from, &to, 1).into_bytes()
        })
    }

    type Seen = (Result<(), Error>, Vec<String>, bool);

    fn with_seam(file: &str, seam: Seam, cells: &str, values: &[Vec<Value>]) -> Seen {
        let dir = scratch("check-seam");
        fs::copy(corpus_file(file), dir.join(file)).expect("copy");
        let before = fs::read(dir.join(file)).expect("read");
        let folder = Folder::new(&dir, crate::testing::locks()).expect("the folder opens");
        let run = RunId::parse(RUN).expect("a run id");
        let range = Range::parse(cells).expect("a range");
        SPOIL.with(|s| *s.borrow_mut() = Some(seam));
        let got = folder.prepare(Path::new(file), &run, "Values", range, values);
        SPOIL.with(|s| *s.borrow_mut() = None);
        let listing = names(&dir);
        let kept = fs::read(dir.join(file)).expect("read") == before;
        let answer = got.map(drop);
        let _ = fs::remove_dir_all(&dir);
        (answer, listing, kept)
    }

    fn missed(label: &str, seam: Seam, cells: &str, values: &[Vec<Value>]) {
        let (got, listing, kept) = with_seam("values.xlsx", seam, cells, values);
        let got = got.err();
        assert_eq!(got, Some(Error::Sheet), "{label}");
        assert_eq!(got.map(|e| e.cause()), Some(Cause::Sheet), "{label}");
        let checkpoint = format!("values.halvo-checkpoint-{RUN}.xlsx");
        let left = vec![checkpoint, "values.xlsx".to_string()];
        assert_eq!(
            listing, left,
            "{label}: the checkpoint alone beside the workbook"
        );
        assert!(kept, "{label}: the workbook is as it was");
    }

    fn n(x: f64) -> Vec<Vec<Value>> {
        vec![vec![Value::Number(x)]]
    }

    #[test]
    fn a_copy_that_the_seam_leaves_as_written_passes() {
        let keep: Seam = Box::new(|_, content| content);
        let (got, listing, kept) = with_seam("values.xlsx", keep, "B5", &n(77.0));
        assert_eq!(got, Ok(()));
        assert_eq!(listing.len(), 3, "{listing:?}");
        assert!(kept);
    }

    #[test]
    fn a_named_cell_out_of_order_in_another_row_twice_or_in_a_row_with_an_attribute_is_a_miss() {
        let b5 = r#"<c r="B5"><v>77</v></c>"#;
        let c5 = r#"<c r="C5"><v>5</v></c>"#;
        let pair = r#"<c r="B5"><v>77</v></c><c r="C5"><v>5</v></c>"#;
        missed(
            "order",
            edit_part(SHEET, pair, format!("{c5}{b5}")),
            "B5",
            &n(77.0),
        );
        let wrongrow: Seam = Box::new(move |name, content| {
            if name != SHEET {
                return content;
            }
            let text = String::from_utf8(content)
                .expect("utf-8")
                .replacen(b5, "", 1);
            let at_c3 = format!(r#"{b5}<c r="C3">"#);
            text.replacen(r#"<c r="C3">"#, &at_c3, 1).into_bytes()
        });
        missed("wrongrow", wrongrow, "B5", &n(77.0));
        missed(
            "twice",
            edit_part(SHEET, b5, format!("{b5}{b5}")),
            "B5",
            &n(77.0),
        );
        let hidden = r#"<row r="30" hidden="1">"#.to_string();
        missed(
            "rowattr",
            edit_part(SHEET, r#"<row r="30">"#, hidden),
            "A30",
            &n(1.0),
        );
    }

    #[test]
    fn a_dimension_set_short_dropped_or_with_another_attribute_or_short_spans_are_a_miss() {
        let cases: [(&str, String); 3] = [
            ("dim", r#"<dimension ref="A1"/>"#.to_string()),
            ("dropped", String::new()),
            (
                "another attribute",
                r#"<dimension ref="A1:F21" x="1"/>"#.to_string(),
            ),
        ];
        for (label, to) in cases {
            let seam = edit_part(SHEET, r#"<dimension ref="A1:F21"/>"#, to);
            missed(label, seam, "F2", &n(1.0));
        }
        let short = r#"<row r="2" spans="1:4">"#.to_string();
        missed(
            "spans short",
            edit_part(SHEET, r#"<row r="2" spans="1:6">"#, short),
            "F2",
            &n(1.0),
        );
    }

    #[test]
    fn a_named_cell_that_does_not_read_back_holds_a_formula_or_cannot_be_read_is_a_miss() {
        let b5 = r#"<c r="B5"><v>77</v></c>"#;
        let cases: [(&str, String); 3] = [
            ("another value", r#"<c r="B5"><v>78</v></c>"#.to_string()),
            (
                "a formula",
                r#"<c r="B5"><f>70+7</f><v>77</v></c>"#.to_string(),
            ),
            (
                "unreadable",
                r#"<c r="B5" t="zz"><v>77</v></c>"#.to_string(),
            ),
        ];
        for (label, to) in cases {
            missed(label, edit_part(SHEET, b5, to), "B5", &n(77.0));
        }
        let c5 = r#"<c r="C5"><v>3</v></c>"#.to_string();
        let second = edit_part(SHEET, r#"<c r="C5"><v>2</v></c>"#, c5);
        let two = vec![vec![Value::Number(1.0), Value::Number(2.0)]];
        missed("the second of two cells", second, "B5:C5", &two);
        let empty = r#"<c r="C5" t="inlineStr"><is><t xml:space="preserve"></t></is></c>"#;
        let gone = edit_part(SHEET, empty, String::new());
        let with_empty = vec![vec![Value::Number(1.0), Value::Text(String::new())]];
        missed(
            "a named cell of the empty text missing",
            gone,
            "B5:C5",
            &with_empty,
        );
    }

    #[test]
    fn a_copy_name_that_stands_there_makes_the_call_take_another() {
        let dir = scratch("copy-name-taken");
        let workbook = dir.join("values.xlsx");
        fs::copy(corpus_file("values.xlsx"), &workbook).expect("copy");
        let run = RunId::parse(RUN).expect("a run id");
        let next = CALLS.load(Ordering::Relaxed);
        let mut taken = Vec::new();
        for call in next..next + 32 {
            let name = prepared_name(&workbook, &run, &own_part(call), &Sha256::of(b"the copy"));
            fs::write(&name, b"a file of another call").expect("write");
            taken.push(name);
        }
        let permissions = fs::metadata(&workbook).expect("metadata").permissions();
        let locks = crate::testing::locks();
        let book = File::open(&workbook).expect("open the workbook");
        let made = Made {
            book: &book,
            workbook: &workbook,
            permissions,
        };
        let (copy, mark, file) =
            write_copy(&made, &run, b"the copy", &locks).expect("another name");
        drop(file);
        drop(mark);
        assert!(!taken.contains(&copy), "{copy:?}");
        assert_eq!(fs::read(&copy).expect("read"), b"the copy");
        for name in &taken {
            assert_eq!(fs::read(name).expect("read"), b"a file of another call");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_prepared_name_is_read_back_only_in_its_own_form_for_its_own_workbook() {
        let run = RunId::parse(RUN).expect("a run id");
        let digest = Sha256::of(b"bytes");
        let own = "00000001000000ff";
        let workbook = Path::new("dir/values.xlsx");
        let name = prepared_name(workbook, &run, own, &digest);
        let file = name.file_name().expect("a name");
        assert_eq!(
            prepared_digest(workbook, file),
            Some(digest.hex()[..16].to_string())
        );
        let hex16 = &digest.hex()[..16];
        for other in [
            format!("Values.halvo-prepared-{RUN}-{own}-{hex16}.xlsx"),
            format!("values.halvo-prepared-{RUN}-{own}-{hex16}.XLSX"),
            format!("budget.halvo-prepared-{RUN}-{own}-{hex16}.xlsx"),
            format!("values.halvo-prepared-{RUN}-{own}.xlsx"),
            format!("values.halvo-prepared-{RUN}-{own}-{hex16}0.xlsx"),
            format!("values.halvo-prepared-{RUN}_{own}-{hex16}.xlsx"),
            format!(
                "values.halvo-prepared-{}-{own}-{hex16}.xlsx",
                RUN.to_uppercase()
            ),
            format!("values.halvo-checkpoint-{RUN}.xlsx"),
            format!("values.halvo-prepared-{RUN}-{own}-{hex16}.xlsx.tmp"),
        ] {
            let got = prepared_digest(workbook, OsStr::new(&other));
            assert_eq!(got, None, "{other}");
        }
        let bare = Path::new("book");
        let name = prepared_name(bare, &run, own, &digest);
        let file = name.file_name().expect("a name");
        assert_eq!(
            prepared_digest(bare, file),
            Some(hex16.to_string()),
            "a workbook with no extension"
        );
    }

    #[test]
    fn a_workbook_part_left_without_its_flag_or_changed_past_the_flag_is_a_miss() {
        let book = "xl/workbook.xml";
        let set = r#"<calcPr calcId="124519" fullCalcOnLoad="1"/>"#;
        let changed = r#"<calcPr calcId="1" fullCalcOnLoad="1"/>"#.to_string();
        missed(
            "another calcId",
            edit_part(book, set, changed),
            "B5",
            &n(77.0),
        );
        let unset = r#"<calcPr calcId="124519"/>"#.to_string();
        missed("the flag gone", edit_part(book, set, unset), "B5", &n(77.0));
    }

    #[test]
    fn a_copy_that_the_check_finds_changed_removes_only_what_this_call_made() {
        let dir = scratch("check-miss");
        fs::copy(corpus_file("values.xlsx"), dir.join("values.xlsx")).expect("copy");
        let before = fs::read(dir.join("values.xlsx")).expect("read");
        let folder = Folder::new(&dir, crate::testing::locks()).expect("the folder opens");
        let run = RunId::parse(RUN).expect("a run id");
        let b5 = Range::parse("B5").expect("a range");
        let values = n(77.0);
        let spoiled_prepare = || {
            let seam = edit_part("[Content_Types].xml", "</Types>", " </Types>".to_string());
            SPOIL.with(|s| *s.borrow_mut() = Some(seam));
            let got = folder.prepare(Path::new("values.xlsx"), &run, "Values", b5, &values);
            SPOIL.with(|s| *s.borrow_mut() = None);
            got
        };
        let checkpoint = format!("values.halvo-checkpoint-{RUN}.xlsx");
        let left = vec![checkpoint.clone(), "values.xlsx".to_string()];
        let got = spoiled_prepare();
        assert_eq!(got.as_ref().err(), Some(&Error::Sheet));
        assert_eq!(
            names(&dir),
            left,
            "the miss removes its copy and leaves the checkpoint"
        );
        assert_eq!(fs::read(dir.join("values.xlsx")).expect("read"), before);
        assert_eq!(fs::read(dir.join(&checkpoint)).expect("read"), before);
        let held = folder
            .prepare(Path::new("values.xlsx"), &run, "Values", b5, &values)
            .expect("prepared");
        assert_eq!(spoiled_prepare().err(), Some(Error::Sheet));
        let listing = names(&dir);
        assert_eq!(listing.len(), 3, "{listing:?}");
        assert!(
            listing.contains(&checkpoint),
            "the miss leaves the checkpoint"
        );
        drop(held);
        assert_eq!(
            names(&dir),
            left,
            "the drop removes its copy and leaves the checkpoint"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_debug_form_of_a_prepared_copy_and_of_the_folder_names_no_path_and_no_byte() {
        let dir = scratch("debug-form");
        fs::copy(corpus_file("values.xlsx"), dir.join("values.xlsx")).expect("copy");
        let folder = Folder::new(&dir, crate::testing::locks()).expect("the folder opens");
        let run = RunId::parse(RUN).expect("a run id");
        let b5 = Range::parse("B5").expect("a range");
        let prepared = folder
            .prepare(Path::new("values.xlsx"), &run, "Values", b5, &n(77.0))
            .expect("prepared");
        for shown in [format!("{prepared:?}"), format!("{folder:?}")] {
            let names_a_path = shown.contains("values") || shown.contains("debug-form");
            assert!(
                !names_a_path && !shown.contains('/') && !shown.contains('\\'),
                "{shown}"
            );
            assert!(shown.len() < 60, "{shown}");
        }
        drop(prepared);
        let _ = fs::remove_dir_all(&dir);
    }
}
