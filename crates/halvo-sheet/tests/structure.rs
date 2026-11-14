mod common;

use std::fs;
use std::path::Path;

use common::{corpus, repack, replaced, Scratch, RUN};
use halvo_sheet::{Cause, Cell, Error, Folder, Range, Refusal, RunId, Value};

const SHEET: &str = "xl/worksheets/sheet1.xml";
const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;

type Each = Box<dyn Fn(&str, Vec<u8>) -> Option<(String, Vec<u8>)>>;

fn in_sheet(from: &'static str, to: String) -> Each {
    Box::new(move |name, bytes| {
        let bytes = if name == SHEET {
            replaced(bytes, from, &to)
        } else {
            bytes
        };
        Some((name.to_string(), bytes))
    })
}

fn sheet_is(xml: String) -> Each {
    Box::new(move |name, bytes| {
        let bytes = if name == SHEET {
            xml.clone().into_bytes()
        } else {
            bytes
        };
        Some((name.to_string(), bytes))
    })
}

fn made(scratch: &Scratch, each: &Each) {
    repack(
        &corpus("values.xlsx"),
        &scratch.path("book.xlsx"),
        each.as_ref(),
        &[],
    );
}

fn prepare(scratch: &Scratch, cells: &str) -> Result<halvo_sheet::Prepared, Error> {
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    let range = Range::parse(cells).expect("a range");
    let values = vec![vec![Value::Number(1.0); range.cols() as usize]; range.rows() as usize];
    folder.prepare(Path::new("book.xlsx"), &run, "Values", range, &values)
}

fn refused(scratch: &Scratch, label: &str, cells: &str, want: Refusal) {
    let before = fs::read(scratch.path("book.xlsx")).expect("read");
    let got = prepare(scratch, cells).err();
    assert_eq!(got, Some(Error::Workbook(want)), "{label}");
    assert_eq!(got.map(|e| e.cause()), Some(Cause::Workbook), "{label}");
    assert_eq!(scratch.names(""), vec!["book.xlsx".to_string()], "{label}");
    assert_eq!(
        fs::read(scratch.path("book.xlsx")).expect("read"),
        before,
        "{label}"
    );
}

#[test]
fn a_sheet_part_out_of_the_writers_form_is_refused_before_any_write_and_never_panics() {
    let cases: [(&str, Each); 6] = [
        (
            "no sheet data (m20)",
            Box::new(|name: &str, bytes: Vec<u8>| {
                let bytes = if name == SHEET {
                    let text = String::from_utf8(bytes).expect("utf-8");
                    text.replace("<sheetData>", "<sheetDatx>")
                        .replace("</sheetData>", "</sheetDatx>")
                        .into_bytes()
                } else {
                    bytes
                };
                Some((name.to_string(), bytes))
            }),
        ),
        (
            "a row numbered 0 (m09)",
            in_sheet(r#"<row r="1" "#, r#"<row r="0" "#.to_string()),
        ),
        (
            "a row number twice (m10, M21)",
            in_sheet(r#"<row r="3" "#, r#"<row r="2" "#.to_string()),
        ),
        (
            "a cell twice in a row (m12, M22)",
            in_sheet(r#"<c r="C2">"#, r#"<c r="B2">"#.to_string()),
        ),
        (
            "a dimension with dollar signs",
            in_sheet(
                r#"<dimension ref="A1:D21"/>"#,
                r#"<dimension ref="$A$1:$D$21"/>"#.to_string(),
            ),
        ),
        (
            "a dimension with no ref",
            in_sheet(r#"<dimension ref="A1:D21"/>"#, "<dimension/>".to_string()),
        ),
    ];
    for (label, each) in cases {
        let scratch = Scratch::new("structure-form");
        made(&scratch, &each);
        refused(&scratch, label, "B5", Refusal::Part);
    }
}

fn wide(first: &str, last: &str) -> String {
    let row = |cell: &str| {
        let digits = cell.trim_start_matches(|c: char| c.is_ascii_alphabetic());
        format!(r#"<row r="{digits}"><c r="{cell}"><v>1</v></c></row>"#)
    };
    format!(
        r#"<worksheet {NS}><sheetData>{}{}</sheetData></worksheet>"#,
        row(first),
        row(last)
    )
}

/// The runner of one-rule changes skips this test: with a rule of the bound taken out, the
/// reader would build a grid of 17 billion cells.
#[test]
fn the_first_and_the_last_cell_of_a_sheet_are_refused_by_prepare_and_by_read() {
    let corners = wide("A1", "XFD1048576");
    assert!(
        corners.len() < 400,
        "a few hundred bytes: {}",
        corners.len()
    );
    let scratch = Scratch::new("structure-corners");
    made(&scratch, &sheet_is(corners));
    refused(
        &scratch,
        "the first and the last cell of a sheet",
        "B5",
        Refusal::Size,
    );
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let b5 = Range::parse("B5").expect("a range");
    let read = folder.read(Path::new("book.xlsx"), "Values", b5);
    assert_eq!(
        read,
        Err(Error::Workbook(Refusal::Size)),
        "read refuses it too"
    );
}

#[test]
fn a_sheet_whose_used_cells_and_the_range_span_more_than_a_million_cells_is_refused() {
    let scratch = Scratch::new("structure-over");
    made(&scratch, &sheet_is(wide("A1", "ALL1001")));
    refused(&scratch, "used cells one row over", "B5", Refusal::Size);
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let a1 = Range::parse("A1").expect("a range");
    let read = folder.read(Path::new("book.xlsx"), "Values", a1);
    assert_eq!(
        read,
        Err(Error::Workbook(Refusal::Size)),
        "read refuses it too"
    );
    let scratch = Scratch::new("structure-union");
    made(&scratch, &sheet_is(wide("A1", "B2")));
    refused(
        &scratch,
        "a range far from the used cells",
        "ALL1001",
        Refusal::Size,
    );
    let scratch = Scratch::new("structure-million");
    made(&scratch, &sheet_is(wide("A1", "ALL1000")));
    let prepared = prepare(&scratch, "B5").expect("a rectangle of 1,000,000 cells is taken");
    drop(prepared);
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let read = folder.read(
        Path::new("book.xlsx"),
        "Values",
        Range::parse("A1").expect("a range"),
    );
    assert_eq!(read, Ok(vec![vec![Cell::Number(1.0)]]));
}

#[test]
fn the_read_back_and_the_earlier_read_take_the_named_sheet_alone() {
    let broken = |name: &str, bytes: Vec<u8>| {
        let bytes = if name == "xl/worksheets/sheet2.xml" {
            replaced(bytes, r#"<c r="B2""#, r#"<c r="B""#)
        } else {
            bytes
        };
        Some((name.to_string(), bytes))
    };
    let scratch = Scratch::new("structure-other-sheet");
    repack(
        &corpus("two-sheets.xlsx"),
        &scratch.path("book.xlsx"),
        &broken,
        &[],
    );
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    let b3 = Range::parse("B3").expect("a range");
    let values = vec![vec![Value::Number(25.0)]];
    let prepared = folder
        .prepare(Path::new("book.xlsx"), &run, "Input", b3, &values)
        .expect("a sheet that the reader cannot read is not read");
    folder.replace(&prepared).expect("replaced");
    let read = folder.read(Path::new("book.xlsx"), "Input", b3);
    assert_eq!(read, Ok(vec![vec![Cell::Number(25.0)]]));
    let summary = folder.prepare(
        Path::new("book.xlsx"),
        &RunId::parse("fedcba9876543210fedcba9876543210").expect("a run id"),
        "Summary",
        b3,
        &values,
    );
    assert_eq!(summary.err(), Some(Error::Workbook(Refusal::Part)));
}

#[test]
fn a_named_sheet_that_the_reader_cannot_read_is_refused_before_any_write() {
    let cases: [(&str, Each); 2] = [
        (
            "an unknown cell type",
            in_sheet(
                r#"<c r="C2"><v>1.25</v></c>"#,
                r#"<c r="C2" t="zz"><v>1.25</v></c>"#.to_string(),
            ),
        ),
        (
            "a shared string past the table",
            in_sheet(
                r#"<c r="C2"><v>1.25</v></c>"#,
                r#"<c r="C2" t="s"><v>99999</v></c>"#.to_string(),
            ),
        ),
    ];
    for (label, each) in cases {
        let scratch = Scratch::new("structure-unreadable");
        made(&scratch, &each);
        refused(&scratch, label, "B5", Refusal::Unreadable);
    }
}

#[test]
fn an_entity_in_the_reference_of_the_dimension_refuses_the_workbook_as_unreadable() {
    let scratch = Scratch::new("structure-dimension-entity");
    let entity = in_sheet(
        r#"<dimension ref="A1:D21"/>"#,
        r#"<dimension ref="A1:D2&#49;"/>"#.to_string(),
    );
    made(&scratch, &entity);
    refused(
        &scratch,
        "an entity in the dimension's ref",
        "B5",
        Refusal::Unreadable,
    );
}

#[test]
fn each_spelling_of_the_changed_places_is_written_and_checked_as_the_plain_one() {
    let plain_row = r#"<row r="2" spans="1:4">"#;
    let plain_dim = r#"<dimension ref="A1:D21"/>"#;
    let cases: [(&str, &str, &str); 6] = [
        (
            "a tab",
            "<row\tr=\"2\"\tspans=\"1:4\">",
            "<dimension\tref=\"A1:D21\"/>",
        ),
        (
            "a newline",
            "<row\nr=\"2\"\nspans=\"1:4\">",
            "<dimension\nref=\"A1:D21\"/>",
        ),
        (
            "spaces around the equals sign",
            "<row r = \"2\" spans = \"1:4\">",
            "<dimension ref = \"A1:D21\"/>",
        ),
        (
            "single quotes",
            "<row r='2' spans='1:4'>",
            "<dimension ref='A1:D21'/>",
        ),
        ("an entity", "<row r=\"2\" spans=\"1:&#52;\">", plain_dim),
        ("the plain spelling", plain_row, plain_dim),
    ];
    for (label, row, dimension) in cases {
        let scratch = Scratch::new("structure-spelling");
        let each: Each = Box::new(move |name, bytes| {
            let bytes = if name == SHEET {
                let bytes = replaced(bytes, plain_row, row);
                replaced(bytes, plain_dim, dimension)
            } else {
                bytes
            };
            Some((name.to_string(), bytes))
        });
        made(&scratch, &each);
        let source = common::entry(&scratch.path("book.xlsx"), SHEET);
        let prepared = prepare(&scratch, "F2").unwrap_or_else(|e| panic!("{label}: {e:?}"));
        let written = String::from_utf8(common::entry(prepared.copy(), SHEET)).expect("utf-8");
        let grow = |tag: &str, from: &str, to: &str| tag.replacen(from, to, 1);
        let want = String::from_utf8(source)
            .expect("utf-8")
            .replacen(row, &grow(&row.replace("1:&#52;", "1:4"), "1:4", "1:6"), 1)
            .replacen(dimension, &grow(dimension, "A1:D21", "A1:F21"), 1)
            .replacen(
                r#"<c r="D2" t="b"><v>0</v></c></row>"#,
                r#"<c r="D2" t="b"><v>0</v></c><c r="F2"><v>1</v></c></row>"#,
                1,
            );
        assert_eq!(written, want, "{label}");
    }
}

#[test]
fn a_prefixed_part_and_a_row_of_another_namespace_outside_the_sheet_data_are_carried() {
    let prefixed = |name: &str, bytes: Vec<u8>| {
        let bytes = if name == SHEET {
            let text = String::from_utf8(bytes).expect("utf-8");
            let body_at = text.find("<worksheet").expect("the root");
            let body = text[body_at..]
                .replace("</", "\u{1}")
                .replace('<', "<x:")
                .replace('\u{1}', "</x:")
                .replacen(r#"xmlns="#, r#"xmlns:x="#, 1);
            format!("{}{body}", &text[..body_at]).into_bytes()
        } else {
            bytes
        };
        Some((name.to_string(), bytes))
    };
    let outside = in_sheet(
        "</sheetData>",
        r#"</sheetData><o:row xmlns:o="urn:other" r="5"><o:c r="B5"><o:v>3</o:v></o:c></o:row>"#
            .to_string(),
    );
    for (label, each) in [
        ("prefixed", Box::new(prefixed) as Each),
        ("outside", outside),
    ] {
        let scratch = Scratch::new("structure-namespace");
        made(&scratch, &each);
        let source =
            String::from_utf8(common::entry(&scratch.path("book.xlsx"), SHEET)).expect("utf-8");
        let prepared = prepare(&scratch, "B5").unwrap_or_else(|e| panic!("{label}: {e:?}"));
        let written = String::from_utf8(common::entry(prepared.copy(), SHEET)).expect("utf-8");
        let (old, new) = if label == "prefixed" {
            (
                r#"<x:c r="B5"><x:v>12</x:v></x:c>"#,
                r#"<x:c r="B5"><x:v>1</x:v></x:c>"#,
            )
        } else {
            (r#"<c r="B5"><v>12</v></c>"#, r#"<c r="B5"><v>1</v></c>"#)
        };
        assert_eq!(written, source.replacen(old, new, 1), "{label}");
        let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
        folder.replace(&prepared).expect("replaced");
        let read = folder.read(
            Path::new("book.xlsx"),
            "Values",
            Range::parse("B5").expect("a range"),
        );
        assert_eq!(read, Ok(vec![vec![Cell::Number(1.0)]]), "{label}");
    }
}
