use std::fmt;
use std::io::ErrorKind;

use halvo_schema_core::request::ErrorName;
use halvo_sqlcipher::rusqlite;

/// A fault of the store, for which no request is at fault. It holds a result code, an I/O error kind or a fixed
/// word, and never a value of a record, so it may be logged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// SQLite answered with this extended result code.
    Db(i32),
    /// A file operation failed with this kind.
    Io(ErrorKind),
    /// A stored row that its own reader refuses: the store wrote the row, so the database changed outside this
    /// library.
    Row,
    /// The truncating checkpoint of the write-ahead log left frames behind.
    Checkpoint,
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fault::Db(code) => write!(f, "the database answered the result code {code}"),
            Fault::Io(kind) => write!(f, "a file operation failed: {kind:?}"),
            Fault::Row => f.write_str("a stored row is refused by its reader"),
            Fault::Checkpoint => f.write_str("the truncating checkpoint left frames in the log"),
        }
    }
}

impl std::error::Error for Fault {}

pub(crate) fn db_fault(e: rusqlite::Error) -> Fault {
    match e {
        rusqlite::Error::SqliteFailure(failure, _) => Fault::Db(failure.extended_code),
        _ => Fault::Row,
    }
}

pub(crate) fn io_fault(e: std::io::Error) -> Fault {
    Fault::Io(e.kind())
}

/// Why `Store::open` refused. Its text names the fault and never a value of a record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// Another open store holds the writer lock of `derived/` or the database's exclusive lock.
    Locked,
    /// The key does not open the database, or the file is no database.
    Sealed,
    /// `kinds::check_entries` reports these faults for the entries that the store was opened with.
    Entries(Vec<String>),
    /// The entries lack this kind of the workflow run, or hold it changed.
    MissingKind(&'static str),
    /// A step or a handler of the host names this family, which is `workflow` or outside the form of a family.
    Family(&'static str),
    /// This setting of the connection is not in effect after the store set it.
    Setting(&'static str),
    /// The database holds a schema that is not this library's, or a schema with a table or an index missing.
    Schema,
    /// A fault of the database or of a file.
    Fault(Fault),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Locked => f.write_str("another store holds the derived database"),
            OpenError::Sealed => f.write_str("the key does not open the derived database"),
            OpenError::Entries(reports) => {
                write!(f, "the kind entries are refused: {}", reports.join("; "))
            }
            OpenError::MissingKind(kind) => {
                write!(
                    f,
                    "the kind entries lack the kind {kind} of the workflow run"
                )
            }
            OpenError::Family(family) => {
                write!(f, "a step or a handler names the family {family:?}")
            }
            OpenError::Setting(name) => write!(f, "the setting {name} is not in effect"),
            OpenError::Schema => f.write_str("the database holds another schema"),
            OpenError::Fault(fault) => fault.fmt(f),
        }
    }
}

impl std::error::Error for OpenError {}

impl From<Fault> for OpenError {
    fn from(fault: Fault) -> Self {
        OpenError::Fault(fault)
    }
}

/// The failure of a call of the store: an error name of the reply form, which the owner answers to the request,
/// or a fault of the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The request is refused with this error name.
    Refused(ErrorName),
    /// The store failed.
    Fault(Fault),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Refused(name) => write!(f, "refused: {name}"),
            Error::Fault(fault) => fault.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

impl From<Fault> for Error {
    fn from(fault: Fault) -> Self {
        Error::Fault(fault)
    }
}

impl From<ErrorName> for Error {
    fn from(name: ErrorName) -> Self {
        Error::Refused(name)
    }
}
