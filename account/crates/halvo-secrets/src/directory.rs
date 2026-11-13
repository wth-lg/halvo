use std::path::{Component, Path, PathBuf};

/// The absolute path of the directory that a file-backed store keeps its item in, with no
/// `..` step, kept without a trailing separator or a `.` step so that its last step is the
/// directory itself and a link there is seen as a link. The crate compiles in no path: the
/// caller gives this one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Directory(PathBuf);

impl Directory {
    /// Takes `path`, or answers `None` when it is not absolute or holds a `..` step.
    pub fn new(path: PathBuf) -> Option<Directory> {
        if !path.is_absolute() || path.components().any(|step| step == Component::ParentDir) {
            return None;
        }
        Some(Directory(path.components().collect()))
    }

    /// The path, without a trailing separator or a `.` step.
    pub fn as_path(&self) -> &Path {
        &self.0
    }
}
