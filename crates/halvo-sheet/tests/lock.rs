mod common;

use std::fs;
use std::path::Path;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use common::{corpus, link_dir, locks, locks_waiting, Scratch, RUN};
use halvo_sheet::{
    Cell, Error, Folder, Kept, LockError, Locks, PathRefusal, Prepared, Range, RunId, Value,
};

const OTHER: &str = "fedcba9876543210fedcba9876543210";

fn run(text: &str) -> RunId {
    RunId::parse(text).expect("a run id")
}

fn prepare(folder: &Folder, file: &str, run_id: &str, cell: &str, value: f64) -> Prepared {
    let range = Range::parse(cell).expect("a range");
    let values = [vec![Value::Number(value)]];
    folder
        .prepare(Path::new(file), &run(run_id), "Values", range, &values)
        .expect("prepared")
}

fn cell(folder: &Folder, file: &str, cell: &str) -> Cell {
    let range = Range::parse(cell).expect("a range");
    let read = folder.read(Path::new(file), "Values", range).expect("read");
    read[0][0].clone()
}

#[test]
fn a_replace_that_cannot_take_the_lock_is_refused() {
    let scratch = Scratch::new("lock-replace");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let before = fs::read(scratch.path("values.xlsx")).expect("read");
    let short = locks_waiting(Duration::from_millis(200));
    let folder = Folder::new(&scratch.dir, short.clone()).expect("the folder opens");
    let prepared = prepare(&folder, "values.xlsx", RUN, "B5", 77.0);
    let held = short
        .hold(&scratch.dir)
        .expect("another call holds the lock");
    let refused = Err(Error::NotReplaced(Kept::Lock(LockError::Busy)));
    assert_eq!(folder.replace(&prepared), refused);
    drop(held);
    assert_eq!(
        folder.replace(&prepared),
        refused,
        "the first answer stands"
    );
    assert_eq!(fs::read(scratch.path("values.xlsx")).expect("read"), before);
    assert!(!prepared.copy().exists(), "the prepared copy is gone");
    assert_eq!(fs::read(prepared.checkpoint()).expect("read"), before);
}

#[test]
fn a_prepare_that_cannot_take_the_lock_writes_nothing() {
    let scratch = Scratch::new("lock-prepare");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let short = locks_waiting(Duration::from_millis(200));
    let folder = Folder::new(&scratch.dir, short.clone()).expect("the folder opens");
    let held = short
        .hold(&scratch.dir)
        .expect("another call holds the lock");
    let range = Range::parse("B5").expect("a range");
    let values = [vec![Value::Number(77.0)]];
    let got = folder.prepare(
        Path::new("values.xlsx"),
        &run(RUN),
        "Values",
        range,
        &values,
    );
    assert_eq!(got.err(), Some(Error::Lock(LockError::Busy)));
    assert_eq!(scratch.names(""), vec!["values.xlsx".to_string()]);
    drop(held);
    assert!(folder
        .prepare(
            Path::new("values.xlsx"),
            &run(RUN),
            "Values",
            range,
            &values
        )
        .is_ok());
}

#[test]
fn two_runs_on_one_workbook_lose_no_edit() {
    let scratch = Scratch::new("lock-two-runs");
    let folder = Arc::new(Folder::new(&scratch.dir, locks()).expect("the folder opens"));
    let rounds = 40;
    let (mut lost, mut both) = (Vec::new(), 0);
    for round in 0..rounds {
        let file = format!("book-{round}.xlsx");
        fs::copy(corpus("values.xlsx"), scratch.path(&file)).expect("copy");
        let barrier = Arc::new(Barrier::new(2));
        let one = |run_id: &'static str, at: &'static str, value: f64| {
            let (folder, barrier, file) = (folder.clone(), barrier.clone(), file.clone());
            std::thread::spawn(move || {
                let prepared = prepare(&folder, &file, run_id, at, value);
                barrier.wait();
                folder.replace(&prepared)
            })
        };
        let a = one(RUN, "B5", 1001.0);
        let b = one(OTHER, "C7", 2002.0);
        let answers = [
            (a.join().expect("run a"), "B5", 1001.0),
            (b.join().expect("run b"), "C7", 2002.0),
        ];
        if answers.iter().all(|(answer, _, _)| answer.is_ok()) {
            both += 1;
        }
        for (answer, at, value) in answers {
            if answer.is_ok() && cell(&folder, &file, at) != Cell::Number(value) {
                lost.push(format!("round {round}: {at}"));
            }
        }
    }
    assert_eq!(
        lost,
        Vec::<String>::new(),
        "edits lost; both Ok in {both} rounds"
    );
}

#[test]
fn a_directory_swapped_for_a_link_while_prepare_waits_for_the_lock_is_never_written_through() {
    let scratch = Scratch::new("lock-prepare-swap");
    fs::create_dir_all(scratch.path("folder/sub")).expect("mkdir");
    fs::create_dir_all(scratch.path("outside")).expect("mkdir");
    fs::copy(
        corpus("values.xlsx"),
        scratch.path("folder/sub/values.xlsx"),
    )
    .expect("copy");
    fs::copy(corpus("values.xlsx"), scratch.path("outside/values.xlsx")).expect("copy");
    let shared = locks();
    let folder = Folder::new(&scratch.path("folder"), shared.clone()).expect("the folder opens");
    let held = shared.hold(&scratch.path("folder/sub")).expect("held");
    let worker = std::thread::spawn(move || {
        let range = Range::parse("B5").expect("a range");
        let values = [vec![Value::Number(5.0)]];
        let run = RunId::parse(RUN).expect("a run id");
        folder
            .prepare(Path::new("sub/values.xlsx"), &run, "Values", range, &values)
            .map(drop)
    });
    std::thread::sleep(Duration::from_millis(800));
    fs::rename(
        scratch.path("folder/sub"),
        scratch.path("folder/sub-before"),
    )
    .expect("move");
    link_dir(&scratch.path("outside"), &scratch.path("folder/sub"));
    drop(held);
    let got = worker.join().expect("the prepare thread");
    assert_eq!(got, Err(Error::Path(PathRefusal::Link)));
    assert_eq!(scratch.names("outside"), vec!["values.xlsx".to_string()]);
}

#[test]
fn a_live_prepared_copy_is_marked_in_agents_locks_and_no_handle_holds_the_copy() {
    let scratch = Scratch::new("lock-mark");
    fs::create_dir_all(scratch.path("data")).expect("mkdir");
    fs::create_dir_all(scratch.path("folder")).expect("mkdir");
    fs::copy(corpus("values.xlsx"), scratch.path("folder/values.xlsx")).expect("copy");
    let own = Locks::open(&scratch.path("data"), Duration::from_secs(10)).expect("the locks");
    let folder = Folder::new(&scratch.path("folder"), own).expect("the folder opens");
    let prepared = prepare(&folder, "values.xlsx", RUN, "B5", 5.0);
    let kinds = |names: Vec<String>| -> Vec<String> {
        let mut kinds: Vec<String> = names
            .iter()
            .map(|name| name.rsplit('.').next().unwrap_or_default().to_string())
            .collect();
        kinds.sort();
        kinds
    };
    let marks = kinds(scratch.names("data/agents/locks"));
    assert_eq!(marks, vec!["live".to_string(), "lock".to_string()]);
    let mut other = fs::OpenOptions::new()
        .append(true)
        .open(prepared.copy())
        .expect("another handle opens the copy");
    std::io::Write::write_all(&mut other, b"x").expect("another handle writes the copy");
    drop(other);
    let moved = scratch.path("moved");
    fs::rename(scratch.path("folder"), &moved).expect("the folder can be moved");
    fs::rename(&moved, scratch.path("folder")).expect("and moved back");
    drop(prepared);
    assert_eq!(
        kinds(scratch.names("data/agents/locks")),
        vec!["lock".to_string()]
    );
}
