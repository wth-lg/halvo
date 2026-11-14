//! The cases of the coordinator's ruling CP-124 that use calls the ruling added.
mod common;

use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::time::Duration;

use common::{corpus, Scratch, RUN};
use halvo_checkpoint::{
    name_units, names_fit, removing_name, Marked, Removal, Root, Undecided, NAME_MAX,
};
use halvo_sheet::{Error, Folder, Locks, PathRefusal, Range, RunId, Sha256, Value};

const OTHER: &str = "fedcba9876543210fedcba9876543210";

fn own_locks(data: &Path) -> Locks {
    Locks::open(data, Duration::from_secs(10)).expect("the locks open")
}

fn prepare_at(folder: &Folder, workbook: &str, run: &str) -> Result<halvo_sheet::Prepared, Error> {
    let run = RunId::parse(run).expect("a run id");
    let range = Range::parse("B5").expect("a range");
    folder.prepare(
        Path::new(workbook),
        &run,
        "Values",
        range,
        &[vec![Value::Number(5.0)]],
    )
}

/// E3 of CP-124, the verifier's case I5: the answer of `prepare` counts the entry that it left
/// because it has no mark.
#[test]
fn prepare_counts_an_own_copy_with_no_mark() {
    let scratch = Scratch::new("cp124-api-unmarked");
    let data = Scratch::new("cp124-api-unmarked-data");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let bytes = b"a copy whose mark is not in this data directory".as_slice();
    let digest = Sha256::of(bytes).hex();
    let orphan = format!(
        "values.halvo-prepared-{OTHER}-00000000000000ff-{}.xlsx",
        &digest[..16]
    );
    scratch.write(&orphan, bytes);
    let changed = format!(
        "values.halvo-prepared-{OTHER}-00000000000000fe-{}.xlsx",
        &digest[..16]
    );
    scratch.write(&changed, b"bytes of another digest");
    let folder = Folder::new(&scratch.dir, own_locks(&data.dir)).expect("the folder opens");
    let held = prepare_at(&folder, "values.xlsx", RUN).expect("prepared");
    assert_eq!(held.unmarked(), 1, "the one own copy with no mark");
    assert!(scratch.names("").contains(&orphan));
    drop(held);
}

/// E2 of CP-124: the refusal names the bound, and the bound reads the builders of both crates:
/// a marker that grows moves the longest name that fits.
#[test]
fn a_name_over_the_bound_is_refused_as_too_long_and_the_bound_reads_the_builders() {
    let scratch = Scratch::new("cp124-api-long");
    let data = Scratch::new("cp124-api-long-data");
    let name = format!("{}.xlsx", "a".repeat(169));
    fs::copy(corpus("values.xlsx"), scratch.path(&name)).expect("copy");
    let folder = Folder::new(&scratch.dir, own_locks(&data.dir)).expect("the folder opens");
    let answer = prepare_at(&folder, &name, RUN).map(|_| ());
    assert_eq!(answer, Err(Error::Path(PathRefusal::TooLong)));
    let run = RunId::parse(RUN).expect("a run id");
    let fits =
        |units: usize| names_fit(Path::new(&format!("{}.xlsx", "a".repeat(units - 5))), &run);
    assert!(
        fits(173) && !fits(174),
        "the checkpoint and its removing name"
    );
    let checkpoint = OsStr::new("x.halvo-checkpoint-0123456789abcdef0123456789abcdef.xlsx");
    let moved = removing_name(checkpoint, u64::MAX);
    assert_eq!(name_units(&moved), name_units(checkpoint) + 32);
    assert_eq!(NAME_MAX, 255);
}

/// E4 of CP-124: the Debug and Display forms of every public type of both crates that holds a
/// path name no directory of the test.
#[test]
fn no_debug_or_display_form_names_a_directory_of_the_test() {
    let scratch = Scratch::new("cp124-api-forms");
    let data = Scratch::new("cp124-api-forms-data");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let locks = own_locks(&data.dir);
    let held = locks.hold(&scratch.dir).expect("the lock");
    let mark = locks
        .mark(&scratch.dir, OsStr::new("a.xlsx"))
        .expect("a mark");
    let live = locks
        .marked(&scratch.dir, OsStr::new("a.xlsx"))
        .expect("marked");
    assert!(matches!(live, Marked::Live));
    locks
        .mark(&scratch.dir, OsStr::new("b.xlsx"))
        .expect("a mark")
        .keep();
    let left = locks
        .marked(&scratch.dir, OsStr::new("b.xlsx"))
        .expect("marked");
    assert!(matches!(left, Marked::Left(_)));
    let unmarked = locks
        .marked(&scratch.dir, OsStr::new("c.xlsx"))
        .expect("marked");
    let root = Root::new(&scratch.dir).expect("the root");
    let held_form = format!("{held:?}");
    drop(held);
    let folder = Folder::new(&scratch.dir, locks.clone()).expect("the folder opens");
    let prepared = prepare_at(&folder, "values.xlsx", RUN).expect("prepared");
    let refused = prepare_at(&folder, "absent.xlsx", RUN)
        .map(|_| ())
        .unwrap_err();
    let busy = Undecided::Lock(halvo_sheet::LockError::Busy);
    let forms = [
        held_form,
        format!("{locks:?}"),
        format!("{mark:?}"),
        format!("{live:?}"),
        format!("{left:?}"),
        format!("{unmarked:?}"),
        format!("{root:?}"),
        format!("{folder:?}"),
        format!("{prepared:?}"),
        format!("{refused:?}"),
        format!("{refused}"),
        format!("{:?}", Removal::Removed),
        format!("{busy:?}"),
    ];
    let names: Vec<String> = [&scratch.dir, &data.dir]
        .iter()
        .map(|dir| {
            dir.file_name()
                .expect("a name")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    for form in &forms {
        for name in &names {
            assert!(!form.contains(name.as_str()), "{form} names {name}");
        }
        assert!(!form.contains("agents"), "{form} names agents/");
    }
    println!("cp124 forms={} names_checked={}", forms.len(), names.len());
    drop(prepared);
    drop(mark);
}
