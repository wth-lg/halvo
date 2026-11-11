use std::io::Cursor;
use std::panic::{catch_unwind, AssertUnwindSafe};

use calamine::{Data, Reader, Xlsx};

use crate::error::{Error, Refusal};
use crate::range::Range;

/// The value of a cell as [`crate::Folder::read`] returns it.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    /// The cell holds nothing.
    Empty,
    /// A number; a date or a time is its serial number.
    Number(f64),
    /// A text.
    Text(String),
    /// A boolean.
    Bool(bool),
    /// An error value such as `#DIV/0!`.
    Error,
}

pub(crate) fn unreadable() -> Error {
    Error::Workbook(Refusal::Unreadable)
}

pub(crate) fn guarded<T>(work: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or_else(|_| Err(unreadable()))
}

pub(crate) fn open(bytes: &[u8]) -> Result<Xlsx<Cursor<&[u8]>>, Error> {
    Xlsx::new(Cursor::new(bytes)).map_err(|_| unreadable())
}

pub(crate) fn read(bytes: &[u8], sheet: &str, range: Range) -> Result<Vec<Vec<Cell>>, Error> {
    guarded(|| {
        let mut xlsx = open(bytes)?;
        if !xlsx.sheet_names().iter().any(|name| name == sheet) {
            return Err(Error::NoSheet);
        }
        let values = xlsx.worksheet_range(sheet).map_err(|_| unreadable())?;
        let mut rows = Vec::new();
        for row in range.first.0..=range.last.0 {
            let cells = (range.first.1..=range.last.1)
                .map(|col| cell(values.get_value((row, col))))
                .collect();
            rows.push(cells);
        }
        Ok(rows)
    })
}

fn cell(value: Option<&Data>) -> Cell {
    match value {
        None | Some(Data::Empty) => Cell::Empty,
        Some(Data::Float(x)) => Cell::Number(*x),
        Some(Data::Int(x)) => Cell::Number(*x as f64),
        Some(Data::String(text))
        | Some(Data::DateTimeIso(text))
        | Some(Data::DurationIso(text)) => Cell::Text(text.clone()),
        Some(Data::Bool(b)) => Cell::Bool(*b),
        Some(Data::DateTime(date)) => Cell::Number(date.as_f64()),
        Some(Data::Error(_)) => Cell::Error,
    }
}
