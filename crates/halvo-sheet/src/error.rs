use std::fmt;
use std::io;

/// A cause of the closed list of contracts.md A7.4 that an error of this crate gives the runner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    /// `workbook`: the workbook holds a part kind that the sheet trial's verdict refuses, or it
    /// is not a workbook that the crate can read (A11.3).
    Workbook,
    /// `sheet`: the changed copy did not read back the same (A11.3).
    Sheet,
    /// `step_failed`: any other failure of the call.
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
    /// `.halvo-checkpoint-` or `.halvo-prepared-`. `prepare` takes no such file as a workbook.
    OwnFile,
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
    /// A cell on this line has the form of a number that no finite `f64` holds.
    Number { line: usize },
}

/// Why the crate refuses a workbook, with the cause `workbook`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The file is not a workbook that the crate can read: not a zip archive of the parts of a
    /// workbook, a part that does not parse, or a library that fails or panics on it.
    Unreadable,
    /// The workbook holds these kinds of the verdict's refuses list ([`crate::Kind::REFUSED`]).
    Kinds(Vec<crate::Kind>),
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
    /// The text is not a run id of the form of contracts.md A0.6.
    RunId,
    /// A checkpoint of this run stands beside the workbook and is not a copy of it as it is.
    Checkpoint,
    /// The prepared copy did not read back with the sameness of A13.4, or it lost a part kind
    /// of the workbook, with the cause `sheet`.
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
            | Error::RunId
            | Error::Checkpoint
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
            Error::RunId => f.write_str("the text is not a run id"),
            Error::Checkpoint => f.write_str("a checkpoint of this run is not the workbook"),
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
