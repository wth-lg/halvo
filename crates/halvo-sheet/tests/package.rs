mod common;

use std::fs;
use std::io::Read;
use std::path::Path;

use common::{corpus, Scratch, CARRIED, RUN};
use halvo_sheet::{Folder, Range, RunId, Value};

const SHEET: &str = "xl/worksheets/sheet1.xml";

fn entries(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).expect("open")).expect("a zip");
    (0..archive.len())
        .map(|i| {
            let mut entry = archive.by_index(i).expect("entry");
            let name = entry.name().expect("name").to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read");
            (name, bytes)
        })
        .collect()
}

fn cut_element(text: &mut String, open: &str) {
    if let Some(start) = text.find(open) {
        let tag_end = start + text[start..].find('>').expect("a tag end");
        let end = if text[..tag_end].ends_with('/') {
            tag_end + 1
        } else {
            let close = text[tag_end..].find("</c>").expect("a cell end");
            tag_end + close + "</c>".len()
        };
        text.replace_range(start..end, "");
    }
}

fn cut_attribute(text: &mut String, name: &str) {
    while let Some(start) = text.find(&format!(" {name}=\"")) {
        let value = start + name.len() + 3;
        let end = value + text[value..].find('"').expect("a quote");
        text.replace_range(start..end + 1, "");
    }
}

fn cut_attribute_in_tag(text: &mut String, tag: &str, name: &str) {
    if let Some(start) = text.find(tag) {
        let end = start + text[start..].find('>').expect("a tag end");
        let mut inside = text[start..end].to_string();
        cut_attribute(&mut inside, name);
        text.replace_range(start..end, &inside);
    }
}

fn row_of(cell: &str) -> &str {
    cell.trim_start_matches(|c: char| c.is_ascii_alphabetic())
}

fn rows(text: &str) -> Vec<String> {
    text.match_indices("<row r=\"")
        .map(|(at, _)| {
            let rest = &text[at + 8..];
            rest[..rest.find('"').expect("a quote")].to_string()
        })
        .collect()
}

fn sheet_without_the_allowed_places(text: &str, cells: &[&str], known: &[String]) -> String {
    let mut text = text.to_string();
    for cell in cells {
        cut_element(&mut text, &format!("<c r=\"{cell}\""));
        cut_attribute_in_tag(&mut text, &format!("<row r=\"{}\"", row_of(cell)), "spans");
    }
    cut_attribute_in_tag(&mut text, "<dimension ", "ref");
    for row in rows(&text) {
        if !known.contains(&row) {
            let empty = format!("<row r=\"{row}\"></row>");
            text = text.replace(&empty, "");
        }
    }
    text
}

fn without_flag(text: &str) -> String {
    let mut text = text.to_string();
    cut_attribute(&mut text, "fullCalcOnLoad");
    text.replace("<calcPr/>", "")
}

pub fn package_differences(original: &Path, saved: &Path, cells: &[&str]) -> Vec<String> {
    let (before, after) = (entries(original), entries(saved));
    let names = |all: &[(String, Vec<u8>)]| all.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    if names(&before) != names(&after) {
        return vec![format!(
            "entry names or order differ: {:?} against {:?}",
            names(&before),
            names(&after)
        )];
    }
    let mut wrong = Vec::new();
    for ((name, old), (_, new)) in before.iter().zip(&after) {
        let same = if name == SHEET {
            let old = String::from_utf8_lossy(old);
            let new = String::from_utf8_lossy(new);
            let known = rows(&old);
            sheet_without_the_allowed_places(&old, cells, &known)
                == sheet_without_the_allowed_places(&new, cells, &known)
        } else if name == "xl/workbook.xml" {
            old == new
                || (String::from_utf8_lossy(new).contains("fullCalcOnLoad=\"1\"")
                    && without_flag(&String::from_utf8_lossy(old))
                        == without_flag(&String::from_utf8_lossy(new)))
        } else {
            old == new
        };
        if !same {
            wrong.push(format!("{name} differs"));
        }
    }
    wrong
}

fn written(
    scratch: &Scratch,
    source: &Path,
    cells: &[(&str, Value)],
) -> Result<(), halvo_sheet::Error> {
    fs::copy(source, scratch.path("book.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    let first = cells.first().expect("a cell").0;
    let last = cells.last().expect("a cell").0;
    let range = Range::parse(&format!("{first}:{last}")).expect("a range");
    let values = vec![cells.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>()];
    let sheet = sheet_name(source);
    folder
        .prepare(Path::new("book.xlsx"), &run, &sheet, range, &values)
        .and_then(|prepared| folder.replace(&prepared))
}

fn sheet_name(source: &Path) -> String {
    let book = entries(source)
        .into_iter()
        .find(|(n, _)| n == "xl/workbook.xml")
        .expect("a workbook part")
        .1;
    let text = String::from_utf8(book).expect("utf-8");
    let at = text.find("<sheet name=\"").expect("a sheet") + 13;
    text[at..at + text[at..].find('"').expect("a quote")].to_string()
}

type Edits = Vec<(&'static str, &'static str, &'static str)>;

#[test]
fn a_lower_case_merge_and_an_array_or_shared_formula_elsewhere_are_carried_byte_for_byte() {
    let sheet = SHEET;
    let fixtures: [(&str, Edits); 3] = [
        ("merge-lower-case", vec![(sheet, "</sheetData>", "</sheetData><mergeCells count=\"1\"><mergeCell ref=\"f10:g11\"/></mergeCells>")]),
        (
            "array-formula",
            vec![
                (sheet, "<c r=\"D2\" t=\"b\"><v>0</v></c></row>", "<c r=\"D2\" t=\"b\"><v>0</v></c><c r=\"F2\"><f t=\"array\" ref=\"F2:F3\">B2:B3*2</f><v>6</v></c></row>"),
                (sheet, "<c r=\"D3\" t=\"b\"><v>1</v></c></row>", "<c r=\"D3\" t=\"b\"><v>1</v></c><c r=\"F3\"><v>12</v></c></row>"),
            ],
        ),
        (
            "shared-formula",
            vec![
                (sheet, "<c r=\"D2\" t=\"b\"><v>0</v></c></row>", "<c r=\"D2\" t=\"b\"><v>0</v></c><c r=\"E2\"><f t=\"shared\" ref=\"E2:E3\" si=\"0\">B2*2</f><v>6</v></c></row>"),
                (sheet, "<c r=\"D3\" t=\"b\"><v>1</v></c></row>", "<c r=\"D3\" t=\"b\"><v>1</v></c><c r=\"E3\"><f t=\"shared\" si=\"0\"/><v>12</v></c></row>"),
            ],
        ),
    ];
    for (label, edits) in fixtures {
        let scratch = Scratch::new("package-fixture");
        common::rewrite(&corpus("values.xlsx"), &scratch.path("source.xlsx"), &edits);
        let done = written(
            &scratch,
            &scratch.path("source.xlsx"),
            &[("B5", Value::Number(77.0))],
        );
        assert_eq!(done, Ok(()), "{label}");
        let differ = package_differences(
            &scratch.path("source.xlsx"),
            &scratch.path("book.xlsx"),
            &["B5"],
        );
        assert_eq!(differ, Vec::<String>::new(), "{label}");
    }
}

#[test]
fn a_fill_a_new_row_and_a_cell_outside_the_spans_differ_only_where_cp32_allows() {
    let n = Value::Number;
    let t = |s: &str| Value::Text(s.to_string());
    let cases: [(&str, Vec<(&str, Value)>); 4] = [
        (
            "formulas.xlsx",
            vec![("B2", t("2026-10-01")), ("C2", t("Water")), ("D2", n(41.2))],
        ),
        ("chart.xlsx", vec![("A8", t("Jul")), ("B8", n(180.0))]),
        ("values.xlsx", vec![("F2", t(" padded ")), ("G2", n(-0.5))]),
        ("merged.xlsx", vec![("A9", n(1.0)), ("B9", n(2.0))]),
    ];
    for (file, cells) in cases {
        let scratch = Scratch::new("package-forms");
        assert_eq!(written(&scratch, &corpus(file), &cells), Ok(()), "{file}");
        let names: Vec<&str> = cells.iter().map(|(r, _)| *r).collect();
        let differ = package_differences(&corpus(file), &scratch.path("book.xlsx"), &names);
        assert_eq!(differ, Vec::<String>::new(), "{file}");
    }
}

#[test]
fn each_carried_workbook_differs_from_its_source_only_where_cp32_allows() {
    let run = RunId::parse(RUN).expect("a run id");
    let mut wrong = Vec::new();
    for change in CARRIED {
        let scratch = Scratch::new("package-carried");
        fs::copy(corpus(change.file), scratch.path(change.file)).expect("copy");
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
        let differ = package_differences(
            &corpus(change.file),
            &scratch.path(change.file),
            &[change.cell],
        );
        if !differ.is_empty() {
            wrong.push(format!("{}: {differ:?}", change.file));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn an_inline_text_of_every_kind_reads_back_through_calamine_as_written() {
    let texts = [
        " padded ",
        "a & b < c > d \" e ' f",
        "line\rbreak",
        "_x0041_",
        "",
        "tab\there",
        "\u{e9} \u{1f642}",
    ];
    let cells = ["F2", "G2", "H2", "I2", "J2", "K2", "L2"];
    let scratch = Scratch::new("package-inline");
    let values: Vec<(&str, Value)> = cells
        .iter()
        .zip(texts)
        .map(|(c, t)| (*c, Value::Text(t.to_string())))
        .collect();
    assert_eq!(written(&scratch, &corpus("values.xlsx"), &values), Ok(()));
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let read = folder
        .read(
            Path::new("book.xlsx"),
            "Values",
            Range::parse("F2:L2").expect("a range"),
        )
        .expect("the book reads");
    for ((cell, text), got) in cells.iter().zip(texts).zip(&read[0]) {
        let same = match got {
            halvo_sheet::Cell::Text(read) => read == text,
            halvo_sheet::Cell::Empty => text.is_empty(),
            _ => false,
        };
        assert!(same, "{cell}: wrote {text:?}, read {got:?}");
    }
    let differ = package_differences(&corpus("values.xlsx"), &scratch.path("book.xlsx"), &cells);
    assert_eq!(differ, Vec::<String>::new());
}

#[test]
fn the_calculation_flag_is_set_when_it_was_absent_or_zero_or_its_element_was_missing() {
    let book = "xl/workbook.xml";
    let full = r#"<calcPr calcId="124519" fullCalcOnLoad="1"/>"#;
    let cases: [(&str, &str, &str); 3] = [
        (
            "absent",
            r#"<calcPr calcId="124519"/>"#,
            r#"<calcPr fullCalcOnLoad="1" calcId="124519"/>"#,
        ),
        (
            "zero",
            r#"<calcPr calcId="124519" fullCalcOnLoad="0"/>"#,
            r#"<calcPr calcId="124519" fullCalcOnLoad="1"/>"#,
        ),
        ("no-element", "", r#"<calcPr fullCalcOnLoad="1"/>"#),
    ];
    for (label, made, want) in cases {
        let scratch = Scratch::new("package-calc");
        common::rewrite(
            &corpus("values.xlsx"),
            &scratch.path("source.xlsx"),
            &[(book, full, made)],
        );
        let done = written(
            &scratch,
            &scratch.path("source.xlsx"),
            &[("B5", Value::Number(77.0))],
        );
        assert_eq!(done, Ok(()), "{label}");
        let after = entries(&scratch.path("book.xlsx"))
            .into_iter()
            .find(|(n, _)| n == book)
            .expect("a workbook part")
            .1;
        let after = String::from_utf8(after).expect("utf-8");
        assert!(
            after.contains(&format!("{want}</workbook>")),
            "{label}: {after}"
        );
        let differ = package_differences(
            &scratch.path("source.xlsx"),
            &scratch.path("book.xlsx"),
            &["B5"],
        );
        assert_eq!(differ, Vec::<String>::new(), "{label}");
    }
}

#[test]
fn a_changed_part_keeps_the_method_and_the_time_of_its_entry_and_the_rest_keep_their_data() {
    use std::io::Write;
    let book = "xl/workbook.xml";
    let when = zip::DateTime::from_date_and_time(2020, 2, 29, 13, 37, 58).expect("a time");
    let scratch = Scratch::new("package-method");
    let mut archive =
        zip::ZipArchive::new(fs::File::open(corpus("values.xlsx")).expect("open")).expect("a zip");
    let mut out =
        zip::ZipWriter::new(fs::File::create(scratch.path("source.xlsx")).expect("create"));
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("entry");
        let name = entry.name().expect("name").to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read");
        if name == book {
            let text = String::from_utf8(bytes).expect("utf-8");
            bytes = text.replacen(r#" fullCalcOnLoad="1""#, "", 1).into_bytes();
        }
        let method = if name == SHEET {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        };
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(method)
            .last_modified_time(when);
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(&bytes).expect("write");
    }
    out.finish().expect("finish");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    let b5 = Range::parse("B5").expect("a range");
    let prepared = folder
        .prepare(
            Path::new("source.xlsx"),
            &run,
            "Values",
            b5,
            &[vec![Value::Number(77.0)]],
        )
        .expect("prepared");
    let open =
        |path: &Path| zip::ZipArchive::new(fs::File::open(path).expect("open")).expect("a zip");
    let (mut source, mut copy) = (open(&scratch.path("source.xlsx")), open(prepared.copy()));
    for (name, method) in [
        (SHEET, zip::CompressionMethod::Stored),
        (book, zip::CompressionMethod::Deflated),
    ] {
        let entry = copy.by_name(name).expect("the part");
        assert_eq!(entry.compression(), method, "{name} keeps its method");
        assert_eq!(entry.last_modified(), Some(when), "{name} keeps its time");
    }
    for i in 0..source.len() {
        let raw = |archive: &mut zip::ZipArchive<fs::File>| {
            let mut entry = archive.by_index_raw(i).expect("entry");
            let name = entry.name().expect("name").to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read");
            (name, bytes)
        };
        let (name, old) = raw(&mut source);
        let (other, new) = raw(&mut copy);
        assert_eq!(name, other);
        if name != SHEET && name != book {
            assert!(old == new, "{name} keeps its compressed data");
        }
    }
}
