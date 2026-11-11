mod common;

use common::Scratch;
use halvo_sheet::{Cause, Error, Folder, SourceRefusal, Value};

fn read(scratch: &Scratch, bytes: &[u8]) -> Result<Vec<Vec<Value>>, Error> {
    scratch.write("source.csv", bytes);
    let folder = Folder::new(&scratch.dir).expect("the folder opens");
    folder.read_source(std::path::Path::new("source.csv"))
}

fn n(x: f64) -> Value {
    Value::Number(x)
}

fn t(s: &str) -> Value {
    Value::Text(s.to_string())
}

#[test]
fn a_source_cell_is_a_number_only_in_the_form_of_a11_3() {
    let scratch = Scratch::new("source-cells");
    let cases: [(&str, Value); 17] = [
        ("12", n(12.0)),
        ("-3.5", n(-3.5)),
        ("41.20", n(41.2)),
        ("1.", n(1.0)),
        (".5", n(0.5)),
        ("-.5", n(-0.5)),
        ("007", n(7.0)),
        ("-", t("-")),
        (".", t(".")),
        ("-.", t("-.")),
        ("1.2.3", t("1.2.3")),
        ("+5", t("+5")),
        (" 5", t(" 5")),
        ("1e5", t("1e5")),
        ("--1", t("--1")),
        ("2026-10-01", t("2026-10-01")),
        ("", t("")),
    ];
    let mut wrong = Vec::new();
    for (cell, want) in cases {
        let got = read(&scratch, format!("{cell}\n").as_bytes());
        if got != Ok(vec![vec![want.clone()]]) {
            wrong.push(format!("{cell:?}: got {got:?}, want {want:?}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn the_seed_rows_read_as_five_rows_of_three_cells() {
    let scratch = Scratch::new("source-seed");
    let text = "2026-10-01,Water meter reading,41.20\n\
                2026-10-08,Power bill,63.05\n\
                2026-10-15,Groceries,87.40\n\
                2026-10-22,Bus pass,30.00\n\
                2026-10-29,Phone plan,19.99\n";
    let rows = read(&scratch, text.as_bytes()).expect("the seed rows read");
    assert_eq!(
        rows,
        vec![
            vec![t("2026-10-01"), t("Water meter reading"), n(41.2)],
            vec![t("2026-10-08"), t("Power bill"), n(63.05)],
            vec![t("2026-10-15"), t("Groceries"), n(87.4)],
            vec![t("2026-10-22"), t("Bus pass"), n(30.0)],
            vec![t("2026-10-29"), t("Phone plan"), n(19.99)],
        ]
    );
}

#[test]
fn a_quote_is_text_and_an_empty_file_has_no_row() {
    let scratch = Scratch::new("source-quote");
    assert_eq!(
        read(&scratch, b"\"a\",b\"\n"),
        Ok(vec![vec![t("\"a\""), t("b\"")]])
    );
    assert_eq!(read(&scratch, b""), Ok(Vec::new()));
    assert_eq!(read(&scratch, b"\n"), Ok(vec![vec![t("")]]));
    assert_eq!(
        read(&scratch, b"1,x\n2,y"),
        Ok(vec![vec![n(1.0), t("x")], vec![n(2.0), t("y")]])
    );
}

#[test]
fn a_source_out_of_the_form_is_refused_and_names_no_cell() {
    let scratch = Scratch::new("source-refused");
    let long = format!("1,{}\n", "9".repeat(400));
    let cases: [(&[u8], SourceRefusal); 6] = [
        (b"secret,1\r\n", SourceRefusal::CarriageReturn),
        (b"\xef\xbb\xbfsecret,1\n", SourceRefusal::ByteOrderMark),
        (b"secret,\xff\n", SourceRefusal::Encoding),
        (b"secret,1\nsecret\n", SourceRefusal::Ragged { line: 2 }),
        (b"a,b\nsecret,1,2\n", SourceRefusal::Ragged { line: 2 }),
        (long.as_bytes(), SourceRefusal::Number { line: 1 }),
    ];
    let mut wrong = Vec::new();
    for (bytes, want) in cases {
        match read(&scratch, bytes) {
            Err(error) if error == Error::Source(want) => {
                let shown = format!("{error} {error:?}");
                if shown.contains("secret") || shown.contains("999") {
                    wrong.push(format!("{want:?}: the error shows a cell: {shown}"));
                }
                if error.cause() != Cause::StepFailed {
                    wrong.push(format!("{want:?}: cause {:?}", error.cause()));
                }
            }
            other => wrong.push(format!("{want:?}: got {other:?}")),
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}
