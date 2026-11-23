use crate::error::{Error, SourceRefusal};

/// A value that the runner writes into a cell: a number or a text, as [`crate::Folder::read_source`]
/// reads a source cell.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A finite number.
    Number(f64),
    /// A text, which may be empty.
    Text(String),
}

pub(crate) fn parse(bytes: &[u8]) -> Result<Vec<Vec<Value>>, Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Source(SourceRefusal::Encoding))?;
    if text.starts_with('\u{feff}') {
        return Err(Error::Source(SourceRefusal::ByteOrderMark));
    }
    if text.contains('\r') {
        return Err(Error::Source(SourceRefusal::CarriageReturn));
    }
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    let mut rows: Vec<Vec<Value>> = Vec::new();
    for (at, line) in body.split('\n').enumerate() {
        let line_number = at + 1;
        let row: Vec<Value> = line.split(',').map(value).collect();
        if rows.first().is_some_and(|first| first.len() != row.len()) {
            return Err(Error::Source(SourceRefusal::Ragged { line: line_number }));
        }
        rows.push(row);
    }
    Ok(rows)
}

fn value(cell: &str) -> Value {
    match cell.parse::<f64>() {
        Ok(x) if is_number(cell) && x.is_finite() => Value::Number(x),
        _ => Value::Text(cell.to_string()),
    }
}

/// A11.3's form of a number: digits with an optional leading `-` and at most one `.`; no zero
/// before another digit of the whole part; at most 15 significant digits, which an `f64` holds
/// as typed.
fn is_number(cell: &str) -> bool {
    let body = cell.strip_prefix('-').unwrap_or(cell);
    let digits = body.bytes().filter(u8::is_ascii_digit).count();
    let dots = body.bytes().filter(|b| *b == b'.').count();
    if digits == 0 || dots > 1 || digits + dots != body.len() {
        return false;
    }
    let whole = body.split('.').next().unwrap_or_default();
    if whole.len() > 1 && whole.starts_with('0') {
        return false;
    }
    let significant = body
        .bytes()
        .filter(u8::is_ascii_digit)
        .skip_while(|b| *b == b'0')
        .count();
    significant <= 15
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_of_a_number_holds_a_digit_and_at_most_one_point() {
        for text in ["12", "-3.5", ".5", "1.", "-.5", "0", "41.20"] {
            assert!(is_number(text), "{text}");
        }
        for text in [
            "", ".", "-", "-.", "--1", "1.2.3", "..5", "1..", "-1.2.3", "+5", "1e5",
        ] {
            assert!(!is_number(text), "{text}");
        }
    }

    fn n(x: f64) -> Value {
        Value::Number(x)
    }

    fn t(s: &str) -> Value {
        Value::Text(s.to_string())
    }

    fn shown(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).escape_debug().to_string()
    }

    #[test]
    fn a_file_as_excel_on_windows_saves_csv_utf8_reads_as_its_plain_form() {
        let excel: &[u8] = b"\xef\xbb\xbf2026-10-01,Water meter reading,41.20\r\n\
                             2026-10-08,\"Power, gas and water\",63.05\r\n\
                             2026-10-15,Groceries,87.40\r\n";
        let plain: &[u8] = b"2026-10-01,Water meter reading,41.20\n\
                             2026-10-08,\"Power, gas and water\",63.05\n\
                             2026-10-15,Groceries,87.40\n";
        let want = vec![
            vec![t("2026-10-01"), t("Water meter reading"), n(41.2)],
            vec![t("2026-10-08"), t("Power, gas and water"), n(63.05)],
            vec![t("2026-10-15"), t("Groceries"), n(87.4)],
        ];
        let mut wrong = Vec::new();
        for (name, bytes) in [
            ("as Excel saves CSV UTF-8", excel),
            ("its plain form", plain),
        ] {
            let got = parse(bytes);
            if got != Ok(want.clone()) {
                wrong.push(format!("{name}: got {got:?}"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn a_file_as_notepad_saves_it_reads_as_its_plain_form() {
        let want = vec![
            vec![t("2026-10-01"), t("Water meter reading"), n(41.2)],
            vec![t("2026-10-15"), t("Groceries"), n(87.4)],
        ];
        let cases: [(&[u8], Vec<Vec<Value>>); 8] = [
            (
                b"2026-10-01,Water meter reading,41.20\n2026-10-15,Groceries,87.40\n",
                want.clone(),
            ),
            (
                b"2026-10-01,Water meter reading,41.20\r\n2026-10-15,Groceries,87.40\r\n",
                want.clone(),
            ),
            (
                b"2026-10-01,Water meter reading,41.20\r\n2026-10-15,Groceries,87.40",
                want.clone(),
            ),
            (
                b"\xef\xbb\xbf2026-10-01,Water meter reading,41.20\r\n2026-10-15,Groceries,87.40\r\n",
                want.clone(),
            ),
            (
                b"\xef\xbb\xbf2026-10-01,Water meter reading,41.20\r\n2026-10-15,Groceries,87.40",
                want.clone(),
            ),
            (b"\xef\xbb\xbf", Vec::new()),
            (b"\r\n", vec![vec![t("")]]),
            (b"\xef\xbb\xbf\xef\xbb\xbf1\r\n", vec![vec![t("\u{feff}1")]]),
        ];
        let mut wrong = Vec::new();
        for (bytes, want) in cases {
            let got = parse(bytes);
            if got != Ok(want) {
                wrong.push(format!("{}: got {got:?}", shown(bytes)));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn a_quoted_field_holds_commas_and_a_doubled_quote_as_rfc_4180_has_it() {
        assert_eq!(
            parse(b"\"a, b\",\"say \"\"hi\"\"\",\"\",\",\",\"12\"\" pipe\",\"\"\"\"\n"),
            Ok(vec![vec![
                t("a, b"),
                t("say \"hi\""),
                t(""),
                t(","),
                t("12\" pipe"),
                t("\""),
            ]])
        );
    }

    #[test]
    fn a_quoted_field_is_typed_as_its_text_would_be_unquoted() {
        assert_eq!(
            parse(b"\"123\",123,\"1,234\",\"-3.5\",\"007\"\n"),
            Ok(vec![vec![
                n(123.0),
                n(123.0),
                t("1,234"),
                n(-3.5),
                t("007")
            ]])
        );
    }

    #[test]
    fn each_form_that_stays_refused_is_refused_and_names_no_cell() {
        use SourceRefusal::{CarriageReturn, Encoding, Quote, QuotedLineBreak, Ragged};
        let cases: [(&[u8], SourceRefusal); 15] = [
            (b"secret,1\nsecret\n", Ragged { line: 2 }),
            (b"secret,1\r\n\"secret\"\r\n", Ragged { line: 2 }),
            (b"\"secret\nx\",1\n", QuotedLineBreak { line: 1 }),
            (b"a,1\r\n\"secret\r\nx\",1\r\n", QuotedLineBreak { line: 2 }),
            (b"a,1\n\"secret,1\n", QuotedLineBreak { line: 2 }),
            (b"secret,1\rsecret,2\r", CarriageReturn),
            (b"\"secret\rx\",1\n", CarriageReturn),
            (b"secret,1\r", CarriageReturn),
            (b"secret,\xff\n", Encoding),
            (b"secret,Caf\xe9\r\n", Encoding),
            (b"\xff\xfes\x00e\x00c\x00r\x00e\x00t\x00", Encoding),
            (b"\"secret\"x,1\n", Quote { line: 1 }),
            (b"\"secret\" ,1\n", Quote { line: 1 }),
            (b"\"secret\";\"1\"\n", Quote { line: 1 }),
            (b"a,1\n\"secret,1", Quote { line: 2 }),
        ];
        let mut wrong = Vec::new();
        for (bytes, want) in cases {
            match parse(bytes) {
                Err(error) if error == Error::Source(want) => {
                    let text = format!("{error} {error:?}");
                    if text.contains("secret") || text.contains("pipe") {
                        wrong.push(format!("{}: the error shows a cell: {text}", shown(bytes)));
                    }
                    if error.cause() != crate::Cause::StepFailed {
                        wrong.push(format!("{}: cause {:?}", shown(bytes), error.cause()));
                    }
                }
                other => wrong.push(format!("{}: got {other:?}, want {want:?}", shown(bytes))),
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// Edge (1) of the brief, which the coordinator has not ruled yet: a double quote inside a
    /// field that does not begin with one, a space before the quote among them (edge (8)).
    #[test]
    fn a_quote_inside_a_field_that_does_not_begin_with_one_is_refused() {
        let cases: [(&[u8], usize); 3] = [
            (b"secret,12\" pipe\n", 1),
            (b" \"secret\",1\n", 1),
            (b"a,1\r\nsecret\",1\r\n", 2),
        ];
        let mut wrong = Vec::new();
        for (bytes, line) in cases {
            let got = parse(bytes);
            if got != Err(Error::Source(SourceRefusal::Quote { line })) {
                wrong.push(format!("{}: got {got:?}", shown(bytes)));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }
}
