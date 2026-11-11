//! The sheet executor of the agent-setup run (contracts.md A11.3).

mod error;
mod path;
mod source;

use std::path::{Path, PathBuf};

pub use error::{Cause, Error, PathRefusal, SourceRefusal};
pub use source::Value;

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
}
