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
