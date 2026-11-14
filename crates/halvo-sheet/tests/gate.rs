mod common;

use std::fs;
use std::path::Path;

use common::{book, corpus, number_at, same_but, Scratch, CARRIED, REFUSED, RUN};
use halvo_sheet::{Cause, Error, Folder, Kind, Range, Refusal, RunId, Value};

#[test]
fn each_workbook_of_the_trial_is_changed_in_one_cell_with_the_sameness_of_a13_4_or_refused() {
    let run = RunId::parse(RUN).expect("a run id");
    let mut wrong = Vec::new();
    for change in CARRIED {
        let scratch = Scratch::new("gate-carried");
        fs::copy(corpus(change.file), scratch.path(change.file)).expect("copy");
        let original = book(&scratch.path(change.file));
        let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
        let range = Range::parse(change.cell).expect("a range");
        let values = vec![vec![Value::Number(change.new)]];
        let done = folder
            .prepare(Path::new(change.file), &run, change.sheet, range, &values)
            .and_then(|prepared| folder.replace(&prepared));
        if let Err(error) = done {
            wrong.push(format!("{}: {error}", change.file));
            continue;
        }
        let saved = book(&scratch.path(change.file));
        let differ = same_but(&original, &saved, change.sheet, &[change.pos]);
        if !differ.is_empty() {
            wrong.push(format!("{}: {differ:?}", change.file));
        }
        if number_at(&saved, change.sheet, change.pos) != Some(change.new) {
            wrong.push(format!(
                "{}: {} does not hold the new value",
                change.file, change.cell
            ));
        }
        println!(
            "gate file={} cell={}!{} same=yes",
            change.file, change.sheet, change.cell
        );
    }
    let scratch = Scratch::new("gate-refused");
    fs::copy(corpus(REFUSED.file), scratch.path(REFUSED.file)).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let range = Range::parse(REFUSED.cell).expect("a range");
    let values = vec![vec![Value::Number(REFUSED.new)]];
    let got = folder.prepare(Path::new(REFUSED.file), &run, REFUSED.sheet, range, &values);
    match got {
        Err(error) if error == Error::Workbook(Refusal::Kinds(vec![Kind::Validation])) => {
            assert_eq!(error.cause(), Cause::Workbook);
            println!("gate file={} cause={}", REFUSED.file, error.cause().name());
        }
        other => wrong.push(format!("{}: {other:?}", REFUSED.file)),
    }
    if scratch.names("") != vec![REFUSED.file.to_string()] {
        wrong.push(format!(
            "{}: beside it {:?}",
            REFUSED.file,
            scratch.names("")
        ));
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
