//! The checkpoint of the sheet executor, with no reader of workbooks (contracts.md A11.3, A6.6,
//! A7.4): the name of the copy that `prepare` keeps beside a workbook, the walk of a path inside
//! a folder, the SHA-256 of a file, and the lock of a workbook's directory, which `prepare` and
//! `replace` hold. The sheet executor and the supervisor library both use it, so that the app
//! links no reader of workbooks (the coordinator's ruling CP-98 (2)).

mod at;
mod digest;
mod identity;
mod lock;
mod name;
mod remove;
mod run;
mod walk;

pub use digest::Sha256;
pub use identity::{same_file, FileId};
pub use lock::{Held, LockError, Locks, Mark, Marked};
pub use name::{checkpoint_name, name_units, names_fit, own_name, removing_name, NAME_MAX};
pub use remove::{remove, Expect, Left, Removal, Row, Undecided};
pub use run::RunId;
pub use walk::{open_regular, regular, same_entry, Root, Walk};

/// The bound of a workbook file and so of a checkpoint, 64 MiB (contracts.md A11.3): a file is
/// read up to it and no further.
pub const FILE_MAX: u64 = 64 << 20;
