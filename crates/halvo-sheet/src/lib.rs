//! The sheet executor of the agent-setup run (contracts.md A11.3).

mod book;
mod carry;
mod error;
#[doc(hidden)]
pub mod made;
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
pub use source::{number_texts, Data, Letters, Value};
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
    /// [`Error::Folder`] when it is not a directory, and [`Error::Io`] with the kind
    /// `PermissionDenied` when the system refuses the way to it for the permission, which a
    /// stat of the directory tells after its walk fails (r3 U11.4).
    pub fn new(dir: &Path, locks: Locks) -> Result<Folder, Error> {
        let root = Root::new(dir).map_err(|_| match std::fs::metadata(dir) {
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                Error::Io(error.kind())
            }
            _ => Error::Folder,
        })?;
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
    /// ([`Error::NoSheet`]) or whose name two of its sheets bear ([`Error::TwoSheets`]); and a
    /// sheet whose used cells, found by a pass of the crate's own
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

    /// Returns the rows of the source file at `source`: a CSV file as RFC 4180 (its section 2)
    /// has it, with one separator, the comma, the semicolon or the tab, and no header row; an
    /// empty file has no row. One UTF-8 byte-order mark at the file's first byte is skipped (a
    /// mark at another place is text). A file that begins with the mark of UTF-16, FF FE or FE FF,
    /// is read as UTF-16 in that byte order behind its mark, as Excel's file type Unicode Text
    /// saves it; the mark alone reads no row (r3 U11.10). On Windows a file with no mark that is
    /// not UTF-8 is read by the code page that the system gives as the computer's own (`GetACP`),
    /// the one by which Excel's file type CSV (Comma delimited) writes its letters, where that page
    /// reads each of its bytes as a letter that is no control sign; in such a file a cell that holds
    /// a sign beyond ASCII is a text, the numbers are those of the file with every such sign a plain
    /// letter, and the file is refused where its separator hangs on what such a sign is (the
    /// coordinator's rule of log line 23819); [`Folder::read_data`] says so. A line ends with LF or CRLF, and the last line reads the
    /// same with its line end and without it. A sign holds where every line that is not empty,
    /// read as a row of that sign (a double quote at the line's start or right behind the sign
    /// opens a quoted field, two double quotes in it are one, and the signs inside it are not
    /// counted), holds one or more of the sign and as many as every other such line; an empty
    /// line counts for nothing (r3 U11.5). A file in which the semicolon or the tab holds is read
    /// by that sign, and a file in which neither holds by its commas, so a file whose lines hold
    /// different counts of semicolons is read by its commas. Where the semicolon (or the tab) and
    /// the comma both hold, the file is read by commas only where it speaks plainly for the comma
    /// (r3 U11.6): its read by commas stands too, some comma outside its quoted fields does not
    /// stand beside a digit with a digit or the edge of its field on its other side, no field of
    /// the semicolon's (or the tab's) reading is, without the spaces at its ends and one sign of
    /// money (€, $, £, ¥) or percent sign at either end, a number written with a comma (digits,
    /// points and commas alone, a minus in front at most, a digit at each end, one comma or more),
    /// and a field of that reading that holds a letter begins with a space and does not end with
    /// one, where no field of the comma's reading begins with one; every other such file is read by
    /// the semicolon (or the tab), and no count of numbers decides. A field that begins with a
    /// double quote holds the separator, and two double quotes in it are one; a double quote inside
    /// a field that does not begin with one is text, and so is one behind a space; the two signs
    /// that are not the separator are text. The quotes give the field's bounds and nothing else. In
    /// a file read by commas a field, quoted or not, is a number when its text is digits with an
    /// optional leading `-` and at most one `.`, with no zero before another digit of its whole
    /// part and at most 15 significant digits (`007` and a number of 20 digits are not, and
    /// `"123"` is the number 123). In a file read by semicolons or by tabs a field of that form
    /// with no decimal sign is a number, and the file has one decimal sign: the comma where a
    /// field is a number with a decimal comma and none is one with a decimal point, the point the
    /// other way round. Such a number holds its sign once, with a digit on each side, and is not
    /// of the shape of a thousands group, one to three digits other than a lone `0`, the sign and
    /// three digits, which has two readings (`1,234` and `1.234` stay texts and decide nothing;
    /// `0,125` and `1234,567` have one). In a file with a number of each sign, each of them stays
    /// a text. Every other field is an amount as a spreadsheet program shows it, or a text as
    /// typed (r3 U11.8). An amount is, without the spaces at its ends, one sign of money (`$`,
    /// `€`, `£`, `¥`) at either end with a space or none between, a minus (`-` or U+2212) in
    /// front, or brackets with a sign of money inside them, in front of them or behind them for
    /// a negative amount: digits with at most one decimal sign and a thousands sign of another
    /// kind in groups of three (`$12.50`, `12,50 €`, `1,234.56`, `1.234,56`, `($1,234.56)`,
    /// `(12,50) €`, `1'234`, `1 234,56` with a no-break space). It is its number where its
    /// decimal sign, where it has one, is the file's: the point in a file read by commas, but the
    /// comma for the amounts of a column that holds an amount with a decimal comma and a sign of
    /// money or a thousands sign, where no field of the file tells the point (a number with a
    /// point that is not of the thousands point shape, or an amount with a decimal point) and the
    /// column turns whole (r3 U11.10); in a
    /// file read by semicolons or by tabs the sign that its numbers tell, or where none tells
    /// one, the sign that its amounts tell where they tell one alone; a file with a number of
    /// each sign has none. In a file read by commas whose own amounts show the decimal comma, a
    /// field of the thousands point shape (`1.250`, `12.500`) is no number but a text, and as such
    /// it blocks its column (r3 U11.10). Halvo turns the amounts of a column into numbers only where it can
    /// read every value of that column: every field that holds a digit and gets no number blocks
    /// its column, but a field of the first row that holds a letter (a header); a blocked column
    /// reads as before r3 U11.8, and a field that is a number by the forms above stays that
    /// number. Texts stay: the shape of a thousands group with a sign of money (`$1,234`), a
    /// thousands sign repeated with no decimal sign (`1,234,567`), letters (`500 EUR`), a
    /// percent, brackets with no sign of money, a minus and brackets both (`(-$12.50)`), a minus
    /// behind and a dash that is no minus.
    /// Refused with [`Error::Source`], in this order where two hold: the file is not UTF-8, not
    /// UTF-16 by its mark and not, on Windows, text of the computer's page: a file of the mark of
    /// UTF-16 with an odd count of bytes or a lone surrogate behind it, a file that begins
    /// FF FE 00 00 (the mark of UTF-32LE), and on Windows a file that the page cannot read or reads
    /// with a control sign (below U+0020 but the tab, CR and LF, or U+007F to U+009F), a page
    /// outside the ANSI pages, a file of the page 936 with a four-byte letter, and a file whose
    /// separator hangs on what a sign beyond ASCII is; its cause
    /// by [`Error::source_cause`] is `data_encoding`; then, with the cause `data_form`: the
    /// semicolon and the tab both hold as its separator; or, read by the separator that is told,
    /// it holds a carriage return that no line feed follows, a line break inside a quoted field,
    /// text behind a closing quote or a quote that is never closed, the first of them in the file
    /// named, or its rows have different lengths. A file in which the semicolon or the tab holds
    /// is refused for a fault of its read by that sign, even where the comma holds too. Last, with
    /// the cause `data_narrow`: a file so read in which a field, quoted or not and without the
    /// spaces at its ends, is one number sign `#` or more and nothing else, as a spreadsheet
    /// program writes a value whose column was too narrow to show it when the file was saved, the
    /// first such line named; a field of the first row that is one `#` alone is a title and passes
    /// (r3 U11.9).
    pub fn read_source(&self, source: &Path) -> Result<Vec<Vec<Value>>, Error> {
        self.read_data(source).map(|data| data.rows)
    }

    /// The rows of the source file at `source`, as [`Folder::read_source`] gives them, and how
    /// its letters were read: [`Letters::Page`] where Halvo read them by the computer's page, a
    /// guess that the run's record keeps (r3 U11.10).
    pub fn read_data(&self, source: &Path) -> Result<Data, Error> {
        let full = self.file(source)?;
        source::data(&std::fs::read(full)?, source::system_page())
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
    /// ([`Error::NoSheet`], with the cause `no_sheet`), or whose name two of its sheets bear,
    /// counted over every sheet, the hidden and the very hidden ones too ([`Error::TwoSheets`],
    /// with the cause `two_sheets`); a sheet whose used cells and the range
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
    /// workbook, so a later `prepare` of the run answers [`Error::Checkpoint`]; and after a
    /// `replace` that this process called for the run's checkpoint of that workbook and that
    /// was refused, a later `prepare` of the run on the workbook answers [`Error::Checkpoint`]
    /// too, before it takes the lock and with nothing written, and the checkpoint stays. That
    /// answer comes after the refusals of the workbook itself in the order above: a workbook
    /// that the person changed and that now lacks the sheet answers [`Error::NoSheet`], and
    /// the checkpoint error comes once the workbook passes them. The process remembers each
    /// such call for its life, and a later checkpoint is the same when its path is the path that
    /// `prepare` built then (also in a directory made again at that path), when its name and
    /// its directory are, or when it is the same file, compared by
    /// [`halvo_checkpoint::same_file`] (so another spelling of the workbook on a volume that
    /// folds case is the same): a runner is one process for one run. `replace` holds
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
    /// link on the way, and only when the file at the copy's name is the file that `prepare`
    /// made and still holds the bytes that `prepare` checked: a file that another party put at
    /// that name after the write, of any bytes, or the copy written in place with other bytes,
    /// stays, and of Halvo's files the checkpoint is the one that stays beside the workbook
    /// after either end; when the system refuses the removal of the copy (a read-only
    /// directory, an immutable copy), or the file that `prepare` made cannot be read to tell
    /// its bytes, the answer is [`Error::CopyLeft`] and the copy stays. It renames into the
    /// workbook's place only the file that `prepare` made, compared by
    /// [`halvo_checkpoint::same_file`], and only while it holds the bytes that were checked. A
    /// second call with
    /// the same [`Prepared`] returns the answer of the first call that a folder holding the
    /// workbook made, and touches no file; a call refused as [`PathRefusal::Outside`] is no
    /// such first call.
    pub fn replace(&self, prepared: &Prepared) -> Result<(), Error> {
        self.root.names(&prepared.workbook)?;
        prepared
            .answer
            .get_or_init(|| {
                write::note_replace_called(prepared);
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
                    Err(Error::NotReplaced(why)) => match prepared.own_copy() {
                        write::CopyAt::Own(file, copy)
                            if write::remove_made(&file, &copy).is_err() =>
                        {
                            Err(Error::CopyLeft(why))
                        }
                        write::CopyAt::Unread => Err(Error::CopyLeft(why)),
                        _ => Err(Error::NotReplaced(why)),
                    },
                    other => other,
                };
                drop(held);
                answer
            })
            .clone()
    }
}

/// The most names that [`sheet_names`] answers (r4 U3.22c).
pub const SHEET_NAMES_MAX: usize = parts::NAMES_MAX;

/// The most bytes of one name that [`sheet_names`] answers (r4 U3.22c).
pub const SHEET_NAME_BYTES_MAX: usize = parts::NAME_BYTES_MAX;

/// The names of the worksheets of the workbook at the full path `workbook`, in the workbook's
/// own order, and no cell (r4 U3.22c): the names by which [`Folder::read`] and
/// [`Folder::prepare`] find a sheet. A hidden worksheet is among them; a very hidden one, a chart
/// sheet, a dialog sheet and a macro sheet are not. Writes nothing and takes no lock, as
/// [`Folder::read`] takes none.
///
/// It walks the path as a [`Folder`] of the workbook's directory does (the folder that the
/// runner gives a file step): a link on the way, a directory and a file that is not regular are
/// refused. Refused, in this order: a directory that is not there or no directory
/// ([`Error::Folder`]), or that the system refuses for the permission ([`Error::Io`] with the
/// kind `PermissionDenied`); a path that the folder refuses; a file of more than 64 MiB, before
/// it is read, and a package over the bounds of its entries ([`Refusal::Size`]); a file that is
/// no workbook that the crate can read ([`Refusal::Unreadable`], [`Refusal::Relationship`],
/// [`Refusal::Part`]); a workbook that names two sheets alike, counted over every sheet and
/// compared as a run finds a sheet by its name, which refuses a run on that name with
/// [`Error::TwoSheets`] ([`Refusal::Relationship`]); a sheet whose relationship does not name its
/// part as the reader reads it ([`Refusal::Relationship`]); and more than [`SHEET_NAMES_MAX`]
/// names or a name of more
/// than [`SHEET_NAME_BYTES_MAX`] bytes ([`Refusal::Size`]): a refusal, never a cut list.
pub fn sheet_names(workbook: &Path) -> Result<Vec<String>, Error> {
    let dir = workbook.parent().unwrap_or(Path::new(""));
    let root = Root::new(dir).map_err(|_| match std::fs::metadata(dir) {
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            Error::Io(error.kind())
        }
        _ => Error::Folder,
    })?;
    let full = root.file(workbook)?;
    let bytes = parts::package_file(&full)?;
    let entries = parts::entries(&bytes)?;
    parts::worksheet_names(&entries)
}

#[cfg(test)]
mod names_tests {
    use super::{sheet_names, Error, Refusal};

    /// r4 U3.22c: the names of a workbook's worksheets come in the workbook's order, and a
    /// workbook of more worksheets than the bound of the count is refused, with no name; so is a
    /// workbook that names two sheets alike, a very hidden sheet in front of a worksheet of the
    /// same name among them (the coordinator's ruling of its log line 19558).
    #[test]
    fn the_names_come_in_the_workbooks_order_and_a_workbook_over_the_bound_of_the_count_is_refused()
    {
        let dir = crate::testing::scratch("sheet-names");
        let three = dir.join("three.xlsx");
        let names = ["Budget", "Q1 & Q2 <draft>", "\u{dc}bersicht"];
        std::fs::write(&three, crate::made::workbook(&names)).expect("write the workbook");
        assert_eq!(
            sheet_names(&three),
            Ok(names.iter().map(|n| n.to_string()).collect::<Vec<_>>())
        );
        let many: Vec<String> = (1..=super::SHEET_NAMES_MAX + 1)
            .map(|n| format!("Sheet{n}"))
            .collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let over = dir.join("over.xlsx");
        std::fs::write(&over, crate::made::workbook(&many)).expect("write the workbook");
        assert_eq!(sheet_names(&over), Err(Error::Workbook(Refusal::Size)));
        let twice = crate::made::workbook(&["Data", "Data"]);
        let mut hidden = crate::parts::entries(&twice).expect("the entries");
        crate::testing::edit(
            &mut hidden,
            "xl/workbook.xml",
            r#"<sheet name="Data" sheetId="1""#,
            r#"<sheet name="Data" sheetId="1" state="veryHidden""#,
        );
        for (name, bytes) in [
            ("twice.xlsx", twice),
            ("hidden.xlsx", crate::testing::zipped(&hidden)),
        ] {
            let alike = dir.join(name);
            std::fs::write(&alike, bytes).expect("write the workbook");
            assert_eq!(
                sheet_names(&alike),
                Err(Error::Workbook(Refusal::Relationship)),
                "{name}"
            );
        }
    }
}

#[cfg(test)]
mod two_sheets_tests {
    use super::{Error, Folder, Range, RunId, Value};
    use crate::testing::{edit, locks, scratch, zipped};

    /// The workbook of the sheets `names` with the edit `from` to `to` of its workbook part.
    fn edited(names: &[&str], from: &str, to: &str) -> Vec<u8> {
        let mut all = crate::parts::entries(&crate::made::workbook(names)).expect("the entries");
        edit(&mut all, "xl/workbook.xml", from, to);
        zipped(&all)
    }

    /// r3 U11.9: a run whose sheet name two sheets of the workbook bear, alike as
    /// `parts::part_of` finds a sheet by its name (the name with its entities decoded, byte for
    /// byte, over every element `sheet`), is refused with `two_sheets` before any write: two
    /// worksheets, a hidden or a very hidden sheet beside a worksheet, and two names alike once
    /// decoded. The folder then holds the workbook alone, as it was. A name that one sheet alone
    /// bears goes ahead: beside a name that differs in the case of a letter alone, and beside two
    /// other sheets that are alike.
    #[test]
    fn a_run_on_a_workbook_in_which_two_sheets_bear_its_name_is_refused_before_any_write() {
        let hidden = |state: &str| {
            let to = format!(r#"<sheet name="Data" sheetId="1" state="{state}""#);
            edited(&["Data", "Data"], r#"<sheet name="Data" sheetId="1""#, &to)
        };
        let decoded = edited(
            &["A&B", "A&B"],
            r#"name="A&amp;B" sheetId="2""#,
            r#"name="A&#38;B" sheetId="2""#,
        );
        let cases: [(&str, Vec<u8>, &str, bool); 6] = [
            (
                "two worksheets",
                crate::made::workbook(&["Data", "Data"]),
                "Data",
                false,
            ),
            ("a hidden one", hidden("hidden"), "Data", false),
            ("a very hidden one", hidden("veryHidden"), "Data", false),
            ("alike once decoded", decoded, "A&B", false),
            (
                "another case",
                crate::made::workbook(&["Data", "data"]),
                "data",
                true,
            ),
            (
                "two others alike",
                crate::made::workbook(&["Data", "Data", "Report"]),
                "Report",
                true,
            ),
        ];
        let run = RunId::parse(&"0c".repeat(16)).expect("a run id");
        let range = Range::parse("A1:B1").expect("a range");
        let values = [vec![Value::Number(1.0), Value::Text("x".to_string())]];
        for (n, (label, bytes, sheet, ahead)) in cases.into_iter().enumerate() {
            let dir = scratch(&format!("two-sheets-{n}"));
            std::fs::write(dir.join("book.xlsx"), &bytes).expect("write the workbook");
            let folder = Folder::new(&dir, locks()).expect("the folder");
            let got = folder.prepare(
                std::path::Path::new("book.xlsx"),
                &run,
                sheet,
                range,
                &values,
            );
            if ahead {
                assert!(got.is_ok(), "{label}: {:?}", got.err());
                continue;
            }
            let error = got.expect_err(label);
            assert_eq!(
                (&error, error.cause().name()),
                (&Error::TwoSheets, "two_sheets"),
                "{label}"
            );
            let held: Vec<String> = std::fs::read_dir(&dir)
                .expect("list the folder")
                .map(|e| {
                    e.expect("an entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            assert_eq!(
                held,
                ["book.xlsx"],
                "{label}: nothing written beside the workbook"
            );
            let kept = std::fs::read(dir.join("book.xlsx")).expect("read the workbook");
            assert!(kept == bytes, "{label}: the workbook as it was");
        }
    }
}
