use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use crate::book::{self, guarded};
use crate::error::{Error, Kept, Refusal};
use crate::parts;
use crate::patch;
use crate::path;
use crate::range::Range;
use crate::same;
use crate::source::Value;
use crate::xml;

/// A run id of the form of contracts.md A0.6: 32 lower-case hex digits. It names the
/// checkpoint and the prepared copies of a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunId(String);

impl RunId {
    /// Takes 32 lower-case hex digits; [`Error::RunId`] for any other text.
    pub fn parse(text: &str) -> Result<RunId, Error> {
        let hex = |b: u8| b.is_ascii_digit() || (b'a'..=b'f').contains(&b);
        if text.len() == 32 && text.bytes().all(hex) {
            Ok(RunId(text.to_string()))
        } else {
            Err(Error::RunId)
        }
    }
}

/// What [`crate::Folder::prepare`] made beside the workbook and what
/// [`crate::Folder::replace`] needs: the checkpoint `<name>.halvo-checkpoint-<run id><ext>`, a
/// copy of the workbook as it was, which all calls of the run share; and a prepared copy of
/// this call's own, `<name>.halvo-prepared-<run id>-<part of its own><ext>`, the workbook with
/// the new cells, which the check of `prepare` read.
///
/// It holds the workbook as `prepare` read it and the prepared copy in memory until it is
/// dropped. Dropping one that no `replace` put in place removes its prepared copy when no link
/// stands on the way to it and it still holds the bytes that `prepare` checked. It never
/// removes the checkpoint: once a run's checkpoint is made, no call of this crate removes it,
/// and it stays until the run's owner removes it at the run's end.
pub struct Prepared {
    pub(crate) root: PathBuf,
    pub(crate) workbook: PathBuf,
    pub(crate) checkpoint: PathBuf,
    pub(crate) copy: PathBuf,
    pub(crate) original: Vec<u8>,
    pub(crate) changed: Vec<u8>,
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

    /// This call's prepared copy, which `replace` renames into the workbook's place.
    pub fn copy(&self) -> &Path {
        &self.copy
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if self.answer.get() == Some(&Ok(())) {
            return;
        }
        let roots = std::slice::from_ref(&self.root);
        let holding = |path: &Path, bytes: &[u8]| {
            path::file(&self.root, roots, path)
                .ok()
                .filter(|full| fs::read(full).is_ok_and(|read| read == bytes))
        };
        if let Some(copy) = holding(&self.copy, &self.changed) {
            let _ = fs::remove_file(copy);
        }
    }
}

fn beside(workbook: &Path, tag: &str) -> PathBuf {
    let mut name = OsString::new();
    name.push(workbook.file_stem().unwrap_or_default());
    name.push(format!(".halvo-{tag}"));
    if let Some(ext) = workbook.extension() {
        name.push(".");
        name.push(ext);
    }
    workbook.with_file_name(name)
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

pub(crate) fn prepare(
    root: &Path,
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
    let meta = fs::metadata(&workbook)?;
    if read_only(&meta) {
        return Err(Error::ReadOnly);
    }
    let original = parts::package_file(&workbook)?;
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
    let checkpoint = beside(&workbook, &format!("checkpoint-{}", run.0));
    keep_checkpoint(&checkpoint, &original, meta.permissions())?;
    let copy = write_copy(&workbook, run, &changed, meta.permissions())?;
    drop(changed);
    let checked = fs::read(&copy).map_err(Error::from).and_then(|disk| {
        check(&original, &disk, (&part, &book_part), sheet, range, values)?;
        Ok(disk)
    });
    match checked {
        Ok(disk) => Ok(Prepared {
            root: root.to_path_buf(),
            workbook,
            checkpoint,
            copy,
            original,
            changed: disk,
            answer: OnceLock::new(),
        }),
        Err(error) => {
            let _ = fs::remove_file(&copy);
            Err(error)
        }
    }
}

pub(crate) fn replace(prepared: &Prepared) -> Result<(), Error> {
    let holds = |path: &Path, bytes: &[u8]| {
        path::regular(path).is_ok() && fs::read(path).is_ok_and(|read| read == bytes)
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

/// Creates `path`, which must not exist, with the bytes and then the workbook's permissions.
/// On Unix the file is made readable by its owner alone until it is full.
fn create(path: &Path, bytes: &[u8], permissions: Permissions) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    let filled = fill(file, bytes).and_then(|()| fs::set_permissions(path, permissions));
    if filled.is_err() {
        let _ = fs::remove_file(path);
    }
    filled
}

fn fill(mut file: File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()
}

/// Makes the run's checkpoint, or takes one of the run that is already there and equal to the
/// workbook without writing it again.
fn keep_checkpoint(
    checkpoint: &Path,
    original: &[u8],
    permissions: Permissions,
) -> Result<(), Error> {
    match create(checkpoint, original, permissions) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            path::regular(checkpoint).map_err(|_| Error::Checkpoint)?;
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

/// Writes the prepared copy under a name that no other call has: the run id, the process and
/// a count of the calls of this process, the count taken again when a file of that name
/// stands there. No file is removed or written over.
fn write_copy(
    workbook: &Path,
    run: &RunId,
    bytes: &[u8],
    permissions: Permissions,
) -> Result<PathBuf, Error> {
    for _ in 0..64 {
        let call = CALLS.fetch_add(1, Ordering::Relaxed);
        let own = format!("{:08x}{call:08x}", std::process::id());
        let copy = beside(workbook, &format!("prepared-{}-{own}", run.0));
        match create(&copy, bytes, permissions.clone()) {
            Ok(()) => return Ok(copy),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
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
        let folder = Folder::new(&dir).expect("the folder opens");
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
            let own = format!("{:08x}{call:08x}", std::process::id());
            let name = beside(&workbook, &format!("prepared-{}-{own}", run.0));
            fs::write(&name, b"a file of another call").expect("write");
            taken.push(name);
        }
        let permissions = fs::metadata(&workbook).expect("metadata").permissions();
        let copy = write_copy(&workbook, &run, b"the copy", permissions).expect("another name");
        assert!(!taken.contains(&copy), "{copy:?}");
        assert_eq!(fs::read(&copy).expect("read"), b"the copy");
        for name in &taken {
            assert_eq!(fs::read(name).expect("read"), b"a file of another call");
        }
        let _ = fs::remove_dir_all(&dir);
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
        let folder = Folder::new(&dir).expect("the folder opens");
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
        let folder = Folder::new(&dir).expect("the folder opens");
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
