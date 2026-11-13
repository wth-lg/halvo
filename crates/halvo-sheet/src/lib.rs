//! The sheet executor of the agent-setup run (contracts.md A11.3).

mod book;
mod carry;
mod error;
mod parts;
mod patch;
mod path;
mod range;
mod same;
mod source;
#[cfg(test)]
mod testing;
mod write;
mod xml;

use std::fmt;
use std::path::{Path, PathBuf};

use halvo_checkpoint::Root;

pub use book::Cell;
pub use error::{Cause, Error, Kept, PathRefusal, Refusal, SourceRefusal};
pub use halvo_checkpoint::{LockError, Locks, RunId, Sha256};
pub use parts::Kind;
pub use range::Range;
pub use source::Value;
pub use write::Prepared;

/// The directory that the permission `user_files` names, as the runner gives it. Every path
/// that a call takes is the runner's: a path relative to this directory, or an absolute path
/// that starts with the directory as the runner gave it or with its canonical form (on Windows
/// also that form without its `\\?\` prefix); any other absolute path is refused as outside.
/// A call refuses a path that is a link, holds a link on its way, is a directory or is not a
/// regular file. It reads the workbook through a handle that follows no link and closes it
/// before it waits for the lock; under the lock it opens the workbook again the same way,
/// checks that it holds the bytes that it read, and copies from that handle; it gives each
/// file that it makes its permissions, attributes, access
/// list and streams through the handle that made it, never through a path; and it removes only
/// a file that it opened without following a link. What stays open: between the walk and an
/// open or a rename by path, a directory of the way can become a link; the walk is done again
/// under the lock and refuses such a link, and after it the open of the workbook, the making
/// of a file and the rename of `replace` still act by path, so that window stays. The walk
/// of a path is the one of the crate halvo-checkpoint, whose `remove` acts on a directory that
/// it opened part by part without following a link.
pub struct Folder {
    root: Root,
    locks: Locks,
}

impl fmt::Debug for Folder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Folder").finish_non_exhaustive()
    }
}

impl Folder {
    /// Takes the directory as the runner gives it, and the locks of `agents/locks/` under
    /// Halvo's data directory, which [`Folder::prepare`] and [`Folder::replace`] hold for the
    /// directory of the workbook. The directory itself may be reached through a link;
    /// [`Error::Folder`] when it is not a directory.
    pub fn new(dir: &Path, locks: Locks) -> Result<Folder, Error> {
        let root = Root::new(dir).map_err(|_| Error::Folder)?;
        Ok(Folder { root, locks })
    }

    fn file(&self, path: &Path) -> Result<PathBuf, Error> {
        Ok(self.root.file(path)?)
    }

    /// Returns the values of `range` on the sheet named `sheet` (compared byte for byte) of
    /// the workbook at `workbook`, one row of the range after the other, with
    /// [`Cell::Empty`] for a cell that holds nothing and the cached result for a formula cell.
    /// Writes nothing. It holds memory for every cell of the range, and for the rectangle of
    /// the sheet's used cells, which the reader builds whatever range is asked for.
    ///
    /// Refused, in this order: a range of more than 100,000 cells ([`Error::RangeSize`]),
    /// before anything else is done with it; a path that the folder refuses; a file of more
    /// than 64 MiB, before it is read, and a package over the bounds of its entries
    /// ([`Refusal::Size`]); a file that is no workbook that the crate can read
    /// ([`Refusal::Unreadable`], [`Refusal::Relationship`]); a sheet that the workbook lacks
    /// ([`Error::NoSheet`]); and a sheet whose used cells, found by a pass of the crate's own
    /// before the reader sees the sheet, span a rectangle of more than 1,000,000 cells
    /// ([`Refusal::Size`]).
    pub fn read(
        &self,
        workbook: &Path,
        sheet: &str,
        range: Range,
    ) -> Result<Vec<Vec<Cell>>, Error> {
        if write::range_cells(range) > parts::RANGE_MAX {
            return Err(Error::RangeSize);
        }
        let full = self.file(workbook)?;
        let bytes = parts::package_file(&full)?;
        let entries = parts::entries(&bytes)?;
        let part = parts::part_of(&entries, sheet)?;
        let source = entries.get(&part).ok_or_else(book::unreadable)?;
        let text = std::str::from_utf8(source).map_err(|_| Error::Workbook(Refusal::Part))?;
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        if !xml::fits(xml::extent(text)?, None, parts::CELLS_MAX) {
            return Err(parts::size());
        }
        drop(entries);
        book::read(&bytes, sheet, range)
    }

    /// Returns the rows of the source file at `source` (contracts.md A1.2): one row per line,
    /// ended by LF, cells separated by commas, no header and no quoting; an empty file has no
    /// row. A cell is a number when its text is digits with an optional leading `-` and at most
    /// one `.`, with no zero before another digit of its whole part and at most 15 significant
    /// digits; every other cell is a text, as typed (`007` and a number of 20 digits stay
    /// texts). Refused with [`Error::Source`] when the file is not UTF-8, starts with a
    /// byte-order mark, holds a carriage return or has rows of different lengths.
    pub fn read_source(&self, source: &Path) -> Result<Vec<Vec<Value>>, Error> {
        let full = self.file(source)?;
        source::parse(&std::fs::read(full)?)
    }

    /// The first half of a file edit (contracts.md A11.3, A5.2), which changes nothing of the
    /// workbook at `workbook`.
    ///
    /// Before it writes any file it refuses, in this order: a range of more than 100,000 cells
    /// ([`Error::RangeSize`]), and values whose cell elements alone would hold more than
    /// 32 MiB, the bound of an entry that the next read keeps ([`Error::FillSize`]), both before
    /// anything else is done with them; a path that the folder
    /// refuses, or a workbook whose name is one that it gives a checkpoint or a prepared copy,
    /// in any letter case ([`PathRefusal::OwnFile`]); a workbook whose name is so long that a
    /// name that `prepare` or the removal of the checkpoint builds would hold more than 255
    /// units as the crate counts them, bytes on macOS and UTF-16 code units on Windows: every
    /// such name adds 82 units to the workbook's name, so a name of more than 173 units
    /// ([`PathRefusal::TooLong`], with the
    /// cause `step_failed`; a volume whose names are shorter refuses a longer name with
    /// [`Error::Io`] as the call writes); `values` that do not have the rows and columns of `range`
    /// ([`Error::Shape`]), or that hold a number that is not finite or a text with a character
    /// that XML 1.0 does not allow or with more than the 32,767 UTF-16 code units of a cell
    /// ([`Error::Value`]); a workbook that the person cannot write ([`Error::ReadOnly`]). Then,
    /// with [`Error::Workbook`] and the cause `workbook`: a file of more than 64 MiB, before it
    /// is read, or a package whose entries declare or hold more than 32 MiB in one or 128 MiB
    /// in all ([`Refusal::Size`]); a file that is no zip package, is encrypted or holds an entry
    /// name twice, names compared as the package format compares them ([`Refusal::Unreadable`]);
    /// a relationship of a type that the crate or its reader reads as a part (a worksheet, a
    /// table, a macro project, a pivot part and the other sheet types) that is marked `External`,
    /// or a relationship inside the package that does not name a part as that format names it
    /// ([`Refusal::Relationship`]; a relationship of another type marked `External`, such as a
    /// hyperlink, is skipped: its address is not read as a part name and is never a reason to
    /// refuse); a workbook that holds a kind of the
    /// executor's refused list
    /// ([`Refusal::Kinds`]: `pivot`, `macro`, `validation`); a sheet that the workbook lacks
    /// ([`Error::NoSheet`], with the cause `step_failed`); a sheet whose used cells and the range
    /// together span more than 1,000,000 cells ([`Refusal::Size`]); a cell of the range in the
    /// header row of a table ([`Refusal::TableHeader`]); a worksheet part that the writer cannot
    /// take ([`Refusal::Part`]); a cell of the range in a shared formula
    /// ([`Refusal::SharedFormula`]) or in the range of an array or data-table formula
    /// ([`Refusal::ArrayFormula`]); a copy whose sheet part or another entry would hold more
    /// than 32 MiB, whose entries would hold more than 128 MiB in all, or whose file would
    /// hold more than 64 MiB, the bounds that the next read keeps ([`Error::FillSize`], with the
    /// cause `step_failed`); and a sheet that the reader of the read-back cannot read
    /// ([`Refusal::Unreadable`]): among them a sheet whose `dimension` holds an entity in its
    /// `ref`, which the writer itself would take as the plain spelling.
    ///
    /// Then it copies the workbook to the run's checkpoint `<name>.halvo-checkpoint-<run
    /// id><ext>` beside it, or takes a checkpoint of the run that is already there and equal to
    /// the workbook without writing it again ([`Error::Checkpoint`] when it differs or is no
    /// regular file). It writes a prepared copy under a name of this call's own,
    /// `<name>.halvo-prepared-<run id>-<part of its own>-<digest><ext>`, `<digest>` being the
    /// first 16 hex digits of the copy's SHA-256. It makes the copy's mark in `agents/locks/`
    /// before the copy and holds it while the [`Prepared`] lives, so that the sweep of no other
    /// call of the same data directory removes the copy; when the copy cannot be made, the mark
    /// goes. No handle of the call stays open on a file of the person's folder. Under the lock
    /// of the directory it walks the workbook's path again (a directory that became a link
    /// while it waited is [`PathRefusal::Link`]; a workbook that no longer holds the bytes
    /// that it read is [`Error::Io`] with the kind `Interrupted`) and, before it writes, sweeps
    /// the prepared
    /// copies of this workbook that a killed run left: it removes an entry only when its own
    /// name is such a name of this workbook byte for byte, it is a regular file and no link,
    /// `agents/locks/` holds a mark of this directory and this name that no live call holds,
    /// and its bytes still have the digest that its name says; the copy goes first, then its
    /// mark. It leaves every other entry. An entry with such a name and its own digest but no
    /// mark is left, and [`Prepared::unmarked`] counts it: a copy of a call that opened another
    /// data directory, which the sweep must not take while that call lives, or a copy that a
    /// call killed between its mark and its copy, or before marks were made, left; such a copy
    /// stays until the person removes it. Both get the workbook's permissions: its permission bits on Unix, its read-only
    /// attribute on Windows; and what the workbook holds besides its bytes: on macOS its
    /// extended attributes (a quarantine mark among them, which the system writes again with
    /// its flags kept) and its access list, on Windows its named streams (the zone of a file
    /// from the web among them) and its access list. When the system cannot carry them,
    /// `prepare` removes the file that it made and fails with [`Error::Io`]; a checkpoint of
    /// the run that is already there is taken as it is. Once the run's checkpoint is made, no
    /// call of this crate removes
    /// it, neither on a miss nor on a drop: no call removes a checkpoint of a run that has not
    /// ended, and the one call that removes one is `remove` of the crate halvo-checkpoint,
    /// which the supervisor library makes for a run whose end is stored, with the SHA-256,
    /// the length and the name that [`Prepared`] gives. The copy holds the entries of the
    /// workbook's package in their order, each copied with its name, its method and its
    /// compressed data, but two, which keep the method and the time of their entries. The
    /// worksheet part of `sheet` is the source's bytes with the element of each cell of the
    /// range replaced, or inserted in column order, a row that does not exist inserted in row
    /// order, and the `spans` of a row and the `ref` of `dimension` grown where a cell of the
    /// range lies outside them. A number is written as a number and a text as an inline string,
    /// so the shared strings part does not change; an empty text makes a cell that holds an
    /// empty inline string. A cell keeps its style and loses its formula; an inserted cell takes
    /// no style. In the workbook part the attribute `fullCalcOnLoad` of `calcPr` is `1`: set
    /// where it was absent or another value, with the element added in its place where there
    /// was none; where it was `1` or `true` already the part is copied as it is.
    ///
    /// It reads the copy again and checks it with a routine of its own: the same entry names in
    /// the same order, every other entry byte for byte, the workbook part with the flag set and
    /// byte for byte or with that one change, and the worksheet part byte for byte once the
    /// places above are taken out of both. Each of those places must hold what the write
    /// allows: every cell of the range once, in its own row, in column order; an inserted row
    /// with its number and cells of the range alone; `spans` and the `ref` of `dimension` that
    /// hold the source's and every cell of the range. Then the cells of the range read back
    /// through calamine, which reads that sheet alone, with their new values and no formula.
    /// On a miss it removes its prepared copy, leaves the run's checkpoint, and returns
    /// [`Error::Sheet`], with the cause `sheet`; the workbook was not touched.
    ///
    /// It holds the lock of the workbook's directory, which it waits for up to the wait of the
    /// [`Locks`], from before its sweep until its prepared copy is made: over the sweep, the
    /// checkpoint, the copy's mark and the copy. When it cannot take the lock, it answers
    /// [`Error::Lock`] and has written nothing. Two calls exclude each other only when their
    /// [`Locks`] opened the same data directory.
    ///
    /// One `replace` of a workbook per run: after it, the run's checkpoint is no longer the
    /// workbook, so a later `prepare` of the run answers [`Error::Checkpoint`]. `replace` holds
    /// the same lock from its compare to its rename, so of two runs that prepared one workbook
    /// from the same file, the `replace` that comes second finds the workbook changed and
    /// leaves it: no edit is lost.
    ///
    /// What goes stale: a cached copy of a changed cell's value elsewhere in the package is not
    /// updated: a formula's cached result, a chart's cached points, the counts of the shared
    /// strings part. An application that computes (Excel, Numbers, LibreOffice) computes them
    /// again when it opens the file, which is what the calculation flag asks for; a viewer that
    /// does not compute shows the old figures.
    pub fn prepare(
        &self,
        workbook: &Path,
        run: &RunId,
        sheet: &str,
        range: Range,
        values: &[Vec<Value>],
    ) -> Result<Prepared, Error> {
        if write::range_cells(range) > parts::RANGE_MAX {
            return Err(Error::RangeSize);
        }
        if patch::least_bytes(range, values) > parts::ENTRY_MAX {
            return Err(Error::FillSize);
        }
        let full = self.file(workbook)?;
        let name = full.file_name().unwrap_or_default().to_string_lossy();
        let name = name.to_ascii_lowercase();
        if name.contains(".halvo-checkpoint-") || name.contains(".halvo-prepared-") {
            return Err(Error::Path(PathRefusal::OwnFile));
        }
        if !write::names_fit_for(&full, run) {
            return Err(Error::Path(PathRefusal::TooLong));
        }
        write::prepare(&self.root, &self.locks, full, run, sheet, range, values)
    }

    /// The second half of a file edit (contracts.md A11.3, A5.2), one step: it puts the
    /// prepared copy in the workbook's place by one rename. It returns `Ok` when the workbook is
    /// the prepared copy, and an error when the call left the workbook as it was:
    /// [`Error::Path`] with [`PathRefusal::Outside`] when this folder does not hold the
    /// workbook, which touches no file, and otherwise [`Error::NotReplaced`] when it cannot
    /// take the lock of the workbook's directory within the wait of the [`Locks`]
    /// ([`Kept::Lock`]), or when, under that lock, which it holds from its compare to its
    /// rename, the workbook's path is refused or the file is not the one that `prepare` read,
    /// the checkpoint is not that file either, the prepared copy is not the copy that
    /// `prepare` checked, or the system refuses the rename. With
    /// [`Error::NotReplaced`] it has removed the prepared copy when it could reach it with no
    /// link on the way, so the checkpoint is the one file that stays beside the workbook after
    /// either end; when the system refuses that removal too (a read-only directory, an
    /// immutable copy) the answer is [`Error::CopyLeft`] and the copy stays. A second call with
    /// the same [`Prepared`] returns the answer of the first call that a folder holding the
    /// workbook made, and touches no file; a call refused as [`PathRefusal::Outside`] is no
    /// such first call.
    pub fn replace(&self, prepared: &Prepared) -> Result<(), Error> {
        self.root.names(&prepared.workbook)?;
        prepared
            .answer
            .get_or_init(|| {
                let held = self.locks.hold(write::directory(&prepared.workbook));
                let done = held
                    .as_ref()
                    .map_err(|why| Error::NotReplaced(Kept::Lock(*why)))
                    .and_then(|_| {
                        self.file(&prepared.workbook)
                            .map_err(|_| Error::NotReplaced(Kept::Workbook))
                    })
                    .and_then(|_| write::replace(prepared));
                let answer = match done {
                    Err(Error::NotReplaced(why)) => match self.file(&prepared.copy) {
                        Ok(copy) => match halvo_checkpoint::open_regular(&copy) {
                            Ok(file) if write::remove_made(&file, &copy).is_ok() => {
                                Err(Error::NotReplaced(why))
                            }
                            _ => Err(Error::CopyLeft(why)),
                        },
                        Err(_) => Err(Error::NotReplaced(why)),
                    },
                    other => other,
                };
                drop(held);
                answer
            })
            .clone()
    }
}
