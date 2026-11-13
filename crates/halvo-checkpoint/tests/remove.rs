mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{folds_case, link_dir, link_file, Scratch, RUN};
use halvo_checkpoint::{
    checkpoint_name, remove, Expect, Left, LockError, Locks, Removal, Row, RunId, Sha256,
    Undecided, FILE_MAX,
};

const COPY: &[u8] = b"the workbook as prepare read it";

fn run() -> RunId {
    RunId::parse(RUN).expect("a run id")
}

fn locks(scratch: &Scratch) -> Locks {
    fs::create_dir_all(scratch.path("data")).expect("make the data directory");
    Locks::open(&scratch.path("data"), Duration::from_millis(200)).expect("the locks open")
}

fn copy_name(workbook: &str) -> String {
    let name = checkpoint_name(Path::new(workbook), &run()).expect("a name");
    name.into_string().expect("utf-8")
}

fn made(bytes: &[u8]) -> Expect {
    Expect::Made {
        sha256: Sha256::of(bytes),
        bytes: bytes.len() as u64,
    }
}

fn with_checkpoint(scratch: &Scratch, rel: &str) -> PathBuf {
    let workbook = scratch.write(&format!("folder/{rel}"), b"the workbook now");
    let copy = workbook.with_file_name(copy_name(rel));
    fs::write(&copy, COPY).expect("write the checkpoint");
    copy
}

fn answer(scratch: &Scratch, workbook: &Path, expect: Expect) -> Result<Removal, Undecided> {
    let run = run();
    let folder = scratch.path("folder");
    let row = Row {
        folder: &folder,
        workbook,
        run: &run,
        expect,
    };
    remove(&locks(scratch), &row)
}

#[test]
fn a_checkpoint_whose_checks_all_hold_is_removed() {
    let scratch = Scratch::new("remove-ok");
    let copy = with_checkpoint(&scratch, "sub/values.xlsx");
    scratch.write("folder/sub/other.xlsx", b"another file");
    let workbook = scratch.path("folder/sub/values.xlsx");
    let got = answer(&scratch, &workbook, made(COPY));
    assert_eq!(got, Ok(Removal::Removed));
    assert!(!copy.exists());
    let left = vec!["other.xlsx".to_string(), "values.xlsx".to_string()];
    assert_eq!(scratch.names("folder/sub"), left);
    assert_eq!(fs::read(&workbook).expect("read"), b"the workbook now");
    let relative = with_checkpoint(&scratch, "values.xlsx");
    let got = answer(&scratch, Path::new("values.xlsx"), made(COPY));
    assert_eq!(got, Ok(Removal::Removed), "a path relative to the folder");
    assert!(!relative.exists());
}

#[test]
fn an_absent_file_counts_as_removed() {
    let scratch = Scratch::new("remove-absent");
    scratch.write("folder/values.xlsx", b"the workbook now");
    let cases = [
        scratch.path("folder/values.xlsx"),
        scratch.path("folder/gone/values.xlsx"),
        scratch.path("folder/no-workbook.xlsx"),
    ];
    for workbook in cases {
        let got = answer(&scratch, &workbook, made(COPY));
        assert_eq!(got, Ok(Removal::Absent), "{workbook:?}");
    }
    let run = run();
    let gone = scratch.path("gone");
    let row = Row {
        folder: &gone,
        workbook: &gone.join("values.xlsx"),
        run: &run,
        expect: made(COPY),
    };
    assert_eq!(
        remove(&locks(&scratch), &row),
        Ok(Removal::Absent),
        "no folder"
    );
    assert_eq!(Removal::Absent.state(), "gone");
    assert_eq!(Removal::Removed.state(), "gone");
}

#[test]
fn a_link_on_the_way_or_in_the_place_of_the_checkpoint_leaves_the_file() {
    let scratch = Scratch::new("remove-link");
    let real = with_checkpoint(&scratch, "real/values.xlsx");
    link_dir(&scratch.path("folder/real"), &scratch.path("folder/alias"));
    let workbook = scratch.path("folder/alias/values.xlsx");
    let got = answer(&scratch, &workbook, made(COPY));
    assert_eq!(got, Ok(Removal::Left(Left::Link)), "a link on the way");
    assert_eq!(fs::read(&real).expect("read"), COPY);
    let target = scratch.write("outside/kept.xlsx", COPY);
    scratch.write("folder/values.xlsx", b"the workbook now");
    let place = scratch.path("folder").join(copy_name("values.xlsx"));
    link_file(&target, &place);
    let got = answer(&scratch, &scratch.path("folder/values.xlsx"), made(COPY));
    assert_eq!(got, Ok(Removal::Left(Left::Link)), "a link in its place");
    assert_eq!(fs::read(&target).expect("read"), COPY);
    assert!(fs::symlink_metadata(&place).is_ok(), "the link stays");
    assert_eq!(Removal::Left(Left::Link).state(), "left");
}

#[test]
fn an_entry_that_is_no_regular_file_is_left() {
    let scratch = Scratch::new("remove-not-file");
    scratch.write("folder/values.xlsx", b"the workbook now");
    let place = scratch.path("folder").join(copy_name("values.xlsx"));
    fs::create_dir(&place).expect("a directory in the checkpoint's place");
    fs::write(place.join("inside.xlsx"), COPY).expect("write");
    let got = answer(&scratch, &scratch.path("folder/values.xlsx"), made(COPY));
    assert_eq!(got, Ok(Removal::Left(Left::NotFile)));
    assert_eq!(fs::read(place.join("inside.xlsx")).expect("read"), COPY);
    scratch.write("folder/plain", b"x");
    let through = scratch.path("folder/plain/values.xlsx");
    let got = answer(&scratch, &through, made(COPY));
    assert_eq!(got, Ok(Removal::Left(Left::NotFile)), "a file on the way");
}

#[test]
fn an_entry_name_that_differs_in_case_is_left() {
    let scratch = Scratch::new("remove-case");
    scratch.write("folder/values.xlsx", b"the workbook now");
    let other_case = scratch.path("folder").join(copy_name("Values.xlsx"));
    fs::write(&other_case, COPY).expect("write the copy in another case");
    let got = answer(&scratch, &scratch.path("folder/values.xlsx"), made(COPY));
    if folds_case(&scratch.path("folder")) {
        assert_eq!(got, Ok(Removal::Left(Left::Name)));
    } else {
        assert_eq!(got, Ok(Removal::Absent));
    }
    assert_eq!(fs::read(&other_case).expect("read"), COPY);
    let listed = scratch.names("folder");
    assert!(listed.contains(&copy_name("Values.xlsx")), "{listed:?}");
}

#[test]
fn a_length_that_differs_from_the_row_leaves_the_file() {
    let scratch = Scratch::new("remove-length");
    let copy = with_checkpoint(&scratch, "values.xlsx");
    let workbook = scratch.path("folder/values.xlsx");
    let longer = Expect::Made {
        sha256: Sha256::of(COPY),
        bytes: COPY.len() as u64 + 1,
    };
    let got = answer(&scratch, &workbook, longer);
    assert_eq!(got, Ok(Removal::Left(Left::Changed)));
    let mut changed = COPY.to_vec();
    changed.push(b'!');
    fs::write(&copy, &changed).expect("the person adds a byte");
    let got = answer(&scratch, &workbook, made(COPY));
    assert_eq!(got, Ok(Removal::Left(Left::Changed)));
    assert_eq!(fs::read(&copy).expect("read"), changed);
}

#[test]
fn a_sha256_that_differs_from_the_row_leaves_the_file() {
    let scratch = Scratch::new("remove-digest");
    let copy = with_checkpoint(&scratch, "values.xlsx");
    let workbook = scratch.path("folder/values.xlsx");
    let other = Expect::Made {
        sha256: Sha256::of(b"another content of the same length!"),
        bytes: COPY.len() as u64,
    };
    let got = answer(&scratch, &workbook, other);
    assert_eq!(got, Ok(Removal::Left(Left::Changed)));
    let mut changed = COPY.to_vec();
    changed[0] = b'T';
    fs::write(&copy, &changed).expect("the person changes a byte");
    let got = answer(&scratch, &workbook, made(COPY));
    assert_eq!(got, Ok(Removal::Left(Left::Changed)));
    assert_eq!(fs::read(&copy).expect("read"), changed);
    assert_eq!(Left::Changed.name(), "changed");
}

#[test]
fn a_path_outside_the_rows_folder_is_never_reached() {
    let scratch = Scratch::new("remove-outside");
    scratch.write("folder/values.xlsx", b"the workbook now");
    let outside = scratch.write("other/values.xlsx", b"another workbook");
    let copy = outside.with_file_name(copy_name("values.xlsx"));
    fs::write(&copy, COPY).expect("write a copy outside the folder");
    let spellings = [
        outside.clone(),
        scratch.path("folder/../other/values.xlsx"),
        PathBuf::from("../other/values.xlsx"),
    ];
    for workbook in spellings {
        let got = answer(&scratch, &workbook, made(COPY));
        assert_eq!(got, Ok(Removal::Left(Left::NotFile)), "{workbook:?}");
        assert_eq!(fs::read(&copy).expect("read"), COPY, "{workbook:?}");
    }
    let got = answer(&scratch, &scratch.path("folder"), made(COPY));
    assert_eq!(got, Ok(Removal::Left(Left::NotFile)), "the folder itself");
}

#[test]
fn the_removal_leaves_the_file_when_it_cannot_take_the_lock() {
    let scratch = Scratch::new("remove-lock");
    let copy = with_checkpoint(&scratch, "values.xlsx");
    let held = locks(&scratch).hold(&scratch.path("folder")).expect("held");
    let got = answer(&scratch, &scratch.path("folder/values.xlsx"), made(COPY));
    assert_eq!(got, Err(Undecided::Lock(LockError::Busy)));
    assert_eq!(fs::read(&copy).expect("read"), COPY);
    drop(held);
    let got = answer(&scratch, &scratch.path("folder/values.xlsx"), made(COPY));
    assert_eq!(got, Ok(Removal::Removed));
}

#[test]
fn a_planned_row_removes_a_copy_equal_to_the_workbook_and_leaves_another() {
    let scratch = Scratch::new("remove-planned");
    let workbook = scratch.write("folder/values.xlsx", COPY);
    let copy = workbook.with_file_name(copy_name("values.xlsx"));
    fs::write(&copy, COPY).expect("write");
    let got = answer(&scratch, &workbook, Expect::Planned);
    assert_eq!(got, Ok(Removal::Removed), "equal to the workbook");
    fs::write(&copy, b"the copy before the change").expect("write");
    let got = answer(&scratch, &workbook, Expect::Planned);
    assert_eq!(got, Ok(Removal::Left(Left::Changed)), "not the workbook");
    fs::remove_file(&workbook).expect("the person removes the workbook");
    let got = answer(&scratch, &workbook, Expect::Planned);
    assert_eq!(got, Ok(Removal::Left(Left::Changed)), "no workbook");
    assert_eq!(
        fs::read(&copy).expect("read"),
        b"the copy before the change"
    );
    fs::remove_file(&copy).expect("remove");
    let got = answer(&scratch, &workbook, Expect::Planned);
    assert_eq!(got, Ok(Removal::Absent), "no copy");
}

#[test]
fn a_file_over_the_bound_is_not_read() {
    let scratch = Scratch::new("remove-bound");
    let copy = with_checkpoint(&scratch, "values.xlsx");
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&copy)
        .expect("open");
    file.set_len(FILE_MAX + 1).expect("grow the file");
    drop(file);
    let over = Expect::Made {
        sha256: Sha256::of(COPY),
        bytes: FILE_MAX + 1,
    };
    let workbook = scratch.path("folder/values.xlsx");
    let got = answer(&scratch, &workbook, over);
    assert_eq!(got, Ok(Removal::Left(Left::Unreadable)));
    let got = answer(&scratch, &workbook, made(COPY));
    assert_eq!(got, Ok(Removal::Left(Left::Changed)));
    assert_eq!(fs::metadata(&copy).expect("stat").len(), FILE_MAX + 1);
}

#[cfg(unix)]
#[test]
fn a_removal_that_the_system_refuses_is_undecided_and_leaves_the_file() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("remove-refused");
    let copy = with_checkpoint(&scratch, "sub/values.xlsx");
    let sub = scratch.path("folder/sub");
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o555)).expect("chmod");
    let got = answer(&scratch, &sub.join("values.xlsx"), made(COPY));
    fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).expect("chmod back");
    assert_eq!(
        got,
        Err(Undecided::Remove(std::io::ErrorKind::PermissionDenied))
    );
    assert_eq!(fs::read(&copy).expect("read"), COPY);
}

#[cfg(windows)]
#[test]
fn a_removal_that_the_system_refuses_is_undecided_and_leaves_the_file() {
    use std::os::windows::fs::OpenOptionsExt;
    let scratch = Scratch::new("remove-refused");
    let copy = with_checkpoint(&scratch, "values.xlsx");
    let open_elsewhere = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&copy)
        .expect("an application holds the copy open without delete sharing");
    let got = answer(&scratch, &scratch.path("folder/values.xlsx"), made(COPY));
    drop(open_elsewhere);
    assert!(matches!(got, Err(Undecided::Remove(_))), "{got:?}");
    assert_eq!(fs::read(&copy).expect("read"), COPY);
}

#[cfg(windows)]
#[test]
fn a_part_with_a_colon_names_no_entry() {
    let scratch = Scratch::new("remove-stream");
    with_checkpoint(&scratch, "values.xlsx");
    let workbook = scratch.path("folder/values.xlsx:stream");
    let got = answer(&scratch, &workbook, made(COPY));
    assert_eq!(got, Ok(Removal::Left(Left::Name)));
}

#[test]
fn each_reason_that_leaves_a_file_has_the_name_of_the_lists_row() {
    let names: Vec<&str> = [
        Left::Changed,
        Left::Link,
        Left::NotFile,
        Left::Name,
        Left::Unreadable,
    ]
    .into_iter()
    .map(Left::name)
    .collect();
    assert_eq!(names, ["changed", "link", "not_file", "name", "unreadable"]);
}
