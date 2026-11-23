use halvo_checkpoint::Walk;

use crate::error::{Error, PathRefusal};

impl From<Walk> for Error {
    fn from(walk: Walk) -> Error {
        match walk {
            Walk::Folder => Error::Folder,
            Walk::Outside => Error::Path(PathRefusal::Outside),
            Walk::Link => Error::Path(PathRefusal::Link),
            Walk::Directory => Error::Path(PathRefusal::Directory),
            Walk::NotFile => Error::Path(PathRefusal::NotFile),
            Walk::Name => Error::Path(PathRefusal::Name),
            Walk::Io(kind) => Error::Io(kind),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Cause, Folder};
    use std::path::Path;

    /// r4 U3.22b sentence 3, the sheet crate's side: where a data file may lie against its
    /// workbook, over ONE table that a test of halvo-coord reads too
    /// (crates/halvo-coord/tests/typed/folders.txt). For each row of this build's form the
    /// workbook's folder is a scratch directory, given as the runner gives the folder of the run,
    /// and the data file's line names it in that folder's place; `Folder::read_source` refuses the
    /// data file's place, the cause `data_outside`, exactly where the row says refused (no data
    /// file stands there, so a taken place answers another cause).
    #[test]
    fn u322b_the_data_file_lies_in_the_workbooks_folder_exactly_where_the_shared_table_says() {
        let form = if cfg!(windows) { "windows" } else { "posix" };
        let dir = crate::testing::scratch("u322b-folders");
        let given = dir.to_str().expect("a scratch path in UTF-8");
        let folder = Folder::new(&dir, crate::testing::locks()).expect("the folder");
        let table = include_str!("../../halvo-coord/tests/typed/folders.txt");
        let mut read = 0;
        for line in table.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = line.split('\t').collect();
            assert_eq!(f.len(), 4, "{line}");
            if f[0] != form {
                continue;
            }
            let (workbook_folder, _) = f[2].rsplit_once(['/', '\\']).expect("a folder");
            let data = match f[3].strip_prefix(workbook_folder) {
                Some(rest) => format!("{given}{rest}"),
                None => f[3].to_owned(),
            };
            let error = folder
                .read_source(Path::new(&data))
                .expect_err("no data file stands there");
            assert_eq!(
                error.source_cause() == Cause::DataOutside,
                f[1] == "refused",
                "{line}: {error:?}"
            );
            read += 1;
        }
        assert!(read > 0, "rows of the form {form}");
    }
}
