//! The mode `--sheet-names` of the runner (r4 U3.22c): it reads the names of the worksheets of
//! ONE workbook by the sheet crate, prints them and ends. It starts no agent, no run and no
//! browser, opens no store, no link and no lock, and writes nothing.

use std::io::Write;
use std::path::Path;

/// Why the mode prints no name: `cause`, the name of A7.4 that the sheet crate's
/// `Error::cause` gives (`no_workbook`, `no_access`, `workbook` among them), or `usage`, and a
/// detail that holds no content of the workbook.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Refused {
    pub cause: &'static str,
    pub detail: String,
}

/// The names of the worksheets of the workbook at the full path `workbook`, in the workbook's
/// order (`halvo_sheet::sheet_names`), or why there are none to print: `usage` for a path that
/// is not absolute; the sheet crate's cause for a path that is not there, a file that the system
/// does not let it open and a file that the crate refuses; and `workbook` for a name that holds
/// a line feed or a carriage return, which cannot be printed as a line of its own.
pub(crate) fn names(workbook: &Path) -> Result<Vec<String>, Refused> {
    if !workbook.is_absolute() {
        return Err(Refused {
            cause: "usage",
            detail: "--sheet-names takes a full path".to_string(),
        });
    }
    let names = halvo_sheet::sheet_names(workbook).map_err(|error| Refused {
        cause: error.cause().name(),
        detail: error.to_string(),
    })?;
    if names.iter().any(|name| name.contains(['\n', '\r'])) {
        return Err(Refused {
            cause: halvo_sheet::Cause::Workbook.name(),
            detail: "a sheet's name holds a line break".to_string(),
        });
    }
    Ok(names)
}

/// Runs the mode for `workbook`: each name and a line feed on standard output, in the
/// workbook's order, and 0; 1 when standard output takes no line; for a refusal no name, one
/// line `REFUSED <cause>: <detail>` on standard error, and 2.
pub(crate) fn main(workbook: &Path) -> i32 {
    match names(workbook) {
        Ok(names) => {
            let mut out = std::io::stdout().lock();
            let written = names
                .iter()
                .try_for_each(|name| writeln!(out, "{name}"))
                .and_then(|()| out.flush());
            if written.is_ok() {
                0
            } else {
                1
            }
        }
        Err(refused) => {
            crate::line(&format!("REFUSED {}: {}", refused.cause, refused.detail));
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::names;

    fn scratch(tag: &str) -> PathBuf {
        let exe = std::env::current_exe().expect("the test binary");
        let target = exe
            .ancestors()
            .nth(3)
            .expect("the target directory above deps");
        halvo_test_dir::make(&target.join("tmp").join("halvo-runner-unit"), tag)
            .expect("make the scratch directory")
    }

    /// r4 U3.22c: over a workbook of three sheets that the sheet crate's testing code wrote, the
    /// mode answers the three names in the workbook's order; over a file that is no workbook and
    /// over a path that is not there it answers its refusal, with its cause, and no name.
    #[test]
    fn the_mode_answers_the_names_in_the_workbooks_order_and_a_refusal_for_no_workbook_or_no_file()
    {
        let dir = scratch("sheet-names");
        let book = dir.join("book.xlsx");
        let sheets = ["January", "February", "Summary"];
        std::fs::write(&book, halvo_sheet::made::workbook(&sheets)).expect("write the workbook");
        assert_eq!(
            names(&book),
            Ok(sheets.iter().map(|s| s.to_string()).collect::<Vec<_>>())
        );
        let text = dir.join("notes.xlsx");
        std::fs::write(&text, b"these are notes, not a workbook").expect("write the file");
        assert_eq!(names(&text).map_err(|r| r.cause), Err("workbook"));
        let gone = dir.join("gone.xlsx");
        assert_eq!(names(&gone).map_err(|r| r.cause), Err("no_workbook"));
    }
}
