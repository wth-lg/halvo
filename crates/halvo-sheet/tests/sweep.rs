mod common;

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

use common::{corpus, link_file, locks, Scratch, RUN};
use halvo_sheet::{Folder, Locks, Range, RunId, Sha256, Value};

const OTHER: &str = "fedcba9876543210fedcba9876543210";
const CHILD_DIR: &str = "HALVO_SHEET_SWEEP_CHILD_DIR";
const CHILD_DATA: &str = "HALVO_SHEET_SWEEP_CHILD_DATA";

fn own_locks(data: &Path) -> Locks {
    Locks::open(data, std::time::Duration::from_secs(10)).expect("the locks open")
}

fn marks(data: &Scratch) -> Vec<String> {
    let names = data.names("agents/locks");
    names.into_iter().filter(|n| n.ends_with(".live")).collect()
}

fn prepare(folder: &Folder, run: &str, cell: &str) -> halvo_sheet::Prepared {
    let run = RunId::parse(run).expect("a run id");
    let range = Range::parse(cell).expect("a range");
    let values = [vec![Value::Number(5.0)]];
    folder
        .prepare(Path::new("values.xlsx"), &run, "Values", range, &values)
        .expect("prepared")
}

fn copies_of(scratch: &Scratch, run: &str) -> Vec<String> {
    let mark = format!(".halvo-prepared-{run}-");
    let names = scratch.names("");
    names.into_iter().filter(|n| n.contains(&mark)).collect()
}

fn orphan_name(stem: &str, run: &str, bytes: &[u8]) -> String {
    let digest = Sha256::of(bytes).hex();
    format!(
        "{stem}.halvo-prepared-{run}-00000000000000ff-{}.xlsx",
        &digest[..16]
    )
}

#[test]
fn sweep_child() {
    let (Ok(dir), Ok(data)) = (std::env::var(CHILD_DIR), std::env::var(CHILD_DATA)) else {
        return;
    };
    let folder = Folder::new(Path::new(&dir), own_locks(Path::new(&data))).expect("the folder");
    let _held = prepare(&folder, OTHER, "B5");
    println!("child prepared");
    std::thread::sleep(std::time::Duration::from_secs(120));
}

#[test]
fn a_prepared_copy_that_a_killed_run_left_is_swept_by_the_next_prepare() {
    let scratch = Scratch::new("sweep-killed");
    let data = Scratch::new("sweep-killed-data");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let mut child = Command::new(std::env::current_exe().expect("the test binary"))
        .args(["--exact", "sweep_child", "--nocapture", "--test-threads=1"])
        .env(CHILD_DIR, &scratch.dir)
        .env(CHILD_DATA, &data.dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the child");
    let stdout = child.stdout.take().expect("the child's output");
    let prepared = BufReader::new(stdout)
        .lines()
        .map_while(Result::ok)
        .any(|line| line.contains("child prepared"));
    assert!(prepared, "the child prepared");
    let folder = Folder::new(&scratch.dir, own_locks(&data.dir)).expect("the folder opens");
    let live = prepare(&folder, RUN, "C7");
    assert_eq!(
        copies_of(&scratch, OTHER).len(),
        1,
        "a live run's copy stays"
    );
    drop(live);
    child.kill().expect("kill the child");
    child.wait().expect("the child ends");
    assert_eq!(
        copies_of(&scratch, OTHER).len(),
        1,
        "the killed run left its copy"
    );
    let left = marks(&data);
    assert_eq!(left.len(), 1, "the killed run left its mark: {left:?}");
    let next = prepare(&folder, RUN, "C7");
    let now = marks(&data);
    assert!(
        !now.contains(&left[0]),
        "the sweep takes the killed run's mark: {now:?}"
    );
    assert_eq!(now.len(), 1, "the call's own mark: {now:?}");
    assert_eq!(copies_of(&scratch, OTHER), Vec::<String>::new(), "swept");
    assert_eq!(copies_of(&scratch, RUN).len(), 1, "the call's own copy");
    let checkpoint = format!("values.halvo-checkpoint-{OTHER}.xlsx");
    assert!(
        scratch.names("").contains(&checkpoint),
        "the checkpoint stays"
    );
    folder.replace(&next).expect("replaced");
}

#[test]
fn a_copy_that_a_live_call_holds_is_not_swept_and_still_replaces() {
    let scratch = Scratch::new("sweep-live");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, locks()).expect("the folder opens");
    let first = prepare(&folder, OTHER, "B5");
    let second = prepare(&folder, RUN, "C7");
    assert!(first.copy().exists() && second.copy().exists());
    folder.replace(&first).expect("the first call replaces");
    drop(second);
}

#[test]
fn a_file_with_a_prepared_name_and_a_left_mark_that_is_changed_a_link_a_directory_or_another_workbooks_stays(
) {
    let scratch = Scratch::new("sweep-kept");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let bytes = b"a copy that a killed run left".as_slice();
    let orphan = orphan_name("values", OTHER, bytes);
    scratch.write(&orphan, bytes);
    let changed = orphan_name("values", OTHER, b"what the copy held");
    scratch.write(&changed, b"what the person saved over it");
    let target = scratch.write("outside.xlsx", bytes);
    let link = orphan_name("values", RUN, b"the link's own name");
    link_file(&target, &scratch.path(&link));
    let directory = orphan_name("values", "00112233445566778899aabbccddeeff", bytes);
    fs::create_dir(scratch.path(&directory)).expect("mkdir");
    let other_book = orphan_name("budget", OTHER, bytes);
    scratch.write(&other_book, bytes);
    let short = format!("values.halvo-prepared-{OTHER}-00ff.xlsx");
    scratch.write(&short, bytes);
    let locks = locks();
    for name in [&orphan, &changed, &link, &directory, &other_book, &short] {
        let mark = locks.mark(&scratch.dir, std::ffi::OsStr::new(name));
        mark.expect("a mark that a killed call left").keep();
    }
    let folder = Folder::new(&scratch.dir, locks).expect("the folder opens");
    let held = prepare(&folder, RUN, "B5");
    let names = scratch.names("");
    assert!(!names.contains(&orphan), "the orphan is swept: {names:?}");
    assert_eq!(held.unmarked(), 0, "no entry without a mark");
    for kept in [&changed, &link, &directory, &other_book, &short] {
        assert!(names.contains(kept), "{kept} stays: {names:?}");
    }
    assert_eq!(fs::read(&target).expect("read"), bytes);
    drop(held);
}

#[test]
fn a_prepared_name_in_another_case_is_not_the_workbooks() {
    let scratch = Scratch::new("sweep-case");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let bytes = b"a copy of another spelling".as_slice();
    let upper = orphan_name("VALUES", OTHER, bytes);
    scratch.write(&upper, bytes);
    let folder = Folder::new(&scratch.dir, locks()).expect("the folder opens");
    let held = prepare(&folder, RUN, "B5");
    assert!(scratch.names("").contains(&upper));
    drop(held);
}
