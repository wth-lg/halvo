use std::collections::{BTreeMap, BTreeSet};

use quick_xml::events::Event;
use std::path::Path;

use calamine::{open_workbook, Data, Reader, Xlsx};

pub type Pos = (u32, u32);

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub value: Data,
    pub formula: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    pub sheets: Vec<(String, BTreeMap<Pos, Cell>)>,
}

impl Book {
    pub fn sheet(&self, name: &str) -> Option<&BTreeMap<Pos, Cell>> {
        self.sheets
            .iter()
            .find(|(sheet, _)| sheet == name)
            .map(|(_, cells)| cells)
    }

    pub fn cell(&self, sheet: &str, pos: Pos) -> Option<&Cell> {
        self.sheet(sheet).and_then(|cells| cells.get(&pos))
    }
}

pub fn parse_ref(cell: &str) -> Option<Pos> {
    let letters: String = cell
        .chars()
        .take_while(|c| c.is_ascii_uppercase())
        .collect();
    let digits = &cell[letters.len()..];
    if letters.is_empty() || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let col = letters
        .bytes()
        .fold(0u32, |acc, b| acc * 26 + u32::from(b - b'A' + 1));
    let row: u32 = digits.parse().ok()?;
    if row == 0 {
        return None;
    }
    Some((row - 1, col - 1))
}

pub fn cell_ref(pos: Pos) -> String {
    let mut col = pos.1 + 1;
    let mut letters = Vec::new();
    while col > 0 {
        let rem = (col - 1) % 26;
        letters.push(b'A' + rem as u8);
        col = (col - 1) / 26;
    }
    letters.reverse();
    format!("{}{}", String::from_utf8_lossy(&letters), pos.0 + 1)
}

pub fn read(path: &Path) -> Result<Book, String> {
    let mut workbook: Xlsx<_> = open_workbook(path).map_err(|e| format!("calamine: {e}"))?;
    let mut book = Book::default();
    for name in workbook.sheet_names() {
        let values = workbook
            .worksheet_range(&name)
            .map_err(|e| format!("calamine {name}: {e}"))?;
        let formulas = workbook
            .worksheet_formula(&name)
            .map_err(|e| format!("calamine {name}: {e}"))?;
        let mut cells: BTreeMap<Pos, Cell> = BTreeMap::new();
        if let Some((row0, col0)) = values.start() {
            for (row, col, value) in values.used_cells() {
                if is_empty(value) {
                    continue;
                }
                cells.insert(
                    (row0 + row as u32, col0 + col as u32),
                    Cell {
                        value: value.clone(),
                        formula: None,
                    },
                );
            }
        }
        if let Some((row0, col0)) = formulas.start() {
            for (row, col, formula) in formulas.used_cells() {
                if formula.is_empty() {
                    continue;
                }
                cells
                    .entry((row0 + row as u32, col0 + col as u32))
                    .or_insert(Cell {
                        value: Data::Empty,
                        formula: None,
                    })
                    .formula = Some(formula.clone());
            }
        }
        book.sheets.push((name, cells));
    }
    Ok(book)
}

pub fn is_empty(value: &Data) -> bool {
    match value {
        Data::Empty => true,
        Data::String(text) => text.is_empty(),
        _ => false,
    }
}

pub fn same_except(original: &Book, saved: &Book, sheet: &str, target: Pos) -> bool {
    let names =
        |book: &Book| -> Vec<String> { book.sheets.iter().map(|(name, _)| name.clone()).collect() };
    if names(original) != names(saved) {
        return false;
    }
    original
        .sheets
        .iter()
        .zip(&saved.sheets)
        .all(|((name, before), (_, after))| {
            let positions: std::collections::BTreeSet<&Pos> =
                before.keys().chain(after.keys()).collect();
            positions
                .into_iter()
                .filter(|pos| !(name == sheet && **pos == target))
                .all(|pos| match (before.get(pos), after.get(pos)) {
                    (Some(old), Some(new)) => match &old.formula {
                        Some(text) => new.formula.as_ref() == Some(text),
                        None => new.formula.is_none() && new.value == old.value,
                    },
                    _ => false,
                })
        })
}

pub fn number(value: &Data) -> Option<f64> {
    match value {
        Data::Float(x) => Some(*x),
        Data::Int(x) => Some(*x as f64),
        _ => None,
    }
}

pub fn array_formulas(path: &Path) -> Result<BTreeSet<(String, String, String)>, String> {
    let entries = crate::parts::read_entries(path)?;
    let mut found = BTreeSet::new();
    for (sheet, part) in crate::parts::sheet_parts(&entries) {
        let Some(xml) = entries.get(&part) else {
            continue;
        };
        let mut reader = crate::parts::new_reader(xml);
        let mut buf = Vec::new();
        let mut cell = String::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Eof) => break,
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let attr = |key: &str| {
                        e.attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == key)
                            .map(|a| a.value.to_string())
                    };
                    match crate::parts::local(e.name().as_ref()) {
                        "c" => cell = attr("r").unwrap_or_default(),
                        "f" if attr("t").as_deref() == Some("array") => {
                            found.insert((
                                sheet.clone(),
                                cell.clone(),
                                attr("ref").unwrap_or_default(),
                            ));
                        }
                        _ => {}
                    }
                }
                Ok(_) => {}
                Err(e) => return Err(format!("xml {part}: {e}")),
            }
            buf.clear();
        }
    }
    Ok(found)
}
