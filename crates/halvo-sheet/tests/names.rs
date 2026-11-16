mod common;

use std::fs;
use std::path::Path;

use common::{corpus, repack, replaced, Repack, Scratch, RUN};
use halvo_sheet::{Cause, Error, Folder, Kind, Range, Refusal, RunId, Value};

const SHEET_RELS: &str = "xl/worksheets/_rels/sheet1.xml.rels";
const BOOK_RELS: &str = "xl/_rels/workbook.xml.rels";
const TABLE_TARGET: &str = r#"Target="../tables/table1.xml""#;
const TABLE_TYPE: &str = r#"relationships/table" Target"#;
const MS: &str = "http://schemas.microsoft.com/office/2006/relationships";
const OD: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

fn refused(scratch: &Scratch, label: &str, sheet: &str, cell: &str, want: Refusal) {
    let path = scratch.path("book.xlsx");
    let before = fs::read(&path).expect("read");
    let listing = scratch.names("");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    let range = Range::parse(cell).expect("a range");
    let values = [vec![Value::Number(1.0)]];
    let got = folder.prepare(Path::new("book.xlsx"), &run, sheet, range, &values);
    let got = got.err();
    assert_eq!(got, Some(Error::Workbook(want)), "{label} {cell}");
    assert_eq!(got.map(|e| e.cause()), Some(Cause::Workbook), "{label}");
    assert_eq!(
        scratch.names(""),
        listing,
        "{label}: nothing beside the workbook"
    );
    assert_eq!(
        fs::read(&path).expect("read"),
        before,
        "{label}: the workbook is as it was"
    );
}

fn in_part(
    part: &'static str,
    from: &'static str,
    to: &'static str,
) -> impl Fn(&str, Vec<u8>) -> Option<(String, Vec<u8>)> {
    move |name: &str, bytes: Vec<u8>| {
        let bytes = if name == part {
            replaced(bytes, from, to)
        } else {
            bytes
        };
        Some((name.to_string(), bytes))
    }
}

fn renamed(
    from: &'static str,
    to: &'static str,
) -> impl Fn(&str, Vec<u8>) -> Option<(String, Vec<u8>)> {
    move |name: &str, bytes: Vec<u8>| {
        let name = if name == from { to } else { name };
        Some((name.to_string(), bytes))
    }
}

#[test]
fn a_table_named_in_another_case_or_spelling_still_refuses_a_write_into_its_header_row() {
    let target_case = in_part(SHEET_RELS, TABLE_TARGET, r#"Target="../tables/Table1.xml""#);
    let entry_case = renamed("xl/tables/table1.xml", "xl/tables/TABLE1.xml");
    let rels_case = renamed(SHEET_RELS, "xl/worksheets/_rels/Sheet1.xml.rels");
    let encoded = in_part(
        SHEET_RELS,
        TABLE_TARGET,
        r#"Target="../tables/table%31.xml""#,
    );
    let cases: [(&str, Repack<'_>); 4] = [
        ("a target in another case", &target_case),
        ("an entry in another case", &entry_case),
        ("a relationship part in another case", &rels_case),
        ("a percent-encoded target", &encoded),
    ];
    for (label, each) in cases {
        let scratch = Scratch::new("names-table");
        repack(&corpus("chart.xlsx"), &scratch.path("book.xlsx"), each, &[]);
        for cell in ["A1", "B1"] {
            refused(&scratch, label, "Sales", cell, Refusal::TableHeader);
        }
    }
}

#[test]
fn a_type_in_another_case_an_encoded_separator_or_a_target_with_no_entry_refuses_the_workbook() {
    let type_case = in_part(SHEET_RELS, TABLE_TYPE, r#"relationships/Table" Target"#);
    let slash = in_part(
        SHEET_RELS,
        TABLE_TARGET,
        r#"Target="..%2Ftables%2Ftable1.xml""#,
    );
    let backslash = in_part(
        SHEET_RELS,
        TABLE_TARGET,
        r#"Target="..%5ctables%5ctable1.xml""#,
    );
    let broken = in_part(
        SHEET_RELS,
        TABLE_TARGET,
        r#"Target="../tables/table%3.xml""#,
    );
    let missing = in_part(SHEET_RELS, TABLE_TARGET, r#"Target="../tables/table9.xml""#);
    let cases: [(&str, Repack<'_>); 5] = [
        ("a table type in another case", &type_case),
        ("an encoded slash", &slash),
        ("an encoded backslash", &backslash),
        ("a percent sign without two hex digits", &broken),
        ("a table target that names no entry", &missing),
    ];
    for (label, each) in cases {
        let scratch = Scratch::new("names-relationship");
        repack(&corpus("chart.xlsx"), &scratch.path("book.xlsx"), each, &[]);
        refused(&scratch, label, "Sales", "C5", Refusal::Relationship);
    }
}

fn values_with(scratch: &Scratch, relationship: &str, extra: &[(&str, &[u8])]) {
    let rel = relationship.to_string();
    let each = move |name: &str, bytes: Vec<u8>| {
        let bytes = if name == BOOK_RELS {
            replaced(bytes, "</Relationships>", &format!("{rel}</Relationships>"))
        } else {
            bytes
        };
        Some((name.to_string(), bytes))
    };
    repack(
        &corpus("values.xlsx"),
        &scratch.path("book.xlsx"),
        &each,
        extra,
    );
}

#[test]
fn a_read_type_in_another_case_or_a_read_relationship_with_no_entry_refuses_the_workbook() {
    let rel = |kind: String, target: &str| {
        format!(r#"<Relationship Id="rId9" Type="{kind}" Target="{target}"/>"#)
    };
    let cases = [
        (
            "a macro project type in another case",
            rel(format!("{MS}/VBAProject"), "code.bin"),
            "xl/code.bin",
        ),
        (
            "a pivot type in another case",
            rel(format!("{OD}/PivotCacheDefinition"), "other/p.xml"),
            "xl/other/p.xml",
        ),
        (
            "a macro sheet type in another case",
            rel(format!("{MS}/XLMacrosheet"), "other/m.xml"),
            "xl/other/m.xml",
        ),
        (
            "a worksheet type in another case",
            rel(format!("{OD}/Worksheet"), "worksheets/sheet1.xml"),
            "xl/other/unused.xml",
        ),
        (
            "a worksheet that names no entry",
            rel(format!("{OD}/worksheet"), "worksheets/sheet9.xml"),
            "xl/other/unused.xml",
        ),
        (
            "a macro project that names no entry",
            rel(format!("{MS}/vbaProject"), "code.bin"),
            "xl/other/unused.xml",
        ),
        (
            "a pivot table that names no entry",
            rel(format!("{OD}/pivotTable"), "pivotTables/p.xml"),
            "xl/other/unused.xml",
        ),
    ];
    for (label, relationship, part) in cases {
        let scratch = Scratch::new("names-types");
        values_with(&scratch, &relationship, &[(part, b"<x/>")]);
        refused(&scratch, label, "Values", "B5", Refusal::Relationship);
    }
}

#[test]
fn two_entries_whose_names_differ_in_case_alone_are_an_entry_name_twice() {
    for twin in [
        "XL/worksheets/sheet1.xml",
        "xl/Styles.xml",
        "xl\\worksheets\\sheet1.xml",
    ] {
        let scratch = Scratch::new("names-twice");
        let keep = |name: &str, bytes: Vec<u8>| Some((name.to_string(), bytes));
        repack(
            &corpus("values.xlsx"),
            &scratch.path("book.xlsx"),
            &keep,
            &[(twin, b"<worksheet/>")],
        );
        refused(&scratch, twin, "Values", "B5", Refusal::Unreadable);
    }
}

#[test]
fn a_macro_sheet_of_either_kind_is_the_kind_macro() {
    for kind in ["xlMacrosheet", "xlIntlMacrosheet"] {
        let scratch = Scratch::new("names-macro-sheet");
        let relationship = format!(
            r#"<Relationship Id="rId9" Type="{MS}/{kind}" Target="macrosheets/sheet1.xml"/>"#
        );
        let sheet: &[u8] =
            br#"<xm:macrosheet xmlns:xm="http://schemas.microsoft.com/office/excel/2006/main"/>"#;
        values_with(
            &scratch,
            &relationship,
            &[("xl/macrosheets/sheet1.xml", sheet)],
        );
        refused(
            &scratch,
            kind,
            "Values",
            "B5",
            Refusal::Kinds(vec![Kind::Macro]),
        );
    }
}

#[test]
fn the_writer_and_the_reader_find_the_named_sheet_in_one_part_or_the_workbook_is_refused() {
    let sheet = "xl/worksheets/sheet1.xml";
    let encoded_twin = |name: &str, bytes: Vec<u8>| {
        let bytes = if name == BOOK_RELS {
            replaced(
                bytes,
                r#"Target="worksheets/sheet1.xml""#,
                r#"Target="worksheets/sheet%31.xml""#,
            )
        } else {
            bytes
        };
        Some((name.to_string(), bytes))
    };
    let scratch = Scratch::new("names-two-parts");
    let original = common::entry(&corpus("values.xlsx"), sheet);
    repack(
        &corpus("values.xlsx"),
        &scratch.path("book.xlsx"),
        &encoded_twin,
        &[("xl/worksheets/sheet%31.xml", &original)],
    );
    refused(
        &scratch,
        "a sheet target that the reader takes as another entry",
        "Values",
        "B5",
        Refusal::Relationship,
    );
    let other_book = in_part(
        "_rels/.rels",
        r#"Target="xl/workbook.xml""#,
        r#"Target="xl/other.xml""#,
    );
    let scratch = Scratch::new("names-office-document");
    let book = common::entry(&corpus("values.xlsx"), "xl/workbook.xml");
    repack(
        &corpus("values.xlsx"),
        &scratch.path("book.xlsx"),
        &other_book,
        &[("xl/other.xml", &book)],
    );
    refused(
        &scratch,
        "an office document other than xl/workbook.xml",
        "Values",
        "B5",
        Refusal::Relationship,
    );
}

#[test]
fn a_sheet_target_or_an_entry_in_another_case_is_written_where_the_reader_reads_it() {
    let cases: [(&str, Repack<'_>); 2] = [
        (
            "a sheet target in another case",
            &in_part(
                BOOK_RELS,
                r#"Target="worksheets/sheet1.xml""#,
                r#"Target="Worksheets/SHEET1.xml""#,
            ),
        ),
        (
            "a sheet entry in another case",
            &renamed("xl/worksheets/sheet1.xml", "xl/Worksheets/Sheet1.XML"),
        ),
    ];
    for (label, each) in cases {
        let scratch = Scratch::new("names-sheet-case");
        repack(
            &corpus("values.xlsx"),
            &scratch.path("book.xlsx"),
            each,
            &[],
        );
        let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
        let run = RunId::parse(RUN).expect("a run id");
        let range = Range::parse("B5").expect("a range");
        let prepared = folder
            .prepare(
                Path::new("book.xlsx"),
                &run,
                "Values",
                range,
                &[vec![Value::Number(77.0)]],
            )
            .unwrap_or_else(|e| panic!("{label}: {e:?}"));
        folder
            .replace(&prepared)
            .unwrap_or_else(|e| panic!("{label}: {e:?}"));
        let read = folder.read(Path::new("book.xlsx"), "Values", range);
        assert_eq!(
            read,
            Ok(vec![vec![halvo_sheet::Cell::Number(77.0)]]),
            "{label}"
        );
    }
}

#[test]
fn a_sheet_name_is_compared_byte_for_byte() {
    let scratch = Scratch::new("names-sheet");
    fs::copy(corpus("formulas.xlsx"), scratch.path("formulas.xlsx")).expect("copy");
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    let run = RunId::parse(RUN).expect("a run id");
    let range = Range::parse("B2").expect("a range");
    let values = vec![vec![Value::Number(1.0)]];
    for name in ["ledger", "LEDGER", "Ledger ", " Ledger"] {
        let got = folder.prepare(Path::new("formulas.xlsx"), &run, name, range, &values);
        assert_eq!(got.err(), Some(Error::NoSheet), "{name:?}");
        let read = folder.read(Path::new("formulas.xlsx"), name, range);
        assert_eq!(read.err(), Some(Error::NoSheet), "{name:?}");
    }
    assert_eq!(scratch.names(""), vec!["formulas.xlsx".to_string()]);
}

fn external(kind: &str, target: &str) -> String {
    format!(r#"<Relationship Id="rId9" Type="{kind}" Target="{target}" TargetMode="External"/>"#)
}

#[test]
fn a_table_relationship_marked_external_refuses_the_workbook_before_any_write() {
    let marked = in_part(
        SHEET_RELS,
        TABLE_TARGET,
        r#"Target="../tables/table1.xml" TargetMode="External""#,
    );
    let address = format!(
        "{}</Relationships>",
        external(&format!("{OD}/table"), "https://example.com/%2Ftable")
    );
    let far = move |name: &str, bytes: Vec<u8>| {
        let bytes = if name == SHEET_RELS {
            replaced(bytes, "</Relationships>", &address)
        } else {
            bytes
        };
        Some((name.to_string(), bytes))
    };
    let cases: [(&str, Repack<'_>); 2] = [
        ("the sheet's own table, marked External", &marked),
        ("a table at an address outside the package", &far),
    ];
    for (label, each) in cases {
        let scratch = Scratch::new("names-external-table");
        repack(&corpus("chart.xlsx"), &scratch.path("book.xlsx"), each, &[]);
        for cell in ["A1", "B3"] {
            refused(&scratch, label, "Sales", cell, Refusal::Relationship);
        }
    }
}

#[test]
fn a_worksheet_macro_pivot_or_package_relationship_marked_external_refuses_the_workbook() {
    let cases = [
        (
            "a worksheet",
            format!("{OD}/worksheet"),
            "worksheets/sheet1.xml",
        ),
        (
            "a worksheet at an address",
            format!("{OD}/worksheet"),
            "https://example.com/50%off",
        ),
        ("a chart sheet", format!("{OD}/chartsheet"), "other/c.xml"),
        ("a dialog sheet", format!("{OD}/dialogsheet"), "other/d.xml"),
        ("a macro project", format!("{MS}/vbaProject"), "code.bin"),
        ("a macro sheet", format!("{MS}/xlMacrosheet"), "other/m.xml"),
        (
            "an international macro sheet",
            format!("{MS}/xlIntlMacrosheet"),
            "other/m.xml",
        ),
        ("a pivot table", format!("{OD}/pivotTable"), "other/p.xml"),
        (
            "a pivot cache",
            format!("{OD}/pivotCacheDefinition"),
            "other/p.xml",
        ),
        (
            "pivot records",
            format!("{OD}/pivotCacheRecords"),
            "other/p.xml",
        ),
        (
            "a table type in another case",
            format!("{OD}/Table"),
            "tables/t.xml",
        ),
        (
            "an office document",
            format!("{OD}/officeDocument"),
            "workbook.xml",
        ),
    ];
    for (label, kind, target) in cases {
        let scratch = Scratch::new("names-external-part");
        let parts: &[(&str, &[u8])] = &[
            ("xl/code.bin", b"x"),
            ("xl/other/c.xml", b"<x/>"),
            ("xl/other/d.xml", b"<x/>"),
            ("xl/other/m.xml", b"<x/>"),
            ("xl/other/p.xml", b"<x/>"),
            ("xl/tables/t.xml", b"<x/>"),
        ];
        values_with(&scratch, &external(&kind, target), parts);
        refused(&scratch, label, "Values", "B5", Refusal::Relationship);
    }
    let scratch = Scratch::new("names-external-root");
    let root = in_part(
        "_rels/.rels",
        r#"Target="xl/workbook.xml""#,
        r#"Target="xl/workbook.xml" TargetMode="External""#,
    );
    repack(
        &corpus("values.xlsx"),
        &scratch.path("book.xlsx"),
        &root,
        &[],
    );
    let label = "the package's office document, marked External";
    refused(&scratch, label, "Values", "B5", Refusal::Relationship);
}

#[test]
fn a_hyperlink_or_another_type_that_is_no_part_of_the_reader_stays_skipped_when_external() {
    let cases = [
        (format!("{OD}/hyperlink"), "https://example.com/%2Fhome"),
        (format!("{OD}/hyperlink"), "../tables/table1.xml"),
        (format!("{OD}/oleObject"), "file:///C:/Budget%20Q1.xlsx"),
        (format!("{OD}/externalLinkPath"), "other.xlsx"),
        (format!("{OD}/image"), "https://example.com/a.png"),
    ];
    for (kind, target) in cases {
        let scratch = Scratch::new("names-external-skipped");
        let link = format!("{}</Relationships>", external(&kind, target));
        let with_link = move |name: &str, bytes: Vec<u8>| {
            let bytes = if name == SHEET_RELS {
                replaced(bytes, "</Relationships>", &link)
            } else {
                bytes
            };
            Some((name.to_string(), bytes))
        };
        repack(
            &corpus("chart.xlsx"),
            &scratch.path("book.xlsx"),
            &with_link,
            &[],
        );
        let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
        let run = RunId::parse(RUN).expect("a run id");
        let b3 = Range::parse("B3").expect("a range");
        let values = [vec![Value::Number(1.0)]];
        let got = folder.prepare(Path::new("book.xlsx"), &run, "Sales", b3, &values);
        assert!(got.is_ok(), "{kind} {target}: {:?}", got.err());
    }
}

#[test]
fn an_address_outside_the_package_is_no_part_name_and_its_link_is_carried_as_it_was() {
    let addresses = [
        "https://example.com/login?next=%2Fhome",
        "https://example.com/50%off",
        "https://example.com/caf%E9",
        r"file:///\\fileserver\finance\Budget%20Q1%2F2026.xlsx",
    ];
    for address in addresses {
        let scratch = Scratch::new("names-external");
        let link = format!(
            r#"<Relationship Id="rId9" Type="{OD}/hyperlink" Target="{address}" TargetMode="External"/></Relationships>"#
        );
        let with_link = move |name: &str, bytes: Vec<u8>| {
            let bytes = if name == SHEET_RELS {
                replaced(bytes, "</Relationships>", &link)
            } else {
                bytes
            };
            Some((name.to_string(), bytes))
        };
        repack(
            &corpus("chart.xlsx"),
            &scratch.path("book.xlsx"),
            &with_link,
            &[],
        );
        let source_rels = common::entry(&scratch.path("book.xlsx"), SHEET_RELS);
        let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
        let run = RunId::parse(RUN).expect("a run id");
        let b3 = Range::parse("B3").expect("a range");
        let values = [vec![Value::Number(640.0)]];
        let prepared = folder
            .prepare(Path::new("book.xlsx"), &run, "Sales", b3, &values)
            .unwrap_or_else(|e| panic!("{address}: {e:?}"));
        let copy_rels = common::entry(prepared.copy(), SHEET_RELS);
        assert_eq!(copy_rels, source_rels, "{address}: the link's bytes");
        let text = String::from_utf8(copy_rels).expect("utf-8");
        assert!(
            text.contains(&format!(r#"Target="{address}""#)),
            "{address}"
        );
        folder.replace(&prepared).expect("replaced");
        let read = folder.read(Path::new("book.xlsx"), "Sales", b3);
        assert_eq!(
            read,
            Ok(vec![vec![halvo_sheet::Cell::Number(640.0)]]),
            "{address}"
        );
    }
}
