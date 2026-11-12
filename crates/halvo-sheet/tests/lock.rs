mod common;

use std::fs;
use std::path::Path;
use std::sync::{Arc, Barrier};
use std::time::Duration;

use common::{corpus, locks, locks_waiting, Scratch, RUN};
use halvo_sheet::{Cell, Error, Folder, Kept, LockError, Prepared, Range, RunId, Value};

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
