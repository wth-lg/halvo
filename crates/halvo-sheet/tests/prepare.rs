mod common;

use std::fs;
use std::path::Path;

use common::{
    book, corpus, element_counts, entry_names, number_at, same_but, Scratch, CARRIED, REFUSED, RUN,
};
use halvo_sheet::{Cause, Error, Folder, Kind, Prepared, Range, Refusal, RunId, Sha256, Value};

fn run() -> RunId {
    RunId::parse(RUN).expect("a run id")
}

fn checkpoint_name(file: &str) -> String {
    let (stem, ext) = file.rsplit_once('.').expect("an extension");
    format!("{stem}.halvo-checkpoint-{RUN}.{ext}")
}

fn copy_name(prepared: &Prepared) -> String {
    let name = prepared.copy().file_name().expect("a name");
    let name = name.to_string_lossy().into_owned();
    let stem = name.split(".halvo-prepared-").next().expect("a stem");
    let rest = &name[stem.len() + ".halvo-prepared-".len()..];
    let (own, ext) = rest.split_once('.').expect("an extension");
    let (run, parts) = own.split_once('-').expect("a part of its own");
    let (part, digest) = parts.split_once('-').expect("a digest");
    assert_eq!(run, RUN, "{name}");
    for hex in [part, digest] {
        assert!(
            hex.len() == 16 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
            "{name}"
        );
    }
    let bytes = fs::read(prepared.copy()).expect("read the copy");
    assert!(Sha256::of(&bytes).hex().starts_with(digest), "{name}");
    assert!(!ext.is_empty(), "{name}");
    name
}

fn sorted(mut names: Vec<String>) -> Vec<String> {
    names.sort();
    names
}

fn one(x: f64) -> Vec<Vec<Value>> {
    vec![vec![Value::Number(x)]]
}

#[test]
fn prepare_carries_each_workbook_that_the_verdict_does_not_refuse() {
    let mut wrong = Vec::new();
    for change in CARRIED {
        let scratch = Scratch::new("prepare-carried");
        let original = scratch.path(change.file);
        fs::copy(corpus(change.file), &original).expect("copy");
        let before = fs::read(&original).expect("read");
        let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
        let range = Range::parse(change.cell).expect("a range");
        let prepared = folder
            .prepare(
                Path::new(change.file),
                &run(),
                change.sheet,
                range,
                &one(change.new),
            )
            .unwrap_or_else(|e| panic!("{}: {e}", change.file));
        let (checkpoint, copy) = (checkpoint_name(change.file), copy_name(&prepared));
        let listing = sorted(vec![
            change.file.to_string(),
            checkpoint.clone(),
            copy.clone(),
        ]);
        if scratch.names("") != listing {
            wrong.push(format!(
                "{}: beside it {:?}",
                change.file,
                scratch.names("")
            ));
        }
        if fs::read(&original).expect("read") != before {
            wrong.push(format!("{}: prepare changed the workbook", change.file));
        }
        if fs::read(scratch.path(&checkpoint)).expect("read") != before {
            wrong.push(format!(
                "{}: the checkpoint is not the workbook",
                change.file
            ));
        }
        if prepared.checkpoint() != scratch.path(&checkpoint).canonicalize().expect("canon") {
            wrong.push(format!("{}: checkpoint() names another file", change.file));
        }
        let saved = book(&scratch.path(&copy));
        let differ = same_but(&book(&original), &saved, change.sheet, &[change.pos]);
        if !differ.is_empty() {
            wrong.push(format!("{}: {differ:?}", change.file));
        }
        if number_at(&saved, change.sheet, change.pos) != Some(change.new) {
            wrong.push(format!(
                "{}: the cell does not hold the new value",
                change.file
            ));
        }
        let (had, has) = (
            element_counts(&original),
            element_counts(&scratch.path(&copy)),
        );
        for ((tag, n), (_, m)) in had.iter().zip(&has) {
            if m < n {
                wrong.push(format!("{}: {tag} {n} became {m}", change.file));
            }
        }
        if entry_names(&original) != entry_names(&scratch.path(&copy)) {
            wrong.push(format!("{}: the archive entries differ", change.file));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

fn with_entry(scratch: &Scratch, file: &str, entry: &str) -> String {
    use std::io::{Read, Write};
    let mut archive =
        zip::ZipArchive::new(fs::File::open(corpus("values.xlsx")).expect("open")).expect("a zip");
    let mut out = zip::ZipWriter::new(fs::File::create(scratch.path(file)).expect("create"));
    let options = zip::write::SimpleFileOptions::default();
    for i in 0..archive.len() {
        let mut part = archive.by_index(i).expect("entry");
        let name = part.name().expect("name").to_string();
        let mut bytes = Vec::new();
        part.read_to_end(&mut bytes).expect("read");
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(&bytes).expect("write");
    }
    out.start_file(entry, options).expect("start");
    out.write_all(b"<x/>").expect("write");
    out.finish().expect("finish");
    file.to_string()
}

#[test]
fn prepare_refuses_a_workbook_with_a_refused_kind_before_it_writes_anything() {
    let scratch = Scratch::new("prepare-refused");
    fs::copy(corpus(REFUSED.file), scratch.path(REFUSED.file)).expect("copy");
    let pivot = with_entry(&scratch, "pivot.xlsx", "xl/pivotTables/pivotTable1.xml");
    let macro_book = with_entry(&scratch, "macro.xlsm", "xl/vbaProject.bin");
    let cases = [
        (REFUSED.file, REFUSED.sheet, Kind::Validation),
        (pivot.as_str(), "Values", Kind::Pivot),
        (macro_book.as_str(), "Values", Kind::Macro),
    ];
    let listing = scratch.names("");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    for (file, sheet, kind) in cases {
        let before = fs::read(scratch.path(file)).expect("read");
        let range = Range::parse("B3").expect("a range");
        let got = folder.prepare(Path::new(file), &run(), sheet, range, &one(7.0));
        assert_eq!(
            got.as_ref().err(),
            Some(&Error::Workbook(Refusal::Kinds(vec![kind]))),
            "{file}"
        );
        let error = got.expect_err("an error");
        assert_eq!(error.cause(), Cause::Workbook, "{file}");
        assert_eq!(error.cause().name(), "workbook");
        assert_eq!(scratch.names(""), listing, "{file}");
        assert_eq!(
            fs::read(scratch.path(file)).expect("read"),
            before,
            "{file}"
        );
    }
}

fn source_rows() -> Vec<Vec<Value>> {
    let t = |s: &str| Value::Text(s.to_string());
    let n = Value::Number;
    vec![
        vec![t("2026-10-01"), t("Water meter reading"), n(41.2)],
        vec![t("2026-10-08"), t("Power bill"), n(63.05)],
        vec![t("2026-10-15"), t("Groceries"), n(87.4)],
        vec![t("2026-10-22"), t("Bus pass"), n(30.0)],
        vec![t("2026-10-29"), t("Phone plan"), n(19.99)],
    ]
}

#[test]
fn the_fill_writes_the_seed_rows_into_b2_d6_of_formulas_xlsx() {
    let scratch = Scratch::new("prepare-fill");
    fs::copy(corpus("formulas.xlsx"), scratch.path("formulas.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let range = Range::parse("B2:D6").expect("a range");
    let held = folder
        .prepare(
            Path::new("formulas.xlsx"),
            &run(),
            "Ledger",
            range,
            &source_rows(),
        )
        .expect("prepared");
    let copy = copy_name(&held);
    let targets: Vec<(u32, u32)> = (1..=5).flat_map(|r| (1..=3).map(move |c| (r, c))).collect();
    let original = book(&scratch.path("formulas.xlsx"));
    let saved = book(&scratch.path(&copy));
    assert_eq!(
        same_but(&original, &saved, "Ledger", &targets),
        Vec::<String>::new()
    );
    let read = folder
        .read(
            Path::new(&copy),
            "Ledger",
            Range::parse("B2:D7").expect("a range"),
        )
        .expect("the copy reads");
    let mut want: Vec<Vec<halvo_sheet::Cell>> = source_rows()
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|v| match v {
                    Value::Number(x) => halvo_sheet::Cell::Number(x),
                    Value::Text(s) => halvo_sheet::Cell::Text(s),
                })
                .collect()
        })
        .collect();
    want.push(vec![
        halvo_sheet::Cell::Empty,
        halvo_sheet::Cell::Empty,
        halvo_sheet::Cell::Number(0.0),
    ]);
    assert_eq!(read, want);
    let (_, ledger) = &saved[0];
    assert_eq!(
        ledger.get(&(6, 3)).and_then(|c| c.1.clone()),
        Some("SUM(D2:D6)".to_string())
    );
}

#[test]
fn prepare_refuses_a_shape_or_a_sheet_that_does_not_fit_before_it_writes() {
    let scratch = Scratch::new("prepare-shape");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    let b5c6 = Range::parse("B5:C6").expect("a range");
    let wide = vec![vec![Value::Number(1.0), Value::Number(2.0)]];
    let ragged = vec![
        vec![Value::Number(1.0), Value::Number(2.0)],
        vec![Value::Number(3.0)],
    ];
    let tall = vec![vec![Value::Number(1.0)], vec![Value::Number(2.0)]];
    let cases: [(&str, Range, Vec<Vec<Value>>, Error); 6] = [
        ("Values", b5, wide.clone(), Error::Shape),
        ("Values", b5, tall, Error::Shape),
        ("Values", b5, Vec::new(), Error::Shape),
        ("Values", b5c6, ragged, Error::Shape),
        ("Values", b5c6, wide, Error::Shape),
        ("Nowhere", b5, one(1.0), Error::NoSheet),
    ];
    for (sheet, range, values, want) in cases {
        let got = folder.prepare(Path::new("values.xlsx"), &run(), sheet, range, &values);
        assert_eq!(got.err(), Some(want.clone()), "{sheet} {values:?}");
        assert_eq!(want.cause(), Cause::StepFailed);
        assert_eq!(scratch.names(""), vec!["values.xlsx".to_string()]);
    }
}

#[test]
fn each_prepare_of_a_run_writes_a_copy_of_its_own_and_the_first_makes_the_one_checkpoint() {
    let scratch = Scratch::new("prepare-twice");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let range = Range::parse("B5").expect("a range");
    let checkpoint = checkpoint_name("values.xlsx");
    let first = folder
        .prepare(
            Path::new("values.xlsx"),
            &run(),
            "Values",
            range,
            &one(77.0),
        )
        .expect("the first prepare");
    let made = fs::metadata(scratch.path(&checkpoint)).expect("the checkpoint");
    let second = folder
        .prepare(
            Path::new("values.xlsx"),
            &run(),
            "Values",
            range,
            &one(88.0),
        )
        .expect("the second prepare");
    let (one_copy, other_copy) = (copy_name(&first), copy_name(&second));
    assert_ne!(one_copy, other_copy);
    let listing = sorted(vec![
        "values.xlsx".to_string(),
        checkpoint.clone(),
        one_copy.clone(),
        other_copy.clone(),
    ]);
    assert_eq!(scratch.names(""), listing);
    let kept = fs::metadata(scratch.path(&checkpoint)).expect("the checkpoint");
    assert_eq!(
        kept.modified().expect("a time"),
        made.modified().expect("a time"),
        "the second prepare does not write the checkpoint again"
    );
    let b5 = |copy: &str| folder.read(Path::new(copy), "Values", range);
    assert_eq!(
        b5(&one_copy),
        Ok(vec![vec![halvo_sheet::Cell::Number(77.0)]])
    );
    assert_eq!(
        b5(&other_copy),
        Ok(vec![vec![halvo_sheet::Cell::Number(88.0)]])
    );
    drop(second);
    let left = sorted(vec![
        "values.xlsx".to_string(),
        checkpoint.clone(),
        one_copy,
    ]);
    assert_eq!(
        scratch.names(""),
        left,
        "a drop leaves the checkpoint that its call did not make"
    );
    drop(first);
    assert_eq!(
        scratch.names(""),
        sorted(vec![checkpoint, "values.xlsx".to_string()]),
        "the maker's drop leaves it too: no prepare removes a checkpoint"
    );
}

#[test]
fn a_second_prepare_of_the_run_leaves_the_first_copy_as_its_check_read_it() {
    let scratch = Scratch::new("prepare-twice-held");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    let first = folder
        .prepare(Path::new("values.xlsx"), &run(), "Values", b5, &one(77.0))
        .expect("the first prepare");
    let checked = fs::read(first.copy()).expect("read");
    let second = folder
        .prepare(Path::new("values.xlsx"), &run(), "Values", b5, &one(88.0))
        .expect("the second prepare");
    assert_eq!(fs::read(first.copy()).expect("read"), checked);
    folder.replace(&second).expect("the second replace");
    let read = folder.read(Path::new("values.xlsx"), "Values", b5);
    assert_eq!(read, Ok(vec![vec![halvo_sheet::Cell::Number(88.0)]]));
    assert_eq!(
        folder.replace(&first),
        Err(Error::NotReplaced(halvo_sheet::Kept::Workbook))
    );
    let again = folder.prepare(Path::new("values.xlsx"), &run(), "Values", b5, &one(99.0));
    assert_eq!(
        again.err(),
        Some(Error::Checkpoint),
        "one replace of a workbook per run"
    );
}

#[test]
fn a_checkpoint_of_the_run_that_is_not_the_workbook_is_kept_and_refused() {
    let scratch = Scratch::new("prepare-checkpoint");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let checkpoint = checkpoint_name("values.xlsx");
    scratch.write(&checkpoint, b"another file");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let range = Range::parse("B5").expect("a range");
    let got = folder.prepare(
        Path::new("values.xlsx"),
        &run(),
        "Values",
        range,
        &one(77.0),
    );
    assert_eq!(got.err(), Some(Error::Checkpoint));
    assert_eq!(
        fs::read(scratch.path(&checkpoint)).expect("read"),
        b"another file"
    );
    assert_eq!(
        scratch.names(""),
        sorted(vec!["values.xlsx".to_string(), checkpoint])
    );
}

type Edge = (&'static str, &'static str, &'static str, (u32, u32), Value);

#[test]
fn prepare_writes_over_a_formula_inside_a_merged_range_and_inside_a_table() {
    let cases: [Edge; 3] = [
        ("formulas.xlsx", "Ledger", "D7", (6, 3), Value::Number(5.0)),
        ("merged.xlsx", "Merged", "B10", (9, 1), Value::Number(8.0)),
        (
            "chart.xlsx",
            "Sales",
            "A3",
            (2, 0),
            Value::Text("February".to_string()),
        ),
    ];
    for (file, sheet, cell, pos, value) in cases {
        let scratch = Scratch::new("prepare-edges");
        fs::copy(corpus(file), scratch.path(file)).expect("copy");
        let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
        let range = Range::parse(cell).expect("a range");
        let held = folder
            .prepare(
                Path::new(file),
                &run(),
                sheet,
                range,
                &[vec![value.clone()]],
            )
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        let copy = copy_name(&held);
        let saved = book(&scratch.path(&copy));
        assert_eq!(
            same_but(&book(&scratch.path(file)), &saved, sheet, &[pos]),
            Vec::<String>::new(),
            "{file}"
        );
        let (_, cells) = saved.iter().find(|(n, _)| n == sheet).expect("the sheet");
        let want = match value {
            Value::Number(x) => calamine::Data::Float(x),
            Value::Text(s) => calamine::Data::String(s),
        };
        assert_eq!(cells.get(&pos), Some(&(want, None)), "{file}");
    }
}

#[test]
fn text_of_every_kind_comes_back_as_it_was_written() {
    let scratch = Scratch::new("prepare-text");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let texts = [
        " padded ",
        "a & b < c > d \" e ' f",
        "é ü ß 東京 🙂",
        "=SUM(A1:A2)",
        "TRUE",
        "12",
        "tab\there",
    ];
    let values = vec![texts
        .iter()
        .map(|s| Value::Text(s.to_string()))
        .collect::<Vec<_>>()];
    let range = Range::parse("F2:L2").expect("a range");
    let held = folder
        .prepare(Path::new("values.xlsx"), &run(), "Values", range, &values)
        .expect("prepared");
    let copy = copy_name(&held);
    let read = folder
        .read(Path::new(&copy), "Values", range)
        .expect("the copy reads");
    let want: Vec<halvo_sheet::Cell> = texts
        .iter()
        .map(|s| halvo_sheet::Cell::Text(s.to_string()))
        .collect();
    assert_eq!(read, vec![want]);
}

fn text(s: &str) -> Vec<Vec<Value>> {
    vec![vec![Value::Text(s.to_string())]]
}

#[test]
fn a_text_with_a_character_that_xml_cannot_hold_is_refused_before_any_write() {
    let scratch = Scratch::new("prepare-character");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let f2 = Range::parse("F2").expect("a range");
    for bad in [
        "\u{1}", "x\u{0}y", "a\u{b}b", "\u{1f}", "\u{fffe}", "\u{ffff}",
    ] {
        let got = folder.prepare(Path::new("values.xlsx"), &run(), "Values", f2, &text(bad));
        assert_eq!(
            got.err(),
            Some(Error::Value),
            "{:?}",
            bad.escape_unicode().to_string()
        );
        assert_eq!(Error::Value.cause(), Cause::StepFailed);
        assert_eq!(scratch.names(""), vec!["values.xlsx".to_string()]);
    }
}

#[test]
fn a_text_longer_than_a_cell_holds_is_refused_before_any_write() {
    let f2 = Range::parse("F2").expect("a range");
    let longest = "é".repeat(32_767);
    let longer = format!("{longest}x");
    let wide = "🙂".repeat(16_384);
    for bad in [longer, wide] {
        let scratch = Scratch::new("prepare-length");
        fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
        let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
        let got = folder.prepare(Path::new("values.xlsx"), &run(), "Values", f2, &text(&bad));
        assert_eq!(
            got.err(),
            Some(Error::Value),
            "a text of {} bytes",
            bad.len()
        );
        assert_eq!(scratch.names(""), vec!["values.xlsx".to_string()]);
    }
    let scratch = Scratch::new("prepare-length-fits");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    folder
        .prepare(
            Path::new("values.xlsx"),
            &run(),
            "Values",
            f2,
            &text(&longest),
        )
        .expect("a text of 32,767 characters fits a cell");
}

#[test]
fn a_number_that_is_not_finite_is_refused_before_any_write() {
    let scratch = Scratch::new("prepare-finite");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let got = folder.prepare(Path::new("values.xlsx"), &run(), "Values", b5, &one(x));
        assert_eq!(got.err(), Some(Error::Value), "{x}");
        assert_eq!(scratch.names(""), vec!["values.xlsx".to_string()]);
    }
}

#[test]
fn a_checkpoint_or_a_prepared_copy_is_taken_as_no_workbook() {
    let scratch = Scratch::new("prepare-own-file");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    let held = folder
        .prepare(Path::new("values.xlsx"), &run(), "Values", b5, &one(77.0))
        .expect("prepared");
    let (checkpoint, copy) = (checkpoint_name("values.xlsx"), copy_name(&held));
    let listing = scratch.names("");
    let other = RunId::parse("fedcba9876543210fedcba9876543210").expect("a run id");
    for file in [checkpoint.as_str(), copy.as_str()] {
        let before = fs::read(scratch.path(file)).expect("read");
        let got = folder.prepare(Path::new(file), &other, "Values", b5, &one(1.0));
        let refused = Error::Path(halvo_sheet::PathRefusal::OwnFile);
        assert_eq!(got.err(), Some(refused), "{file}");
        assert_eq!(
            fs::read(scratch.path(file)).expect("read"),
            before,
            "{file}"
        );
        assert_eq!(scratch.names(""), listing, "{file}");
    }
}

#[test]
fn a_checkpoint_name_that_is_a_link_to_a_file_outside_is_refused() {
    let scratch = Scratch::new("prepare-checkpoint-link");
    fs::create_dir_all(scratch.path("folder")).expect("make the folder");
    fs::copy(corpus("values.xlsx"), scratch.path("folder/values.xlsx")).expect("copy");
    let outside = scratch.write(
        "outside.xlsx",
        &fs::read(corpus("values.xlsx")).expect("read"),
    );
    let checkpoint = checkpoint_name("values.xlsx");
    common::link_file(&outside, &scratch.path("folder").join(&checkpoint));
    let folder = Folder::new(&scratch.path("folder"), common::locks()).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    let got = folder.prepare(Path::new("values.xlsx"), &run(), "Values", b5, &one(77.0));
    assert_eq!(got.err(), Some(Error::Checkpoint));
    assert_eq!(
        fs::read(&outside).expect("read"),
        fs::read(corpus("values.xlsx")).expect("read")
    );
    assert_eq!(
        scratch.names("folder"),
        sorted(vec![checkpoint, "values.xlsx".to_string()])
    );
}

#[test]
fn a_number_written_into_a_date_formatted_cell_holds_that_number() {
    let scratch = Scratch::new("prepare-date-cell");
    fs::copy(corpus("formats.xlsx"), scratch.path("formats.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let c2 = Range::parse("C2").expect("a range");
    let prepared = folder
        .prepare(
            Path::new("formats.xlsx"),
            &run(),
            "Formats",
            c2,
            &one(46100.0),
        )
        .expect("a number fits a date-formatted cell");
    folder.replace(&prepared).expect("replaced");
    let read = folder.read(Path::new("formats.xlsx"), "Formats", c2);
    assert_eq!(read, Ok(vec![vec![halvo_sheet::Cell::Number(46100.0)]]));
}

#[test]
fn a_text_of_more_than_1024_bytes_is_written_whole() {
    let scratch = Scratch::new("prepare-long-text");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let f2 = Range::parse("F2").expect("a range");
    let long = "Water meter reading, ".repeat(250);
    assert!(long.len() > 5_000);
    let prepared = folder
        .prepare(Path::new("values.xlsx"), &run(), "Values", f2, &text(&long))
        .expect("prepared");
    folder.replace(&prepared).expect("replaced");
    let read = folder.read(Path::new("values.xlsx"), "Values", f2);
    assert_eq!(read, Ok(vec![vec![halvo_sheet::Cell::Text(long)]]));
}

fn set_read_only(path: &Path, read_only: bool) {
    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(read_only);
    fs::set_permissions(path, permissions).expect("set the permissions");
}

#[test]
fn a_workbook_that_the_person_cannot_write_is_refused_before_any_write() {
    let scratch = Scratch::new("prepare-read-only");
    let workbook = scratch.path("values.xlsx");
    fs::copy(corpus("values.xlsx"), &workbook).expect("copy");
    set_read_only(&workbook, true);
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    let got = folder.prepare(Path::new("values.xlsx"), &run(), "Values", b5, &one(77.0));
    let names = scratch.names("");
    set_read_only(&workbook, false);
    assert_eq!(got.as_ref().err(), Some(&Error::ReadOnly));
    assert_eq!(got.err().map(|e| e.cause()), Some(Cause::StepFailed));
    assert_eq!(names, vec!["values.xlsx".to_string()]);
}

#[cfg(unix)]
#[test]
fn the_checkpoint_and_the_copy_get_the_workbooks_permission_bits() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &Path| fs::metadata(path).expect("metadata").permissions().mode() & 0o7777;
    let scratch = Scratch::new("prepare-mode");
    let workbook = scratch.path("values.xlsx");
    fs::copy(corpus("values.xlsx"), &workbook).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    let runs = [
        "a0000000000000000000000000000640",
        "a0000000000000000000000000000600",
        "a0000000000000000000000000000664",
    ];
    for (bits, id) in [0o640, 0o600, 0o664].into_iter().zip(runs) {
        fs::set_permissions(&workbook, fs::Permissions::from_mode(bits)).expect("chmod");
        let own = RunId::parse(id).expect("a run id");
        let prepared = folder
            .prepare(Path::new("values.xlsx"), &own, "Values", b5, &one(77.0))
            .expect("prepared");
        assert_eq!(mode(prepared.checkpoint()), bits, "{bits:o}");
        assert_eq!(mode(prepared.copy()), bits, "{bits:o}");
    }
    fs::set_permissions(&workbook, fs::Permissions::from_mode(0o460)).expect("chmod");
    let got = folder.prepare(Path::new("values.xlsx"), &run(), "Values", b5, &one(77.0));
    assert_eq!(
        got.err(),
        Some(Error::ReadOnly),
        "the owner has no write bit"
    );
    fs::set_permissions(&workbook, fs::Permissions::from_mode(0o640)).expect("chmod");
    let prepared = folder
        .prepare(Path::new("values.xlsx"), &run(), "Values", b5, &one(77.0))
        .expect("prepared");
    folder.replace(&prepared).expect("replaced");
    assert_eq!(
        mode(&workbook),
        0o640,
        "the replaced workbook keeps its bits"
    );
}

#[cfg(windows)]
#[test]
fn the_checkpoint_and_the_copy_of_a_writable_workbook_are_writable() {
    let scratch = Scratch::new("prepare-mode");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    let prepared = folder
        .prepare(Path::new("values.xlsx"), &run(), "Values", b5, &one(77.0))
        .expect("prepared");
    for path in [prepared.checkpoint(), prepared.copy()] {
        let read_only = fs::metadata(path)
            .expect("metadata")
            .permissions()
            .readonly();
        assert!(!read_only, "{path:?}");
    }
}

#[test]
fn two_threads_of_one_run_or_of_two_leave_the_original_or_a_checked_copy() {
    let other = RunId::parse("fedcba9876543210fedcba9876543210").expect("a run id");
    for runs in [[run(), run()], [run(), other]] {
        for round in 0..8 {
            let scratch = Scratch::new("prepare-threads");
            fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
            let original = fs::read(scratch.path("values.xlsx")).expect("read");
            let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
            let b5 = Range::parse("B5").expect("a range");
            let barrier = std::sync::Barrier::new(2);
            let checked: Vec<Option<Vec<u8>>> = std::thread::scope(|s| {
                let workers: Vec<_> = runs
                    .iter()
                    .enumerate()
                    .map(|(at, id)| {
                        let (folder, barrier) = (&folder, &barrier);
                        s.spawn(move || {
                            barrier.wait();
                            let value = one(100.0 + at as f64);
                            let path = Path::new("values.xlsx");
                            let prepared = folder.prepare(path, id, "Values", b5, &value);
                            let bytes = prepared
                                .as_ref()
                                .ok()
                                .map(|p| fs::read(p.copy()).expect("read the copy"));
                            barrier.wait();
                            if let Ok(prepared) = &prepared {
                                let _ = folder.replace(prepared);
                            }
                            bytes
                        })
                    })
                    .collect();
                workers
                    .into_iter()
                    .map(|w| w.join().expect("joined"))
                    .collect()
            });
            let now = fs::read(scratch.path("values.xlsx")).expect("read");
            let allowed = now == original || checked.iter().flatten().any(|c| *c == now);
            assert!(
                allowed,
                "round {round}: the workbook is neither the original nor a checked copy"
            );
        }
    }
}

#[test]
fn a_drop_of_the_call_that_made_the_checkpoint_beside_another_calls_replace_never_takes_it() {
    let scratch = Scratch::new("prepare-drop-race");
    let source = fs::read(corpus("values.xlsx")).expect("read");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    for round in 0..50 {
        let name = format!("round{round}.xlsx");
        fs::write(scratch.path(&name), &source).expect("write");
        let first = folder
            .prepare(Path::new(&name), &run(), "Values", b5, &one(7.0))
            .expect("the first prepare makes the checkpoint");
        let second = folder
            .prepare(Path::new(&name), &run(), "Values", b5, &one(8.0))
            .expect("the second prepare takes it");
        let checkpoint = second.checkpoint().to_path_buf();
        let barrier = std::sync::Barrier::new(2);
        let answer = std::thread::scope(|s| {
            let dropper = s.spawn(|| {
                barrier.wait();
                drop(first);
            });
            let replacer = s.spawn(|| {
                barrier.wait();
                folder.replace(&second)
            });
            dropper.join().expect("joined");
            replacer.join().expect("joined")
        });
        assert_eq!(answer, Ok(()), "round {round}");
        assert_eq!(
            fs::read(&checkpoint).expect("the checkpoint stays"),
            source,
            "round {round}"
        );
    }
}

#[test]
fn a_run_id_is_32_lower_case_hex_digits() {
    assert!(RunId::parse(RUN).is_some());
    for bad in [
        "",
        "0123456789ABCDEF0123456789abcdef",
        "0123456789abcdef0123456789abcde",
        "0123456789abcdef0123456789abcdef0",
        "g123456789abcdef0123456789abcdef",
        "../23456789abcdef0123456789abcdef",
    ] {
        assert_eq!(RunId::parse(bad), None, "{bad:?}");
    }
}

#[test]
fn prepare_gives_the_checkpoints_sha256_length_and_name() {
    let scratch = Scratch::new("prepare-made");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let source = fs::read(scratch.path("values.xlsx")).expect("read");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let name = checkpoint_name("values.xlsx");
    let mut seen = Vec::new();
    for cell in ["B5", "C7"] {
        let range = Range::parse(cell).expect("a range");
        let values = [vec![Value::Number(77.0)]];
        let prepared = folder
            .prepare(Path::new("values.xlsx"), &run(), "Values", range, &values)
            .expect("prepared");
        assert_eq!(prepared.checkpoint_name(), std::ffi::OsStr::new(&name));
        let file = prepared.checkpoint().file_name().expect("a name");
        assert_eq!(file, std::ffi::OsStr::new(&name), "{cell}");
        assert_eq!(prepared.checkpoint_bytes(), source.len() as u64, "{cell}");
        assert_eq!(prepared.checkpoint_sha256(), Sha256::of(&source), "{cell}");
        assert_eq!(fs::read(prepared.checkpoint()).expect("read"), source);
        seen.push(prepared);
    }
    let built = halvo_checkpoint::checkpoint_name(Path::new("values.xlsx"), &run());
    assert_eq!(built, Some(std::ffi::OsString::from(&name)));
}
