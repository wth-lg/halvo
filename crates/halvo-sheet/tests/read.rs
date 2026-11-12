mod common;

use std::fs;
use std::path::Path;

use common::{corpus, Scratch};
use halvo_sheet::{Cause, Cell, Error, Folder, Range, Refusal};

fn n(x: f64) -> Cell {
    Cell::Number(x)
}

fn t(s: &str) -> Cell {
    Cell::Text(s.to_string())
}

fn range(text: &str) -> Range {
    Range::parse(text).expect("a range")
}

fn copy(scratch: &Scratch, file: &str) -> Folder {
    fs::copy(corpus(file), scratch.path(file)).expect("copy the corpus workbook");
    Folder::new(&scratch.dir, common::locks()).expect("the folder opens")
}

#[test]
fn read_returns_text_numbers_booleans_and_empty_cells_of_a_range() {
    let scratch = Scratch::new("read-values");
    let folder = copy(&scratch, "values.xlsx");
    let got = folder.read(Path::new("values.xlsx"), "Values", range("A1:D4"));
    assert_eq!(
        got,
        Ok(vec![
            vec![t("Item"), t("Count"), t("Price"), t("Paid")],
            vec![t("item-1"), n(3.0), n(1.25), Cell::Bool(false)],
            vec![t("item-2"), n(6.0), n(2.5), Cell::Bool(true)],
            vec![t("item-3"), n(9.0), n(3.75), Cell::Empty],
        ])
    );
    assert_eq!(
        folder.read(Path::new("values.xlsx"), "Values", range("E1:F2")),
        Ok(vec![vec![Cell::Empty, Cell::Empty]; 2])
    );
}

#[test]
fn read_gives_a_formula_cell_its_cached_result_and_a_date_its_serial() {
    let scratch = Scratch::new("read-formulas");
    let folder = copy(&scratch, "formulas.xlsx");
    let got = folder
        .read(Path::new("formulas.xlsx"), "Ledger", range("A1:D7"))
        .expect("the ledger reads");
    assert_eq!(got[0], vec![t("Id"), t("Date"), t("Payee"), t("Amount")]);
    assert_eq!(
        got[1],
        vec![t("INV-1"), Cell::Empty, Cell::Empty, Cell::Empty]
    );
    assert_eq!(got[6], vec![Cell::Empty, Cell::Empty, Cell::Empty, n(0.0)]);
    let folder = copy(&scratch, "formats.xlsx");
    assert_eq!(
        folder.read(Path::new("formats.xlsx"), "Formats", range("b2:c2")),
        Ok(vec![vec![n(1000.25), n(46023.0)]])
    );
}

#[test]
fn read_changes_nothing_and_leaves_nothing_beside_the_workbook() {
    let scratch = Scratch::new("read-nothing");
    let folder = copy(&scratch, "chart.xlsx");
    let before = fs::read(scratch.path("chart.xlsx")).expect("read the copy");
    folder
        .read(Path::new("chart.xlsx"), "Sales", range("A1:B9"))
        .expect("the sheet reads");
    assert_eq!(fs::read(scratch.path("chart.xlsx")).expect("read"), before);
    assert_eq!(scratch.names(""), vec!["chart.xlsx".to_string()]);
}

#[test]
fn a_sheet_that_the_workbook_lacks_is_refused() {
    let scratch = Scratch::new("read-no-sheet");
    let folder = copy(&scratch, "values.xlsx");
    for sheet in ["values", "Missing", ""] {
        let got = folder.read(Path::new("values.xlsx"), sheet, range("A1"));
        assert_eq!(got, Err(Error::NoSheet), "{sheet:?}");
        assert_eq!(Error::NoSheet.cause(), Cause::StepFailed);
    }
}

#[test]
fn a_file_that_is_no_workbook_gets_the_cause_workbook_and_shows_nothing_of_it() {
    let scratch = Scratch::new("read-no-workbook");
    scratch.write("plain.xlsx", b"secret,1\n");
    scratch.write(
        "cut.xlsx",
        &fs::read(corpus("values.xlsx")).expect("read")[..900],
    );
    let folder = Folder::new(&scratch.dir, common::locks()).expect("the folder opens");
    for file in ["plain.xlsx", "cut.xlsx"] {
        let got = folder.read(Path::new(file), "Values", range("A1"));
        assert_eq!(got, Err(Error::Workbook(Refusal::Unreadable)), "{file}");
        let error = got.unwrap_err();
        assert_eq!(error.cause(), Cause::Workbook);
        assert_eq!(error.cause().name(), "workbook");
        assert!(!format!("{error} {error:?}").contains("secret"));
    }
}

#[test]
fn a_range_is_a1_form_with_its_first_cell_first() {
    let cases = [
        ("B2:D6", Some((5, 3))),
        ("b2:d6", Some((5, 3))),
        ("B2", Some((1, 1))),
        ("A1:XFD1048576", Some((1_048_576, 16_384))),
        ("B2:B2", Some((1, 1))),
        ("", None),
        ("B", None),
        ("2", None),
        ("B0", None),
        ("B02", None),
        ("D6:B2", None),
        ("B6:D2", None),
        ("D2:B6", None),
        ("XFE1", None),
        ("AAAA1", None),
        ("A1048577", None),
        ("B2:", None),
        (":B2", None),
        ("B2:D6:E7", None),
        ("$B$2", None),
        (" B2", None),
        ("B2 ", None),
        ("B-2", None),
    ];
    let mut wrong = Vec::new();
    for (text, want) in cases {
        let got = Range::parse(text).map(|r| (r.rows(), r.cols()));
        let want = want.ok_or(Error::Range);
        if got != want {
            wrong.push(format!("{text:?}: got {got:?}, want {want:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
