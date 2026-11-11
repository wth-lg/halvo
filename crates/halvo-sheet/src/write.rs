use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use calamine::Reader;

use crate::book::{self, guarded, unreadable};
use crate::error::{Error, Kept, PathRefusal, Refusal};
use crate::parts::{self, Inventory};
use crate::path;
use crate::range::Range;
use crate::source::Value;

/// A run id of the form of contracts.md A0.6: 32 lower-case hex digits. It names the
/// checkpoint and the prepared copy of a run.
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
/// copy of the workbook as it was, and the prepared copy `<name>.halvo-prepared-<run id><ext>`,
/// the workbook with the new cells, whose content the sameness check of `prepare` read.
/// Dropping a `Prepared` that no `replace` put in place removes the prepared copy when no
/// link stands on the way to it and it still holds the bytes that `prepare` checked, and
/// leaves the checkpoint.
#[derive(Debug)]
pub struct Prepared {
    pub(crate) root: PathBuf,
    pub(crate) workbook: PathBuf,
    pub(crate) checkpoint: PathBuf,
    pub(crate) copy: PathBuf,
    pub(crate) original: Vec<u8>,
    pub(crate) changed: Vec<u8>,
    pub(crate) answer: OnceLock<Result<(), Error>>,
}

impl Prepared {
    /// The workbook that `replace` replaces.
    pub fn workbook(&self) -> &Path {
        &self.workbook
    }

    /// The checkpoint: a copy of the workbook as `prepare` found it, which stays beside it.
    pub fn checkpoint(&self) -> &Path {
        &self.checkpoint
    }
}

impl Drop for Prepared {
    fn drop(&mut self) {
        if self.answer.get() != Some(&Ok(())) {
            let root = std::slice::from_ref(&self.root);
            if let Ok(copy) = path::file(&self.root, root, &self.copy) {
                if fs::read(&copy).is_ok_and(|bytes| bytes == self.changed) {
                    let _ = fs::remove_file(copy);
                }
            }
        }
    }
}

fn beside(workbook: &Path, tag: &str, run: &RunId) -> PathBuf {
    let mut name = OsString::new();
    name.push(workbook.file_stem().unwrap_or_default());
    name.push(format!(".halvo-{tag}-{}", run.0));
    if let Some(ext) = workbook.extension() {
        name.push(".");
        name.push(ext);
    }
    workbook.with_file_name(name)
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
        Value::Number(_) => true,
        Value::Text(text) => text.chars().all(writable) && text.encode_utf16().count() <= 32_767,
    });
    if !texts_ok {
        return Err(Error::Value);
    }
    let original = fs::read(&workbook)?;
    let inventory = parts::inventory(&parts::entries(&original)?)?;
    let refused = inventory.refused();
    if !refused.is_empty() {
        return Err(Error::Workbook(Refusal::Kinds(refused)));
    }
    let changed = write_cells(&original, sheet, range, values)?;
    let checkpoint = beside(&workbook, "checkpoint", run);
    let copy = beside(&workbook, "prepared", run);
    let made = keep_checkpoint(&checkpoint, &original)?;
    let checked = write_copy(&copy, &changed).and_then(|()| {
        let disk = fs::read(&copy)?;
        check(&original, &inventory, &disk, sheet, range, values)?;
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
            if made {
                let _ = fs::remove_file(&checkpoint);
            }
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

fn write_cells(
    original: &[u8],
    sheet: &str,
    range: Range,
    values: &[Vec<Value>],
) -> Result<Vec<u8>, Error> {
    guarded(|| {
        let xlsx = book::open(original)?;
        if !xlsx.sheet_names().iter().any(|name| name == sheet) {
            return Err(Error::NoSheet);
        }
        let mut workbook = umya_spreadsheet::reader::xlsx::read_reader(Cursor::new(original), true)
            .map_err(|_| unreadable())?;
        let worksheet = workbook
            .sheet_by_name_mut(sheet)
            .map_err(|_| unreadable())?;
        for (row, cells) in (range.first.0..).zip(values) {
            for (col, value) in (range.first.1..).zip(cells) {
                let cell = worksheet.cell_mut((col + 1, row + 1));
                match value {
                    Value::Number(x) => cell.set_value_number(*x),
                    Value::Text(text) => cell.set_value_string(text.clone()),
                };
            }
        }
        let mut out = Vec::new();
        umya_spreadsheet::writer::xlsx::write_writer(&workbook, &mut out)
            .map_err(|_| unreadable())?;
        Ok(out)
    })
}

fn fill(mut file: File, bytes: &[u8]) -> io::Result<()> {
    file.write_all(bytes)?;
    file.sync_all()
}

fn keep_checkpoint(checkpoint: &Path, original: &[u8]) -> Result<bool, Error> {
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(checkpoint)
    {
        Ok(file) => match fill(file, original) {
            Ok(()) => Ok(true),
            Err(error) => {
                let _ = fs::remove_file(checkpoint);
                Err(error.into())
            }
        },
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            path::regular(checkpoint).map_err(|_| Error::Checkpoint)?;
            if fs::read(checkpoint)? == original {
                Ok(false)
            } else {
                Err(Error::Checkpoint)
            }
        }
        Err(error) => Err(error.into()),
    }
}

fn write_copy(copy: &Path, bytes: &[u8]) -> Result<(), Error> {
    match fs::symlink_metadata(copy) {
        Ok(meta) if meta.is_dir() => return Err(Error::Path(PathRefusal::Directory)),
        Ok(_) => fs::remove_file(copy)?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let file = OpenOptions::new().write(true).create_new(true).open(copy)?;
    fill(file, bytes)?;
    Ok(())
}

fn check(
    original: &[u8],
    inventory: &Inventory,
    disk: &[u8],
    sheet: &str,
    range: Range,
    values: &[Vec<Value>],
) -> Result<(), Error> {
    let same = guarded(|| book::same(original, disk, sheet, range, values));
    if same != Ok(true) {
        return Err(Error::Sheet);
    }
    let saved = parts::entries(disk).and_then(|all| parts::inventory(&all));
    match saved {
        Ok(saved) if parts::lost(inventory, &saved).is_empty() => Ok(()),
        _ => Err(Error::Sheet),
    }
}
