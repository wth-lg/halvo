//! The sheet executor of the agent-setup run (contracts.md A11.3).

mod book;
mod error;
mod parts;
mod path;
mod range;
mod source;
mod write;

use std::path::{Path, PathBuf};

pub use book::Cell;
pub use error::{Cause, Error, Kept, PathRefusal, Refusal, SourceRefusal};
pub use parts::Kind;
pub use range::Range;
pub use source::Value;
pub use write::{Prepared, RunId};

/// The directory that the permission `user_files` names, as the runner gives it. Every path
/// that a call takes is the runner's: an absolute path under this directory or a path relative
/// to it. A call follows no link, refuses a path that is a link, holds a link on its way, is a
/// directory or is not a regular file, and reads or writes no file outside this directory.
#[derive(Debug)]
pub struct Folder {
    root: PathBuf,
    spellings: Vec<PathBuf>,
}

impl Folder {
    /// Takes the directory as the runner gives it. The directory itself may be reached
    /// through a link; [`Error::Folder`] when it is not a directory.
    pub fn new(dir: &Path) -> Result<Folder, Error> {
        let (root, spellings) = path::folder(dir)?;
        Ok(Folder { root, spellings })
    }

    fn file(&self, path: &Path) -> Result<PathBuf, Error> {
        path::file(&self.root, &self.spellings, path)
    }

    /// Returns the values of `range` on the sheet named `sheet` (compared byte for byte) of
    /// the workbook at `workbook`, one row of the range after the other, with
    /// [`Cell::Empty`] for a cell that holds nothing and the cached result for a formula cell.
    /// Writes nothing. [`Error::NoSheet`] when the workbook has no such sheet, and
    /// [`Error::Workbook`] with [`Refusal::Unreadable`] when the file is no workbook that the
    /// crate can read.
    pub fn read(
        &self,
        workbook: &Path,
        sheet: &str,
        range: Range,
    ) -> Result<Vec<Vec<Cell>>, Error> {
        let full = self.file(workbook)?;
        let bytes = std::fs::read(full)?;
        book::read(&bytes, sheet, range)
    }

    /// Returns the rows of the source file at `source` (contracts.md A1.2): one row per line,
    /// ended by LF, cells separated by commas, no header and no quoting; an empty file has no
    /// row. A cell is a number when its text is digits with an optional leading `-` and at most
    /// one `.`, and text otherwise. Refused with [`Error::Source`] when the file is not UTF-8,
    /// starts with a byte-order mark, holds a carriage return, has rows of different lengths,
    /// or holds a number that no finite `f64` holds.
    pub fn read_source(&self, source: &Path) -> Result<Vec<Vec<Value>>, Error> {
        let full = self.file(source)?;
        source::parse(&std::fs::read(full)?)
    }

    /// The first half of a file edit (contracts.md A11.3, A5.2), which changes nothing of the
    /// workbook at `workbook`. In this order, and before it writes any file: refuses `values`
    /// that do not have the rows and columns of `range` ([`Error::Shape`]) or that hold a text
    /// with a character that XML 1.0 does not allow ([`Error::Value`]); refuses with
    /// [`Error::Workbook`] a workbook that holds a part kind of the sheet trial's refuses list
    /// ([`Refusal::Kinds`], found by the rule of A13.4) or that the crate cannot read; refuses a
    /// sheet that the workbook lacks ([`Error::NoSheet`]); and writes the values into a copy of
    /// the workbook in memory, a number as a number and a text as a text. Then it copies the
    /// workbook to its checkpoint `<name>.halvo-checkpoint-<run id><ext>` beside it, or keeps a
    /// checkpoint of the run that is already there and equal to the workbook
    /// ([`Error::Checkpoint`] when it differs), writes the changed copy to
    /// `<name>.halvo-prepared-<run id><ext>` beside it, reads that file again and requires the
    /// sameness of A13.4: the sheet names and their order are the same, each cell of the range
    /// holds its new value and no formula, every other cell holds the same value or, for a
    /// formula cell, the same formula text, the array formulas are the same, and no part kind
    /// of A13.4 is lost by the trial's rule. On a miss it removes the prepared copy and the
    /// checkpoint that it made and returns [`Error::Sheet`]. A formula's cached result is
    /// neither compared nor computed: a formula that reads a changed cell keeps its old result
    /// until a spreadsheet application calculates it again.
    pub fn prepare(
        &self,
        workbook: &Path,
        run: &RunId,
        sheet: &str,
        range: Range,
        values: &[Vec<Value>],
    ) -> Result<Prepared, Error> {
        let full = self.file(workbook)?;
        write::prepare(full, run, sheet, range, values)
    }

    /// The second half of a file edit (contracts.md A11.3, A5.2), one step: it puts the
    /// prepared copy in the workbook's place by one rename. It returns `Ok` when the workbook is
    /// the prepared copy, and an error when the call left the workbook as it was:
    /// [`Error::Path`] with [`PathRefusal::Outside`] when this folder does not hold the
    /// workbook, which touches no file, and otherwise [`Error::NotReplaced`] when, at the
    /// moment of the rename, the workbook's path is refused or the file is not the one that
    /// `prepare` read, the checkpoint is not that file either, the prepared copy is not the
    /// copy that `prepare` checked, or the system refuses the rename. With
    /// [`Error::NotReplaced`] it removes the prepared copy when it can reach it with no link on
    /// the way, so the checkpoint is the one file that stays beside the workbook after either
    /// end. A second call with the same [`Prepared`] returns the answer of the first and
    /// touches no file.
    pub fn replace(&self, prepared: &Prepared) -> Result<(), Error> {
        path::inside(&self.spellings, &prepared.workbook)?;
        prepared
            .answer
            .get_or_init(|| {
                let done = self
                    .file(&prepared.workbook)
                    .map_err(|_| Error::NotReplaced(Kept::Workbook))
                    .and_then(|_| write::replace(prepared));
                if done.is_err() {
                    if let Ok(copy) = self.file(&prepared.copy) {
                        let _ = std::fs::remove_file(copy);
                    }
                }
                done
            })
            .clone()
    }
}
