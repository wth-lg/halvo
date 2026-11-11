mod common;

use std::fs;
use std::path::Path;

use common::{book, corpus, number_at, same_but, Scratch, RUN};
use halvo_sheet::{Cause, Error, Folder, Kept, Prepared, Range, RunId, Value};

const FILE: &str = "values.xlsx";
const CHECKPOINT: &str = "values.halvo-checkpoint-0123456789abcdef0123456789abcdef.xlsx";
const COPY: &str = "values.halvo-prepared-0123456789abcdef0123456789abcdef.xlsx";

fn sorted(names: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
    names.sort();
    names
}

fn setup(scratch: &Scratch) -> (Folder, Prepared, Vec<u8>) {
    fs::copy(corpus(FILE), scratch.path(FILE)).expect("copy");
    let before = fs::read(scratch.path(FILE)).expect("read");
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    let range = Range::parse("B5").expect("a range");
    let values = vec![vec![Value::Number(77.0)]];
    let prepared = folder
        .prepare(Path::new(FILE), &run, "Values", range, &values)
        .expect("prepared");
    (folder, prepared, before)
}

#[test]
fn replace_puts_the_prepared_copy_in_place_and_a_second_call_answers_as_the_first() {
    let scratch = Scratch::new("replace-ok");
    let (folder, prepared, before) = setup(&scratch);
    let copy = fs::read(scratch.path(COPY)).expect("read the copy");
    assert_eq!(folder.replace(&prepared), Ok(()));
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), copy);
    assert_eq!(fs::read(scratch.path(CHECKPOINT)).expect("read"), before);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT]));
    let saved = book(&scratch.path(FILE));
    assert_eq!(number_at(&saved, "Values", (4, 1)), Some(77.0));
    assert_eq!(
        same_but(
            &book(&scratch.path(CHECKPOINT)),
            &saved,
            "Values",
            &[(4, 1)]
        ),
        Vec::<String>::new()
    );
    assert_eq!(folder.replace(&prepared), Ok(()));
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), copy);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT]));
}

fn kept(scratch: &Scratch, folder: &Folder, prepared: &Prepared, why: Kept, file: &[u8]) {
    let want = Err(Error::NotReplaced(why));
    assert_eq!(folder.replace(prepared), want);
    assert_eq!(Error::NotReplaced(why).cause(), Cause::StepFailed);
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), file);
    assert!(!scratch.path(COPY).exists(), "the copy is left");
    assert_eq!(folder.replace(prepared), want, "the second answer");
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), file);
}

#[test]
fn a_workbook_changed_after_prepare_is_kept_as_the_person_left_it() {
    let scratch = Scratch::new("replace-changed");
    let (folder, prepared, _) = setup(&scratch);
    let theirs = fs::read(corpus("formulas.xlsx")).expect("read");
    fs::write(scratch.path(FILE), &theirs).expect("the person saves the workbook");
    kept(&scratch, &folder, &prepared, Kept::Workbook, &theirs);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT]));
}

#[test]
fn a_workbook_gone_or_turned_into_a_link_is_not_replaced() {
    let scratch = Scratch::new("replace-gone");
    let (folder, prepared, before) = setup(&scratch);
    fs::rename(scratch.path(FILE), scratch.path("moved.xlsx")).expect("move it away");
    assert_eq!(
        folder.replace(&prepared),
        Err(Error::NotReplaced(Kept::Workbook))
    );
    assert!(!scratch.path(FILE).exists(), "replace made a workbook");
    assert_eq!(scratch.names(""), sorted(&["moved.xlsx", CHECKPOINT]));
    let scratch = Scratch::new("replace-link");
    let (folder, prepared, _) = setup(&scratch);
    fs::rename(scratch.path(FILE), scratch.path("real.xlsx")).expect("move it away");
    common::link_file(&scratch.path("real.xlsx"), &scratch.path(FILE));
    assert_eq!(
        folder.replace(&prepared),
        Err(Error::NotReplaced(Kept::Workbook))
    );
    assert_eq!(fs::read(scratch.path("real.xlsx")).expect("read"), before);
    assert!(fs::symlink_metadata(scratch.path(FILE))
        .expect("the link")
        .file_type()
        .is_symlink());
}

#[test]
fn a_directory_turned_into_a_link_after_prepare_is_not_written_through() {
    let scratch = Scratch::new("replace-dir-link");
    fs::create_dir_all(scratch.path("sub")).expect("make the directory");
    fs::copy(corpus(FILE), scratch.path("sub").join(FILE)).expect("copy");
    let before = fs::read(scratch.path("sub").join(FILE)).expect("read");
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    let range = Range::parse("B5").expect("a range");
    let values = vec![vec![Value::Number(77.0)]];
    let prepared = folder
        .prepare(Path::new("sub/values.xlsx"), &run, "Values", range, &values)
        .expect("prepared");
    fs::rename(scratch.path("sub"), scratch.path("real")).expect("move the directory");
    common::link_dir(&scratch.path("real"), &scratch.path("sub"));
    assert_eq!(
        folder.replace(&prepared),
        Err(Error::NotReplaced(Kept::Workbook))
    );
    assert_eq!(
        fs::read(scratch.path("real").join(FILE)).expect("read"),
        before
    );
    assert_eq!(scratch.names("real"), sorted(&[FILE, CHECKPOINT, COPY]));
}

#[test]
fn a_checkpoint_gone_or_changed_keeps_the_workbook() {
    let scratch = Scratch::new("replace-checkpoint");
    let (folder, prepared, before) = setup(&scratch);
    fs::write(scratch.path(CHECKPOINT), b"not the workbook").expect("write");
    kept(&scratch, &folder, &prepared, Kept::Checkpoint, &before);
    let scratch = Scratch::new("replace-checkpoint-gone");
    let (folder, prepared, before) = setup(&scratch);
    fs::rename(scratch.path(CHECKPOINT), scratch.path("elsewhere.xlsx")).expect("move");
    kept(&scratch, &folder, &prepared, Kept::Checkpoint, &before);
}

#[test]
fn a_prepared_copy_gone_or_changed_keeps_the_workbook() {
    let scratch = Scratch::new("replace-copy");
    let (folder, prepared, before) = setup(&scratch);
    fs::write(
        scratch.path(COPY),
        fs::read(corpus("formulas.xlsx")).expect("read"),
    )
    .expect("write");
    kept(&scratch, &folder, &prepared, Kept::Copy, &before);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT]));
    let scratch = Scratch::new("replace-copy-gone");
    let (folder, prepared, before) = setup(&scratch);
    fs::rename(scratch.path(COPY), scratch.path("elsewhere.xlsx")).expect("move");
    kept(&scratch, &folder, &prepared, Kept::Copy, &before);
}

#[test]
fn a_folder_that_does_not_hold_the_workbook_touches_nothing_and_is_no_first_replace() {
    let scratch = Scratch::new("replace-folder");
    let (folder, prepared, before) = setup(&scratch);
    let other = Scratch::new("replace-other-folder");
    let elsewhere = Folder::new(&other.dir).expect("the folder opens");
    assert_eq!(
        elsewhere.replace(&prepared),
        Err(Error::Path(halvo_sheet::PathRefusal::Outside))
    );
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), before);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT, COPY]));
    assert_eq!(folder.replace(&prepared), Ok(()));
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT]));
}

#[cfg(target_os = "macos")]
#[test]
fn a_rename_that_the_system_refuses_keeps_the_workbook_on_macos() {
    let scratch = Scratch::new("replace-immutable");
    let (folder, prepared, before) = setup(&scratch);
    let flag = |value: &str| {
        let status = std::process::Command::new("chflags")
            .arg(value)
            .arg(scratch.path(FILE))
            .status()
            .expect("chflags runs");
        assert!(status.success(), "chflags {value}");
    };
    flag("uchg");
    let got = folder.replace(&prepared);
    let second = folder.replace(&prepared);
    let file = fs::read(scratch.path(FILE)).expect("read");
    let left = scratch.names("");
    flag("nouchg");
    let refused = Err(Error::NotReplaced(Kept::Rename(
        std::io::ErrorKind::PermissionDenied,
    )));
    assert_eq!(got, refused);
    assert_eq!(second, refused);
    assert_eq!(file, before);
    assert_eq!(left, sorted(&[FILE, CHECKPOINT]));
}

#[cfg(windows)]
#[test]
fn a_rename_that_the_system_refuses_keeps_the_workbook_on_windows() {
    use std::os::windows::fs::OpenOptionsExt;
    let scratch = Scratch::new("replace-open");
    let (folder, prepared, before) = setup(&scratch);
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(scratch.path(FILE))
        .expect("open without delete sharing");
    let got = folder.replace(&prepared);
    let second = folder.replace(&prepared);
    drop(held);
    assert!(
        matches!(got, Err(Error::NotReplaced(Kept::Rename(_)))),
        "{got:?}"
    );
    assert_eq!(second, got);
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), before);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT]));
}
