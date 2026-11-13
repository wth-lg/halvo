//! The cases of the coordinator's ruling CP-134 for the bound of a file in `remove` (the
//! verifier's M39 and its planned-bound row): files over 64 MiB, made sparse with `set_len`.
mod common;

use std::fs::File;
use std::path::Path;
use std::time::Duration;

use common::{Scratch, RUN};
use halvo_checkpoint::{
    checkpoint_name, remove, Expect, Left, Locks, Removal, Row, RunId, FILE_MAX,
};

fn sparse(path: &Path, len: u64) {
    File::create(path)
        .expect("make the file")
        .set_len(len)
        .expect("set its length");
}

fn answer(scratch: &Scratch, workbook_len: u64, copy_len: u64) -> Result<Removal, String> {
    let run = RunId::parse(RUN).expect("a run id");
    let folder = scratch.path("folder");
    std::fs::create_dir_all(&folder).expect("make the folder");
    std::fs::create_dir_all(scratch.path("data")).expect("make the data directory");
    sparse(&folder.join("values.xlsx"), workbook_len);
    let name = checkpoint_name(Path::new("values.xlsx"), &run).expect("a name");
    sparse(&folder.join(&name), copy_len);
    let locks = Locks::open(&scratch.path("data"), Duration::from_secs(5)).expect("the locks");
    let row = Row {
        folder: &folder,
        workbook: Path::new("values.xlsx"),
        run: &run,
        expect: Expect::Planned,
    };
    remove(&locks, &row).map_err(|why| format!("{why:?}"))
}

/// A planned row whose workbook holds one byte more than the bound, and a copy that holds the
/// workbook's first 64 MiB: the read of the workbook takes one byte past the bound, the most that
/// it reads, to tell a longer workbook, so the copy is not
/// the workbook and stays (`changed`). A read that stopped at the bound would take the copy for
/// the workbook and remove it.
#[test]
fn a_copy_of_the_first_64_mib_of_a_longer_workbook_is_not_the_workbook() {
    let scratch = Scratch::new("cp134-m39");
    let got = answer(&scratch, FILE_MAX + 1, FILE_MAX);
    println!("cp134 m39 answer={got:?}");
    assert_eq!(got, Ok(Removal::Left(Left::Changed)));
    let copy = scratch.path("folder").join(
        checkpoint_name(
            Path::new("values.xlsx"),
            &RunId::parse(RUN).expect("a run id"),
        )
        .expect("a name"),
    );
    assert_eq!(
        std::fs::metadata(copy).expect("the copy stays").len(),
        FILE_MAX
    );
}

/// A planned row whose workbook and copy both hold one byte more than the bound: the
/// workbook is over the bound, so the copy is answered `changed`, before its own length is
/// compared with the bound.
#[test]
fn a_planned_row_whose_workbook_is_over_the_bound_is_changed() {
    let scratch = Scratch::new("cp134-planned-bound");
    let got = answer(&scratch, FILE_MAX + 1, FILE_MAX + 1);
    println!("cp134 planned-bound answer={got:?}");
    assert_eq!(got, Ok(Removal::Left(Left::Changed)));
}
