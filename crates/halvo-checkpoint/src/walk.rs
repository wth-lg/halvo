use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::identity::{same_file, FileId};

/// Why the walk of a path refused it, or what the system answered on the way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Walk {
    /// The folder is not a directory that can be opened.
    Folder,
    /// The path is absolute and not under the folder, or it names a parent directory.
    Outside,
    /// The path or a directory on the way to it is a link (a symbolic link, or on Windows a
    /// junction or another name-surrogate reparse point).
    Link,
    /// The path is a directory.
    Directory,
    /// The path is the folder itself, a part of the way is not a directory, or the path is not
    /// a regular file.
    NotFile,
    /// A part of the path holds a colon, which on Windows names a stream of a file.
    Name,
    /// The system answered an error of this kind, `NotFound` for a part that is absent.
    Io(io::ErrorKind),
}

impl From<io::Error> for Walk {
    fn from(error: io::Error) -> Walk {
        Walk::Io(error.kind())
    }
}

/// A folder that a call works inside alone: the directory of the permission `user_files`, as
/// the runner gives it or as the list's row stores it (contracts.md A11.3, A7.4). A path of a
/// call is a path relative to the folder, or an absolute path that starts with the folder as it
/// was given or with its canonical form (on Windows also that form without its `\\?\` prefix).
/// The folder itself may be reached through a link; no part of the way below it may be one.
#[derive(Clone)]
pub struct Root {
    root: PathBuf,
    spellings: Vec<PathBuf>,
}

impl fmt::Debug for Root {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Root").finish_non_exhaustive()
    }
}

impl Root {
    /// Takes the folder. [`Walk::Io`] with `NotFound` when it is absent, [`Walk::Folder`] when
    /// it cannot be opened or is not a directory.
    pub fn new(dir: &Path) -> Result<Root, Walk> {
        let root = fs::canonicalize(dir).map_err(|error| match error.kind() {
            io::ErrorKind::NotFound => Walk::Io(io::ErrorKind::NotFound),
            _ => Walk::Folder,
        })?;
        let meta = fs::metadata(&root).map_err(|_| Walk::Folder)?;
        if !meta.is_dir() {
            return Err(Walk::Folder);
        }
        let mut spellings = vec![root.clone(), dir.to_path_buf()];
        spellings.extend(plain(&root));
        Ok(Root { root, spellings })
    }

    /// The folder's canonical form.
    pub fn dir(&self) -> &Path {
        &self.root
    }

    /// The names of `path` below the folder, in order; [`Walk::Outside`] for an absolute path
    /// under no spelling of the folder and for a path with a parent step or a root.
    pub fn names<'a>(&self, path: &'a Path) -> Result<Vec<&'a OsStr>, Walk> {
        let rel = if path.is_absolute() {
            self.spellings
                .iter()
                .find_map(|spelling| path.strip_prefix(spelling).ok())
                .ok_or(Walk::Outside)?
        } else {
            path
        };
        let mut names = Vec::new();
        for component in rel.components() {
            match component {
                Component::Normal(name) => names.push(name),
                Component::CurDir => {}
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err(Walk::Outside)
                }
            }
        }
        Ok(names)
    }

    /// The directory that holds the last name of `path`, walked from the folder, and that last
    /// name, which is not looked at: each directory on the way is a directory and no link.
    /// [`Walk::NotFile`] for the folder itself.
    pub fn parent<'a>(&self, path: &'a Path) -> Result<(PathBuf, &'a OsStr), Walk> {
        let names = self.names(path)?;
        let Some((last, dirs)) = names.split_last() else {
            return Err(Walk::NotFile);
        };
        let mut full = self.root.clone();
        for name in dirs {
            named(name)?;
            full.push(name);
            let meta = fs::symlink_metadata(&full)?;
            if meta.file_type().is_symlink() {
                return Err(Walk::Link);
            }
            if !meta.is_dir() {
                return Err(Walk::NotFile);
            }
        }
        named(last)?;
        Ok((full, last))
    }

    /// The full path of the regular file that `path` names, walked as [`Root::parent`] walks,
    /// the file itself no link, no directory and a regular file.
    pub fn file(&self, path: &Path) -> Result<PathBuf, Walk> {
        let (dir, last) = self.parent(path)?;
        let full = dir.join(last);
        regular(&full)?;
        Ok(full)
    }
}

/// Whether `full` is a regular file, read without following a link.
pub fn regular(full: &Path) -> Result<(), Walk> {
    let meta = fs::symlink_metadata(full)?;
    if meta.file_type().is_symlink() {
        Err(Walk::Link)
    } else if meta.is_dir() {
        Err(Walk::Directory)
    } else if !meta.is_file() {
        Err(Walk::NotFile)
    } else {
        Ok(())
    }
}

/// Opens the regular file at `full` for reading without following a link: [`Walk::Link`] when
/// it is a link, [`Walk::Directory`] or [`Walk::NotFile`] when it is no regular file, and also
/// [`Walk::NotFile`] when another file took its place between the look and the open.
pub fn open_regular(full: &Path) -> Result<File, Walk> {
    regular(full)?;
    let named = FileId::at(full)?;
    let file = no_follow(full).map_err(|error| {
        if is_loop(&error) {
            Walk::Link
        } else {
            Walk::from(error)
        }
    })?;
    let seen = file.metadata()?;
    if seen.file_type().is_symlink() {
        return Err(Walk::Link);
    }
    if !seen.is_file() || !same_file(&named, &FileId::of(&file)?) {
        return Err(Walk::NotFile);
    }
    Ok(file)
}

/// Whether the entry at `full`, read without following a link, is a regular file and the file
/// that `file` has open, compared by [`same_file`].
pub fn same_entry(file: &File, full: &Path) -> bool {
    let regular = fs::symlink_metadata(full)
        .is_ok_and(|named| !named.file_type().is_symlink() && named.is_file());
    regular
        && match (FileId::of(file), FileId::at(full)) {
            (Ok(opened), Ok(named)) => same_file(&opened, &named),
            _ => false,
        }
}

pub(crate) fn no_follow(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(0x0100);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x0020_0000);
    }
    options.open(path)
}

#[cfg(target_os = "macos")]
pub(crate) fn is_loop(error: &io::Error) -> bool {
    error.raw_os_error() == Some(62)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn is_loop(_: &io::Error) -> bool {
    false
}

#[cfg(windows)]
fn plain(root: &Path) -> Option<PathBuf> {
    let rest = root.to_str()?.strip_prefix(r"\\?\")?;
    let bytes = rest.as_bytes();
    let disk =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    disk.then(|| PathBuf::from(rest))
}

#[cfg(not(windows))]
fn plain(_: &Path) -> Option<PathBuf> {
    None
}

#[cfg(windows)]
fn named(name: &OsStr) -> Result<(), Walk> {
    if name.to_string_lossy().contains(':') {
        Err(Walk::Name)
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn named(_: &OsStr) -> Result<(), Walk> {
    Ok(())
}
