mod common;

use std::fs;
use std::path::Path;

use common::{
    book, corpus, element_counts, entry_names, number_at, same_but, Scratch, CARRIED, REFUSED, RUN,
};
use halvo_sheet::{Cause, Error, Folder, Kind, Range, Refusal, RunId, Value};

fn run() -> RunId {
    RunId::parse(RUN).expect("a run id")
}

fn names(file: &str) -> (String, String) {
    let (stem, ext) = file.rsplit_once('.').expect("an extension");
    (
        format!("{stem}.halvo-checkpoint-{RUN}.{ext}"),
        format!("{stem}.halvo-prepared-{RUN}.{ext}"),
    )
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
        let folder = Folder::new(&scratch.dir).expect("the folder opens");
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
        let (checkpoint, copy) = names(change.file);
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
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
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
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
    let range = Range::parse("B2:D6").expect("a range");
    let _held = folder
        .prepare(
            Path::new("formulas.xlsx"),
            &run(),
            "Ledger",
            range,
            &source_rows(),
        )
        .expect("prepared");
    let (_, copy) = names("formulas.xlsx");
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
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    let b5c6 = Range::parse("B5:C6").expect("a range");
    let wide = vec![vec![Value::Number(1.0), Value::Number(2.0)]];
    let ragged = vec![
        vec![Value::Number(1.0), Value::Number(2.0)],
        vec![Value::Number(3.0)],
    ];
    let cases: [(&str, Range, Vec<Vec<Value>>, Error); 5] = [
        ("Values", b5, wide.clone(), Error::Shape),
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
fn a_second_prepare_of_the_same_run_keeps_one_checkpoint_and_one_copy() {
    let scratch = Scratch::new("prepare-twice");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
    let range = Range::parse("B5").expect("a range");
    for _ in 0..2 {
        let _held = folder
            .prepare(
                Path::new("values.xlsx"),
                &run(),
                "Values",
                range,
                &one(77.0),
            )
            .expect("prepared");
        let (checkpoint, copy) = names("values.xlsx");
        assert_eq!(
            scratch.names(""),
            sorted(vec!["values.xlsx".to_string(), checkpoint, copy])
        );
    }
}

#[test]
fn a_checkpoint_of_the_run_that_is_not_the_workbook_is_kept_and_refused() {
    let scratch = Scratch::new("prepare-checkpoint");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let (checkpoint, _) = names("values.xlsx");
    scratch.write(&checkpoint, b"another file");
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
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
        let folder = Folder::new(&scratch.dir).expect("the folder opens");
        let range = Range::parse(cell).expect("a range");
        let _held = folder
            .prepare(
                Path::new(file),
                &run(),
                sheet,
                range,
                &[vec![value.clone()]],
            )
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        let (_, copy) = names(file);
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
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
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
    let _held = folder
        .prepare(Path::new("values.xlsx"), &run(), "Values", range, &values)
        .expect("prepared");
    let (_, copy) = names("values.xlsx");
    let read = folder
        .read(Path::new(&copy), "Values", range)
        .expect("the copy reads");
    let want: Vec<halvo_sheet::Cell> = texts
        .iter()
        .map(|s| halvo_sheet::Cell::Text(s.to_string()))
        .collect();
    assert_eq!(read, vec![want]);
}

type Edits = Vec<(&'static str, &'static str, &'static str)>;

#[test]
fn a_copy_that_keeps_a_part_kind_otherwise_or_rewrites_an_array_formula_gives_the_cause_sheet() {
    let sheet = "xl/worksheets/sheet1.xml";
    let cases: [(&str, Edits); 2] = [
        (
            "merge-lower-case",
            vec![(sheet, "</sheetData>", "</sheetData><mergeCells count=\"1\"><mergeCell ref=\"f10:g11\"/></mergeCells>")],
        ),
        (
            "array-formula",
            vec![
                (sheet, "<c r=\"D2\" t=\"b\"><v>0</v></c></row>", "<c r=\"D2\" t=\"b\"><v>0</v></c><c r=\"F2\"><f t=\"array\" ref=\"F2:F3\">B2:B3*2</f><v>6</v></c></row>"),
                (sheet, "<c r=\"D3\" t=\"b\"><v>1</v></c></row>", "<c r=\"D3\" t=\"b\"><v>1</v></c><c r=\"F3\"><v>12</v></c></row>"),
            ],
        ),
    ];
    for (label, edits) in cases {
        let scratch = Scratch::new("prepare-kept-otherwise");
        common::rewrite(&corpus("values.xlsx"), &scratch.path("values.xlsx"), &edits);
        let before = fs::read(scratch.path("values.xlsx")).expect("read");
        let folder = Folder::new(&scratch.dir).expect("the folder opens");
        let range = Range::parse("B5").expect("a range");
        let got = folder.prepare(
            Path::new("values.xlsx"),
            &run(),
            "Values",
            range,
            &one(77.0),
        );
        assert_eq!(got.err(), Some(Error::Sheet), "{label}");
        assert_eq!(
            scratch.names(""),
            vec!["values.xlsx".to_string()],
            "{label}"
        );
        assert_eq!(
            fs::read(scratch.path("values.xlsx")).expect("read"),
            before,
            "{label}"
        );
    }
}

fn text(s: &str) -> Vec<Vec<Value>> {
    vec![vec![Value::Text(s.to_string())]]
}

#[test]
fn a_miss_after_the_write_gives_the_cause_sheet_and_removes_what_this_call_made() {
    let scratch = Scratch::new("prepare-miss");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let before = fs::read(scratch.path("values.xlsx")).expect("read");
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
    let f2 = Range::parse("F2").expect("a range");
    let got = folder.prepare(
        Path::new("values.xlsx"),
        &run(),
        "Values",
        f2,
        &text("_x0041_"),
    );
    assert_eq!(got.as_ref().err(), Some(&Error::Sheet));
    assert_eq!(got.err().map(|e| e.cause()), Some(Cause::Sheet));
    assert_eq!(scratch.names(""), vec!["values.xlsx".to_string()]);
    assert_eq!(fs::read(scratch.path("values.xlsx")).expect("read"), before);
    folder
        .prepare(
            Path::new("values.xlsx"),
            &run(),
            "Values",
            f2,
            &text("plain"),
        )
        .expect("prepared");
    let got = folder.prepare(
        Path::new("values.xlsx"),
        &run(),
        "Values",
        f2,
        &text("_x0041_"),
    );
    assert_eq!(got.err(), Some(Error::Sheet));
    let (checkpoint, _) = names("values.xlsx");
    assert_eq!(
        scratch.names(""),
        sorted(vec!["values.xlsx".to_string(), checkpoint])
    );
}

#[test]
fn a_text_with_a_character_that_xml_cannot_hold_is_refused_before_any_write() {
    let scratch = Scratch::new("prepare-character");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
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
        let folder = Folder::new(&scratch.dir).expect("the folder opens");
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
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
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
fn a_second_prepare_of_the_run_while_the_first_is_held_writes_the_copy_again() {
    let scratch = Scratch::new("prepare-twice-held");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    let first = folder
        .prepare(Path::new("values.xlsx"), &run(), "Values", b5, &one(77.0))
        .expect("the first prepare");
    let second = folder
        .prepare(Path::new("values.xlsx"), &run(), "Values", b5, &one(88.0))
        .expect("the second prepare");
    let (checkpoint, copy) = names("values.xlsx");
    let listing = sorted(vec!["values.xlsx".to_string(), checkpoint, copy.clone()]);
    assert_eq!(scratch.names(""), listing);
    let read = folder.read(Path::new(&copy), "Values", b5);
    assert_eq!(read, Ok(vec![vec![halvo_sheet::Cell::Number(88.0)]]));
    drop(first);
    assert_eq!(
        scratch.names(""),
        listing,
        "the first drop keeps the second's copy"
    );
    drop(second);
    assert_eq!(
        scratch.names("").len(),
        2,
        "the second drop removes its copy"
    );
}

#[test]
fn a_checkpoint_or_a_prepared_copy_is_taken_as_no_workbook() {
    let scratch = Scratch::new("prepare-own-file");
    fs::copy(corpus("values.xlsx"), scratch.path("values.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    let _held = folder
        .prepare(Path::new("values.xlsx"), &run(), "Values", b5, &one(77.0))
        .expect("prepared");
    let (checkpoint, copy) = names("values.xlsx");
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
    let (checkpoint, _) = names("values.xlsx");
    common::link_file(&outside, &scratch.path("folder").join(&checkpoint));
    let folder = Folder::new(&scratch.path("folder")).expect("the folder opens");
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
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
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
fn a_run_id_is_32_lower_case_hex_digits() {
    assert!(RunId::parse(RUN).is_ok());
    for bad in [
        "",
        "0123456789ABCDEF0123456789abcdef",
        "0123456789abcdef0123456789abcde",
        "0123456789abcdef0123456789abcdef0",
        "g123456789abcdef0123456789abcdef",
        "../23456789abcdef0123456789abcdef",
    ] {
        assert_eq!(RunId::parse(bad).err(), Some(Error::RunId), "{bad:?}");
    }
}
