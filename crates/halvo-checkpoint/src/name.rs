use std::ffi::OsString;
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
