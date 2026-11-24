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
/// read by commas, as before r3 U11.5; a file in which the semicolon or the tab holds is read by
/// that sign, and a fault of that read refuses it, as r3 U11.5 has it. Where the semicolon (or the
/// tab) and the comma both hold, the file is read by commas only where it speaks plainly for the
/// comma: its read by commas stands too, and all three hold: (i) some comma outside its quoted
/// fields does not stand beside a digit with a digit or the edge of its field on its other side
/// (`in_numbers`); (ii) no field of the semicolon's reading, without the spaces at its ends, is a
/// number written with a comma (`comma_number`); (iii) a field of the semicolon's reading that
/// holds a letter and does not end with a space begins with one, and no field of the comma's
/// reading begins with one (`spaced`): a text behind a semicolon is a word, a cell of signs and
/// digits alone is not, and a field with a space at each end is a cell that a program padded. In
/// every other such file the semicolon (or the tab) stands. No count of numbers decides (r3 U11.6).
fn reading(text: &str) -> Result<(u8, Rows<'_>), Error> {
    let sign = match (holds(text, b';'), holds(text, b'\t')) {
        (true, true) => return Err(Error::Source(SourceRefusal::TwoSeparators)),
        (true, false) => b';',
        (false, true) => b'\t',
        (false, false) => return Ok((b',', read(text, b',')?)),
    };
    let theirs = read(text, sign)?;
    let texts = theirs
        .iter()
        .flatten()
        .filter(|cell| cell.chars().any(char::is_alphabetic) && !cell.ends_with(' '));
    let stands = !holds(text, b',')
        || in_numbers(text, sign)
        || theirs.iter().flatten().any(|cell| comma_number(cell))
        || !spaced(texts);
    if stands {
        return Ok((sign, theirs));
    }
    match read(text, b',') {
        Ok(commas) if !spaced(commas.iter().flatten()) => Ok((b',', commas)),
        _ => Ok((sign, theirs)),
    }
}

/// The signs of money and the percent sign that a number may carry at one of its ends
/// (`number_make`).
const NUMBER_SIGNS: [char; 5] = ['€', '$', '£', '¥', '%'];

/// Whether `cell`, without the spaces at its ends and without ONE sign of `NUMBER_SIGNS` at either
/// end with its spaces, has a number's make, whatever the file's numbers make of it
/// (`Numbers::of`): a minus in front at most, then digits, points and commas alone, a digit at
/// each end (`500`, ` 3.250,50 € `, `€ 500`, ` 12 %`, `-1.234,56`, `38,40,42`); letters
/// (`500 EUR`) keep a field a text. Of `comma_number`, condition (ii) of `reading`.
fn number_make(cell: &str) -> bool {
    let cell = cell.trim_matches(' ');
    let cell = NUMBER_SIGNS
        .iter()
        .find_map(|&sign| {
            let front = cell
                .strip_prefix(sign)
                .map(|rest| rest.trim_start_matches(' '));
            front.or_else(|| {
                cell.strip_suffix(sign)
                    .map(|rest| rest.trim_end_matches(' '))
            })
        })
        .unwrap_or(cell);
    let body = cell.strip_prefix('-').unwrap_or(cell);
    let bytes = body.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_digit)
        && bytes.last().is_some_and(u8::is_ascii_digit)
        && bytes
            .iter()
            .all(|&b| b.is_ascii_digit() || b == b',' || b == b'.')
}

/// Whether `cell` is a number written with a comma: of a number's make (`number_make`), with one
/// comma or more (`3.250,50`, ` 3.250,50 € `, `1,250`, `3,250.50`, `38,40,42`). Condition (ii) of
/// `reading`.
fn comma_number(cell: &str) -> bool {
    number_make(cell) && cell.contains(',')
}

/// Whether one of `cells` begins with a space: a sign inside a text is followed by a space, and a
/// sign that a program writes between values is not. Condition (iii) of `reading`.
fn spaced<'a>(mut cells: impl Iterator<Item = &'a Cow<'a, str>>) -> bool {
    cells.any(|cell| cell.starts_with(' '))
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
/// `sign`, stands beside a digit, and on its other side a digit or the edge of its field, as in a
/// number with a decimal comma or a thousands comma (`41,20`, `1,234`, `1.234,56`, `,5`, `5,`),
/// and not in a text (`Müller, Hans`). Where it holds, condition (i) of `reading` fails.
fn in_numbers(text: &str, sign: u8) -> bool {
    lines(text).all(|line| {
        let bytes = line.as_bytes();
        let digit = |at: Option<usize>| {
            at.and_then(|at| bytes.get(at))
                .is_some_and(u8::is_ascii_digit)
        };
        let edge = |at: Option<usize>| at.and_then(|at| bytes.get(at)).is_none_or(|&b| b == sign);
        outside_quotes(line, sign)
            .filter(|&at| bytes[at] == b',')
            .all(|at| {
                let (before, after) = (at.checked_sub(1), Some(at + 1));
                (digit(before) && (digit(after) || edge(after))) || (digit(after) && edge(before))
            })
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

    fn value(self, cell: &str) -> Value {
        let number = match self {
            Numbers::Points => point_number(cell),
            Numbers::Sign(sign) => whole_number(cell).or_else(|| with_sign(cell, sign)),
            Numbers::Whole => whole_number(cell),
        };
        match number {
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
        let alone: [(&str, Value); 30] = [
            ("3,5", n(3.5)),
            ("41,20", n(41.2)),
            ("-0,5", n(-0.5)),
            (",5", t(",5")),
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
        let mixed: [(&[&str], Vec<Value>); 5] = [
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

    /// r3 U11.6, the coordinator's rulings (its log lines 19549, 19557 and 19619): a file that
    /// holds by the semicolon, or by the tab, and by the comma (each sign as many times outside
    /// quoted fields in every line that is not empty) is read by commas only where it speaks
    /// plainly for the comma, by the conditions (i), (ii) and (iii) of `reading`, and by the
    /// semicolon (or the tab) in every other such file, a fault of that read refusing it. Each row
    /// of the builder's tables is a file here, named by what it is and by the condition that tells
    /// it, with the rows that it reads to or its refusal; R11, R14 and R15 hold by one sign alone.
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
        let names = vec![
            vec![t("Müller, Hans"), t("Vertrieb"), t("3.250,50")],
            vec![t("Schmidt, Eva"), t("Einkauf"), t("2.980,00")],
        ];
        type Read = Result<Vec<Vec<Value>>, Error>;
        let cases: Vec<(&str, String, Read)> =
            vec![
            (
                "R1, the comma by (i), (ii), (iii): a comma file, one semicolon a row in a text",
                file(&["Item 1; note,1.50,1"; 5], "\n"),
                Ok(five(vec![t("Item 1; note"), n(1.5), n(1.0)])),
            ),
            (
                "R2, the comma by (i), (ii), (iii): a comma file, two semicolons a row in a text",
                file(&["Item 1; note; more,1.50,1"; 5], "\n"),
                Ok(five(vec![t("Item 1; note; more"), n(1.5), n(1.0)])),
            ),
            (
                "R3, the semicolon, (i) fails: a semicolon file with decimal commas",
                file(&["Item 1 note;1,50;1"; 5], "\r\n"),
                Ok(five(vec![t("Item 1 note"), n(1.5), n(1.0)])),
            ),
            (
                "R4, the semicolon, (i) fails: a decimal comma in every number",
                file(&["Wasser;41,20;3,50", "Strom;63,05;7,25"], "\r\n"),
                Ok(water.clone()),
            ),
            (
                "R5, the semicolon, (ii) fails: a comma in a text and decimal commas",
                file(&["Müller, Hans;41,20;7", "Schmidt, Eva;12,00;3"], "\r\n"),
                Ok(vec![
                    vec![t("Müller, Hans"), n(41.2), n(7.0)],
                    vec![t("Schmidt, Eva"), n(12.0), n(3.0)],
                ]),
            ),
            (
                "R6, the semicolon, (iii) fails: texts with a comma in every row",
                file(&["Müller, Hans;Berlin", "Schmidt, Eva;Köln"], "\r\n"),
                Ok(vec![
                    vec![t("Müller, Hans"), t("Berlin")],
                    vec![t("Schmidt, Eva"), t("Köln")],
                ]),
            ),
            (
                "R7, the comma by (i), (ii), (iii): a comma file of texts, a semicolon in each row",
                file(&["Berlin; Mitte,Hans", "Köln; Ehrenfeld,Eva"], "\n"),
                Ok(vec![
                    vec![t("Berlin; Mitte"), t("Hans")],
                    vec![t("Köln; Ehrenfeld"), t("Eva")],
                ]),
            ),
            (
                "R8, the semicolon, (i) fails: a comma in a thousands shape",
                file(&["Miete;1,234", "Strom;2,500"], "\r\n"),
                Ok(vec![
                    vec![t("Miete"), t("1,234")],
                    vec![t("Strom"), t("2,500")],
                ]),
            ),
            (
                "R9, the semicolon, (iii) fails: a comma in a text and decimal points",
                file(&["Wasser, kalt;41.20", "Strom, Tag;63.05"], "\r\n"),
                Ok(vec![
                    vec![t("Wasser, kalt"), n(41.2)],
                    vec![t("Strom, Tag"), n(63.05)],
                ]),
            ),
            (
                "R10, the semicolon, (i) fails: a comma in a quoted text",
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
                "R11, one reading: a comma file with a semicolon in a quoted text",
                file(&["\"a;b\",1.50", "\"c;d\",2.50"], "\n"),
                Ok(vec![vec![t("a;b"), n(1.5)], vec![t("c;d"), n(2.5)]]),
            ),
            (
                "R12, the tab, (iii) fails, rest (2): a comma file, two tabs a row in a text",
                file(&["Item 1\tnote\tmore,1.50,1"; 5], "\n"),
                Ok(five(vec![t("Item 1"), t("note"), t("more,1.50,1")])),
            ),
            (
                "R13, the tab, (i) fails: a decimal comma in every number",
                file(&["Wasser\t41,20\t3,50", "Strom\t63,05\t7,25"], "\r\n"),
                Ok(water),
            ),
            (
                "R14, one reading: one column of decimal commas, two columns of whole numbers",
                file(&["3,5", "41,20"], "\r\n"),
                Ok(vec![vec![n(3.0), n(5.0)], vec![n(41.0), n(20.0)]]),
            ),
            (
                "R15, one reading: a semicolon file with rows of different lengths",
                file(&["Wasser;41,20;3", "Strom;12,00"], "\r\n"),
                Ok(vec![
                    vec![t("Wasser;41"), t("20;3")],
                    vec![t("Strom;12"), t("00")],
                ]),
            ),
            (
                "R16, the semicolon, (i) fails, rest (3): a comma file, its commas in numbers",
                file(&["Room 1; Floor 2,450", "Room 2; Floor 3,380"], "\n"),
                Ok(vec![
                    vec![t("Room 1"), t(" Floor 2,450")],
                    vec![t("Room 2"), t(" Floor 3,380")],
                ]),
            ),
            (
                "R17, the semicolon, (i) fails: a decimal comma at a field's edge",
                file(&["Wasser;,5"], "\r\n"),
                Ok(vec![vec![t("Wasser"), t(",5")]]),
            ),
            (
                "R18, the semicolon, (i) fails: one decimal comma at a field's edge",
                file(&["Wasser;0,5", "Strom;,75"], "\r\n"),
                Ok(vec![vec![t("Wasser"), n(0.5)], vec![t("Strom"), t(",75")]]),
            ),
            (
                "R19, the semicolon, (i) fails: text behind a closing quote, refused",
                file(&["Wasser;\"a\"b;41,20", "Strom;\"c\"d;63,05"], "\r\n"),
                Err(Error::Source(SourceRefusal::Quote { line: 1 })),
            ),
            (
                "R20, the semicolon's read faults: a comma in a text and a fault of the quotes",
                file(
                    &["Müller, Hans;\"Haus\"A;7", "Schmidt, Eva;\"Haus\"B;3"],
                    "\r\n",
                ),
                Err(Error::Source(SourceRefusal::Quote { line: 1 })),
            ),
            (
                "R21, the semicolon, (i) fails: a comma file whose text ends in a semicolon",
                file(&["Gas;,1.50", "Power;,2.25"], "\n"),
                Ok(vec![
                    vec![t("Gas"), t(",1.50")],
                    vec![t("Power"), t(",2.25")],
                ]),
            ),
            (
                "F1, the semicolon, (ii) fails: a name with a comma and an amount",
                file(
                    &["Müller, Hans;Vertrieb;3.250,50", "Schmidt, Eva;Einkauf;2.980,00"],
                    "\r\n",
                ),
                Ok(names.clone()),
            ),
            (
                "F2, the semicolon, (ii) fails: amounts with three decimals",
                file(
                    &["Mehl, Type 405;Regal 1;1,250", "Zucker, fein;Regal 2;0,500"],
                    "\r\n",
                ),
                Ok(vec![
                    vec![t("Mehl, Type 405"), t("Regal 1"), t("1,250")],
                    vec![t("Zucker, fein"), t("Regal 2"), n(0.5)],
                ]),
            ),
            (
                "F3, the semicolon, (ii) fails: bank lines",
                file(
                    &[
                        "01.10.2026;Miete, Oktober;-1.234,56",
                        "02.10.2026;Strom, Abschlag;-89,50",
                    ],
                    "\r\n",
                ),
                Ok(vec![
                    vec![t("01.10.2026"), t("Miete, Oktober"), t("-1.234,56")],
                    vec![t("02.10.2026"), t("Strom, Abschlag"), n(-89.5)],
                ]),
            ),
            (
                "F4, the tab, (ii) fails: F1 in tabs",
                file(
                    &["Müller, Hans\tVertrieb\t3.250,50", "Schmidt, Eva\tEinkauf\t2.980,00"],
                    "\r\n",
                ),
                Ok(names.clone()),
            ),
            (
                "F5, the semicolon, (ii) fails: numbers of both decimal signs",
                file(
                    &["Termin, früh;14.30;41,20", "Termin, spät;16.45;12,50"],
                    "\r\n",
                ),
                Ok(vec![
                    vec![t("Termin, früh"), t("14.30"), t("41,20")],
                    vec![t("Termin, spät"), t("16.45"), t("12,50")],
                ]),
            ),
            (
                "F6, the semicolon, (ii) fails: a thousands comma",
                file(
                    &["Miller, John;Sales;3,250.50", "Smith, Ann;Buying;2,980.00"],
                    "\r\n",
                ),
                Ok(vec![
                    vec![t("Miller, John"), t("Sales"), t("3,250.50")],
                    vec![t("Smith, Ann"), t("Buying"), t("2,980.00")],
                ]),
            ),
            (
                "F7, the semicolon, (ii) fails: the amount in the first column",
                file(
                    &["1,250;Mehl, Type 405;Regal 1", "0,500;Zucker, fein;Regal 2"],
                    "\r\n",
                ),
                Ok(vec![
                    vec![t("1,250"), t("Mehl, Type 405"), t("Regal 1")],
                    vec![n(0.5), t("Zucker, fein"), t("Regal 2")],
                ]),
            ),
            (
                "F8, the semicolon, (ii) fails: a list of sizes",
                file(&["Schuh, braun;38,40,42", "Stiefel, schwarz;39,41,43"], "\r\n"),
                Ok(vec![
                    vec![t("Schuh, braun"), t("38,40,42")],
                    vec![t("Stiefel, schwarz"), t("39,41,43")],
                ]),
            ),
            (
                "F9, the semicolon, (ii) fails: a name with no space behind its comma",
                file(
                    &["Müller,Hans;Vertrieb;3.250,50", "Schmidt,Eva;Einkauf;2.980,00"],
                    "\r\n",
                ),
                Ok(vec![
                    vec![t("Müller,Hans"), t("Vertrieb"), t("3.250,50")],
                    vec![t("Schmidt,Eva"), t("Einkauf"), t("2.980,00")],
                ]),
            ),
            (
                "F10, the semicolon, (iii) fails: an address list",
                file(&["12, rue de la Paix;Paris", "7, avenue Foch;Lyon"], "\r\n"),
                Ok(vec![
                    vec![t("12, rue de la Paix"), t("Paris")],
                    vec![t("7, avenue Foch"), t("Lyon")],
                ]),
            ),
            (
                "F11, the semicolon, (iii) fails, rest (4): a comma file typed with a space",
                file(&["Item 1; a, 1.50, 1", "Item 2; b, 2.50, 2"], "\n"),
                Ok(vec![
                    vec![t("Item 1"), t(" a, 1.50, 1")],
                    vec![t("Item 2"), t(" b, 2.50, 2")],
                ]),
            ),
            (
                "F12, the semicolon, (ii) fails: a semicolon file typed with a space",
                file(
                    &[
                        "Müller, Hans; Vertrieb; 3.250,50",
                        "Schmidt, Eva; Einkauf; 2.980,00",
                    ],
                    "\r\n",
                ),
                Ok(vec![
                    vec![t("Müller, Hans"), t(" Vertrieb"), t(" 3.250,50")],
                    vec![t("Schmidt, Eva"), t(" Einkauf"), t(" 2.980,00")],
                ]),
            ),
            (
                "A, the semicolon, (ii) fails without the spaces: a name with no space, typed `; `",
                file(
                    &[
                        "Müller,Hans; Vertrieb; 3.250,50",
                        "Schmidt,Eva; Einkauf; 2.980,00",
                    ],
                    "\r\n",
                ),
                Ok(vec![
                    vec![t("Müller,Hans"), t(" Vertrieb"), t(" 3.250,50")],
                    vec![t("Schmidt,Eva"), t(" Einkauf"), t(" 2.980,00")],
                ]),
            ),
            (
                "rest (1), the semicolon, (iii) fails: a semicolon with no space in a comma file",
                file(&["Item;note,1.50,1", "Item 2;note,2.50,2"], "\n"),
                Ok(vec![
                    vec![t("Item"), t("note,1.50,1")],
                    vec![t("Item 2"), t("note,2.50,2")],
                ]),
            ),
            (
                "S1, the semicolon, (iii) fails: a whole number with a space in front is no sign",
                file(&["Schraube,verzinkt; 500 ", "Mutter,M8; 250 "], "\r\n"),
                Ok(vec![
                    vec![t("Schraube,verzinkt"), t(" 500 ")],
                    vec![t("Mutter,M8"), t(" 250 ")],
                ]),
            ),
            (
                "S2, the tab, (iii) fails: S1 in tabs",
                file(&["Schraube,verzinkt\t 500 ", "Mutter,M8\t 250 "], "\r\n"),
                Ok(vec![
                    vec![t("Schraube,verzinkt"), t(" 500 ")],
                    vec![t("Mutter,M8"), t(" 250 ")],
                ]),
            ),
            (
                "S3, the semicolon, (iii) fails: a padded number between two texts",
                file(
                    &[
                        "Schraube,verzinkt,M8; 500 ;Lager A",
                        "Mutter,M8,verzinkt; 250 ;Lager B",
                    ],
                    "\r\n",
                ),
                Ok(vec![
                    vec![t("Schraube,verzinkt,M8"), t(" 500 "), t("Lager A")],
                    vec![t("Mutter,M8,verzinkt"), t(" 250 "), t("Lager B")],
                ]),
            ),
            (
                "rest (5), the comma by (i), (ii), (iii): a text cell that begins with a space",
                file(&["S,M; auf Lager", "L,XL; bestellt"], "\r\n"),
                Ok(vec![
                    vec![t("S"), t("M; auf Lager")],
                    vec![t("L"), t("XL; bestellt")],
                ]),
            ),
            (
                "S4, the semicolon, (iii) fails: a whole amount with a sign of money is a number",
                file(&["Schraube,verzinkt; 500 € ", "Mutter,M8; 250 € "], "\r\n"),
                Ok(vec![
                    vec![t("Schraube,verzinkt"), t(" 500 € ")],
                    vec![t("Mutter,M8"), t(" 250 € ")],
                ]),
            ),
            (
                "S5, the semicolon, (ii) fails: an amount with a decimal comma and a sign of money",
                file(
                    &["Schraube,verzinkt; 3.250,50 € ", "Mutter,M8; 250,00 € "],
                    "\r\n",
                ),
                Ok(vec![
                    vec![t("Schraube,verzinkt"), t(" 3.250,50 € ")],
                    vec![t("Mutter,M8"), t(" 250,00 € ")],
                ]),
            ),
            (
                "S6, the semicolon, (iii) fails: a text padded on both sides",
                file(&["Schraube,verzinkt; auf Lager ", "Mutter,M8; bestellt "], "\r\n"),
                Ok(vec![
                    vec![t("Schraube,verzinkt"), t(" auf Lager ")],
                    vec![t("Mutter,M8"), t(" bestellt ")],
                ]),
            ),
            (
                "S7, the semicolon, (iii) fails: an amount with letters, padded on both sides",
                file(&["Schraube,verzinkt; 500 EUR ", "Mutter,M8; 250 EUR "], "\r\n"),
                Ok(vec![
                    vec![t("Schraube,verzinkt"), t(" 500 EUR ")],
                    vec![t("Mutter,M8"), t(" 250 EUR ")],
                ]),
            ),
            (
                "S8, the semicolon, (iii) fails: a number in brackets, a space in front alone",
                file(&["Schraube,verzinkt; (500)", "Mutter,M8; (250)"], "\r\n"),
                Ok(vec![
                    vec![t("Schraube,verzinkt"), t(" (500)")],
                    vec![t("Mutter,M8"), t(" (250)")],
                ]),
            ),
            (
                "S9, the tab, (iii) fails: S8 in tabs",
                file(&["Schraube,verzinkt\t (500)", "Mutter,M8\t (250)"], "\r\n"),
                Ok(vec![
                    vec![t("Schraube,verzinkt"), t(" (500)")],
                    vec![t("Mutter,M8"), t(" (250)")],
                ]),
            ),
            (
                "S10, the semicolon, (iii) fails: a fraction, a space in front alone",
                file(&["Mehl,Type 405; 1/2", "Zucker,fein; 1/4"], "\r\n"),
                Ok(vec![
                    vec![t("Mehl,Type 405"), t(" 1/2")],
                    vec![t("Zucker,fein"), t(" 1/4")],
                ]),
            ),
            (
                "S11, the semicolon, (iii) fails: a price that ends in a dash",
                file(&["Brot; 2,-", "Milch; 1,-"], "\r\n"),
                Ok(vec![
                    vec![t("Brot"), t(" 2,-")],
                    vec![t("Milch"), t(" 1,-")],
                ]),
            ),
            (
                "rest (6), the comma by (i), (ii), (iii): money in letters, no space behind",
                file(&["Schraube,verzinkt; 500 EUR", "Mutter,M8; 250 EUR"], "\r\n"),
                Ok(vec![
                    vec![t("Schraube"), t("verzinkt; 500 EUR")],
                    vec![t("Mutter"), t("M8; 250 EUR")],
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
