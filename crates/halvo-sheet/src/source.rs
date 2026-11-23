use std::borrow::Cow;

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

/// The rows of a source file, in the form that [`crate::Folder::read_source`] states: RFC 4180
/// (its section 2) with the comma as the one separator, after one byte-order mark at the first
/// byte. No line break stands inside a field, so a row is a line and a refusal names the line.
/// A fault of the form stops the read where it stands. Behind the last row, a file that reads as
/// a table by another separator is refused before rows of unequal length are.
pub(crate) fn parse(bytes: &[u8]) -> Result<Vec<Vec<Value>>, Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Source(SourceRefusal::Encoding))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows: Vec<Vec<Value>> = Vec::new();
    let mut signs: Vec<Signs> = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let (row, row_signs, next) = row(text, at, rows.len() + 1)?;
        rows.push(row);
        signs.push(row_signs);
        at = next;
    }
    if another_separator(&signs) {
        return Err(Error::Source(SourceRefusal::OtherSeparator));
    }
    let ragged = rows
        .first()
        .and_then(|first| rows.iter().position(|row| row.len() != first.len()));
    if let Some(at) = ragged {
        return Err(Error::Source(SourceRefusal::Ragged { line: at + 1 }));
    }
    Ok(rows)
}

/// The semicolons and the tabs that a row holds outside its quoted fields.
#[derive(Clone, Copy, Default)]
struct Signs {
    semicolons: usize,
    tabs: usize,
}

/// Whether the rows read as a table by another separator than the comma: the semicolons outside
/// quoted fields, or the tabs outside them, each sign counted on its own, are one or more and as
/// many in every row.
fn another_separator(signs: &[Signs]) -> bool {
    let same = |count: fn(&Signs) -> usize| {
        signs.first().is_some_and(|first| {
            count(first) > 0 && signs.iter().all(|row| count(row) == count(first))
        })
    };
    same(|row| row.semicolons) || same(|row| row.tabs)
}

/// The row of the line `line` that starts at the byte `at` of `text`, the semicolons and tabs
/// that it holds outside its quoted fields, and the byte behind its line end (LF or CRLF) or
/// the end of the text.
fn row(text: &str, mut at: usize, line: usize) -> Result<(Vec<Value>, Signs, usize), Error> {
    let bytes = text.as_bytes();
    let mut row = Vec::new();
    let mut signs = Signs::default();
    loop {
        let quoted = bytes.get(at) == Some(&b'"');
        let (cell, end) = field(text, at, line)?;
        if !quoted {
            signs.semicolons += cell.matches(';').count();
            signs.tabs += cell.matches('\t').count();
        }
        row.push(value(&cell));
        match bytes.get(end) {
            Some(b',') => at = end + 1,
            Some(b'\n') => return Ok((row, signs, end + 1)),
            Some(b'\r') if bytes.get(end + 1) == Some(&b'\n') => return Ok((row, signs, end + 2)),
            Some(b'\r') => return Err(Error::Source(SourceRefusal::CarriageReturn)),
            None => return Ok((row, signs, end)),
            // Text behind the quote that closed the field.
            Some(_) => return Err(Error::Source(SourceRefusal::Quote { line })),
        }
    }
}

/// The text of the field that starts at the byte `at` of `text`, and the byte behind it: for an
/// unquoted field the comma, carriage return or line feed that ends it or the end of the text,
/// for a quoted one the byte behind its closing quote. A double quote inside a field that does
/// not begin with one is text. A quoted field holds commas, and two double quotes in it are one.
fn field(text: &str, at: usize, line: usize) -> Result<(Cow<'_, str>, usize), Error> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'"') {
        let end = bytes[at..]
            .iter()
            .position(|b| matches!(b, b',' | b'\n' | b'\r'))
            .map_or(bytes.len(), |n| at + n);
        return Ok((Cow::Borrowed(&text[at..end]), end));
    }
    let mut close = at + 1;
    loop {
        match bytes.get(close) {
            Some(b'"') if bytes.get(close + 1) == Some(&b'"') => close += 2,
            Some(b'"') => break,
            Some(b'\n') => return Err(Error::Source(SourceRefusal::QuotedLineBreak { line })),
            Some(b'\r') if bytes.get(close + 1) == Some(&b'\n') => {
                return Err(Error::Source(SourceRefusal::QuotedLineBreak { line }))
            }
            Some(b'\r') => return Err(Error::Source(SourceRefusal::CarriageReturn)),
            Some(_) => close += 1,
            None => return Err(Error::Source(SourceRefusal::Quote { line })),
        }
    }
    let cell = text[at + 1..close].replace("\"\"", "\"");
    Ok((Cow::Owned(cell), close + 1))
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

    /// A double quote inside a field that does not begin with one is text, as it was before
    /// quoting was read, a space before the quote among them.
    #[test]
    fn a_quote_inside_a_field_that_does_not_begin_with_one_is_text() {
        let cases: [(&[u8], Vec<Vec<Value>>); 3] = [
            (b"a,12\" pipe\n", vec![vec![t("a"), t("12\" pipe")]]),
            (b" \"a\",1\n", vec![vec![t(" \"a\""), n(1.0)]]),
            (
                b"a,1\r\nb\"c\"\",1\r\n",
                vec![vec![t("a"), n(1.0)], vec![t("b\"c\"\""), n(1.0)]],
            ),
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

    /// Each file is refused as `OtherSeparator`, and the error names no cell.
    fn refused_as_another_separator(cases: &[&[u8]]) {
        let mut wrong = Vec::new();
        for bytes in cases {
            match parse(bytes) {
                Err(error) if error == Error::Source(SourceRefusal::OtherSeparator) => {
                    let text = format!("{error} {error:?}");
                    if text.contains("Water") || text.contains("Power") {
                        wrong.push(format!("{}: the error shows a cell: {text}", shown(bytes)));
                    }
                }
                other => wrong.push(format!("{}: got {other:?}", shown(bytes))),
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    #[test]
    fn a_file_with_semicolons_as_excel_saves_it_where_the_comma_is_the_decimal_sign_is_refused() {
        refused_as_another_separator(&[
            b"Water;41,20;3,50\r\nPower;63,05;7,25\r\n",
            b"\xef\xbb\xbfWater;41,20;3,50\r\nPower;63,05;7,25\r\n",
            b"Water;41,20;3\r\nPower;63;7\r\n",
            b"1;Water;41,20\r\n2;\"Power; gas\";63,05\r\n",
            b"Water;gas\r\nPower;light\r\n\r\n",
        ]);
    }

    #[test]
    fn a_file_with_tabs_as_its_separator_is_refused() {
        refused_as_another_separator(&[
            b"Water\t41,20\t3,50\r\nPower\t63,05\t7,25\r\n",
            b"\xef\xbb\xbfWater\t41,20\t3,50\r\nPower\t63,05\t7,25\r\n",
        ]);
    }

    #[test]
    fn a_semicolon_in_some_texts_or_inside_quotes_in_every_row_reads() {
        let cases: [(&[u8], Vec<Vec<Value>>); 4] = [
            (
                b"2026-10-01,Water; gas,41.20\r\n2026-10-08,Power,63.05\r\n",
                vec![
                    vec![t("2026-10-01"), t("Water; gas"), n(41.2)],
                    vec![t("2026-10-08"), t("Power"), n(63.05)],
                ],
            ),
            (
                b"\"Water; gas\",41.20\r\n\"Power; light\",63.05\r\n",
                vec![
                    vec![t("Water; gas"), n(41.2)],
                    vec![t("Power; light"), n(63.05)],
                ],
            ),
            (
                b"\"Water\tgas\",41.20\n\"Power\tlight\",63.05\n",
                vec![
                    vec![t("Water\tgas"), n(41.2)],
                    vec![t("Power\tlight"), n(63.05)],
                ],
            ),
            (
                b"Water; gas,41.20\nPower\tlight,63.05\n",
                vec![
                    vec![t("Water; gas"), n(41.2)],
                    vec![t("Power\tlight"), n(63.05)],
                ],
            ),
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
}
