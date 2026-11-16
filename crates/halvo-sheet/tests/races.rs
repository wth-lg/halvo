mod common;

mod checkpoint_named_in_another_case {
    use std::fs;
    use std::path::Path;

    use super::common::{corpus, locks, Scratch, RUN};
    use halvo_sheet::{Error, Folder, Range, RunId, Value};

    #[test]
    fn a_workbook_path_that_names_a_runs_checkpoint_in_another_case_does_not_overwrite_it() {
        let scratch = Scratch::new("sweep-case-checkpoint");
        let original = fs::read(corpus("values.xlsx")).expect("read the corpus");
        scratch.write("values.xlsx", &original);
        if !scratch.path("VALUES.XLSX").exists() {
            return;
        }
        let folder = Folder::new(&scratch.dir, locks()).expect("the folder opens");
        let b5 = Range::parse("B5").expect("a range");
        let run1 = RunId::parse(RUN).expect("a run id");
        let first = folder
            .prepare(
                Path::new("values.xlsx"),
                &run1,
                "Values",
                b5,
                &[vec![Value::Number(1.0)]],
            )
            .expect("the first run prepares");
        let checkpoint = scratch.path(&format!("values.halvo-checkpoint-{RUN}.xlsx"));
        drop(first);
        assert_eq!(
            fs::read(&checkpoint).expect("read the checkpoint"),
            original
        );

        let run2 = RunId::parse("fedcba9876543210fedcba9876543210").expect("a run id");
        let variant = format!("values.halvo-Checkpoint-{RUN}.xlsx");
        let got = folder.prepare(
            Path::new(&variant),
            &run2,
            "Values",
            b5,
            &[vec![Value::Number(2.0)]],
        );
        let refused = matches!(got, Err(Error::Path(_)));
        if let Ok(prepared) = &got {
            let _ = folder.replace(prepared);
        }
        drop(got);
        assert_eq!(
            fs::read(&checkpoint).expect("the run's checkpoint is still there"),
            original,
            "a call of halvo-sheet changed the first run's checkpoint through a name in another case"
        );
        assert!(
            refused,
            "prepare took the first run's checkpoint, named in another case, as a workbook"
        );
    }
}

#[cfg(target_os = "macos")]
mod access_list_that_denies_delete {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::common::{corpus, locks, Scratch, RUN};
    use halvo_sheet::{Folder, Range, RunId, Value};

    struct Unlist(PathBuf);

    impl Drop for Unlist {
        fn drop(&mut self) {
            if let Ok(entries) = fs::read_dir(&self.0) {
                for entry in entries.flatten() {
                    let _ = Command::new("/bin/chmod")
                        .arg("-N")
                        .arg(entry.path())
                        .status();
                }
            }
        }
    }

    #[test]
    fn a_dropped_prepared_copy_of_a_workbook_whose_access_list_denies_delete_is_not_left_behind() {
        let scratch = Scratch::new("sweep-acl-delete");
        let _unlist = Unlist(scratch.dir.clone());
        let workbook = scratch.write(
            "values.xlsx",
            &fs::read(corpus("values.xlsx")).expect("read"),
        );
        let set = Command::new("/bin/chmod")
            .arg("+a")
            .arg("everyone deny delete")
            .arg(&workbook)
            .status()
            .expect("chmod runs");
        assert!(set.success(), "the access list is set");
        let folder = Folder::new(&scratch.dir, locks()).expect("the folder opens");
        let run = RunId::parse(RUN).expect("a run id");
        let b5 = Range::parse("B5").expect("a range");
        let prepared = folder
            .prepare(
                Path::new("values.xlsx"),
                &run,
                "Values",
                b5,
                &[vec![Value::Number(77.0)]],
            )
            .expect("a workbook whose access list denies delete prepares");
        assert_eq!(scratch.names("").len(), 3, "{:?}", scratch.names(""));
        drop(prepared);
        let checkpoint = format!("values.halvo-checkpoint-{RUN}.xlsx");
        assert_eq!(
            scratch.names(""),
            vec![checkpoint, "values.xlsx".to_string()],
            "the drop leaves the checkpoint alone beside the workbook: the copy that carried the \
             workbook's access list stays"
        );
    }
}
