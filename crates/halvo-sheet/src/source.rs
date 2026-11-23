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
/// (its section 2) with one separator, the comma, the semicolon or the tab, after one byte-order
/// mark at the first byte. No line break stands inside a field, so a row is a line and a refusal
/// names the line. A file that is not UTF-8 is refused first; then the file's separator is told
/// and the file is read by it (`reading`), where a file whose separator is in doubt is refused, a
/// fault of the form stops the read where it stands, and rows of unequal length are refused. Each
/// field's value is told last, by the file's numbers (`Numbers::of`).
pub(crate) fn parse(bytes: &[u8]) -> Result<Vec<Vec<Value>>, Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Source(SourceRefusal::Encoding))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let (separator, rows) = reading(text)?;
    let numbers = Numbers::of(separator, &rows);
    Ok(rows
        .iter()
        .map(|row| row.iter().map(|cell| numbers.value(cell)).collect())
        .collect())
}

/// The fields of a file's rows, as `read` gives them.
type Rows<'a> = Vec<Vec<Cow<'a, str>>>;

/// The separator of a file and its rows by it (r3 U11.5, r3 U11.6). A sign holds where every
/// line that is not empty holds one or more of it outside its quoted fields and as many as every
/// other such line (`holds`); an empty line counts for nothing. A file in which the semicolon and
/// the tab both hold is refused, as its separator is in doubt; a file in which neither holds is
/// read by commas, as before r3 U11.5; a file in which the semicolon or the tab holds and the
/// comma does not is read by that sign. Where the semicolon (or the tab) and the comma both hold,
/// the file has two readings and is told by what it holds, never by a fixed order alone:
/// (1) by the semicolon where every comma outside its quoted fields stands between two digits
/// (`in_numbers`), told by the signs before any read, so that a fault of that read refuses the
/// file and the comma is not tried; (2) else each is read whole, a reading that meets a fault of
/// the form or rows of unequal length is none, one reading that is left is taken, and of two the
/// one in which more fields of the whole file are numbers (`numbers`); where both fault, the
/// semicolon's fault is named; (3) where the numbers are as many, the semicolon.
fn reading(text: &str) -> Result<(u8, Rows<'_>), Error> {
    let sign = match (holds(text, b';'), holds(text, b'\t')) {
        (true, true) => return Err(Error::Source(SourceRefusal::TwoSeparators)),
        (true, false) => b';',
        (false, true) => b'\t',
        (false, false) => return Ok((b',', read(text, b',')?)),
    };
    if !holds(text, b',') || in_numbers(text, sign) {
        return Ok((sign, read(text, sign)?));
    }
    match (read(text, sign), read(text, b',')) {
        (Ok(theirs), Ok(commas)) if numbers(b',', &commas) > numbers(sign, &theirs) => {
            Ok((b',', commas))
        }
        (Ok(theirs), _) => Ok((sign, theirs)),
        (Err(_), Ok(commas)) => Ok((b',', commas)),
        (Err(error), Err(_)) => Err(error),
    }
}

/// The lines of `text` that are not empty, each without its line end.
fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .filter(|line| !line.is_empty())
}

/// Whether `sign` holds in `text`: every line that is not empty holds one or more of it outside
/// its quoted fields (`outside_quotes`), and as many as every other such line.
fn holds(text: &str, sign: u8) -> bool {
    let mut counts = lines(text).map(|line| {
        outside_quotes(line, sign)
            .filter(|&at| line.as_bytes()[at] == sign)
            .count()
    });
    counts
        .next()
        .is_some_and(|first| first > 0 && counts.all(|count| count == first))
}

/// Whether every comma of `text` outside its quoted fields, each line read as fields separated by
/// `sign`, stands between two digits, as in a number with a decimal comma or a thousands comma
/// (`41,20`, `1,234`, `1.234,56`), and not in a text (`Müller, Hans`) nor at a field's edge
/// (`,5`). Step (1) of `reading`.
fn in_numbers(text: &str, sign: u8) -> bool {
    lines(text).all(|line| {
        let bytes = line.as_bytes();
        let digit = |at: usize| bytes.get(at).is_some_and(u8::is_ascii_digit);
        outside_quotes(line, sign)
            .filter(|&at| bytes[at] == b',')
            .all(|at| at > 0 && digit(at - 1) && digit(at + 1))
    })
}

/// The places of the bytes of `line` that stand outside its quoted fields, the line read as
/// fields separated by `sign`: a double quote at the line's start or right behind `sign` opens a
/// quoted field, and the next double quote that no second one follows closes it. The quotes
/// themselves are not among the places.
fn outside_quotes(line: &str, sign: u8) -> impl Iterator<Item = usize> + '_ {
    let mut start = true;
    let mut quoted = false;
    let mut bytes = line.bytes().enumerate().peekable();
    std::iter::from_fn(move || {
        while let Some((at, byte)) = bytes.next() {
            if quoted {
                if byte == b'"' && bytes.next_if(|&(_, next)| next == b'"').is_none() {
                    quoted = false;
                }
            } else if byte == sign {
                start = true;
                return Some(at);
            } else {
                quoted = start && byte == b'"';
                start = false;
                if !quoted {
                    return Some(at);
                }
            }
        }
        None
    })
}

/// The rows of `text` read by `separator` (`row`): a fault of the form stops the read where it
/// stands; then rows of unequal length are refused, the first row of another length named.
fn read(text: &str, separator: u8) -> Result<Rows<'_>, Error> {
    let mut rows = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let (row, next) = row(text, at, rows.len() + 1, separator)?;
        rows.push(row);
        at = next;
    }
    let ragged = rows
        .first()
        .and_then(|first| rows.iter().position(|row| row.len() != first.len()));
    match ragged {
        Some(at) => Err(Error::Source(SourceRefusal::Ragged { line: at + 1 })),
        None => Ok(rows),
    }
}

/// How many fields of the whole file `rows`, read by `separator`, are numbers, each told by the
/// file's numbers for that separator (`Numbers::of`). Step (2) of `reading`.
fn numbers(separator: u8, rows: &Rows<'_>) -> usize {
    let numbers = Numbers::of(separator, rows);
    rows.iter()
        .flatten()
        .filter(|cell| numbers.number(cell).is_some())
        .count()
}

/// The fields of the line `line` that starts at the byte `at` of `text`, separated by
/// `separator`, and the byte behind its line end (LF or CRLF) or the end of the text.
fn row(
    text: &str,
    mut at: usize,
    line: usize,
    separator: u8,
) -> Result<(Vec<Cow<'_, str>>, usize), Error> {
    let bytes = text.as_bytes();
    let mut row = Vec::new();
    loop {
        let (cell, end) = field(text, at, line, separator)?;
        row.push(cell);
        match bytes.get(end) {
            Some(&b) if b == separator => at = end + 1,
            Some(b'\n') => return Ok((row, end + 1)),
            Some(b'\r') if bytes.get(end + 1) == Some(&b'\n') => return Ok((row, end + 2)),
            Some(b'\r') => return Err(Error::Source(SourceRefusal::CarriageReturn)),
            None => return Ok((row, end)),
            // Text behind the quote that closed the field.
            Some(_) => return Err(Error::Source(SourceRefusal::Quote { line })),
        }
    }
}

/// The text of the field that starts at the byte `at` of `text`, and the byte behind it: for an
/// unquoted field the separator, carriage return or line feed that ends it or the end of the
/// text, for a quoted one the byte behind its closing quote. A double quote inside a field that
/// does not begin with one is text. A quoted field holds the separator, and two double quotes in
/// it are one. The two signs that are not the file's separator are text in any field.
fn field(
    text: &str,
    at: usize,
    line: usize,
    separator: u8,
) -> Result<(Cow<'_, str>, usize), Error> {
    let bytes = text.as_bytes();
    if bytes.get(at) != Some(&b'"') {
        let end = bytes[at..]
            .iter()
            .position(|&b| b == separator || b == b'\n' || b == b'\r')
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

/// How the fields of one file are told into numbers and texts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Numbers {
    /// A file read by commas: A11.3's form (`is_number`), with the point as the decimal sign.
    Points,
    /// A file read by semicolons or by tabs whose fields hold numbers of this one decimal sign,
    /// the comma or the point, and none of the other (r3 U11.5, `with_sign`).
    Sign(char),
    /// A file read by semicolons or by tabs in which no field, or a field of each sign, is a
    /// number with a decimal sign: its whole numbers alone are numbers.
    Whole,
}

impl Numbers {
    /// The numbers of a file read by `separator` with the fields `rows`: a file read by
    /// semicolons or by tabs has one decimal sign, the comma where a field is a number with a
    /// decimal comma and none is one with a decimal point, the point the other way round. A
    /// field of the shape of a thousands group, of either sign, has two readings and decides
    /// nothing.
    fn of(separator: u8, rows: &[Vec<Cow<'_, str>>]) -> Numbers {
        if separator == b',' {
            return Numbers::Points;
        }
        let has = |sign: char| {
            rows.iter()
                .flatten()
                .any(|cell| with_sign(cell, sign).is_some())
        };
        match (has(','), has('.')) {
            (true, false) => Numbers::Sign(','),
            (false, true) => Numbers::Sign('.'),
            _ => Numbers::Whole,
        }
    }

    /// The number of `cell` in such a file, or none where it is a text.
    fn number(self, cell: &str) -> Option<f64> {
        match self {
            Numbers::Points => point_number(cell),
            Numbers::Sign(sign) => whole_number(cell).or_else(|| with_sign(cell, sign)),
            Numbers::Whole => whole_number(cell),
        }
    }

    fn value(self, cell: &str) -> Value {
        match self.number(cell) {
            Some(x) => Value::Number(x),
            None => Value::Text(cell.to_string()),
        }
    }
}

/// The number of a field of A11.3's form (`is_number`).
fn point_number(cell: &str) -> Option<f64> {
    match cell.parse::<f64>() {
        Ok(x) if is_number(cell) && x.is_finite() => Some(x),
        _ => None,
    }
}

/// The number of a field of A11.3's form with no decimal sign.
fn whole_number(cell: &str) -> Option<f64> {
    point_number(cell).filter(|_| !cell.contains('.'))
}

/// The number of a field of a file read by semicolons or by tabs with `sign` as its decimal sign,
/// where the field has that one reading (r3 U11.5): A11.3's form with `sign` in place of the
/// point, which it holds once with a digit on each side; not the shape of a thousands group, a
/// whole part of one to three digits other than a lone `0`, the sign, and three digits (`1,234`
/// and `1.234` are 1234 where the sign groups thousands), whereas `0,125` and `1234,567` have one
/// reading alone.
fn with_sign(cell: &str, sign: char) -> Option<f64> {
    let body = cell.strip_prefix('-').unwrap_or(cell);
    let (whole, fraction) = body.split_once(sign)?;
    let group = whole != "0" && (1..=3).contains(&whole.len()) && fraction.len() == 3;
    if whole.is_empty() || fraction.is_empty() || group {
        return None;
    }
    point_number(&cell.replacen(sign, ".", 1))
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

    /// Each refusal names no cell, and its cause is `data_encoding` for a file that is not UTF-8
    /// and `data_form` for every other (r3 U11.5). A file in which the semicolon and the tab both
    /// hold is refused, and a file read by semicolons meets the faults of the form as one read by
    /// commas does.
    #[test]
    fn each_form_that_stays_refused_is_refused_and_names_no_cell() {
        use SourceRefusal::{
            CarriageReturn, Encoding, Quote, QuotedLineBreak, Ragged, TwoSeparators,
        };
        let cases: [(&[u8], SourceRefusal); 17] = [
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
            (b"\"secret\"x;1\n", Quote { line: 1 }),
            (b"a,1\n\"secret,1", Quote { line: 2 }),
            (b"secret;gas\r\nPower;light\r\n\r\n", Ragged { line: 3 }),
            (b"secret;1\tx\r\nPower;2\ty\r\n", TwoSeparators),
        ];
        let mut wrong = Vec::new();
        for (bytes, want) in cases {
            match parse(bytes) {
                Err(error) if error == Error::Source(want) => {
                    let text = format!("{error} {error:?}");
                    if text.contains("secret") || text.contains("pipe") {
                        wrong.push(format!("{}: the error shows a cell: {text}", shown(bytes)));
                    }
                    let cause = match want {
                        Encoding => crate::Cause::DataEncoding,
                        _ => crate::Cause::DataForm,
                    };
                    if error.source_cause() != cause {
                        wrong.push(format!(
                            "{}: cause {:?}",
                            shown(bytes),
                            error.source_cause()
                        ));
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

    /// r3 U11.5: the files that the reader refused before as read by another separator read by
    /// that separator, their decimal commas as numbers. The fifth file of the semicolons, whose
    /// last line is empty, is refused as rows of unequal length
    /// (`each_form_that_stays_refused_is_refused_and_names_no_cell`).
    #[test]
    fn the_files_refused_before_as_read_by_another_separator_read_by_it() {
        let water = vec![
            vec![t("Water"), n(41.2), n(3.5)],
            vec![t("Power"), n(63.05), n(7.25)],
        ];
        let cases: [(&[u8], Vec<Vec<Value>>); 6] = [
            (b"Water;41,20;3,50\r\nPower;63,05;7,25\r\n", water.clone()),
            (
                b"\xef\xbb\xbfWater;41,20;3,50\r\nPower;63,05;7,25\r\n",
                water.clone(),
            ),
            (
                b"Water;41,20;3\r\nPower;63;7\r\n",
                vec![
                    vec![t("Water"), n(41.2), n(3.0)],
                    vec![t("Power"), n(63.0), n(7.0)],
                ],
            ),
            (
                b"1;Water;41,20\r\n2;\"Power; gas\";63,05\r\n",
                vec![
                    vec![n(1.0), t("Water"), n(41.2)],
                    vec![n(2.0), t("Power; gas"), n(63.05)],
                ],
            ),
            (
                b"Water\t41,20\t3,50\r\nPower\t63,05\t7,25\r\n",
                water.clone(),
            ),
            (
                b"\xef\xbb\xbfWater\t41,20\t3,50\r\nPower\t63,05\t7,25\r\n",
                water,
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

    /// r3 U11.5 (a): one table as a program saves it with commas, with semicolons and with tabs
    /// (where the comma is the decimal sign, and where the point is, as in Switzerland), each with
    /// LF and with CRLF, with and without the byte-order mark, reads to the same rows. A field
    /// that holds the file's own separator is quoted, as Excel quotes it.
    #[test]
    fn one_table_saved_with_commas_semicolons_or_tabs_reads_to_the_same_rows() {
        let want = vec![
            vec![t("2026-10-01"), t("Water meter reading"), n(41.2)],
            vec![t("2026-10-08"), t("Power, gas; water"), n(63.05)],
            vec![t("2026-10-15"), t("Groceries"), n(-87.4)],
        ];
        let saved: [(&str, [&str; 3]); 5] = [
            (
                "commas",
                [
                    "2026-10-01,Water meter reading,41.20",
                    "2026-10-08,\"Power, gas; water\",63.05",
                    "2026-10-15,Groceries,-87.40",
                ],
            ),
            (
                "semicolons",
                [
                    "2026-10-01;Water meter reading;41,20",
                    "2026-10-08;\"Power, gas; water\";63,05",
                    "2026-10-15;Groceries;-87,40",
                ],
            ),
            (
                "tabs",
                [
                    "2026-10-01\tWater meter reading\t41,20",
                    "2026-10-08\tPower, gas; water\t63,05",
                    "2026-10-15\tGroceries\t-87,40",
                ],
            ),
            (
                "semicolons with points",
                [
                    "2026-10-01;Water meter reading;41.20",
                    "2026-10-08;\"Power, gas; water\";63.05",
                    "2026-10-15;Groceries;-87.40",
                ],
            ),
            (
                "tabs with points",
                [
                    "2026-10-01\tWater meter reading\t41.20",
                    "2026-10-08\tPower, gas; water\t63.05",
                    "2026-10-15\tGroceries\t-87.40",
                ],
            ),
        ];
        let mut wrong = Vec::new();
        for (name, lines) in saved {
            for end in ["\n", "\r\n"] {
                for mark in ["", "\u{feff}"] {
                    let text = format!("{mark}{}{end}", lines.join(end));
                    let got = parse(text.as_bytes());
                    if got != Ok(want.clone()) {
                        wrong.push(format!("{name}: {}: got {got:?}", text.escape_debug()));
                    }
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// r3 U11.5 (b), the coordinator's table: in a file read by semicolons or by tabs a field is a
    /// number by the file's one decimal sign, the comma or the point. A file of one field shows
    /// each form alone: a form with one reading is its number, a form with two (the shape of a
    /// thousands group, of either sign) and every other form stay text. In a file with a number
    /// of each sign, both stay text; a thousands shape decides nothing, and a whole number is a
    /// number in any file.
    #[test]
    fn a_decimal_comma_in_a_file_of_semicolons_or_tabs_is_its_number_and_in_doubt_a_text() {
        let alone: [(&str, Value); 29] = [
            ("3,5", n(3.5)),
            ("41,20", n(41.2)),
            ("-0,5", n(-0.5)),
            ("5,", t("5,")),
            ("1.234,56", t("1.234,56")),
            ("1,234", t("1,234")),
            ("1 234,56", t("1 234,56")),
            ("12,3,4", t("12,3,4")),
            ("3,5 kg", t("3,5 kg")),
            ("1e5", t("1e5")),
            ("3.5", n(3.5)),
            ("1.5", n(1.5)),
            ("41.20", n(41.2)),
            ("1.234", t("1.234")),
            ("0.125", n(0.125)),
            ("0,125", n(0.125)),
            ("-12,345", t("-12,345")),
            ("1234,567", n(1234.567)),
            ("1234.567", n(1234.567)),
            ("1,2345", n(1.2345)),
            ("1,234.56", t("1,234.56")),
            (".5", t(".5")),
            ("5.", t("5.")),
            ("00,5", t("00,5")),
            ("1234567890,12345", n(1_234_567_890.123_45)),
            ("12345678901,12345", t("12345678901,12345")),
            ("12", n(12.0)),
            ("-7", n(-7.0)),
            ("007", t("007")),
        ];
        let mixed: [(&[&str], Vec<Value>); 6] = [
            (&[",5", "12"], vec![t(",5"), n(12.0)]),
            (&["3,5", "4.5"], vec![t("3,5"), t("4.5")]),
            (&["0,125", "41.20"], vec![t("0,125"), t("41.20")]),
            (&["3,5", "1.234"], vec![n(3.5), t("1.234")]),
            (&["3.5", "1,234"], vec![n(3.5), t("1,234")]),
            (&["12", "4.5", "1,234"], vec![n(12.0), n(4.5), t("1,234")]),
        ];
        let mut wrong = Vec::new();
        for sign in [";", "\t"] {
            let file = |cells: &[&str]| -> String {
                cells.iter().map(|c| format!("a{sign}{c}\r\n")).collect()
            };
            for (cell, want) in &alone {
                let text = file(&[cell]);
                let got = parse(text.as_bytes());
                if got != Ok(vec![vec![t("a"), want.clone()]]) {
                    wrong.push(format!("{}: got {got:?}", text.escape_debug()));
                }
            }
            for (cells, want) in &mixed {
                let text = file(cells);
                let want: Vec<Vec<Value>> = want.iter().map(|v| vec![t("a"), v.clone()]).collect();
                let got = parse(text.as_bytes());
                if got != Ok(want) {
                    wrong.push(format!("{}: got {got:?}", text.escape_debug()));
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// r3 U11.6, the coordinator's ruling (its log line 19549): a file that holds by the
    /// semicolon, or by the tab, and by the comma (each sign as many times outside quoted fields
    /// in every line that is not empty) has two readings, and it is read by what it holds, never
    /// by a fixed order alone (`reading`). Each row of the builder's table is a file here, named
    /// by what it is and by the step that tells it, with the rows that it reads to or its
    /// refusal; R14 and R15 hold by one sign alone and read as before.
    #[test]
    fn a_file_that_holds_by_the_comma_and_by_the_semicolon_or_the_tab_reads_by_what_it_holds() {
        fn file(lines: &[&str], end: &str) -> String {
            lines.iter().map(|line| format!("{line}{end}")).collect()
        }
        fn five(row: Vec<Value>) -> Vec<Vec<Value>> {
            vec![row; 5]
        }
        let water = vec![
            vec![t("Wasser"), n(41.2), n(3.5)],
            vec![t("Strom"), n(63.05), n(7.25)],
        ];
        type Read = Result<Vec<Vec<Value>>, Error>;
        let cases: Vec<(&str, String, Read)> = vec![
            (
                "R1, step (2), a comma file with one semicolon a row in a text",
                file(&["Item 1; note,1.50,1"; 5], "\n"),
                Ok(five(vec![t("Item 1; note"), n(1.5), n(1.0)])),
            ),
            (
                "R2, step (2), a comma file with two semicolons a row in a text",
                file(&["Item 1; note; more,1.50,1"; 5], "\n"),
                Ok(five(vec![t("Item 1; note; more"), n(1.5), n(1.0)])),
            ),
            (
                "R3, step (1), a semicolon file with decimal commas",
                file(&["Item 1 note;1,50;1"; 5], "\r\n"),
                Ok(five(vec![t("Item 1 note"), n(1.5), n(1.0)])),
            ),
            (
                "R4, step (1), a semicolon file with a decimal comma in every number",
                file(&["Wasser;41,20;3,50", "Strom;63,05;7,25"], "\r\n"),
                Ok(water.clone()),
            ),
            (
                "R5, step (2), a semicolon file with a comma in a text and decimal commas",
                file(&["Müller, Hans;41,20;7", "Schmidt, Eva;12,00;3"], "\r\n"),
                Ok(vec![
                    vec![t("Müller, Hans"), n(41.2), n(7.0)],
                    vec![t("Schmidt, Eva"), n(12.0), n(3.0)],
                ]),
            ),
            (
                "R6, step (3), a semicolon file of texts with a comma in every row",
                file(&["Müller, Hans;Berlin", "Schmidt, Eva;Köln"], "\r\n"),
                Ok(vec![
                    vec![t("Müller, Hans"), t("Berlin")],
                    vec![t("Schmidt, Eva"), t("Köln")],
                ]),
            ),
            (
                "R7, step (3), its named rest: a comma file of texts with a semicolon in every row",
                file(&["Berlin; Mitte,Hans", "Köln; Ehrenfeld,Eva"], "\n"),
                Ok(vec![
                    vec![t("Berlin"), t(" Mitte,Hans")],
                    vec![t("Köln"), t(" Ehrenfeld,Eva")],
                ]),
            ),
            (
                "R8, step (1), a semicolon file with a comma in a thousands shape",
                file(&["Miete;1,234", "Strom;2,500"], "\r\n"),
                Ok(vec![
                    vec![t("Miete"), t("1,234")],
                    vec![t("Strom"), t("2,500")],
                ]),
            ),
            (
                "R9, step (2), a semicolon file with a comma in a text and decimal points",
                file(&["Wasser, kalt;41.20", "Strom, Tag;63.05"], "\r\n"),
                Ok(vec![
                    vec![t("Wasser, kalt"), n(41.2)],
                    vec![t("Strom, Tag"), n(63.05)],
                ]),
            ),
            (
                "R10, step (1), a semicolon file with a comma in a quoted text",
                file(
                    &["\"Müller, Hans\";41,20", "\"Schmidt, Eva\";12,00"],
                    "\r\n",
                ),
                Ok(vec![
                    vec![t("Müller, Hans"), n(41.2)],
                    vec![t("Schmidt, Eva"), n(12.0)],
                ]),
            ),
            (
                "R11, one reading, a comma file with a semicolon in a quoted text",
                file(&["\"a;b\",1.50", "\"c;d\",2.50"], "\n"),
                Ok(vec![vec![t("a;b"), n(1.5)], vec![t("c;d"), n(2.5)]]),
            ),
            (
                "R12, step (2), a comma file with two tabs a row in a text",
                file(&["Item 1\tnote\tmore,1.50,1"; 5], "\n"),
                Ok(five(vec![t("Item 1\tnote\tmore"), n(1.5), n(1.0)])),
            ),
            (
                "R13, step (1), a tab file with a decimal comma in every number",
                file(&["Wasser\t41,20\t3,50", "Strom\t63,05\t7,25"], "\r\n"),
                Ok(water),
            ),
            (
                "R14, one reading, one column of decimal commas: two columns of whole numbers",
                file(&["3,5", "41,20"], "\r\n"),
                Ok(vec![vec![n(3.0), n(5.0)], vec![n(41.0), n(20.0)]]),
            ),
            (
                "R15, one reading, a semicolon file with rows of different lengths",
                file(&["Wasser;41,20;3", "Strom;12,00"], "\r\n"),
                Ok(vec![
                    vec![t("Wasser;41"), t("20;3")],
                    vec![t("Strom;12"), t("00")],
                ]),
            ),
            (
                "R16, step (1), its named rest: a comma file whose commas stand between digits",
                file(&["Room 1; Floor 2,450", "Room 2; Floor 3,380"], "\n"),
                Ok(vec![
                    vec![t("Room 1"), t(" Floor 2,450")],
                    vec![t("Room 2"), t(" Floor 3,380")],
                ]),
            ),
            (
                "R17, step (2), a semicolon file with a decimal comma at a field's edge",
                file(&["Wasser;,5"], "\r\n"),
                Ok(vec![vec![t("Wasser;"), n(5.0)]]),
            ),
            (
                "R18, step (2), a semicolon file with one decimal comma at a field's edge",
                file(&["Wasser;0,5", "Strom;,75"], "\r\n"),
                Ok(vec![
                    vec![t("Wasser;0"), n(5.0)],
                    vec![t("Strom;"), n(75.0)],
                ]),
            ),
            (
                "R19, step (1), a semicolon file with text behind a closing quote: refused",
                file(&["Wasser;\"a\"b;41,20", "Strom;\"c\"d;63,05"], "\r\n"),
                Err(Error::Source(SourceRefusal::Quote { line: 1 })),
            ),
            (
                "R20, step (2), its named clause: a comma in a text and a fault of the quotes",
                file(
                    &["Müller, Hans;\"Haus\"A;7", "Schmidt, Eva;\"Haus\"B;3"],
                    "\r\n",
                ),
                Ok(vec![
                    vec![t("Müller"), t(" Hans;\"Haus\"A;7")],
                    vec![t("Schmidt"), t(" Eva;\"Haus\"B;3")],
                ]),
            ),
        ];
        let mut wrong = Vec::new();
        for (name, text, want) in cases {
            let got = parse(text.as_bytes());
            if got != want {
                wrong.push(format!("{name}: {}: got {got:?}", text.escape_debug()));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }
}
