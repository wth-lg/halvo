use std::path::Path;

use calamine::{open_workbook, Data, Dimensions, Reader, Xlsx};
use rust_xlsxwriter::{Format, Formula, Table, TableColumn, Workbook};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Candidate {
    Umya,
    Rewrite,
}

impl Candidate {
    pub const ALL: [Candidate; 2] = [Candidate::Umya, Candidate::Rewrite];

    pub fn name(self) -> &'static str {
        match self {
            Candidate::Umya => "umya",
            Candidate::Rewrite => "rewrite",
        }
    }

    pub fn parse(name: &str) -> Option<Candidate> {
        Candidate::ALL.into_iter().find(|c| c.name() == name)
    }

    pub fn change(self, path: &Path, sheet: &str, cell: &str, value: f64) -> Result<(), String> {
        match self {
            Candidate::Umya => umya(path, sheet, cell, value),
            Candidate::Rewrite => rewrite(path, sheet, cell, value),
        }
    }
}

fn umya(path: &Path, sheet: &str, cell: &str, value: f64) -> Result<(), String> {
    let mut book =
        umya_spreadsheet::reader::xlsx::read(path).map_err(|e| format!("umya read: {e}"))?;
    book.sheet_by_name_mut(sheet)
        .map_err(|e| format!("umya sheet {sheet}: {e}"))?
        .cell_mut(cell)
        .set_value_number(value);
    umya_spreadsheet::writer::xlsx::write(&book, path).map_err(|e| format!("umya write: {e}"))
}

struct SheetCopy {
    name: String,
    values: Vec<((u32, u32), Data)>,
    formulas: Vec<((u32, u32), String)>,
    merges: Vec<Dimensions>,
    tables: Vec<TableCopy>,
}

struct TableCopy {
    name: String,
    columns: Vec<String>,
    first: (u32, u32),
    last: (u32, u32),
}

struct BookCopy {
    sheets: Vec<SheetCopy>,
    names: Vec<(String, String)>,
}

fn absolute<T>(range: &calamine::Range<T>) -> Vec<((u32, u32), T)>
where
    T: calamine::CellType + Clone,
{
    let Some((row0, col0)) = range.start() else {
        return Vec::new();
    };
    range
        .used_cells()
        .map(|(row, col, value)| ((row0 + row as u32, col0 + col as u32), value.clone()))
        .collect()
}

fn cached_text(value: Option<&Data>) -> String {
    match value {
        Some(Data::Float(x)) => format!("{x}"),
        Some(Data::Int(x)) => format!("{x}"),
        Some(Data::Bool(b)) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Some(Data::String(s)) => s.clone(),
        Some(Data::DateTime(d)) => format!("{}", d.as_f64()),
        _ => String::new(),
    }
}

fn read_copy(path: &Path) -> Result<BookCopy, String> {
    let mut book: Xlsx<_> = open_workbook(path).map_err(|e| format!("calamine: {e}"))?;
    book.load_tables()
        .map_err(|e| format!("calamine tables: {e}"))?;
    let names = book.sheet_names();
    let mut sheets = Vec::new();
    for name in names {
        let values = absolute(
            &book
                .worksheet_range(&name)
                .map_err(|e| format!("calamine {name}: {e}"))?,
        );
        let formulas = absolute(
            &book
                .worksheet_formula(&name)
                .map_err(|e| format!("calamine {name}: {e}"))?,
        );
        let merges = book
            .merge_cells_by_sheet_name(&name)
            .map_err(|e| format!("calamine merges {name}: {e}"))?;
        let table_names: Vec<String> = book
            .table_names_in_sheet(&name)
            .into_iter()
            .cloned()
            .collect();
        let mut tables = Vec::new();
        for table_name in table_names {
            let table = book
                .table_by_name(&table_name)
                .map_err(|e| format!("calamine table {table_name}: {e}"))?;
            let data = table.data();
            let (Some(start), Some(end)) = (data.start(), data.end()) else {
                continue;
            };
            let header = (start.0.saturating_sub(1), start.1);
            tables.push(TableCopy {
                name: table_name,
                columns: table.columns().to_vec(),
                first: header,
                last: end,
            });
        }
        sheets.push(SheetCopy {
            name,
            values,
            formulas: formulas
                .into_iter()
                .filter(|(_, f)| !f.is_empty())
                .collect(),
            merges,
            tables,
        });
    }
    let names = book.defined_names().to_vec();
    Ok(BookCopy { sheets, names })
}

fn rewrite(path: &Path, sheet: &str, cell: &str, value: f64) -> Result<(), String> {
    let target = crate::book::parse_ref(cell).ok_or_else(|| format!("bad cell {cell}"))?;
    let BookCopy { sheets, names } = read_copy(path)?;
    let date = Format::new().set_num_format("yyyy-mm-dd");
    let plain = Format::new();
    let mut workbook = Workbook::new();
    for copy in &sheets {
        let ws = workbook
            .add_worksheet()
            .set_name(&copy.name)
            .map_err(|e| format!("xlsxwriter: {e}"))?;
        let err = |e: rust_xlsxwriter::XlsxError| format!("xlsxwriter {}: {e}", copy.name);
        for TableCopy {
            name,
            columns,
            first: (r0, c0),
            last: (r1, c1),
        } in &copy.tables
        {
            let columns: Vec<TableColumn> = columns
                .iter()
                .map(|header| TableColumn::new().set_header(header))
                .collect();
            let table = Table::new().set_name(name).set_columns(&columns);
            ws.add_table(*r0, *c0 as u16, *r1, *c1 as u16, &table)
                .map_err(err)?;
        }
        for merge in &copy.merges {
            ws.merge_range(
                merge.start.0,
                merge.start.1 as u16,
                merge.end.0,
                merge.end.1 as u16,
                "",
                &plain,
            )
            .map_err(err)?;
        }
        let formula_at: std::collections::BTreeMap<_, _> = copy.formulas.iter().cloned().collect();
        for ((row, col), data) in &copy.values {
            if formula_at.contains_key(&(*row, *col)) {
                continue;
            }
            let (row, col) = (*row, *col as u16);
            match data {
                Data::Empty => {}
                Data::String(s) => {
                    ws.write_string(row, col, s).map_err(err)?;
                }
                Data::Float(x) => {
                    ws.write_number(row, col, *x).map_err(err)?;
                }
                Data::Int(x) => {
                    ws.write_number(row, col, *x as f64).map_err(err)?;
                }
                Data::Bool(b) => {
                    ws.write_boolean(row, col, *b).map_err(err)?;
                }
                Data::DateTime(d) => {
                    ws.write_number_with_format(row, col, d.as_f64(), &date)
                        .map_err(err)?;
                }
                Data::DateTimeIso(s) | Data::DurationIso(s) => {
                    ws.write_string(row, col, s).map_err(err)?;
                }
                Data::Error(e) => {
                    ws.write_string(row, col, e.to_string()).map_err(err)?;
                }
            }
        }
        let values_at: std::collections::BTreeMap<_, _> = copy.values.iter().cloned().collect();
        for ((row, col), text) in &copy.formulas {
            let formula = Formula::new(format!("={text}"))
                .set_result(cached_text(values_at.get(&(*row, *col))));
            ws.write_formula(*row, *col as u16, formula).map_err(err)?;
        }
        if copy.name == sheet {
            ws.write_number(target.0, target.1 as u16, value)
                .map_err(err)?;
        }
    }
    for (name, formula) in &names {
        workbook
            .define_name(name, &format!("={formula}"))
            .map_err(|e| format!("xlsxwriter name {name}: {e}"))?;
    }
    workbook
        .save(path)
        .map_err(|e| format!("xlsxwriter save: {e}"))
}
