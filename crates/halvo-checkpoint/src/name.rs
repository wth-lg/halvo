use std::ffi::{OsStr, OsString};
use std::path::Path;

use crate::RunId;

/// The file name of a file of Halvo's own beside the workbook at `workbook`:
/// `<name>.halvo-<tag><ext>`, where `<ext>` is the workbook's extension with its dot (nothing
/// when it has none) and `<name>` the rest of its file name, split as [`Path::file_stem`] and
/// [`Path::extension`] split it. `None` when the path has no file name.
pub fn own_name(workbook: &Path, tag: &str) -> Option<OsString> {
    let file = Path::new(workbook.file_name()?);
    let mut name = OsString::new();
    name.push(file.file_stem().unwrap_or_default());
    name.push(".halvo-");
    name.push(tag);
    if let Some(ext) = file.extension() {
        name.push(".");
        name.push(ext);
    }
    Some(name)
}

/// The file name of the checkpoint of `run` for the workbook at `workbook`:
/// `<name>.halvo-checkpoint-<run id><ext>` (contracts.md A11.3). This is the one place that
/// builds it: `prepare` makes the checkpoint under it, the runner writes it into the list's row,
/// and [`crate::remove`] removes no file of another name. `None` when the path has no file name.
pub fn checkpoint_name(workbook: &Path, run: &RunId) -> Option<OsString> {
    own_name(workbook, &format!("checkpoint-{}", run.as_str()))
}

/// The most units that one file name may hold, as the crate counts them: 255 bytes of UTF-8 on
/// macOS and 255 UTF-16 code units on Windows (NTFS counts code units). The APFS volume of a Mac
/// counts UTF-16 code units too: on the Mac that this crate was built on it makes a name of 255
/// CJK characters (765 bytes) and refuses one of 256, and refuses a name of 128 characters
/// outside the Basic Multilingual Plane (128 code points, 256 units). A name never has fewer
/// bytes than UTF-16 units, so counting bytes on macOS never takes a name that the volume
/// refuses, and it refuses a non-ASCII name that the volume would take: every name that the
/// crates build adds 82 units to the workbook's name, so a name of more than 173 bytes and at
/// most 173 units. `prepare` refuses a
/// workbook when a name that it or [`crate::remove`] builds for it would hold more
/// ([`names_fit`]). A volume whose names are shorter, such as a share whose server takes fewer
/// units, refuses such a name with an error of the system, which `prepare` answers as it
/// answers any error of a write.
pub const NAME_MAX: usize = 255;

/// The length of the file name `name` in the unit of [`NAME_MAX`]: its bytes on Unix, its
/// UTF-16 code units on Windows.
pub fn name_units(name: &OsStr) -> usize {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        name.encode_wide().count()
    }
    #[cfg(not(windows))]
    {
        name.as_encoded_bytes().len()
    }
}

/// The name that [`crate::remove`] moves the checkpoint `checkpoint` to before it removes it:
/// `<checkpoint>.halvo-removing-<16 hex digits of own>`, the longest name that the crate builds.
pub fn removing_name(checkpoint: &OsStr, own: u64) -> OsString {
    let mut name = checkpoint.to_os_string();
    name.push(format!(".halvo-removing-{own:016x}"));
    name
}

/// Whether each name that the crate builds for the checkpoint of `run` beside `workbook`, the
/// checkpoint's own ([`checkpoint_name`]) and the one that [`crate::remove`] moves it to
/// ([`removing_name`]), holds at most [`NAME_MAX`] units. `false` when the path has no file
/// name.
pub fn names_fit(workbook: &Path, run: &RunId) -> bool {
    checkpoint_name(workbook, run)
        .is_some_and(|name| name_units(&removing_name(&name, u64::MAX)) <= NAME_MAX)
}
