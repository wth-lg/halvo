use std::io::Cursor;
use std::panic::{catch_unwind, AssertUnwindSafe};

use calamine::{Data, Reader, Xlsx};

use crate::error::{Error, Refusal};
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

type Grids = (calamine::Range<Data>, calamine::Range<String>);

/// The values and the formulas of the sheet named `sheet`, and of no other sheet.
fn named_sheet(bytes: &[u8], sheet: &str) -> Result<Grids, Error> {
    let mut xlsx = open(bytes)?;
    if !xlsx.sheet_names().iter().any(|name| name == sheet) {
        return Err(Error::NoSheet);
    }
    let values = xlsx.worksheet_range(sheet).map_err(|_| unreadable())?;
    let formulas = xlsx.worksheet_formula(sheet).map_err(|_| unreadable())?;
    Ok((values, formulas))
}

/// Whether the reader reads the sheet named `sheet` of the source as the read-back will read
/// it of the copy; [`Refusal::Unreadable`] when it cannot.
pub(crate) fn readable(bytes: &[u8], sheet: &str) -> Result<(), Error> {
    guarded(|| named_sheet(bytes, sheet).map(|_| ())).map_err(|_| unreadable())
}

fn holds_new(value: &Value, cell: Option<&Data>) -> bool {
    match (value, cell) {
        (Value::Number(x), Some(Data::Float(y))) => x == y,
        (Value::Number(x), Some(Data::Int(y))) => *x == *y as f64,
        (Value::Number(x), Some(Data::DateTime(date))) => *x == date.as_f64(),
        (Value::Text(text), Some(Data::String(read))) => text == read,
        (Value::Text(text), None | Some(Data::Empty)) => text.is_empty(),
        _ => false,
    }
}

/// Whether every cell of `range` on the sheet named `sheet` of the copy reads back with its
/// value and holds no formula. The reader reads that sheet alone.
pub(crate) fn holds(
    changed: &[u8],
    sheet: &str,
    range: Range,
    values: &[Vec<Value>],
) -> Result<bool, Error> {
    let (cells, formulas) = named_sheet(changed, sheet)?;
    Ok(range
        .cells()
        .zip(values.iter().flatten())
        .all(|(pos, value)| {
            let formula = formulas.get_value(pos).is_some_and(|f| !f.is_empty());
            !formula && holds_new(value, cells.get_value(pos))
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus, edit, zipped};

    const SHEET: &str = "xl/worksheets/sheet1.xml";

    fn bytes(file: &str, edits: &[(&str, &str, &str)]) -> Vec<u8> {
        let mut all = corpus(file);
        for (part, from, to) in edits {
            edit(&mut all, part, from, to);
        }
        zipped(&all)
    }

    fn at(cell: &str) -> Range {
        Range::parse(cell).expect("a range")
    }

    fn one(value: Value) -> Vec<Vec<Value>> {
        vec![vec![value]]
    }

    #[test]
    fn the_cells_of_the_range_read_back_with_their_new_values() {
        let original = bytes("values.xlsx", &[]);
        let b5 = [(
            SHEET,
            r#"<c r="B5"><v>12</v></c>"#,
            r#"<c r="B5"><v>77</v></c>"#,
        )];
        let changed = bytes("values.xlsx", &b5);
        let read =
            |bytes: &[u8], sheet: &str, value: Value| holds(bytes, sheet, at("B5"), &one(value));
        assert_eq!(read(&changed, "Values", Value::Number(77.0)), Ok(true));
        assert_eq!(read(&changed, "Values", Value::Number(78.0)), Ok(false));
        assert_eq!(
            read(&changed, "Values", Value::Text("77".to_string())),
            Ok(false)
        );
        assert_eq!(read(&original, "Values", Value::Number(77.0)), Ok(false));
        assert_eq!(
            read(&changed, "Missing", Value::Number(77.0)),
            Err(Error::NoSheet)
        );
    }

    #[test]
    fn a_named_cell_that_holds_a_formula_does_not_read_back_and_another_sheet_is_not_read() {
        let formula = [(
            SHEET,
            r#"<c r="B5"><v>12</v></c>"#,
            r#"<c r="B5"><f>70+7</f><v>77</v></c>"#,
        )];
        let changed = bytes("values.xlsx", &formula);
        assert_eq!(
            holds(&changed, "Values", at("B5"), &one(Value::Number(77.0))),
            Ok(false)
        );
        let broken = [("xl/worksheets/sheet2.xml", r#"<c r="B2""#, r#"<c r="B""#)];
        let two = bytes("two-sheets.xlsx", &broken);
        assert_eq!(readable(&two, "Input"), Ok(()));
        assert_eq!(readable(&two, "Summary"), Err(unreadable()));
        assert_eq!(readable(&two, "Missing"), Err(unreadable()));
    }

    #[test]
    fn an_empty_text_in_the_range_is_a_cell_that_holds_nothing() {
        let original = bytes("values.xlsx", &[]);
        let empty = one(Value::Text(String::new()));
        assert_eq!(holds(&original, "Values", at("F2"), &empty), Ok(true));
        let text = one(Value::Text("x".to_string()));
        assert_eq!(holds(&original, "Values", at("F2"), &text), Ok(false));
    }

    #[test]
    fn a_number_in_a_date_formatted_cell_reads_back_as_its_serial() {
        let original = bytes("formats.xlsx", &[]);
        let read = |x: f64| holds(&original, "Formats", at("C2"), &one(Value::Number(x)));
        assert_eq!(read(46023.0), Ok(true));
        assert_eq!(read(46024.0), Ok(false));
    }
}
