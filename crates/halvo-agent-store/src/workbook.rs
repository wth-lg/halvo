/// The two forms of an absolute path that the slot `workbook` may hold (A9.4): POSIX, which starts with `/` and
/// whose separator is `/`, and Windows, which starts with a drive letter, `:` and a separator, and whose separators
/// are `\` and `/`. A UNC path and a path with the prefix `\\?\` are no path of the Windows form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathForm {
    Posix,
    Windows,
}

impl PathForm {
    /// The form of the system that the crate is built for, which `set_bindings` checks.
    pub const NATIVE: PathForm = if cfg!(windows) {
        PathForm::Windows
    } else {
        PathForm::Posix
    };

    fn separators(self) -> &'static [char] {
        match self {
            PathForm::Posix => &['/'],
            PathForm::Windows => &['/', '\\'],
        }
    }
}

/// Why `workbook_path` refused a path, first that fits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathRefused {
    /// The path does not start as an absolute path of its form.
    Relative,
    /// An empty step: a doubled separator, or a separator at the end.
    EmptyStep,
    /// A step that is `.` or `..`.
    DotStep,
}

/// Whether `path` is an absolute path in one spelling (A9.4, CP-98 (5)): absolute in `form`, and with no empty
/// step and no `.` or `..` step after its root. Two spellings that differ in letter case stay two paths.
pub fn workbook_path(path: &str, form: PathForm) -> Result<(), PathRefused> {
    let steps = match form {
        PathForm::Posix => path.strip_prefix('/'),
        PathForm::Windows => drive(path),
    }
    .ok_or(PathRefused::Relative)?;
    for step in steps.split(form.separators()) {
        if step.is_empty() {
            return Err(PathRefused::EmptyStep);
        }
        if step == "." || step == ".." {
            return Err(PathRefused::DotStep);
        }
    }
    Ok(())
}

fn drive(path: &str) -> Option<&str> {
    let bytes = path.as_bytes();
    let rooted = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    rooted.then(|| &path[3..])
}
