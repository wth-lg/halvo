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
        let row = line
            .split(',')
            .map(|cell| value(cell, line_number))
            .collect::<Result<Vec<_>, _>>()?;
        if rows.first().is_some_and(|first| first.len() != row.len()) {
            return Err(Error::Source(SourceRefusal::Ragged { line: line_number }));
        }
        rows.push(row);
    }
    Ok(rows)
}

fn value(cell: &str, line: usize) -> Result<Value, Error> {
    if !is_number(cell) {
        return Ok(Value::Text(cell.to_string()));
    }
    match cell.parse::<f64>() {
        Ok(x) if x.is_finite() => Ok(Value::Number(x)),
        _ => Err(Error::Source(SourceRefusal::Number { line })),
    }
}

fn is_number(cell: &str) -> bool {
    let body = cell.strip_prefix('-').unwrap_or(cell);
    let digits = body.bytes().filter(u8::is_ascii_digit).count();
    let dots = body.bytes().filter(|b| *b == b'.').count();
    digits > 0 && dots <= 1 && digits + dots == body.len()
}
