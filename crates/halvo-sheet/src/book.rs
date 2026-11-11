use std::collections::{BTreeMap, BTreeSet};
use std::io::Cursor;
use std::panic::{catch_unwind, AssertUnwindSafe};

use calamine::{Data, Reader, Xlsx};

use crate::error::{Error, Refusal};
use crate::parts;
use crate::range::Range;
use crate::source::Value;

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

type Pos = (u32, u32);
type Sheet = (String, BTreeMap<Pos, (Data, Option<String>)>);

fn is_empty(value: &Data) -> bool {
    match value {
        Data::Empty => true,
        Data::String(text) => text.is_empty(),
        _ => false,
    }
}

fn sheets(bytes: &[u8]) -> Result<Vec<Sheet>, Error> {
    let mut xlsx = open(bytes)?;
    let mut sheets = Vec::new();
    for name in xlsx.sheet_names() {
        let values = xlsx.worksheet_range(&name).map_err(|_| unreadable())?;
        let formulas = xlsx.worksheet_formula(&name).map_err(|_| unreadable())?;
        let mut cells = BTreeMap::new();
        if let Some((row0, col0)) = values.start() {
            for (row, col, value) in values.used_cells() {
                if !is_empty(value) {
                    let pos = (row0 + row as u32, col0 + col as u32);
                    cells.insert(pos, (value.clone(), None));
                }
            }
        }
        if let Some((row0, col0)) = formulas.start() {
            for (row, col, formula) in formulas.used_cells() {
                if !formula.is_empty() {
                    let pos = (row0 + row as u32, col0 + col as u32);
                    cells.entry(pos).or_insert((Data::Empty, None)).1 = Some(formula.clone());
                }
            }
        }
        sheets.push((name, cells));
    }
    Ok(sheets)
}

fn holds_new(value: &Value, cell: Option<&(Data, Option<String>)>) -> bool {
    match (value, cell) {
        (Value::Number(x), Some((Data::Float(y), None))) => x == y,
        (Value::Number(x), Some((Data::Int(y), None))) => *x == *y as f64,
        (Value::Text(text), Some((Data::String(read), None))) => text == read,
        (Value::Text(text), None) => text.is_empty(),
        _ => false,
    }
}

fn kept(before: Option<&(Data, Option<String>)>, after: Option<&(Data, Option<String>)>) -> bool {
    match (before, after) {
        (Some((_, Some(formula))), Some((_, read))) => read.as_ref() == Some(formula),
        (Some((value, None)), Some((read, None))) => value == read,
        (None, None) => true,
        _ => false,
    }
}

pub(crate) fn same(
    original: &[u8],
    changed: &[u8],
    sheet: &str,
    range: Range,
    values: &[Vec<Value>],
) -> Result<bool, Error> {
    let before = sheets(original)?;
    let after = sheets(changed)?;
    let names = |book: &[Sheet]| {
        book.iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>()
    };
    if names(&before) != names(&after) {
        return Ok(false);
    }
    for ((name, old), (_, new)) in before.iter().zip(&after) {
        let mut positions: BTreeSet<Pos> = old.keys().chain(new.keys()).copied().collect();
        if name == sheet {
            positions.extend(range.cells());
        }
        for pos in positions {
            let same = if name == sheet && range.holds(pos) {
                let row = (pos.0 - range.first.0) as usize;
                let col = (pos.1 - range.first.1) as usize;
                holds_new(&values[row][col], new.get(&pos))
            } else {
                kept(old.get(&pos), new.get(&pos))
            };
            if !same {
                return Ok(false);
            }
        }
    }
    let arrays = |bytes: &[u8]| parts::entries(bytes).and_then(|all| parts::array_formulas(&all));
    Ok(arrays(original)? == arrays(changed)?)
}
