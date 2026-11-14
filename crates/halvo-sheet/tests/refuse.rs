mod common;

use std::fs;
use std::path::Path;

use common::{corpus, rewrite, rewrite_bytes, Scratch, RUN};
use halvo_sheet::{Cause, Error, Folder, Range, Refusal, RunId, Value};

const SHEET: &str = "xl/worksheets/sheet1.xml";
const ROW2_END: &str = "<c r=\"D2\" t=\"b\"><v>0</v></c></row>";
const ROW3_END: &str = "<c r=\"D3\" t=\"b\"><v>1</v></c></row>";

fn refused_before_any_write(scratch: &Scratch, file: &str, sheet: &str, cell: &str, want: Refusal) {
    let path = scratch.path(file);
    let before = fs::read(&path).expect("read");
    let listing = scratch.names("");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    let range = Range::parse(cell).expect("a range");
    let values = vec![vec![Value::Number(1.0)]];
    let got = folder.prepare(Path::new(file), &run, sheet, range, &values);
    assert_eq!(
        got.as_ref().err(),
        Some(&Error::Workbook(want.clone())),
        "{file} {cell}"
    );
    assert_eq!(
        got.err().map(|e| e.cause()),
        Some(Cause::Workbook),
        "{file} {cell}"
    );
    assert_eq!(
        scratch.names(""),
        listing,
        "{file} {cell}: nothing beside it"
    );
    assert_eq!(
        fs::read(&path).expect("read"),
        before,
        "{file} {cell}: the file is as it was"
    );
}

#[test]
fn a_cell_in_the_header_row_of_a_table_is_refused_and_the_body_is_not() {
    let scratch = Scratch::new("refuse-table-header");
    fs::copy(corpus("chart.xlsx"), scratch.path("chart.xlsx")).expect("copy");
    for cell in ["A1", "B1"] {
        refused_before_any_write(&scratch, "chart.xlsx", "Sales", cell, Refusal::TableHeader);
    }
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    for cell in ["C1", "B2"] {
        let range = Range::parse(cell).expect("a range");
        let values = vec![vec![Value::Number(1.0)]];
        let got = folder.prepare(Path::new("chart.xlsx"), &run, "Sales", range, &values);
        assert!(got.is_ok(), "{cell}: {:?}", got.err());
    }
}

#[test]
fn a_range_that_holds_a_header_cell_and_body_cells_is_refused() {
    let scratch = Scratch::new("refuse-header-and-body");
    fs::copy(corpus("chart.xlsx"), scratch.path("chart.xlsx")).expect("copy");
    let before = fs::read(scratch.path("chart.xlsx")).expect("read");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    for cells in ["A1:A3", "B1:C2", "A1:B9"] {
        let range = Range::parse(cells).expect("a range");
        let row = vec![Value::Number(1.0); range.cols() as usize];
        let values = vec![row; range.rows() as usize];
        let got = folder.prepare(Path::new("chart.xlsx"), &run, "Sales", range, &values);
        assert_eq!(
            got.err(),
            Some(Error::Workbook(Refusal::TableHeader)),
            "{cells}"
        );
        assert_eq!(scratch.names(""), vec!["chart.xlsx".to_string()], "{cells}");
    }
    assert_eq!(fs::read(scratch.path("chart.xlsx")).expect("read"), before);
}

#[test]
fn a_table_with_no_header_row_takes_a_write_into_its_first_row_and_a_table_of_another_namespace_is_refused(
) {
    let table = "xl/tables/table1.xml";
    let scratch = Scratch::new("refuse-no-header");
    let no_header = (
        table,
        r#" totalsRowShown="0""#,
        r#" totalsRowShown="0" headerRowCount="0""#,
    );
    rewrite(
        &corpus("chart.xlsx"),
        &scratch.path("chart.xlsx"),
        &[no_header],
    );
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    let a1 = Range::parse("A1").expect("a range");
    let got = folder.prepare(
        Path::new("chart.xlsx"),
        &run,
        "Sales",
        a1,
        &[vec![Value::Number(1.0)]],
    );
    assert!(got.is_ok(), "{:?}", got.err());
    drop(got);
    let scratch = Scratch::new("refuse-table-namespace");
    let main = r#"<table xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;
    let other = (table, main, r#"<table xmlns="urn:other""#);
    rewrite(&corpus("chart.xlsx"), &scratch.path("chart.xlsx"), &[other]);
    refused_before_any_write(&scratch, "chart.xlsx", "Sales", "C5", Refusal::Part);
}

#[test]
fn a_cell_of_an_array_or_data_table_formula_or_inside_its_range_is_refused() {
    for kind in ["array", "dataTable"] {
        let scratch = Scratch::new("refuse-array");
        let f2 = format!("<c r=\"D2\" t=\"b\"><v>0</v></c><c r=\"F2\"><f t=\"{kind}\" ref=\"F2:F3\">B2:B3*2</f><v>6</v></c></row>");
        let edits = [
            (SHEET, ROW2_END, f2.as_str()),
            (
                SHEET,
                ROW3_END,
                "<c r=\"D3\" t=\"b\"><v>1</v></c><c r=\"F3\"><v>12</v></c></row>",
            ),
        ];
        rewrite(&corpus("values.xlsx"), &scratch.path("values.xlsx"), &edits);
        for cell in ["F2", "F3"] {
            refused_before_any_write(
                &scratch,
                "values.xlsx",
                "Values",
                cell,
                Refusal::ArrayFormula,
            );
        }
    }
}

#[test]
fn the_first_cell_or_a_member_of_a_shared_formula_is_refused() {
    let scratch = Scratch::new("refuse-shared");
    let edits = [
        (SHEET, ROW2_END, "<c r=\"D2\" t=\"b\"><v>0</v></c><c r=\"E2\"><f t=\"shared\" ref=\"E2:E3\" si=\"0\">B2*2</f><v>6</v></c></row>"),
        (SHEET, ROW3_END, "<c r=\"D3\" t=\"b\"><v>1</v></c><c r=\"E3\"><f t=\"shared\" si=\"0\"/><v>12</v></c></row>"),
    ];
    rewrite(&corpus("values.xlsx"), &scratch.path("values.xlsx"), &edits);
    for cell in ["E2", "E3"] {
        refused_before_any_write(
            &scratch,
            "values.xlsx",
            "Values",
            cell,
            Refusal::SharedFormula,
        );
    }
}

type ByteEdit = Box<dyn Fn(Vec<u8>) -> Vec<u8>>;

#[test]
fn a_part_that_the_writer_cannot_take_is_refused() {
    let swap_rows = |bytes: Vec<u8>| {
        let text = String::from_utf8(bytes).expect("utf-8");
        let row2 = text.find("<row r=\"2\"").expect("row 2");
        let row3 = text.find("<row r=\"3\"").expect("row 3");
        let row4 = text.find("<row r=\"4\"").expect("row 4");
        format!(
            "{}{}{}{}",
            &text[..row2],
            &text[row3..row4],
            &text[row2..row3],
            &text[row4..]
        )
        .into_bytes()
    };
    let cases: [(&str, &str, ByteEdit); 8] = [
        (
            "bom-in-sheet",
            SHEET,
            Box::new(|b: Vec<u8>| [vec![0xef, 0xbb, 0xbf], b].concat()),
        ),
        (
            "bom-in-workbook",
            "xl/workbook.xml",
            Box::new(|b: Vec<u8>| [vec![0xef, 0xbb, 0xbf], b].concat()),
        ),
        (
            "not-utf-8",
            SHEET,
            Box::new(|mut b: Vec<u8>| {
                let at = b.windows(9).position(|w| w == b"<v>15</v>").expect("B6");
                b[at + 3] = 0xff;
                b
            }),
        ),
        ("rows-out-of-order", SHEET, Box::new(swap_rows)),
        (
            "cells-out-of-order",
            SHEET,
            Box::new(|b: Vec<u8>| {
                String::from_utf8(b)
                    .expect("utf-8")
                    .replacen(
                        "<c r=\"B2\"><v>3</v></c><c r=\"C2\"><v>1.25</v></c>",
                        "<c r=\"C2\"><v>1.25</v></c><c r=\"B2\"><v>3</v></c>",
                        1,
                    )
                    .into_bytes()
            }),
        ),
        (
            "cell-with-no-reference",
            SHEET,
            Box::new(|b: Vec<u8>| {
                String::from_utf8(b)
                    .expect("utf-8")
                    .replacen("<c r=\"C2\">", "<c>", 1)
                    .into_bytes()
            }),
        ),
        (
            "row-with-no-reference",
            SHEET,
            Box::new(|b: Vec<u8>| {
                String::from_utf8(b)
                    .expect("utf-8")
                    .replacen("<row r=\"3\" spans=\"1:4\">", "<row spans=\"1:4\">", 1)
                    .into_bytes()
            }),
        ),
        (
            "cell-outside-its-row",
            SHEET,
            Box::new(|b: Vec<u8>| {
                String::from_utf8(b)
                    .expect("utf-8")
                    .replacen("<c r=\"C2\">", "<c r=\"C3\">", 1)
                    .into_bytes()
            }),
        ),
    ];
    for (label, part, edit) in cases {
        let scratch = Scratch::new("refuse-part");
        rewrite_bytes(
            &corpus("values.xlsx"),
            &scratch.path("values.xlsx"),
            part,
            edit.as_ref(),
        );
        let file = format!("{label}.xlsx");
        fs::rename(scratch.path("values.xlsx"), scratch.path(&file)).expect("rename");
        refused_before_any_write(&scratch, &file, "Values", "B5", Refusal::Part);
    }
}

#[test]
fn an_encrypted_file_or_an_archive_with_an_entry_name_twice_is_refused() {
    use std::io::{Read, Write};
    let scratch = Scratch::new("refuse-archive");
    let mut cfb = vec![0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];
    cfb.extend(std::iter::repeat_n(0u8, 504));
    scratch.write("encrypted.xlsx", &cfb);
    refused_before_any_write(
        &scratch,
        "encrypted.xlsx",
        "Values",
        "B5",
        Refusal::Unreadable,
    );
    let mut archive =
        zip::ZipArchive::new(fs::File::open(corpus("values.xlsx")).expect("open")).expect("a zip");
    let mut out =
        zip::ZipWriter::new(fs::File::create(scratch.path("twice.xlsx")).expect("create"));
    let options = zip::write::SimpleFileOptions::default();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("entry");
        let name = entry.name().expect("name").to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read");
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(&bytes).expect("write");
    }
    out.start_file("docProps/apq.xml", options).expect("start");
    out.write_all(b"<x/>").expect("write");
    out.finish().expect("finish");
    let mut bytes = fs::read(scratch.path("twice.xlsx")).expect("read");
    let spots: Vec<usize> = bytes
        .windows(16)
        .enumerate()
        .filter(|(_, w)| *w == b"docProps/apq.xml")
        .map(|(at, _)| at)
        .collect();
    assert_eq!(
        spots.len(),
        2,
        "the name stands in the local header and in the directory"
    );
    for at in spots {
        bytes[at..at + 16].copy_from_slice(b"docProps/app.xml");
    }
    fs::write(scratch.path("twice.xlsx"), &bytes).expect("write");
    refused_before_any_write(&scratch, "twice.xlsx", "Values", "B5", Refusal::Unreadable);
}
