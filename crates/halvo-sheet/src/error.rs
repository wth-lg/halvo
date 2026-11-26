use std::fmt;
use std::io;

use halvo_checkpoint::LockError;

/// A cause of the closed list of contracts.md A7.4 that an error of this crate gives the runner:
/// [`Error::cause`] for a call on the workbook or its folder, [`Error::source_cause`] for
/// [`crate::Folder::read_source`], the one call on the data file. Each fact on which a person acts
/// differently has a cause of its own (r3 U11.3, U11.4, U11.5, U11.9); every other error gives
/// `step_failed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// `workbook`: the cause of [`Error::Workbook`] alone. The crate refused the workbook before
    /// it wrote anything (A11.3).
    Workbook,
    /// `sheet`: the cause of [`Error::Sheet`] alone. The prepared copy differed from what the
    /// write allows, or did not read back with the new values (A11.3).
    Sheet,
    /// `no_workbook`: no folder or no file at the workbook's path.
    NoWorkbook,
    /// `no_sheet`: the workbook has no sheet of that name.
    NoSheet,
    /// `two_sheets`: two sheets of the workbook bear that name (r3 U11.9).
    TwoSheets,
    /// `no_range`: the text of the cells is no range.
    NoRange,
    /// `data_shape`: the data has not the rows and columns of the cells.
    DataShape,
    /// `no_data_file`: no file at the data file's path.
    NoDataFile,
    /// `data_outside`: the data file lies outside the workbook's folder.
    DataOutside,
    /// `data_form`: the data file is out of its form.
    DataForm,
    /// `data_encoding`: the data file is not UTF-8 (r3 U11.5).
    DataEncoding,
    /// `data_narrow`: a field of the data file is number signs alone, as a spreadsheet program
    /// writes a value whose column was too narrow to show it (r3 U11.9).
    DataNarrow,
    /// `read_only`: the person cannot write the workbook.
    ReadOnly,
    /// `no_access`: the system refused, for the permission, the open or the read of the workbook
    /// or of the data file, the walk to it, or the making of a file in the workbook's folder
    /// (r3 U11.4). A refused rename keeps `not_replaced`.
    NoAccess,
    /// `not_replaced`: the system refused the rename of `replace`, as for a workbook that another
    /// program holds open, and the workbook stays as it was.
    NotReplaced,
    /// `step_failed`: the cause of every other error.
    StepFailed,
}

impl Cause {
    /// The cause's name as A7.4 writes it.
    pub fn name(self) -> &'static str {
        match self {
            Cause::Workbook => "workbook",
            Cause::Sheet => "sheet",
            Cause::NoWorkbook => "no_workbook",
            Cause::NoSheet => "no_sheet",
            Cause::TwoSheets => "two_sheets",
            Cause::NoRange => "no_range",
            Cause::DataShape => "data_shape",
            Cause::NoDataFile => "no_data_file",
            Cause::DataOutside => "data_outside",
            Cause::DataForm => "data_form",
            Cause::DataEncoding => "data_encoding",
            Cause::DataNarrow => "data_narrow",
            Cause::ReadOnly => "read_only",
            Cause::NoAccess => "no_access",
            Cause::NotReplaced => "not_replaced",
            Cause::StepFailed => "step_failed",
        }
    }
}

/// Why a path is refused. No call follows a link, and every file that a call reads or writes
/// lies in the folder that the runner gave.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathRefusal {
    /// The path is absolute and not under the folder, or it names a parent directory.
    Outside,
    /// The path or a directory on the way to it is a link (a symbolic link, or on Windows a
    /// junction or another name-surrogate reparse point).
    Link,
    /// The path is a directory.
    Directory,
    /// The path is the folder itself, or it is not a regular file.
    NotFile,
    /// A part of the path holds a colon, which on Windows names a stream of a file.
    Name,
    /// The file name is one that this crate gives a checkpoint or a prepared copy: it holds
    /// `.halvo-checkpoint-` or `.halvo-prepared-` in any letter case, because a volume that folds
    /// case answers for such a name with a copy of the crate's. `prepare` takes no such file
    /// as a workbook.
    OwnFile,
    /// The file name is so long that a name that `prepare` or the removal of a checkpoint
    /// builds for it would hold more than [`halvo_checkpoint::NAME_MAX`] units as the crate
    /// counts them (bytes on macOS, UTF-16 code units on Windows). `prepare` refuses it before
    /// it writes anything.
    TooLong,
}

/// Why a source file is not of the form that [`crate::Folder::read_source`] takes: RFC 4180 with
/// one separator, the comma, the semicolon or the tab, one row per line ended by LF or CRLF, no
/// header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceRefusal {
    /// The file is not UTF-8, not UTF-16 by its mark and not, on Windows, text that the computer's
    /// page reads with no control sign (r3 U11.10). Its cause is `data_encoding` (r3 U11.5); every
    /// other refusal's is `data_form`, but `data_narrow`'s.
    Encoding,
    /// The file holds a carriage return that no line feed follows, inside a quoted field or
    /// outside one.
    CarriageReturn,
    /// A quoted field that starts on this line (counted from 1) holds a line break, LF or CRLF.
    QuotedLineBreak { line: usize },
    /// A field on this line (counted from 1) begins with a double quote, and text follows the
    /// quote that closes it, or no quote closes it by the end of the file. A double quote inside
    /// a field that does not begin with one is text.
    Quote { line: usize },
    /// The file reads as a table by semicolons and as one by tabs, so its separator is in doubt:
    /// in every line that is not empty, read as a row of semicolons, the semicolons outside its
    /// quoted fields are one or more and as many, and the same holds for tabs. A double quote at
    /// the line's start or right behind the sign opens such a field (r3 U11.5).
    TwoSeparators,
    /// The row on this line (counted from 1) has another number of cells than the first row.
    Ragged { line: usize },
    /// A field on this line (counted from 1), without the spaces at its ends, is one number sign
    /// `#` or more and nothing else, as a spreadsheet program writes a value whose column was too
    /// narrow to show it when the file was saved; a field of the first row that is one `#` alone
    /// is a title and passes. Its cause is `data_narrow` (r3 U11.9).
    NumberSigns { line: usize },
}

/// Why the crate refuses a workbook, with the cause `workbook`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The file is not a workbook that the crate can read: not a zip archive of the parts of a
    /// workbook, a part that does not parse, or a library that fails or panics on it.
    Unreadable,
    /// The workbook holds these kinds of the executor's refused list ([`crate::Kind::REFUSED`]).
    Kinds(Vec<crate::Kind>),
    /// A part that the crate parses (a worksheet part, `xl/workbook.xml`, `xl/styles.xml`, a
    /// relationship part) is not UTF-8 or does not parse, the worksheet part or
    /// `xl/workbook.xml` starts with a byte-order mark, or the worksheet part is not in the form
    /// that the writer takes. That form: a `worksheet` of the spreadsheet namespace with one
    /// `sheetData`; a `dimension` before it, if any, whose `ref` is a range of the A1 form;
    /// rows numbered from 1, each number once and in order; cells with a reference of their
    /// own row, each column once and in order; and inside `sheetData` no element named `row`
    /// or `c` other than those rows and cells, nor a second element named `sheetData`, since
    /// the reader would take it as one.
    Part,
    /// A relationship of a type that the crate or its reader reads as a part (a worksheet, a
    /// chart or dialog sheet, a macro sheet, a table, a macro project, a pivot part, the office
    /// document), its type compared without ASCII case, is marked `External`; a relationship of
    /// another type marked `External`, a hyperlink among them, is skipped and never a reason to
    /// refuse. Or a relationship inside the package does not name a part the way the package
    /// format names parts: its target does not decode (a percent sign is not followed by two hex
    /// digits, a `/` or a `\` is percent-encoded, or the bytes are not UTF-8). Or a relationship
    /// of a type that the crate or its reader reads names no entry, or its type equals such a
    /// type only when ASCII case is ignored, in its last segment or in the namespace before
    /// its last slash. Or the package's office document is not
    /// `xl/workbook.xml`, or the crate and the reader would find the named sheet in two
    /// different parts.
    Relationship,
    /// Over a bound: the file holds more than 64 MiB; an entry declares or holds more than
    /// 32 MiB uncompressed, or the entries more than 128 MiB in all; or the rectangle of the
    /// sheet's used cells (in `prepare` with the range added) holds more than 1,000,000 cells.
    Size,
    /// A cell to write lies in the header row of a table that the sheet's relationships link.
    TableHeader,
    /// A cell to write holds an array formula or a data-table formula, or lies inside the
    /// range of one.
    ArrayFormula,
    /// A cell to write is the first cell or a member of a shared formula.
    SharedFormula,
}

/// Why [`crate::Folder::replace`] left the workbook as it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    /// The workbook's path is refused now, or the file is not the one that `prepare` read.
    Workbook,
    /// The checkpoint is gone, or it is no longer the workbook as `prepare` found it.
    Checkpoint,
    /// The prepared copy is gone, or it is no longer the copy that `prepare` checked.
    Copy,
    /// The system refused the rename with an error of this kind.
    Rename(io::ErrorKind),
    /// The call could not take the lock of the workbook's directory, which another call held
    /// for the whole wait ([`LockError::Busy`]) or which could not be opened.
    Lock(LockError),
}

/// An error of a call. No error holds the content of a cell, of a source row or of a part of
/// the workbook.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The folder is not a directory that the crate can open.
    Folder,
    /// The path is refused.
    Path(PathRefusal),
    /// A file could not be read or written.
    Io(io::ErrorKind),
    /// The source file is not of the form of A1.2.
    Source(SourceRefusal),
    /// The text is not a range of the form that [`crate::Range::parse`] takes.
    Range,
    /// The workbook has no sheet of that name.
    NoSheet,
    /// Two sheets of the workbook bear that name, alike as [`Error::NoSheet`] is told: over every
    /// element `sheet` of the workbook part, the hidden and the very hidden ones too, its name
    /// with entities decoded compared byte for byte (r3 U11.9).
    TwoSheets,
    /// The workbook is refused, with the cause `workbook`.
    Workbook(Refusal),
    /// The values do not have the rows and columns of the range.
    Shape,
    /// A text of the values holds a character that XML 1.0 does not allow in a part of a
    /// workbook (a control character other than tab, line feed and carriage return, U+FFFE or
    /// U+FFFF), or more than the 32,767 UTF-16 code units that a cell of Excel holds.
    Value,
    /// The range holds more than 100,000 cells. `prepare` and `read` refuse it before they do
    /// anything else with it.
    RangeSize,
    /// The person cannot write the workbook: it is read-only, or on Unix its owner has no
    /// write permission.
    ReadOnly,
    /// The write would make a copy that the next `read` refuses: the elements of the cells of
    /// the range alone hold more than 32 MiB (refused before the workbook is read), or the copy
    /// would hold a sheet part or another entry of more than 32 MiB, entries of more than
    /// 128 MiB in all, or more than 64 MiB as a file. Refused before any write.
    FillSize,
    /// A checkpoint of this run stands beside the workbook and is not a copy of it as it is, or
    /// this process called a `replace` of the run's copy of the workbook already (one `replace`
    /// of a workbook per run).
    Checkpoint,
    /// `prepare` could not take the lock of the workbook's directory, which another call held
    /// for the whole wait ([`LockError::Busy`]) or which could not be opened; it wrote nothing.
    Lock(LockError),
    /// The check of `prepare` found the prepared copy different from what the write allows, or
    /// the named cells did not read back with their new values, with the cause `sheet`.
    Sheet,
    /// `replace` left the workbook as it was.
    NotReplaced(Kept),
    /// `replace` left the workbook as it was, and the system refused to remove the prepared
    /// copy (a read-only directory, an immutable file), or the call could not read the file that
    /// `prepare` made to tell that it is the copy it checked; the file stays beside the
    /// workbook.
    CopyLeft(Kept),
}

impl Error {
    /// The cause of A7.4 that this error of a call on the workbook or its folder gives: a missing
    /// folder or file is the workbook's (`no_workbook`). A range over 100,000 cells, a link, a
    /// value that a workbook cannot hold, a refused name, a lock, a checkpoint, a copy over the
    /// bounds and a `replace` that kept the workbook for another reason than a refused rename give
    /// `step_failed` (r3 U11.3). A file or a folder that the system refuses for the permission
    /// (`PermissionDenied`) at an open, a read, the walk or the making of a file is `no_access`, and
    /// so is a lock of `prepare` that the system refuses for the permission, as for the open of the
    /// workbook's folder that the lock's key needs; a lock held for the whole wait stays
    /// `step_failed`, and a refused rename keeps `not_replaced`, whatever its kind (r3 U11.4). A
    /// data file that is not UTF-8 is `data_encoding`, a field of number signs alone in it
    /// `data_narrow`, every other refusal of its form `data_form` (r3 U11.5, U11.9). A sheet's name
    /// that two sheets of the workbook bear is `two_sheets` (r3 U11.9).
    pub fn cause(&self) -> Cause {
        match self {
            Error::Workbook(_) => Cause::Workbook,
            Error::Sheet => Cause::Sheet,
            Error::Folder
            | Error::Io(io::ErrorKind::NotFound)
            | Error::Path(PathRefusal::NotFile | PathRefusal::Directory) => Cause::NoWorkbook,
            Error::Io(io::ErrorKind::PermissionDenied)
            | Error::Lock(LockError::Io(io::ErrorKind::PermissionDenied)) => Cause::NoAccess,
            Error::NoSheet => Cause::NoSheet,
            Error::TwoSheets => Cause::TwoSheets,
            Error::Range => Cause::NoRange,
            Error::Shape => Cause::DataShape,
            Error::Source(SourceRefusal::Encoding) => Cause::DataEncoding,
            Error::Source(SourceRefusal::NumberSigns { .. }) => Cause::DataNarrow,
            Error::Source(_) => Cause::DataForm,
            Error::ReadOnly => Cause::ReadOnly,
            Error::NotReplaced(Kept::Rename(_)) | Error::CopyLeft(Kept::Rename(_)) => {
                Cause::NotReplaced
            }
            Error::Path(_)
            | Error::Io(_)
            | Error::Value
            | Error::RangeSize
            | Error::FillSize
            | Error::Checkpoint
            | Error::Lock(_)
            | Error::NotReplaced(_)
            | Error::CopyLeft(_) => Cause::StepFailed,
        }
    }

    /// The cause of A7.4 that this error of [`crate::Folder::read_source`], the one call on the
    /// data file, gives: a missing file at the data file's path is `no_data_file`, a path outside
    /// the workbook's folder `data_outside`; every other error as [`Error::cause`] gives it
    /// (r3 U11.3), a refusal for the permission `no_access` among them (r3 U11.4).
    pub fn source_cause(&self) -> Cause {
        match self {
            Error::Io(io::ErrorKind::NotFound)
            | Error::Path(PathRefusal::NotFile | PathRefusal::Directory) => Cause::NoDataFile,
            Error::Path(PathRefusal::Outside) => Cause::DataOutside,
            _ => self.cause(),
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Error {
        Error::Io(error.kind())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Folder => f.write_str("the folder is not a directory"),
            Error::Path(why) => write!(f, "the path is refused: {why:?}"),
            Error::Io(kind) => write!(f, "a file could not be read or written: {kind:?}"),
            Error::Source(why) => write!(f, "the source is not of the form of A1.2: {why:?}"),
            Error::Range => f.write_str("the text is not a range"),
            Error::NoSheet => f.write_str("the workbook has no sheet of that name"),
            Error::TwoSheets => f.write_str("two sheets of the workbook bear that name"),
            Error::Workbook(why) => write!(f, "the workbook is refused: {why:?}"),
            Error::Shape => f.write_str("the values do not have the shape of the range"),
            Error::Value => f.write_str("a text holds a character that a workbook cannot hold"),
            Error::RangeSize => f.write_str("the range holds more than 100,000 cells"),
            Error::ReadOnly => f.write_str("the workbook cannot be written"),
            Error::FillSize => f.write_str("the write would make a copy over the bounds of a read"),
            Error::Checkpoint => f.write_str("a checkpoint of this run is not the workbook"),
            Error::Lock(why) => write!(f, "the lock of the workbook's folder: {why:?}"),
            Error::Sheet => f.write_str("the prepared copy did not read back the same"),
            Error::NotReplaced(why) => write!(f, "the workbook was not replaced: {why:?}"),
            Error::CopyLeft(why) => write!(
                f,
                "the workbook was not replaced: {why:?}; the prepared copy could not be removed"
            ),
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    /// r3 U11.3: every error of the crate, with the name that a call on the workbook gives it (`Error::cause`) and
    /// the name that `read_source`, the one call on the data file, gives it (`Error::source_cause`). A missing file
    /// is the workbook's at the first and the data file's at the second. r3 U11.4: a file or a folder that the
    /// system refuses for the permission is `no_access` at both, the lock's open of the workbook's folder among
    /// them, and a refused rename stays `not_replaced`. r3 U11.5: a data file that is not UTF-8 is `data_encoding` at
    /// both. r3 U11.9: a field of number signs alone is `data_narrow` at both, and a sheet's name that two sheets bear
    /// `two_sheets`.
    #[test]
    fn each_error_gives_the_name_of_its_fact_on_the_workbook_and_on_the_data_file() {
        use io::ErrorKind::{NotFound, PermissionDenied};
        let kept = [
            Kept::Workbook,
            Kept::Checkpoint,
            Kept::Copy,
            Kept::Rename(PermissionDenied),
            Kept::Lock(LockError::Busy),
        ];
        let not_replaced = |kept: Kept| match kept {
            Kept::Rename(_) => "not_replaced",
            _ => "step_failed",
        };
        let mut table: Vec<(Error, &str, &str)> = vec![
            (Error::Folder, "no_workbook", "no_workbook"),
            (
                Error::Path(PathRefusal::Outside),
                "step_failed",
                "data_outside",
            ),
            (Error::Path(PathRefusal::Link), "step_failed", "step_failed"),
            (
                Error::Path(PathRefusal::Directory),
                "no_workbook",
                "no_data_file",
            ),
            (
                Error::Path(PathRefusal::NotFile),
                "no_workbook",
                "no_data_file",
            ),
            (Error::Path(PathRefusal::Name), "step_failed", "step_failed"),
            (
                Error::Path(PathRefusal::OwnFile),
                "step_failed",
                "step_failed",
            ),
            (
                Error::Path(PathRefusal::TooLong),
                "step_failed",
                "step_failed",
            ),
            (Error::Io(NotFound), "no_workbook", "no_data_file"),
            (Error::Io(PermissionDenied), "no_access", "no_access"),
            (
                Error::Source(SourceRefusal::Encoding),
                "data_encoding",
                "data_encoding",
            ),
            (
                Error::Source(SourceRefusal::CarriageReturn),
                "data_form",
                "data_form",
            ),
            (
                Error::Source(SourceRefusal::QuotedLineBreak { line: 2 }),
                "data_form",
                "data_form",
            ),
            (
                Error::Source(SourceRefusal::Quote { line: 2 }),
                "data_form",
                "data_form",
            ),
            (
                Error::Source(SourceRefusal::TwoSeparators),
                "data_form",
                "data_form",
            ),
            (
                Error::Source(SourceRefusal::Ragged { line: 2 }),
                "data_form",
                "data_form",
            ),
            (
                Error::Source(SourceRefusal::NumberSigns { line: 2 }),
                "data_narrow",
                "data_narrow",
            ),
            (Error::Range, "no_range", "no_range"),
            (Error::NoSheet, "no_sheet", "no_sheet"),
            (Error::TwoSheets, "two_sheets", "two_sheets"),
            (Error::Workbook(Refusal::Unreadable), "workbook", "workbook"),
            (Error::Shape, "data_shape", "data_shape"),
            (Error::Value, "step_failed", "step_failed"),
            (Error::RangeSize, "step_failed", "step_failed"),
            (Error::ReadOnly, "read_only", "read_only"),
            (Error::FillSize, "step_failed", "step_failed"),
            (Error::Checkpoint, "step_failed", "step_failed"),
            (Error::Lock(LockError::Busy), "step_failed", "step_failed"),
            (
                Error::Lock(LockError::Io(PermissionDenied)),
                "no_access",
                "no_access",
            ),
            (Error::Sheet, "sheet", "sheet"),
        ];
        for kept in kept {
            let name = not_replaced(kept);
            table.push((Error::NotReplaced(kept), name, name));
            table.push((Error::CopyLeft(kept), name, name));
        }
        let got: Vec<(String, &str, &str)> = table
            .iter()
            .map(|(error, _, _)| {
                (
                    format!("{error:?}"),
                    error.cause().name(),
                    error.source_cause().name(),
                )
            })
            .collect();
        let want: Vec<(String, &str, &str)> = table
            .iter()
            .map(|(error, workbook, source)| (format!("{error:?}"), *workbook, *source))
            .collect();
        assert_eq!(got, want);
    }
}
