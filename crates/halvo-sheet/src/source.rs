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

/// The rows of `bytes` as `data` reads them where no page is given, as on the Mac: the reader of
/// the tests, whose files read alike on both systems (r3 U11.10).
#[cfg(test)]
pub(crate) fn parse(bytes: &[u8]) -> Result<Vec<Vec<Value>>, Error> {
    data(bytes, None).map(|data| data.rows)
}

/// How the letters of a data file were read (r3 U11.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Letters {
    /// UTF-8, with or without one byte-order mark at its head.
    Utf8,
    /// UTF-16 by the byte-order mark at its head, in either byte order.
    Utf16,
    /// A file with no mark that is not UTF-8, read by the code page that Windows gives as the
    /// computer's own (`GetACP`): Halvo guessed its letters.
    Page(u16),
}

/// The rows of a data file and how its letters were read (r3 U11.10).
#[derive(Clone, Debug, PartialEq)]
pub struct Data {
    /// The rows, as [`crate::Folder::read_source`] gives them.
    pub rows: Vec<Vec<Value>>,
    /// How the letters were read.
    pub letters: Letters,
}

/// The rows of a source file, in the form that [`crate::Folder::read_source`] states: RFC 4180
/// (its section 2) with one separator, the comma, the semicolon or the tab, after one byte-order
/// mark at the first byte. No line break stands inside a field, so a row is a line and a refusal
/// names the line. A file is read as UTF-16 behind its mark, else as UTF-8, and a file that is
/// neither, nor, where `page` is a code page of Windows, text of that page, is refused first
/// (`letters`, r3 U11.10); then the file's separator is told
/// and the file is read by it (`reading`), where a file whose separator is in doubt is refused, a
/// fault of the form stops the read where it stands, and rows of unequal length are refused. Then
/// a file with a field of number signs alone is refused (`number_signs`, r3 U11.9). Each field's
/// value is told last, by the file's numbers (`Numbers::of`) and by its column, in which an amount
/// as a spreadsheet program shows it turns whole or not at all (`turning`).
pub(crate) fn data(bytes: &[u8], page: Option<u16>) -> Result<Data, Error> {
    let (text, letters) = letters(bytes, page)?;
    let rows = rows(&text)?;
    Ok(Data { rows, letters })
}

fn encoding() -> Error {
    Error::Source(SourceRefusal::Encoding)
}

/// The text of `bytes` and how its letters were read (r3 U11.10): UTF-16LE behind the mark
/// FF FE, UTF-16BE behind FE FF, and the head FF FE 00 00, the mark of UTF-32LE, refused; else
/// UTF-8 without one mark at its head; else, where `page` is given, that page (`by_page`); else
/// refused. A file that is UTF-8 never begins with FF or FE, so the marks change no such file.
fn letters(bytes: &[u8], page: Option<u16>) -> Result<(Cow<'_, str>, Letters), Error> {
    if bytes.starts_with(&[0xff, 0xfe, 0x00, 0x00]) {
        return Err(encoding());
    }
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return Ok((
            Cow::Owned(utf_16(rest, u16::from_le_bytes)?),
            Letters::Utf16,
        ));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return Ok((
            Cow::Owned(utf_16(rest, u16::from_be_bytes)?),
            Letters::Utf16,
        ));
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok((
            Cow::Borrowed(text.strip_prefix('\u{feff}').unwrap_or(text)),
            Letters::Utf8,
        )),
        Err(_) => {
            let page = page
                .filter(|page| PAGES.contains(page))
                .ok_or_else(encoding)?;
            let text = by_page(bytes, page)?;
            alike(bytes, &text)?;
            Ok((Cow::Owned(text), Letters::Page(page)))
        }
    }
}

/// The text of the code units of two bytes each in `rest`, each read by `unit`; an odd count of
/// bytes and a lone surrogate are refused.
fn utf_16(rest: &[u8], unit: fn([u8; 2]) -> u16) -> Result<String, Error> {
    let (pairs, odd) = rest.as_chunks::<2>();
    if !odd.is_empty() {
        return Err(encoding());
    }
    let units = pairs.iter().map(|pair| unit(*pair));
    char::decode_utf16(units)
        .collect::<Result<String, _>>()
        .map_err(|_| encoding())
}

/// The text of `bytes` by the code page `page` of Windows (r3 U11.10, the coordinator's ruling
/// W2): the encoding that codepage gives for it, where that encoding reads the bytes 00 to 7F as
/// ASCII and is not UTF-8, read with no replacement; refused where codepage maps no encoding, where
/// a byte has no letter in it, and where a letter is a control sign: below U+0020 but the tab, CR
/// and LF, or U+007F to U+009F (the guard P1); refused too where the page is gbk (936) and the
/// bytes hold a four-byte letter (`four_byte`), and where the text begins with U+FEFF, which the
/// UTF-8 twin of the text would drop (r3 U11.10, the coordinator's rule of log line 23651).
fn by_page(bytes: &[u8], page: u16) -> Result<String, Error> {
    let encoding_of_page = codepage::to_encoding(page)
        .filter(|e| e.is_ascii_compatible() && *e != encoding_rs::UTF_8)
        .ok_or_else(encoding)?;
    let gbk = encoding_of_page == encoding_rs::GBK || encoding_of_page == encoding_rs::GB18030;
    if gbk && four_byte(bytes) {
        return Err(encoding());
    }
    let text = encoding_of_page
        .decode_without_bom_handling_and_without_replacement(bytes)
        .ok_or_else(encoding)?;
    let control = |c: char| {
        (c < ' ' && !matches!(c, '\t' | '\r' | '\n')) || ('\u{7f}'..='\u{9f}').contains(&c)
    };
    if text.chars().any(control) || text.starts_with('\u{feff}') {
        return Err(encoding());
    }
    Ok(text.into_owned())
}

/// The ANSI pages that Windows gives as a computer's own (`GetACP`): the list of the
/// coordinator's rule (r3 U11.10, log line 23651). A computer's page outside it reads no file.
const PAGES: [u16; 14] = [
    874, 932, 936, 949, 950, 1250, 1251, 1252, 1253, 1254, 1255, 1256, 1257, 1258,
];

/// Whether `bytes`, walked as gbk reads them (a byte below 81 or FF alone, a byte 81 to FE with
/// the next), hold the head of a four-byte letter of GB18030: a byte 81 to FE followed by a digit
/// byte 30 to 39 (r3 U11.10: a file of the page 936 with such a letter is refused, since its digit
/// bytes are no digits under that page).
fn four_byte(bytes: &[u8]) -> bool {
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        match byte {
            0x81..=0xfe if bytes.get(at + 1).is_some_and(u8::is_ascii_digit) => return true,
            0x81..=0xfe => at += 2,
            _ => at += 1,
        }
    }
    false
}

/// Whether the file of `bytes`, which its own page reads as `text`, reads alike under every page
/// of `PAGES` that reads its bytes (`by_page`) (r3 U11.10, the coordinator's rule of log line
/// 23651): part (1), by the same separator (`reading`), or by the same refusal of it; part (2), in
/// as many rows of as many cells (`rows`), where no cell is a number under two pages with two
/// values, a number under one page beside a text under another passing. Else the file is refused
/// with `data_encoding`. Part (3) follows: the own page reads as the same text saved as UTF-8,
/// and so cell for cell as the twin of every page that reads the bytes, or is refused.
fn alike(bytes: &[u8], text: &str) -> Result<(), Error> {
    let separator = |text: &str| reading(text).map(|(sign, _)| sign);
    let own = separator(text);
    let mut seen = rows(text).ok().map(|rows| number_bits(&rows));
    for page in PAGES {
        let Ok(other) = by_page(bytes, page) else {
            continue;
        };
        if separator(&other) != own {
            return Err(encoding());
        }
        let (Some(seen), Ok(rows)) = (seen.as_mut(), rows(&other)) else {
            continue;
        };
        let theirs = number_bits(&rows);
        let shape = theirs.len() == seen.len()
            && theirs
                .iter()
                .zip(seen.iter())
                .all(|(a, b)| a.len() == b.len());
        if !shape {
            return Err(encoding());
        }
        for (cell, held) in theirs.iter().flatten().zip(seen.iter_mut().flatten()) {
            match (*cell, *held) {
                (Some(x), Some(y)) if x != y => return Err(encoding()),
                (Some(x), None) => *held = Some(x),
                _ => {}
            }
        }
    }
    Ok(())
}

/// The bits of each number of `rows`, cell for cell, and none for a text.
fn number_bits(rows: &[Vec<Value>]) -> Vec<Vec<Option<u64>>> {
    rows.iter()
        .map(|row| {
            row.iter()
                .map(|value| match value {
                    Value::Number(x) => Some(x.to_bits()),
                    Value::Text(_) => None,
                })
                .collect()
        })
        .collect()
}

/// The code page that Windows gives as the computer's own (`GetACP`), the one by which Excel's
/// file type CSV (Comma delimited) writes its letters; none on another system.
#[cfg(windows)]
pub(crate) fn system_page() -> Option<u16> {
    // SAFETY: GetACP takes no argument and only reads a setting of the system.
    let page = unsafe { windows_sys::Win32::Globalization::GetACP() };
    u16::try_from(page).ok()
}

/// The code page that Windows gives as the computer's own: none on this system.
#[cfg(not(windows))]
pub(crate) fn system_page() -> Option<u16> {
    None
}

/// The rows of `text`, as `parse` states them after the encoding.
fn rows(text: &str) -> Result<Vec<Vec<Value>>, Error> {
    let (separator, rows) = reading(text)?;
    if let Some(line) = number_signs(&rows) {
        return Err(Error::Source(SourceRefusal::NumberSigns { line }));
    }
    let numbers = Numbers::of(separator, &rows);
    let turning = turning(numbers, &rows);
    Ok(rows
        .iter()
        .map(|row| {
            row.iter()
                .enumerate()
                .map(|(at, cell)| numbers.value(cell, turning.get(at) == Some(&true)))
                .collect()
        })
        .collect())
}

/// The fields of a file's rows, as `read` gives them.
type Rows<'a> = Vec<Vec<Cow<'a, str>>>;

/// The line (counted from 1; a row is a line) of the first field of `rows` that, without the
/// spaces (U+0020) at its ends, is one number sign `#` or more and nothing else, quoted or not:
/// what a spreadsheet program writes for a value whose column was too narrow to show it when the
/// file was saved, whose value the file no longer holds (r3 U11.9). A field of the first row that
/// is one `#` alone is the title of a column (of row numbers, as a rule) and passes, since the
/// file's form has no row of titles that `reading` could tell. Every other field passes: `#N/A`,
/// `#REF!` and a word hold another sign.
fn number_signs(rows: &Rows<'_>) -> Option<usize> {
    rows.iter().enumerate().find_map(|(at, row)| {
        row.iter()
            .map(|cell| cell.trim_matches(' '))
            .any(|cell| {
                !cell.is_empty() && cell.bytes().all(|b| b == b'#') && !(at == 0 && cell == "#")
            })
            .then_some(at + 1)
    })
}

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
    /// number with a decimal sign, and no amount (`amount`) or an amount of each sign tells one:
    /// its whole numbers alone are numbers.
    Whole,
}

impl Numbers {
    /// The numbers of a file read by `separator` with the fields `rows`: a file read by
    /// semicolons or by tabs has one decimal sign, the comma where a field is a number with a
    /// decimal comma and none is one with a decimal point, the point the other way round. A
    /// field of the shape of a thousands group, of either sign, has two readings and decides
    /// nothing. Only where no field tells a sign, an amount as a spreadsheet program shows it
    /// (`amount`) tells it the same way (r3 U11.8); where fields tell both signs, no amount
    /// tells one.
    fn of(separator: u8, rows: &[Vec<Cow<'_, str>>]) -> Numbers {
        if separator == b',' {
            return Numbers::Points;
        }
        let has = |sign: char| {
            rows.iter()
                .flatten()
                .any(|cell| with_sign(cell, sign).is_some())
        };
        let tells = |sign: char| {
            rows.iter()
                .flatten()
                .any(|cell| amount(cell).is_some_and(|(_, decimal)| decimal == Some(sign)))
        };
        let one = |comma: bool, point: bool| match (comma, point) {
            (true, false) => Some(Numbers::Sign(',')),
            (false, true) => Some(Numbers::Sign('.')),
            _ => None,
        };
        match (has(','), has('.')) {
            (false, false) => one(tells(','), tells('.')).unwrap_or(Numbers::Whole),
            (comma, point) => one(comma, point).unwrap_or(Numbers::Whole),
        }
    }

    /// The number of `cell` before r3 U11.8.
    fn today(self, cell: &str) -> Option<f64> {
        match self {
            Numbers::Points => point_number(cell),
            Numbers::Sign(sign) => whole_number(cell).or_else(|| with_sign(cell, sign)),
            Numbers::Whole => whole_number(cell),
        }
    }

    /// The number of `cell` as an amount (`amount`) whose decimal sign, where it has one, is the
    /// file's: the point in a file read by commas, the sign that the file tells in a file read by
    /// semicolons or by tabs; in a file that tells none, an amount with no decimal sign alone.
    fn amount_number(self, cell: &str) -> Option<f64> {
        let (x, decimal) = amount(cell)?;
        let sign = match self {
            Numbers::Points => Some('.'),
            Numbers::Sign(sign) => Some(sign),
            Numbers::Whole => None,
        };
        (decimal.is_none() || decimal == sign).then_some(x)
    }

    /// The value of `cell`: its number of before r3 U11.8, else its number as an amount where
    /// the amounts of its column `turn` (`turning`), else its text.
    fn value(self, cell: &str, turn: bool) -> Value {
        let number = self
            .today(cell)
            .or_else(|| turn.then(|| self.amount_number(cell)).flatten());
        match number {
            Some(x) => Value::Number(x),
            None => Value::Text(cell.to_string()),
        }
    }
}

/// Whether the amounts of each column of `rows` turn into their numbers (r3 U11.8): Halvo turns
/// the amounts of a column into numbers only where it can read every value of that column. Every
/// field that holds a digit (of any script) and gets no number blocks its column, but a field of
/// the file's first row that holds a letter (of any script), which is taken for a header. A
/// blocked column reads as before r3 U11.8, and a field that is a number before r3 U11.8 stays
/// that number in every column (`Numbers::value`).
fn turning(numbers: Numbers, rows: &[Vec<Cow<'_, str>>]) -> Vec<bool> {
    let width = rows.first().map_or(0, Vec::len);
    (0..width)
        .map(|at| {
            rows.iter().enumerate().all(|(line, row)| {
                row.get(at).is_none_or(|cell| {
                    (line == 0 && cell.chars().any(char::is_alphabetic))
                        || !cell.chars().any(char::is_numeric)
                        || numbers
                            .today(cell)
                            .or_else(|| numbers.amount_number(cell))
                            .is_some()
                })
            })
        })
        .collect()
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

/// The signs of money that an amount may carry at one of its ends (`amount`).
const MONEY: [char; 4] = ['$', '€', '£', '¥'];

/// The spaces that may stand between an amount's sign of money and its digits (`amount`).
const GAPS: [char; 3] = [' ', '\u{a0}', '\u{202f}'];

/// The signs that group an amount's thousands beside the comma and the point, and that are never
/// a decimal sign (`amount`).
const GROUPS: [char; 4] = ['\'', '\u{2019}', '\u{a0}', '\u{202f}'];

/// The signs that an amount reads as a minus: the plain minus and the true minus sign U+2212; no
/// other dash (`amount`).
const MINUS: [char; 2] = ['-', '\u{2212}'];

/// An amount as a spreadsheet program shows it, where its form has one reading (r3 U11.8): its
/// number, and its decimal sign where it has one. Without the spaces (U+0020) at its ends, ONE
/// sign of money of `MONEY` at either end with the spaces of `GAPS` or none between it and the
/// digits, a minus of `MINUS` in front of the digits or of the sign of money, or brackets round
/// the amount with a sign of money inside them, in front of them or behind them (a negative
/// amount), it is digits with at most one decimal sign, the comma or the point, and a thousands
/// sign of another kind (the other of the two, or one of `GROUPS`) in groups of three behind a
/// first group of one to three digits, with A11.3's form for its digits (`point_number`). A
/// thousands sign of `GROUPS` needs no decimal part (`1'234`); the comma or the point as a
/// thousands sign needs the other sign as the decimal sign (`1,234.56`). Not an amount: the
/// shape of a thousands group with no other sign (`1,234`, `¥1,234`, `1,799 €`), which has two
/// readings; a sign of thousands repeated with no decimal sign (`1,234,567`), which may be a list
/// such as `38,40,42`; the Indian grouping (`12,34,567.89`); a plain space between groups;
/// brackets with no sign of money (`(12.50)`); a minus and brackets both, wherever the minus
/// stands (`(-$12.50)`, `($-12.50)`, `-($12.50)`), which has two readings; letters (`500 EUR`);
/// a percent; a minus behind (`12.50-`); and a dash that is no minus (U+2013).
fn amount(cell: &str) -> Option<(f64, Option<char>)> {
    let cell = cell.trim_matches(' ');
    let (cell, outer) = bracketed(cell);
    let (cell, minus) = match cell.strip_prefix(MINUS) {
        Some(rest) if !outer => (rest, true),
        _ => (cell, false),
    };
    let (cell, money) = without_money(cell);
    let (cell, inner) = if outer || minus {
        (cell, false)
    } else {
        bracketed(cell)
    };
    let (cell, minus_inner) = match cell.strip_prefix(MINUS) {
        Some(rest) if !(outer || minus || inner) => (rest, true),
        _ => (cell, false),
    };
    if (outer || inner) && !money {
        return None;
    }
    let (x, decimal) = amount_body(cell)?;
    let negative = outer || inner || minus || minus_inner;
    Some((if negative { -x } else { x }, decimal))
}

/// `cell` without the brackets round it, and whether it had them.
fn bracketed(cell: &str) -> (&str, bool) {
    match cell
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    {
        Some(inside) => (inside, true),
        None => (cell, false),
    }
}

/// `cell` without ONE sign of money of `MONEY` at either end and the spaces of `GAPS` between it
/// and the rest, and whether it had one.
fn without_money(cell: &str) -> (&str, bool) {
    let front = cell
        .strip_prefix(MONEY)
        .map(|rest| rest.trim_start_matches(GAPS));
    let back = || {
        cell.strip_suffix(MONEY)
            .map(|rest| rest.trim_end_matches(GAPS))
    };
    match front.or_else(back) {
        Some(rest) => (rest, true),
        None => (cell, false),
    }
}

/// The number of an amount's digits and signs (`amount`), and its decimal sign where it has one.
fn amount_body(body: &str) -> Option<(f64, Option<char>)> {
    let digit_ends = body.starts_with(|c: char| c.is_ascii_digit())
        && body.ends_with(|c: char| c.is_ascii_digit());
    if !digit_ends {
        return None;
    }
    let (whole, decimal, fraction) = match body.rfind([',', '.']) {
        Some(at) => (&body[..at], body[at..].chars().next(), &body[at + 1..]),
        None => (body, None, ""),
    };
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut signs = whole.chars().filter(|c| !c.is_ascii_digit());
    let group = signs.next();
    if signs.any(|c| Some(c) != group) {
        return None;
    }
    let digits = match group {
        Some(sign) => {
            let other = [',', '.'].contains(&sign) && Some(sign) != decimal;
            let mut parts = whole.split(sign);
            let first = parts.next().unwrap_or_default();
            let threes = parts.all(|part| part.len() == 3);
            if !(GROUPS.contains(&sign) || other) || !(1..=3).contains(&first.len()) || !threes {
                return None;
            }
            whole.replace(sign, "")
        }
        None => {
            let shape = whole != "0" && (1..=3).contains(&whole.len()) && fraction.len() == 3;
            if decimal.is_some() && shape {
                return None;
            }
            whole.to_string()
        }
    };
    let text = match decimal {
        Some(_) => format!("{digits}.{fraction}"),
        None => digits,
    };
    point_number(&text).map(|x| (x, decimal))
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
            (b"\xff\xfes\x00e\x00c\x00r\x00e\x00t\x00t", Encoding),
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

    /// r3 U11.9: a data file in which a field, quoted or not and without the spaces at its ends,
    /// is one number sign or more and nothing else, as a spreadsheet program writes a value whose
    /// column was too narrow to show it, is refused whole with `data_narrow`, its first such line
    /// named, before the run's write step: read by commas, by semicolons or by tabs, in a column
    /// of numbers or of texts, in the first row too. A field of the first row that is one `#` alone
    /// is a title and passes; `#N/A`, `#REF!`, `#DIV/0!` and a word read as before, and a file
    /// that `reading` refuses keeps its refusal.
    #[test]
    fn a_data_file_with_a_field_of_number_signs_alone_is_refused_before_any_write() {
        let refused: [(&[u8], usize); 11] = [
            (b"Rent,500\nPower,#\n", 2),
            (b"Rent,500\nPower,##\n", 2),
            (b"Rent,500\nPower,########\n", 2),
            (b"Rent,500\nPower,\"########\"\n", 2),
            (b"Rent,500\nPower, ######## \n", 2),
            (b"Rent,500\nPower,\"  ##\"\n", 2),
            (b"Rent;500\r\nPower;########\r\n", 2),
            (b"Rent\t500\nPower\t#####\n", 2),
            (b"Rent,500\n###,63\n", 2),
            (b"########,500\nPower,63\n", 1),
            (b"#,Item\n1,Rent\n2,#\n", 3),
        ];
        for (bytes, line) in refused {
            let got = parse(bytes).map_err(|e| (e.source_cause().name(), e.cause().name(), e));
            let want = Error::Source(SourceRefusal::NumberSigns { line });
            assert_eq!(
                got,
                Err(("data_narrow", "data_narrow", want)),
                "{}",
                shown(bytes)
            );
        }
        let titled = b"#,Item\n1,Rent\n";
        let want = vec![vec![t("#"), t("Item")], vec![n(1.0), t("Rent")]];
        assert_eq!(parse(titled), Ok(want), "{}", shown(titled));
        let others = b"Rent,#N/A\nPower,#REF!\nGas,#DIV/0!\nWater,Total\nHeat,\n";
        let want: Vec<Vec<Value>> = [
            ["Rent", "#N/A"],
            ["Power", "#REF!"],
            ["Gas", "#DIV/0!"],
            ["Water", "Total"],
            ["Heat", ""],
        ]
        .iter()
        .map(|row| row.iter().map(|cell| t(cell)).collect())
        .collect();
        assert_eq!(parse(others), Ok(want), "{}", shown(others));
        let ragged = b"Rent,500\nPower,########,x\n";
        let want = Err(Error::Source(SourceRefusal::Ragged { line: 2 }));
        assert_eq!(parse(ragged), want, "{}", shown(ragged));
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
            ("1.234,56", n(1234.56)),
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
            ("1,234.56", n(1234.56)),
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
            vec![t("Müller, Hans"), t("Vertrieb"), n(3250.5)],
            vec![t("Schmidt, Eva"), t("Einkauf"), n(2980.0)],
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
                    vec![t("01.10.2026"), t("Miete, Oktober"), n(-1234.56)],
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
                    vec![t("Miller, John"), t("Sales"), n(3250.5)],
                    vec![t("Smith, Ann"), t("Buying"), n(2980.0)],
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
                    vec![t("Müller,Hans"), t("Vertrieb"), n(3250.5)],
                    vec![t("Schmidt,Eva"), t("Einkauf"), n(2980.0)],
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
                    vec![t("Müller, Hans"), t(" Vertrieb"), n(3250.5)],
                    vec![t("Schmidt, Eva"), t(" Einkauf"), n(2980.0)],
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
                    vec![t("Müller,Hans"), t(" Vertrieb"), n(3250.5)],
                    vec![t("Schmidt,Eva"), t(" Einkauf"), n(2980.0)],
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
                    vec![t("Schraube,verzinkt"), n(500.0)],
                    vec![t("Mutter,M8"), n(250.0)],
                ]),
            ),
            (
                "S2, the tab, (iii) fails: S1 in tabs",
                file(&["Schraube,verzinkt\t 500 ", "Mutter,M8\t 250 "], "\r\n"),
                Ok(vec![
                    vec![t("Schraube,verzinkt"), n(500.0)],
                    vec![t("Mutter,M8"), n(250.0)],
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
                    vec![t("Schraube,verzinkt,M8"), n(500.0), t("Lager A")],
                    vec![t("Mutter,M8,verzinkt"), n(250.0), t("Lager B")],
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
                    vec![t("Schraube,verzinkt"), n(500.0)],
                    vec![t("Mutter,M8"), n(250.0)],
                ]),
            ),
            (
                "S5, the semicolon, (ii) fails: an amount with a decimal comma and a sign of money",
                file(
                    &["Schraube,verzinkt; 3.250,50 € ", "Mutter,M8; 250,00 € "],
                    "\r\n",
                ),
                Ok(vec![
                    vec![t("Schraube,verzinkt"), n(3250.5)],
                    vec![t("Mutter,M8"), n(250.0)],
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

    /// r3 U11.8, the coordinator's table: an amount as a spreadsheet program shows it is its
    /// number where its form has one reading, and a form of two readings stays a text, as before.
    /// Each form stands alone in a file read by commas (quoted where it holds a comma) and in a
    /// file read by semicolons; then the files of more rows show the file's one decimal sign.
    #[test]
    fn an_amount_as_a_program_shows_it_is_its_number_where_its_form_has_one_reading() {
        let nb = '\u{a0}';
        let nnb = '\u{202f}';
        let minus = '\u{2212}';
        let dash = '\u{2013}';
        // The field, its cell in a file read by commas, its cell in a file read by semicolons.
        let alone: Vec<(String, Value, Value)> = vec![
            // A sign of money at an end, with and without a space.
            ("$12.50".into(), n(12.5), n(12.5)),
            ("$ 12.50".into(), n(12.5), n(12.5)),
            ("€12.50".into(), n(12.5), n(12.5)),
            ("12,50 €".into(), t("12,50 €"), n(12.5)),
            ("€ 12,50".into(), t("€ 12,50"), n(12.5)),
            (format!("12,50{nb}€"), t(&format!("12,50{nb}€")), n(12.5)),
            ("1.250,00 €".into(), t("1.250,00 €"), n(1250.0)),
            ("£1,234.56".into(), n(1234.56), n(1234.56)),
            ("$1,234.56".into(), n(1234.56), n(1234.56)),
            ("¥1,234".into(), t("¥1,234"), t("¥1,234")),
            ("1,799 €".into(), t("1,799 €"), t("1,799 €")),
            ("12.50 USD".into(), t("12.50 USD"), t("12.50 USD")),
            ("CHF 12.50".into(), t("CHF 12.50"), t("CHF 12.50")),
            ("kr 12,50".into(), t("kr 12,50"), t("kr 12,50")),
            ("500 EUR".into(), t("500 EUR"), t("500 EUR")),
            // A whole amount with a sign of money (the ruling's P2).
            ("$500".into(), n(500.0), n(500.0)),
            ("500 €".into(), n(500.0), n(500.0)),
            ("¥500".into(), n(500.0), n(500.0)),
            (" 500 € ".into(), n(500.0), n(500.0)),
            // A thousands sign that is never a decimal sign, with no decimal part (P2).
            ("1'234".into(), n(1234.0), n(1234.0)),
            ("1’234".into(), n(1234.0), n(1234.0)),
            (format!("1{nb}234"), n(1234.0), n(1234.0)),
            (format!("1{nnb}234"), n(1234.0), n(1234.0)),
            // A thousands sign with a decimal part behind the other sign.
            ("1,234.56".into(), n(1234.56), n(1234.56)),
            ("1.234,56".into(), t("1.234,56"), n(1234.56)),
            ("12,345,678.90".into(), n(12_345_678.9), n(12_345_678.9)),
            ("-1,234.56".into(), n(-1234.56), n(-1234.56)),
            ("-1.234,56".into(), t("-1.234,56"), n(-1234.56)),
            (
                format!("1{nb}234,56"),
                t(&format!("1{nb}234,56")),
                n(1234.56),
            ),
            (
                format!("1{nnb}234,56"),
                t(&format!("1{nnb}234,56")),
                n(1234.56),
            ),
            (
                format!("1{nnb}234,56{nb}€"),
                t(&format!("1{nnb}234,56{nb}€")),
                n(1234.56),
            ),
            ("1'234.56".into(), n(1234.56), n(1234.56)),
            ("1’234.56".into(), n(1234.56), n(1234.56)),
            // The shapes of two readings, and forms of more than one group, stay as they were.
            ("1,234".into(), t("1,234"), t("1,234")),
            ("1.234".into(), n(1.234), t("1.234")),
            ("1,234,567".into(), t("1,234,567"), t("1,234,567")),
            ("1.234.567".into(), t("1.234.567"), t("1.234.567")),
            ("12,34,567.89".into(), t("12,34,567.89"), t("12,34,567.89")),
            ("1 234,56".into(), t("1 234,56"), t("1 234,56")),
            ("1,234,56".into(), t("1,234,56"), t("1,234,56")),
            ("$007.50".into(), t("$007.50"), t("$007.50")),
            // Spaces at a field's ends, as an accounting format pads a cell and as a person types.
            (" 12.50".into(), n(12.5), n(12.5)),
            ("12.50 ".into(), n(12.5), n(12.5)),
            (" $ 1,234.56 ".into(), n(1234.56), n(1234.56)),
            (" 12,50".into(), t(" 12,50"), n(12.5)),
            (" 1.234,56 € ".into(), t(" 1.234,56 € "), n(1234.56)),
            // A negative amount in each way that a program writes it.
            ("-12.50".into(), n(-12.5), n(-12.5)),
            ("-$12.50".into(), n(-12.5), n(-12.5)),
            ("$-12.50".into(), n(-12.5), n(-12.5)),
            ("-12,50 €".into(), t("-12,50 €"), n(-12.5)),
            ("($1,234.56)".into(), n(-1234.56), n(-1234.56)),
            (" $ (1,234.56)".into(), n(-1234.56), n(-1234.56)),
            ("(1.234,56 €)".into(), t("(1.234,56 €)"), n(-1234.56)),
            ("(12.50)".into(), t("(12.50)"), t("(12.50)")),
            ("12.50-".into(), t("12.50-"), t("12.50-")),
            ("-($12.50)".into(), t("-($12.50)"), t("-($12.50)")),
            ("(-$12.50)".into(), t("(-$12.50)"), t("(-$12.50)")),
            ("($-12.50)".into(), t("($-12.50)"), t("($-12.50)")),
            ("(12,50) €".into(), t("(12,50) €"), n(-12.5)),
            // The true minus sign U+2212 is a minus; no other dash is.
            (format!("{minus}$3.00"), n(-3.0), n(-3.0)),
            (format!("{minus}12.50"), n(-12.5), n(-12.5)),
            (
                format!("{dash}12.50"),
                t(&format!("{dash}12.50")),
                t(&format!("{dash}12.50")),
            ),
            // A percent.
            ("50%".into(), t("50%"), t("50%")),
            ("12.5 %".into(), t("12.5 %"), t("12.5 %")),
            ("12,5 %".into(), t("12,5 %"), t("12,5 %")),
            // The forms that stay texts whatever is ruled.
            ("007".into(), t("007"), t("007")),
            ("+5".into(), t("+5"), t("+5")),
            ("1e5".into(), t("1e5"), t("1e5")),
            ("2026-10-01".into(), t("2026-10-01"), t("2026-10-01")),
            ("01.10.2026".into(), t("01.10.2026"), t("01.10.2026")),
            (
                "+49 30 1234567".into(),
                t("+49 30 1234567"),
                t("+49 30 1234567"),
            ),
            ("030 1234567".into(), t("030 1234567"), t("030 1234567")),
            ("38,40,42".into(), t("38,40,42"), t("38,40,42")),
            ("$".into(), t("$"), t("$")),
            ("-".into(), t("-"), t("-")),
        ];
        // Files of more rows: the file's one decimal sign (point (b) of the table; P3).
        let files: [(&str, &str, Vec<Value>); 13] = [
            (
                "b1",
                "Miete;1.250,00 €\nStrom;63,05 €\n",
                vec![n(1250.0), n(63.05)],
            ),
            (
                "b1 tabs",
                "Miete\t1.250,00 €\nStrom\t63,05 €\n",
                vec![n(1250.0), n(63.05)],
            ),
            (
                "b2",
                "Miete;1.250,00\nStrom;63,05\n",
                vec![n(1250.0), n(63.05)],
            ),
            (
                "b2'",
                "Miete;1.250\nStrom;63,05\n",
                vec![t("1.250"), n(63.05)],
            ),
            (
                "b3",
                "Miete;1.234,56\nRent;1,234.56\n",
                vec![t("1.234,56"), t("1,234.56")],
            ),
            (
                "b3'",
                "Miete;1.234,56 €\nRent;$1,234.56\n",
                vec![t("1.234,56 €"), t("$1,234.56")],
            ),
            (
                "b4",
                "Miete;1.234,56\nAnzahl;12\n",
                vec![n(1234.56), n(12.0)],
            ),
            (
                "b4'",
                "Miete;1.234,56\nAnzahl;1.234\n",
                vec![t("1.234,56"), t("1.234")],
            ),
            (
                "P3",
                "A;1,5\nB;1.5\nC;1.234,56 €\n",
                vec![t("1,5"), t("1.5"), t("1.234,56 €")],
            ),
            (
                "b5",
                "Rent;$1,250.00\nPower;$63.05\n",
                vec![n(1250.0), n(63.05)],
            ),
            (
                "b6",
                "Strom;63,05\nRent;$1,234.56\n",
                vec![n(63.05), t("$1,234.56")],
            ),
            (
                "c1",
                "Miete,\"1.250,00 €\"\nStrom,\"63,05 €\"\n",
                vec![t("1.250,00 €"), t("63,05 €")],
            ),
            (
                "c3",
                "Rent,\"$1,250.00\"\nPower, $ 63.05 \n",
                vec![n(1250.0), n(63.05)],
            ),
        ];
        let mut wrong = Vec::new();
        for (field, commas, semicolons) in &alone {
            let quoted = if field.contains(',') {
                format!("\"{field}\"")
            } else {
                field.clone()
            };
            for (text, want) in [
                (format!("a,{quoted}\n"), commas),
                (format!("a;{field}\n"), semicolons),
            ] {
                let got = parse(text.as_bytes());
                if got != Ok(vec![vec![t("a"), want.clone()]]) {
                    wrong.push(format!(
                        "{}: got {got:?}, want {want:?}",
                        text.escape_debug()
                    ));
                }
            }
        }
        for (name, text, want) in files {
            let got = parse(text.as_bytes()).map(|rows| rows.into_iter().map(|row| row[1].clone()));
            let got: Result<Vec<Value>, Error> = got.map(Iterator::collect);
            if got != Ok(want.clone()) {
                wrong.push(format!(
                    "{name}: {}: got {got:?}, want {want:?}",
                    text.escape_debug()
                ));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    /// r3 U11.8, the coordinator's rulings P1 and of the round: Halvo turns the amounts of a
    /// column into numbers only where it can read every value of that column. Where one field
    /// that holds a digit gets no number, no amount of its column turns, but for a field of the
    /// first row that holds a letter (a header); a column of amounts that each have one reading
    /// turns whole. Each file is read by commas and by semicolons.
    #[test]
    fn a_column_of_amounts_turns_whole_or_not_at_all() {
        let cases: [(&[&str], Vec<Value>); 6] = [
            (
                &["$950", "$1,234", "$12,500"],
                vec![t("$950"), t("$1,234"), t("$12,500")],
            ),
            (&["$5", "$12", "$40"], vec![n(5.0), n(12.0), n(40.0)]),
            // The round (log line 20002): every field that holds a digit and gets no number
            // blocks its column, but a field of the first row that holds a letter.
            (&["$12.50", "12.50 USD"], vec![t("$12.50"), t("12.50 USD")]),
            (
                &["Amount 2026", "$12.50", "$3.00"],
                vec![t("Amount 2026"), n(12.5), n(3.0)],
            ),
            (&["12.50 USD", "$3.00"], vec![t("12.50 USD"), n(3.0)]),
            (
                &["$12.50", "see note 3"],
                vec![t("$12.50"), t("see note 3")],
            ),
        ];
        let mut wrong = Vec::new();
        for (cells, want) in &cases {
            for sign in [',', ';'] {
                let text: String = cells
                    .iter()
                    .map(|cell| {
                        if sign == ',' && cell.contains(',') {
                            format!("Rent{sign}\"{cell}\"\r\n")
                        } else {
                            format!("Rent{sign}{cell}\r\n")
                        }
                    })
                    .collect();
                let want: Vec<Vec<Value>> =
                    want.iter().map(|v| vec![t("Rent"), v.clone()]).collect();
                let got = parse(text.as_bytes());
                if got != Ok(want.clone()) {
                    wrong.push(format!(
                        "{}: got {got:?}, want {want:?}",
                        text.escape_debug()
                    ));
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }

    fn utf_16(mark: [u8; 2], text: &str, unit: fn(u16) -> [u8; 2]) -> Vec<u8> {
        let mut bytes = mark.to_vec();
        for u in text.encode_utf16() {
            bytes.extend_from_slice(&unit(u));
        }
        bytes
    }

    /// r3 U11.10 (b1): a file in UTF-16 with the byte-order mark at its head, in either byte
    /// order, as Excel's file type Unicode Text saves it, reads to the rows of the same text in
    /// UTF-8; the mark alone reads no rows, as the UTF-8 mark alone does.
    #[test]
    fn a_file_in_utf_16_with_its_mark_reads_as_its_utf_8_form() {
        let text = "Müller\t41,20\r\nStrom\t12,50 €\r\n";
        let want = vec![vec![t("Müller"), n(41.2)], vec![t("Strom"), n(12.5)]];
        assert_eq!(parse(text.as_bytes()), Ok(want.clone()), "the UTF-8 form");
        for (name, bytes) in [
            ("UTF-16LE", utf_16([0xff, 0xfe], text, u16::to_le_bytes)),
            ("UTF-16BE", utf_16([0xfe, 0xff], text, u16::to_be_bytes)),
        ] {
            assert_eq!(parse(&bytes), Ok(want.clone()), "{name}");
        }
        for mark in [[0xff, 0xfe], [0xfe, 0xff]] {
            assert_eq!(parse(&mark), Ok(Vec::new()), "{mark:02x?}");
        }
    }

    /// r3 U11.10 (b1): a file with a mark of UTF-16 whose rest is not UTF-16 stays refused with
    /// `data_encoding`: an odd count of bytes, a lone surrogate, and the mark of UTF-32LE.
    #[test]
    fn a_utf_16_file_out_of_its_form_stays_refused_with_data_encoding() {
        let mut odd = utf_16([0xff, 0xfe], "a,1\r\n", u16::to_le_bytes);
        odd.push(b'2');
        let mut odd_be = utf_16([0xfe, 0xff], "a,1\r\n", u16::to_be_bytes);
        odd_be.push(b'2');
        let mut high = vec![0xff, 0xfe, 0x00, 0xd8];
        high.extend(utf_16([0xff, 0xfe], ",1\r\n", u16::to_le_bytes).split_off(2));
        let mut low = vec![0xfe, 0xff, 0xdc, 0x00];
        low.extend(utf_16([0xfe, 0xff], ",1\r\n", u16::to_be_bytes).split_off(2));
        let last = utf_16([0xff, 0xfe], "a,1\r\n\u{1f600}", u16::to_le_bytes);
        let last = last[..last.len() - 2].to_vec();
        let mut utf_32 = vec![0xff, 0xfe, 0x00, 0x00];
        for c in "a,1\r\n".chars() {
            utf_32.extend_from_slice(&u32::from(c).to_le_bytes());
        }
        let cases: [(&str, Vec<u8>); 6] = [
            ("an odd count, LE", odd),
            ("an odd count, BE", odd_be),
            ("a lone high surrogate, LE", high),
            ("a lone low surrogate, BE", low),
            ("a high surrogate at the end, LE", last),
            ("the mark of UTF-32LE", utf_32),
        ];
        for (name, bytes) in cases {
            let got = parse(&bytes).map_err(|e| (e.source_cause(), e));
            let want = Err((crate::Cause::DataEncoding, encoding()));
            assert_eq!(got, want, "{name}: {}", shown(&bytes));
        }
    }

    /// r3 U11.10 (b2), the coordinator's ruling W2: a file with no mark that is not UTF-8 reads by
    /// the code page that Windows gives as the computer's own, where codepage maps that page to an
    /// encoding that reads every byte of the file as a letter and no byte as a control sign (the
    /// guard P1: below U+0020 but the tab, CR and LF, and U+007F to U+009F); every other such file
    /// stays refused with `data_encoding`, and so does every such file where no page is given, as
    /// on the Mac. A file in UTF-8 or in UTF-16 by its mark reads as such under any page.
    #[test]
    fn a_file_that_is_not_utf_8_reads_by_the_page_that_the_computer_gives() {
        // The case, the file's bytes, the page and the rows that it reads to.
        type Read<'a> = (&'a str, &'a [u8], u16, Vec<Vec<Value>>);
        let read: [Read; 4] = [
            (
                "1252: u-umlaut and the euro sign",
                b"M\xfcller,12.50 \x80\r\n",
                1252,
                vec![vec![t("Müller"), n(12.5)]],
            ),
            (
                "1250",
                b"\xa3\xf3d\x9f,3\r\n",
                1250,
                vec![vec![t("Łódź"), n(3.0)]],
            ),
            ("932", b"\x95\x5c,1\r\n", 932, vec![vec![t("表"), n(1.0)]]),
            (
                "1251",
                b"\xc3\xe0\xe7;41,20\r\n",
                1251,
                vec![vec![t("Газ"), n(41.2)]],
            ),
        ];
        for (name, bytes, page, rows) in read {
            let want = Ok(Data {
                rows,
                letters: Letters::Page(page),
            });
            assert_eq!(data(bytes, Some(page)), want, "{name}");
        }
        let refused: [(&str, &[u8], Option<u16>); 6] = [
            (
                "the guard: the byte 81 under 1252",
                b"M\x81ller,1\r\n",
                Some(1252),
            ),
            (
                "the guard: a NUL under 1252",
                b"M\xfcller,1\x00\r\n",
                Some(1252),
            ),
            ("an invalid pair of 932", b"\x81\x20,1\r\n", Some(932)),
            (
                "a page that codepage does not map (437)",
                b"M\x81ller,1\r\n",
                Some(437),
            ),
            (
                "the page 65001, Windows' UTF-8",
                b"M\xfcller,1\r\n",
                Some(65001),
            ),
            ("no page, as on the Mac", b"M\xfcller,1\r\n", None),
        ];
        for (name, bytes, page) in refused {
            assert_eq!(data(bytes, page), Err(encoding()), "{name}");
        }
        let utf_8 = "Müller,1\r\n".as_bytes();
        let le = utf_16([0xff, 0xfe], "Müller,1\r\n", u16::to_le_bytes);
        let want = vec![vec![t("Müller"), n(1.0)]];
        for (name, bytes, letters) in [
            ("UTF-8", utf_8.to_vec(), Letters::Utf8),
            ("UTF-16LE", le, Letters::Utf16),
        ] {
            let got = data(&bytes, Some(1252));
            let want = Ok(Data {
                rows: want.clone(),
                letters,
            });
            assert_eq!(got, want, "{name} under 1252");
        }
    }

    /// r3 U11.10, the coordinator's P4, on Windows alone: `system_page` asks the system for its
    /// page (`GetACP`), and the file that Excel's CSV (Comma delimited) would write with u-umlaut
    /// and sharp s reads by it, as the product's `Folder::read_data` reads it; the hosted runner's
    /// page is 1252, where the cell reads `Grüße`.
    #[cfg(windows)]
    #[test]
    fn the_page_that_windows_gives_reads_a_file_that_is_not_utf_8() {
        let page = system_page().expect("Windows gives the computer's page");
        let encoding_of_page = codepage::to_encoding(page).expect("codepage maps the page");
        let (word, _, _) = encoding_of_page.decode(b"Gr\xfc\xdfe");
        if page == 1252 {
            assert_eq!(word, "Grüße");
        }
        let want = Ok(Data {
            rows: vec![vec![t(&word), n(12.5)]],
            letters: Letters::Page(page),
        });
        assert_eq!(
            data(b"Gr\xfc\xdfe,12.50\r\n", system_page()),
            want,
            "page {page}"
        );
    }

    /// The file of the reader of r3 U11.10 (verifier-158): two rows of three cells in which the one
    /// byte 80 is the euro sign under most pages of Windows and a letter under 1251, where the
    /// separator of the file hung on it (the coordinator's rule of log line 23651).
    const READERS_FILE: &[u8] = b"41,20 \x80; Strom,Gas; x\r\n12,50 \x80; Wasser,Abwasser; y\r\n";

    /// The ANSI pages that Windows gives as a computer's own, the list of the rule.
    const THE_PAGES: [u16; 14] = [
        874, 932, 936, 949, 950, 1250, 1251, 1252, 1253, 1254, 1255, 1256, 1257, 1258,
    ];

    /// The numbers of `rows`, in order.
    fn numbers(rows: &[Vec<Value>]) -> Vec<f64> {
        rows.iter()
            .flatten()
            .filter_map(|v| match v {
                Value::Number(x) => Some(*x),
                Value::Text(_) => None,
            })
            .collect()
    }

    /// r3 U11.10, the reader's case: under 1251 beside 1252 the reader's file holds no number but
    /// 41.2 and 12.5; a text, or a refusal with `data_encoding`, passes.
    #[test]
    fn the_readers_file_holds_no_other_number_under_1251_beside_1252() {
        for page in [1251, 1252] {
            match data(READERS_FILE, Some(page)) {
                Err(error) => assert_eq!(error, encoding(), "{page}"),
                Ok(read) => {
                    let got = numbers(&read.rows);
                    let other: Vec<&f64> =
                        got.iter().filter(|x| ![41.2, 12.5].contains(*x)).collect();
                    assert!(other.is_empty(), "{page}: {:?}", read.rows);
                }
            }
        }
    }

    /// r3 U11.10, part (3) of the coordinator's rule (log line 23651): under each page of the list
    /// the reader's file reads cell for cell as its twin reads, the same text saved as UTF-8, for
    /// every page that reads its bytes (the same rows and cells, and no cell a number of another
    /// value), or it is refused with `data_encoding`.
    #[test]
    fn under_each_page_a_file_reads_as_its_twins_in_utf_8_or_is_refused() {
        let mut wrong = Vec::new();
        for own in THE_PAGES {
            let read = match data(READERS_FILE, Some(own)) {
                Ok(read) => read.rows,
                Err(error) if error == encoding() => continue,
                Err(error) => {
                    wrong.push(format!("{own}: {error:?}"));
                    continue;
                }
            };
            for page in THE_PAGES {
                let Ok(text) = by_page(READERS_FILE, page) else {
                    continue;
                };
                let twin = parse(text.as_bytes());
                let alike = twin.as_ref().is_ok_and(|twin| {
                    twin.len() == read.len()
                        && twin.iter().zip(&read).all(|(a, b)| {
                            a.len() == b.len()
                                && a.iter().zip(b).all(|pair| match pair {
                                    (Value::Number(x), Value::Number(y)) => {
                                        x.to_bits() == y.to_bits()
                                    }
                                    _ => true,
                                })
                        })
                });
                if !alike {
                    wrong.push(format!(
                        "{own} beside the twin of {page}: {read:?} and {twin:?}"
                    ));
                }
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }
}
