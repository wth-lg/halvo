mod common;

use std::fs;
use std::path::Path;

use common::{book, corpus, number_at, same_but, Scratch, RUN};
use halvo_sheet::{Cause, Error, Folder, Kept, Prepared, Range, RunId, Value};

const FILE: &str = "values.xlsx";
const CHECKPOINT: &str = "values.halvo-checkpoint-0123456789abcdef0123456789abcdef.xlsx";

fn sorted(names: &[&str]) -> Vec<String> {
    let mut names: Vec<String> = names.iter().map(|n| n.to_string()).collect();
    names.sort();
    names
}

fn copy_of(prepared: &Prepared) -> String {
    let name = prepared.copy().file_name().expect("a name");
    name.to_string_lossy().into_owned()
}

struct Setup {
    folder: Folder,
    prepared: Prepared,
    before: Vec<u8>,
    copy: String,
}

fn setup(scratch: &Scratch) -> Setup {
    fs::copy(corpus(FILE), scratch.path(FILE)).expect("copy");
    let before = fs::read(scratch.path(FILE)).expect("read");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    let range = Range::parse("B5").expect("a range");
    let values = vec![vec![Value::Number(77.0)]];
    let prepared = folder
        .prepare(Path::new(FILE), &run, "Values", range, &values)
        .expect("prepared");
    let copy = copy_of(&prepared);
    Setup {
        folder,
        prepared,
        before,
        copy,
    }
}

#[test]
fn replace_puts_the_prepared_copy_in_place_and_a_second_call_answers_as_the_first() {
    let scratch = Scratch::new("replace-ok");
    let s = setup(&scratch);
    let copy = fs::read(scratch.path(&s.copy)).expect("read the copy");
    assert_eq!(s.folder.replace(&s.prepared), Ok(()));
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), copy);
    assert_eq!(fs::read(scratch.path(CHECKPOINT)).expect("read"), s.before);
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
    assert_eq!(s.folder.replace(&s.prepared), Ok(()));
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), copy);
    drop(s.prepared);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT]));
}

fn kept(scratch: &Scratch, s: &Setup, why: Kept, file: &[u8]) {
    let want = Err(Error::NotReplaced(why));
    assert_eq!(s.folder.replace(&s.prepared), want);
    assert_eq!(Error::NotReplaced(why).cause(), Cause::StepFailed);
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), file);
    assert!(!scratch.path(&s.copy).exists(), "the copy is left");
    assert_eq!(s.folder.replace(&s.prepared), want, "the second answer");
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), file);
}

#[test]
fn a_prepared_copy_that_no_replace_put_in_place_goes_when_it_is_dropped_and_the_checkpoint_stays() {
    let scratch = Scratch::new("replace-dropped");
    let s = setup(&scratch);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT, &s.copy]));
    drop(s.prepared);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT]));
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), s.before);
    let scratch = Scratch::new("replace-dropped-after");
    let s = setup(&scratch);
    let copy = fs::read(scratch.path(&s.copy)).expect("read the copy");
    assert_eq!(s.folder.replace(&s.prepared), Ok(()));
    fs::write(scratch.path(&s.copy), b"a file of the same name made later").expect("write");
    drop(s.prepared);
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), copy);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT, &s.copy]));
    let scratch = Scratch::new("replace-dropped-changed");
    let s = setup(&scratch);
    let theirs = b"other bytes written at the copy's name before the drop";
    fs::write(scratch.path(&s.copy), theirs).expect("write");
    drop(s.prepared);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT, &s.copy]));
    assert_eq!(fs::read(scratch.path(&s.copy)).expect("read"), theirs);
}

#[test]
fn a_workbook_changed_after_prepare_is_kept_as_the_person_left_it_with_the_checkpoint() {
    let scratch = Scratch::new("replace-changed");
    let s = setup(&scratch);
    let theirs = fs::read(corpus("formulas.xlsx")).expect("read");
    fs::write(scratch.path(FILE), &theirs).expect("the person saves the workbook");
    kept(&scratch, &s, Kept::Workbook, &theirs);
    drop(s.prepared);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT]));
}

#[test]
fn a_workbook_gone_or_turned_into_a_link_is_not_replaced() {
    let scratch = Scratch::new("replace-gone");
    let s = setup(&scratch);
    fs::rename(scratch.path(FILE), scratch.path("moved.xlsx")).expect("move it away");
    assert_eq!(
        s.folder.replace(&s.prepared),
        Err(Error::NotReplaced(Kept::Workbook))
    );
    assert!(!scratch.path(FILE).exists(), "replace made a workbook");
    assert_eq!(scratch.names(""), sorted(&["moved.xlsx", CHECKPOINT]));
    let scratch = Scratch::new("replace-link");
    let s = setup(&scratch);
    fs::rename(scratch.path(FILE), scratch.path("real.xlsx")).expect("move it away");
    common::link_file(&scratch.path("real.xlsx"), &scratch.path(FILE));
    assert_eq!(
        s.folder.replace(&s.prepared),
        Err(Error::NotReplaced(Kept::Workbook))
    );
    assert_eq!(fs::read(scratch.path("real.xlsx")).expect("read"), s.before);
    assert!(fs::symlink_metadata(scratch.path(FILE))
        .expect("the link")
        .file_type()
        .is_symlink());
}

#[test]
fn a_link_in_the_place_of_the_checkpoint_or_of_the_copy_is_not_renamed_into_place() {
    for place in ["checkpoint", "copy"] {
        let scratch = Scratch::new("replace-link-place");
        let s = setup(&scratch);
        let (name, why) = match place {
            "checkpoint" => (CHECKPOINT.to_string(), Kept::Checkpoint),
            _ => (s.copy.clone(), Kept::Copy),
        };
        let behind = scratch.path("behind.xlsx");
        fs::rename(scratch.path(&name), &behind).expect("move the file behind the link");
        common::link_file(&behind, &scratch.path(&name));
        assert_eq!(
            s.folder.replace(&s.prepared),
            Err(Error::NotReplaced(why)),
            "{place}"
        );
        assert_eq!(
            fs::read(scratch.path(FILE)).expect("read"),
            s.before,
            "{place}"
        );
        let is_file = fs::symlink_metadata(scratch.path(FILE))
            .expect("the workbook")
            .file_type()
            .is_file();
        assert!(is_file, "{place}: the workbook is a file");
    }
}

#[test]
fn a_directory_turned_into_a_link_after_prepare_is_not_written_through() {
    let scratch = Scratch::new("replace-dir-link");
    fs::create_dir_all(scratch.path("sub")).expect("make the directory");
    fs::copy(corpus(FILE), scratch.path("sub").join(FILE)).expect("copy");
    let before = fs::read(scratch.path("sub").join(FILE)).expect("read");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    let range = Range::parse("B5").expect("a range");
    let values = vec![vec![Value::Number(77.0)]];
    let prepared = folder
        .prepare(Path::new("sub/values.xlsx"), &run, "Values", range, &values)
        .expect("prepared");
    let copy = copy_of(&prepared);
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
    assert_eq!(scratch.names("real"), sorted(&[FILE, CHECKPOINT, &copy]));
    drop(prepared);
    assert_eq!(scratch.names("real"), sorted(&[FILE, CHECKPOINT, &copy]));
}

#[test]
fn a_checkpoint_gone_or_changed_keeps_the_workbook() {
    let scratch = Scratch::new("replace-checkpoint");
    let s = setup(&scratch);
    fs::write(scratch.path(CHECKPOINT), b"not the workbook").expect("write");
    kept(&scratch, &s, Kept::Checkpoint, &s.before);
    let scratch = Scratch::new("replace-checkpoint-gone");
    let s = setup(&scratch);
    fs::rename(scratch.path(CHECKPOINT), scratch.path("elsewhere.xlsx")).expect("move");
    kept(&scratch, &s, Kept::Checkpoint, &s.before);
}

#[test]
fn a_prepared_copy_gone_or_changed_keeps_the_workbook() {
    let scratch = Scratch::new("replace-copy");
    let s = setup(&scratch);
    fs::write(
        scratch.path(&s.copy),
        fs::read(corpus("formulas.xlsx")).expect("read"),
    )
    .expect("write");
    kept(&scratch, &s, Kept::Copy, &s.before);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT]));
    let scratch = Scratch::new("replace-copy-gone");
    let s = setup(&scratch);
    fs::rename(scratch.path(&s.copy), scratch.path("elsewhere.xlsx")).expect("move");
    kept(&scratch, &s, Kept::Copy, &s.before);
}

#[test]
fn a_folder_that_does_not_hold_the_workbook_touches_nothing_and_is_no_first_replace() {
    let scratch = Scratch::new("replace-folder");
    let s = setup(&scratch);
    let other = Scratch::new("replace-other-folder");
    let elsewhere = Folder::new(&other.dir, common::locks()).expect("the folder opens");
    assert_eq!(
        elsewhere.replace(&s.prepared),
        Err(Error::Path(halvo_sheet::PathRefusal::Outside))
    );
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), s.before);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT, &s.copy]));
    assert_eq!(s.folder.replace(&s.prepared), Ok(()));
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT]));
}

#[cfg(unix)]
#[test]
fn a_copy_that_a_read_only_directory_keeps_is_named_in_the_answer_and_goes_at_the_drop() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("replace-read-only");
    let s = setup(&scratch);
    let mode = |m: u32| {
        fs::set_permissions(&scratch.dir, fs::Permissions::from_mode(m)).expect("chmod");
    };
    mode(0o555);
    let got = s.folder.replace(&s.prepared);
    let second = s.folder.replace(&s.prepared);
    mode(0o755);
    let left = Err(Error::CopyLeft(Kept::Rename(
        std::io::ErrorKind::PermissionDenied,
    )));
    assert_eq!(got, left);
    assert_eq!(second, left);
    assert_eq!(Error::CopyLeft(Kept::Copy).cause(), Cause::StepFailed);
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), s.before);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT, &s.copy]));
    drop(s.prepared);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT]));
}

#[cfg(target_os = "macos")]
#[test]
fn a_rename_that_the_system_refuses_keeps_the_workbook_on_macos() {
    let scratch = Scratch::new("replace-immutable");
    let s = setup(&scratch);
    let flag = |value: &str| {
        let status = std::process::Command::new("chflags")
            .arg(value)
            .arg(scratch.path(FILE))
            .status()
            .expect("chflags runs");
        assert!(status.success(), "chflags {value}");
    };
    flag("uchg");
    let got = s.folder.replace(&s.prepared);
    let second = s.folder.replace(&s.prepared);
    let file = fs::read(scratch.path(FILE)).expect("read");
    let left = scratch.names("");
    flag("nouchg");
    let refused = Err(Error::NotReplaced(Kept::Rename(
        std::io::ErrorKind::PermissionDenied,
    )));
    assert_eq!(got, refused);
    assert_eq!(second, refused);
    assert_eq!(file, s.before);
    assert_eq!(left, sorted(&[FILE, CHECKPOINT]));
}

#[cfg(windows)]
#[test]
fn a_rename_that_the_system_refuses_keeps_the_workbook_on_windows() {
    use std::os::windows::fs::OpenOptionsExt;
    let scratch = Scratch::new("replace-open");
    let s = setup(&scratch);
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(scratch.path(FILE))
        .expect("open without delete sharing");
    let got = s.folder.replace(&s.prepared);
    let second = s.folder.replace(&s.prepared);
    drop(held);
    assert!(
        matches!(got, Err(Error::NotReplaced(Kept::Rename(_)))),
        "{got:?}"
    );
    assert_eq!(second, got);
    assert_eq!(fs::read(scratch.path(FILE)).expect("read"), s.before);
    assert_eq!(scratch.names(""), sorted(&[FILE, CHECKPOINT]));
}
