//! CP-134 H1: the move aside of `remove` replaces no entry. This file holds one test alone, so
//! that the count of the moves aside of this process starts at 0 and the removing name of the
//! first move is known.
mod common;

use std::fs;
use std::path::Path;
use std::time::Duration;

use common::{Scratch, RUN};
use halvo_checkpoint::{
    checkpoint_name, remove, removing_name, Expect, Locks, Row, RunId, Sha256, Undecided,
};

const COPY: &[u8] = b"the workbook as prepare read it";
const MINE: &[u8] = b"a file of the person at the removing name";

/// A file stands at the name that `remove` moves the checkpoint to: the move fails with
/// `AlreadyExists`, `remove` decides nothing, and both files stay with their bytes. A move that
/// replaced would lose the person's file and answer the checkpoint as removed.
#[test]
fn the_move_aside_replaces_no_file_at_the_removing_name() {
    let scratch = Scratch::new("cp134-aside-no-replace");
    let run = RunId::parse(RUN).expect("a run id");
    let folder = scratch.path("folder");
    scratch.write("folder/values.xlsx", b"the workbook now");
    let name = checkpoint_name(Path::new("values.xlsx"), &run).expect("a name");
    fs::write(folder.join(&name), COPY).expect("the checkpoint");
    let first = u64::from(std::process::id()) << 32;
    let moved = removing_name(&name, first);
    fs::write(folder.join(&moved), MINE).expect("a file at the removing name");
    fs::create_dir_all(scratch.path("data")).expect("make the data directory");
    let locks = Locks::open(&scratch.path("data"), Duration::from_secs(5)).expect("the locks");
    let row = Row {
        folder: &folder,
        workbook: Path::new("values.xlsx"),
        run: &run,
        expect: Expect::Made {
            sha256: Sha256::of(COPY),
            bytes: COPY.len() as u64,
        },
    };
    let got = remove(&locks, &row);
    println!("cp134 aside-no-replace answer={got:?}");
    assert_eq!(
        got,
        Err(Undecided::Remove(std::io::ErrorKind::AlreadyExists))
    );
    assert_eq!(
        fs::read(folder.join(&moved)).expect("the person's file stays"),
        MINE
    );
    assert_eq!(
        fs::read(folder.join(&name)).expect("the checkpoint stays"),
        COPY
    );
}
