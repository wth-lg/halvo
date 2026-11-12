mod common;

use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

use common::{corpus, Scratch, RUN};
use halvo_sheet::{Cause, Cell, Error, Folder, Range, Refusal, RunId, Value};

const MIB: u64 = 1 << 20;

fn run() -> RunId {
    RunId::parse(RUN).expect("a run id")
}

fn one(x: f64) -> Vec<Vec<Value>> {
    vec![vec![Value::Number(x)]]
}

fn refused_by_both(scratch: &Scratch, file: &str, label: &str, want: Error) {
    let before = fs::metadata(scratch.path(file)).expect("metadata").len();
    let listing = scratch.names("");
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    let got = folder
        .prepare(Path::new(file), &run(), "Values", b5, &one(1.0))
        .err();
    assert_eq!(got, Some(want.clone()), "{label}: prepare");
    if let Some(error) = got {
        assert_eq!(error.cause(), want.cause(), "{label}");
    }
    let read = folder.read(Path::new(file), "Values", b5).err();
    assert_eq!(read, Some(want), "{label}: read");
    assert_eq!(
        scratch.names(""),
        listing,
        "{label}: nothing beside the workbook"
    );
    assert_eq!(
        fs::metadata(scratch.path(file)).expect("metadata").len(),
        before,
        "{label}"
    );
}

/// The corpus workbook with `extra` entries added at its end, each with the uncompressed size
/// that its header declares set to `declared` where one is given.
fn with_entries(path: &Path, extra: &[(&str, Vec<u8>, zip::CompressionMethod, Option<u32>)]) {
    use std::io::Read;
    let mut archive =
        zip::ZipArchive::new(fs::File::open(corpus("values.xlsx")).expect("open")).expect("a zip");
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("entry");
        let name = entry.name().expect("name").to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read");
        out.start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .expect("start");
        out.write_all(&bytes).expect("write");
    }
    for (name, bytes, method, _) in extra {
        let options = zip::write::SimpleFileOptions::default().compression_method(*method);
        out.start_file(*name, options).expect("start");
        out.write_all(bytes).expect("write");
    }
    let mut bytes = out.finish().expect("finish").into_inner();
    for (name, _, _, declared) in extra {
        let Some(declared) = declared else { continue };
        let spots: Vec<usize> = bytes
            .windows(name.len())
            .enumerate()
            .filter(|(_, w)| *w == name.as_bytes())
            .map(|(at, _)| at)
            .collect();
        assert_eq!(
            spots.len(),
            2,
            "{name} stands in its local header and in the directory"
        );
        let (local, central) = (spots[0] - 30, spots[1] - 46);
        assert_eq!(&bytes[local..local + 4], b"PK\x03\x04", "{name}");
        assert_eq!(&bytes[central..central + 4], b"PK\x01\x02", "{name}");
        bytes[local + 22..local + 26].copy_from_slice(&declared.to_le_bytes());
        bytes[central + 24..central + 28].copy_from_slice(&declared.to_le_bytes());
    }
    fs::write(path, bytes).expect("write");
}

#[test]
fn a_file_of_more_than_64_mib_is_refused_before_it_is_read() {
    let scratch = Scratch::new("bounds-file");
    let file = fs::File::create(scratch.path("big.xlsx")).expect("create");
    file.set_len(64 * MIB + 1).expect("set the length");
    drop(file);
    refused_by_both(
        &scratch,
        "big.xlsx",
        "a file one byte over",
        Error::Workbook(Refusal::Size),
    );
}

#[cfg(unix)]
#[test]
fn a_file_over_the_bound_is_refused_by_its_size_before_it_is_opened() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("bounds-file-unopened");
    let path = scratch.path("big.xlsx");
    let file = fs::File::create(&path).expect("create");
    file.set_len(64 * MIB + 1).expect("set the length");
    drop(file);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o200)).expect("chmod");
    refused_by_both(
        &scratch,
        "big.xlsx",
        "a file that cannot be opened for reading",
        Error::Workbook(Refusal::Size),
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod");
}

#[test]
fn an_entry_or_the_entries_over_their_bounds_by_declared_size_are_refused_before_inflating() {
    let stored = zip::CompressionMethod::Stored;
    let over = (32 * MIB + 1) as u32;
    let scratch = Scratch::new("bounds-declared");
    with_entries(
        &scratch.path("one.xlsx"),
        &[("docProps/one.bin", b"x".to_vec(), stored, Some(over))],
    );
    refused_by_both(
        &scratch,
        "one.xlsx",
        "one entry over by its declared size",
        Error::Workbook(Refusal::Size),
    );
    let names = [
        "docProps/a.bin",
        "docProps/b.bin",
        "docProps/c.bin",
        "docProps/d.bin",
        "docProps/e.bin",
    ];
    let each = (30 * MIB) as u32;
    let five: Vec<_> = names
        .iter()
        .map(|n| (*n, b"x".to_vec(), stored, Some(each)))
        .collect();
    with_entries(&scratch.path("total.xlsx"), &five);
    refused_by_both(
        &scratch,
        "total.xlsx",
        "entries over 128 MiB in all",
        Error::Workbook(Refusal::Size),
    );
    let four: Vec<_> = five[..4].to_vec();
    with_entries(&scratch.path("four.xlsx"), &four);
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
    let read = folder.read(
        Path::new("four.xlsx"),
        "Values",
        Range::parse("B5").expect("a range"),
    );
    assert_ne!(
        read.err(),
        Some(Error::Workbook(Refusal::Size)),
        "120 MiB declared in all is within the bound"
    );
}

#[test]
fn an_entry_that_holds_more_than_it_declares_is_refused_with_the_cause_workbook() {
    let deflated = zip::CompressionMethod::Deflated;
    let zeros = vec![0u8; (33 * MIB) as usize];
    let scratch = Scratch::new("bounds-held");
    with_entries(
        &scratch.path("held.xlsx"),
        &[("docProps/held.bin", zeros, deflated, Some(100))],
    );
    assert!(
        fs::metadata(scratch.path("held.xlsx"))
            .expect("metadata")
            .len()
            < MIB
    );
    refused_by_both(
        &scratch,
        "held.xlsx",
        "an entry that holds 33 MiB and declares 100 bytes",
        Error::Workbook(Refusal::Unreadable),
    );
}

#[test]
fn a_range_of_more_than_100000_cells_is_refused_before_anything_else() {
    let scratch = Scratch::new("bounds-range");
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
    let over = Range::parse("A1:A100001").expect("a range");
    let wide = Range::parse("A1:XFD7").expect("a range");
    for range in [over, wide] {
        let got = folder.prepare(Path::new("missing.xlsx"), &run(), "Values", range, &[]);
        assert_eq!(got.as_ref().err(), Some(&Error::RangeSize));
        assert_eq!(got.err().map(|e| e.cause()), Some(Cause::StepFailed));
        let read = folder.read(Path::new("missing.xlsx"), "Values", range);
        assert_eq!(read.err(), Some(Error::RangeSize));
    }
    let fits = Range::parse("A1:A100000").expect("a range");
    let got = folder.prepare(Path::new("missing.xlsx"), &run(), "Values", fits, &[]);
    let missing = Some(Error::Io(std::io::ErrorKind::NotFound));
    assert_eq!(
        got.err(),
        missing,
        "100,000 cells pass on to the next check"
    );
}

#[test]
fn a_fill_of_100000_cells_is_written_checked_and_read_back() {
    let scratch = Scratch::new("bounds-fill");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
    let range = Range::parse("F1:O10000").expect("a range");
    let values: Vec<Vec<Value>> = (0..10_000)
        .map(|row| {
            (0..10)
                .map(|col| Value::Number(f64::from(row * 10 + col)))
                .collect()
        })
        .collect();
    let start = Instant::now();
    let prepared = folder
        .prepare(Path::new("values.xlsx"), &run(), "Values", range, &values)
        .expect("prepared");
    let took = start.elapsed();
    folder.replace(&prepared).expect("replaced");
    eprintln!("prepare of 100,000 cells took {:.2} s", took.as_secs_f64());
    let corners = Range::parse("O10000").expect("a range");
    let read = folder.read(Path::new("values.xlsx"), "Values", corners);
    assert_eq!(read, Ok(vec![vec![Cell::Number(99_999.0)]]));
    let first = folder.read(
        Path::new("values.xlsx"),
        "Values",
        Range::parse("E1:F1").expect("a range"),
    );
    assert_eq!(first.map(|rows| rows[0][1].clone()), Ok(Cell::Number(0.0)));
}
