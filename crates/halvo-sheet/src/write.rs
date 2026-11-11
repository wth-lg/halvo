use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::book::{self, guarded};
use crate::error::{Error, Kept, PathRefusal, Refusal};
use crate::parts;
use crate::patch;
use crate::path;
use crate::range::Range;
use crate::same;
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
        Value::Number(x) => x.is_finite(),
        Value::Text(text) => text.chars().all(writable) && text.encode_utf16().count() <= 32_767,
    });
    if !texts_ok {
        return Err(Error::Value);
    }
    let original = fs::read(&workbook)?;
    let entries = parts::entries(&original)?;
    let inventory = parts::inventory(&entries)?;
    let refused = inventory.refused();
    if !refused.is_empty() {
        return Err(Error::Workbook(Refusal::Kinds(refused)));
    }
    let part = patch::part_of(&entries, sheet)?;
    let changed = spoiled(patch::write(&original, &entries, &part, range, values)?);
    let checkpoint = beside(&workbook, "checkpoint", run);
    let copy = beside(&workbook, "prepared", run);
    let made = keep_checkpoint(&checkpoint, &original)?;
    let checked = write_copy(&copy, &changed).and_then(|()| {
        let disk = fs::read(&copy)?;
        check(&original, &disk, &part, sheet, range, values)?;
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
    disk: &[u8],
    part: &str,
    sheet: &str,
    range: Range,
    values: &[Vec<Value>],
) -> Result<(), Error> {
    let cells: Vec<(u32, u32)> = range.cells().collect();
    if !same::package(original, disk, part, &cells) {
        return Err(Error::Sheet);
    }
    if guarded(|| book::holds(disk, sheet, range, values)) != Ok(true) {
        return Err(Error::Sheet);
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    pub(crate) static SPOIL: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
fn spoiled(bytes: Vec<u8>) -> Vec<u8> {
    if SPOIL.with(std::cell::Cell::get) {
        crate::testing::spoiled(&bytes)
    } else {
        bytes
    }
}

#[cfg(not(test))]
fn spoiled(bytes: Vec<u8>) -> Vec<u8> {
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus_file, scratch};
    use crate::Folder;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .expect("list")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_copy_that_the_check_finds_changed_gives_the_cause_sheet_and_removes_what_this_call_made() {
        let dir = scratch("check-miss");
        fs::copy(corpus_file("values.xlsx"), dir.join("values.xlsx")).expect("copy");
        let before = fs::read(dir.join("values.xlsx")).expect("read");
        let folder = Folder::new(&dir).expect("the folder opens");
        let run = RunId::parse("0123456789abcdef0123456789abcdef").expect("a run id");
        let b5 = Range::parse("B5").expect("a range");
        let values = vec![vec![Value::Number(77.0)]];
        let spoiled_prepare = || {
            SPOIL.with(|s| s.set(true));
            let got = folder.prepare(Path::new("values.xlsx"), &run, "Values", b5, &values);
            SPOIL.with(|s| s.set(false));
            got
        };
        let got = spoiled_prepare();
        assert_eq!(got.as_ref().err(), Some(&Error::Sheet));
        assert_eq!(got.err().map(|e| e.cause()), Some(crate::Cause::Sheet));
        assert_eq!(names(&dir), vec!["values.xlsx".to_string()]);
        assert_eq!(fs::read(dir.join("values.xlsx")).expect("read"), before);
        drop(
            folder
                .prepare(Path::new("values.xlsx"), &run, "Values", b5, &values)
                .expect("prepared"),
        );
        let checkpoint = "values.halvo-checkpoint-0123456789abcdef0123456789abcdef.xlsx";
        assert_eq!(spoiled_prepare().err(), Some(Error::Sheet));
        assert_eq!(
            names(&dir),
            vec![checkpoint.to_string(), "values.xlsx".to_string()]
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
