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

#[cfg(test)]
mod core_table {
    use super::{workbook_path, PathForm};

    /// r4 U3.22a sentence 3: the core checks a typed path by this rule before anything is sent,
    /// over ONE table that its own test reads too (crates/halvo-coord/tests/typed/paths.txt). For
    /// every row of both forms, `workbook_path` takes the line that the core checks exactly where
    /// the row says that the core takes it, so that no typed line of a person reaches the refusal
    /// `binding` of `set_bindings`.
    #[test]
    fn workbook_path_takes_exactly_the_lines_that_the_cores_table_of_typed_paths_takes() {
        let table = include_str!("../../halvo-coord/tests/typed/paths.txt");
        let mut rows = 0;
        for line in table.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = line.split('\t').collect();
            assert_eq!(f.len(), 4, "{line}");
            let form = if f[0] == "windows" {
                PathForm::Windows
            } else {
                assert_eq!(f[0], "posix", "{line}");
                PathForm::Posix
            };
            assert_eq!(workbook_path(f[3], form).is_ok(), f[1] == "taken", "{line}");
            rows += 1;
        }
        assert!(rows > 0, "the table holds rows");
    }
}
