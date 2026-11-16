mod common;

use std::fs;
use std::path::Path;

use common::{corpus, locks, Scratch, RUN};
use halvo_checkpoint::{remove, Expect, Left, Removal, Row};
use halvo_sheet::{Folder, Range, RunId, Value};

#[test]
fn the_row_that_prepare_gives_removes_its_checkpoint_and_leaves_one_that_the_person_changed() {
    for change in [false, true] {
        let scratch = Scratch::new("checkpoint-row");
        fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
        let folder = Folder::new(&scratch.dir, locks()).expect("the folder opens");
        let run = RunId::parse(RUN).expect("a run id");
        let range = Range::parse("B5").expect("a range");
        let values = [vec![Value::Number(77.0)]];
        let workbook = scratch.path("values.xlsx");
        let prepared = folder
            .prepare(&workbook, &run, "Values", range, &values)
            .expect("prepared");
        folder.replace(&prepared).expect("replaced");
        let checkpoint = prepared.checkpoint().to_path_buf();
        let expect = Expect::Made {
            sha256: prepared.checkpoint_sha256(),
            bytes: prepared.checkpoint_bytes(),
        };
        let name = halvo_checkpoint::checkpoint_name(&workbook, &run).expect("a name");
        assert_eq!(prepared.checkpoint_name(), name.as_os_str());
        drop(prepared);
        if change {
            let mut bytes = fs::read(&checkpoint).expect("read");
            bytes.push(0);
            fs::write(&checkpoint, &bytes).expect("the person saves the copy");
        }
        let row = Row {
            folder: &scratch.dir,
            workbook: &workbook,
            run: &run,
            expect,
        };
        let got = remove(&locks(), &row);
        if change {
            assert_eq!(got, Ok(Removal::Left(Left::Changed)));
            assert!(checkpoint.exists());
        } else {
            assert_eq!(got, Ok(Removal::Removed));
            assert!(!checkpoint.exists());
        }
        let read = folder.read(Path::new("values.xlsx"), "Values", range);
        assert_eq!(read, Ok(vec![vec![halvo_sheet::Cell::Number(77.0)]]));
    }
}
