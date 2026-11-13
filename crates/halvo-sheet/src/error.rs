use std::fmt;
use std::io;

use halvo_checkpoint::LockError;

/// A cause of the closed list of contracts.md A7.4 that an error of this crate gives the runner.
/// Two errors have a cause of their own; every other error gives `step_failed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// `workbook`: the cause of [`Error::Workbook`] alone. The crate refused the workbook before
    /// it wrote anything (A11.3).
    Workbook,
    /// `sheet`: the cause of [`Error::Sheet`] alone. The prepared copy differed from what the
    /// write allows, or did not read back with the new values (A11.3).
    Sheet,
    /// `step_failed`: the cause of every other error.
    StepFailed,
}

impl Cause {
    /// The cause's name as A7.4 writes it.
    pub fn name(self) -> &'static str {
        match self {
            Cause::Workbook => "workbook",
            Cause::Sheet => "sheet",
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

/// Why a source file is not of the form of contracts.md A1.2: one row per line, ended by LF,
/// cells separated by commas, no header and no quoting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceRefusal {
    /// The file is not UTF-8.
    Encoding,
    /// The file starts with a byte-order mark.
    ByteOrderMark,
    /// The file holds a carriage return.
    CarriageReturn,
    /// The row on this line (counted from 1) has another number of cells than the first row.
    Ragged { line: usize },
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
    /// type only when ASCII case is ignored. Or the package's office document is not
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
    /// A checkpoint of this run stands beside the workbook and is not a copy of it as it is.
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
    /// copy (a read-only directory, an immutable file), which stays beside the workbook.
    CopyLeft(Kept),
}

impl Error {
    /// The cause of A7.4 that this error gives.
    pub fn cause(&self) -> Cause {
        match self {
            Error::Workbook(_) => Cause::Workbook,
            Error::Sheet => Cause::Sheet,
            Error::Folder
            | Error::Path(_)
            | Error::Io(_)
            | Error::Source(_)
            | Error::Range
            | Error::NoSheet
            | Error::Shape
            | Error::Value
            | Error::RangeSize
            | Error::ReadOnly
            | Error::FillSize
            | Error::Checkpoint
            | Error::Lock(_)
            | Error::NotReplaced(_)
            | Error::CopyLeft(_) => Cause::StepFailed,
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
