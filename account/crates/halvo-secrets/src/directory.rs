use std::path::{Path, PathBuf};

/// The absolute path of the directory that a file-backed store keeps its item in. The crate
/// compiles in no path: the caller gives this one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Directory(PathBuf);

impl Directory {
    /// Takes `path`, or answers `None` when it is not absolute.
    pub fn new(path: PathBuf) -> Option<Directory> {
        if path.is_absolute() {
            Some(Directory(path))
        } else {
            None
        }
    }

    /// The path as it was given.
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}
