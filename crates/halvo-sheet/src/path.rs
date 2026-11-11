use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::{Error, PathRefusal};

pub(crate) fn folder(dir: &Path) -> Result<(PathBuf, Vec<PathBuf>), Error> {
    let root = fs::canonicalize(dir).map_err(|_| Error::Folder)?;
    let meta = fs::metadata(&root).map_err(|_| Error::Folder)?;
    if !meta.is_dir() {
        return Err(Error::Folder);
    }
    let mut spellings = vec![root.clone(), dir.to_path_buf()];
    spellings.extend(plain(&root));
    Ok((root, spellings))
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

pub(crate) fn file(root: &Path, spellings: &[PathBuf], path: &Path) -> Result<PathBuf, Error> {
    let rel = if path.is_absolute() {
        spellings
            .iter()
            .find_map(|spelling| path.strip_prefix(spelling).ok())
            .ok_or(Error::Path(PathRefusal::Outside))?
    } else {
        path
    };
    let mut names = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(name) => names.push(name),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(Error::Path(PathRefusal::Outside))
            }
        }
    }
    let Some((last, dirs)) = names.split_last() else {
        return Err(Error::Path(PathRefusal::NotFile));
    };
    let mut full = root.to_path_buf();
    for name in dirs {
        named(name)?;
        full.push(name);
        let meta = fs::symlink_metadata(&full)?;
        if meta.file_type().is_symlink() {
            return Err(Error::Path(PathRefusal::Link));
        }
        if !meta.is_dir() {
            return Err(Error::Path(PathRefusal::NotFile));
        }
    }
    named(last)?;
    full.push(last);
    regular(&full)?;
    Ok(full)
}

pub(crate) fn regular(full: &Path) -> Result<(), Error> {
    let meta = fs::symlink_metadata(full)?;
    if meta.file_type().is_symlink() {
        Err(Error::Path(PathRefusal::Link))
    } else if meta.is_dir() {
        Err(Error::Path(PathRefusal::Directory))
    } else if !meta.is_file() {
        Err(Error::Path(PathRefusal::NotFile))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn named(name: &std::ffi::OsStr) -> Result<(), Error> {
    if name.to_string_lossy().contains(':') {
        Err(Error::Path(PathRefusal::Name))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn named(_: &std::ffi::OsStr) -> Result<(), Error> {
    Ok(())
}
