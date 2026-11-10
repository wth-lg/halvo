use std::path::Path;

use rust_xlsxwriter::{
    Chart, ChartType, Color, DataValidation, DataValidationRule, DocProperties, ExcelDateTime,
    Format, Formula, Note, Table, TableColumn, Workbook, XlsxError,
};

pub const FILES: [&str; 8] = [
    "values.xlsx",
    "formulas.xlsx",
    "formats.xlsx",
    "merged.xlsx",
    "validation.xlsx",
    "chart.xlsx",
    "two-sheets.xlsx",
    "rows-5000.xlsx",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Dependent {
    pub sheet: &'static str,
    pub cell: &'static str,
    pub before: f64,
    pub after: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub file: &'static str,
    pub sheet: &'static str,
    pub cell: &'static str,
    pub before: Option<f64>,
    pub new: f64,
    pub dependents: Vec<Dependent>,
}

fn value_count(n: u32) -> f64 {
    f64::from(n * 3)
}

fn ledger_amounts() -> [Option<f64>; 5] {
    [None; 5]
}

const SALES: [f64; 6] = [120.0, 135.0, 150.0, 110.0, 160.0, 170.0];
const MONTHS: [&str; 6] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun"];
const INPUTS: [f64; 4] = [10.0, 20.0, 30.0, 40.0];
const RATE: f64 = 3.0;
const ROWS: u32 = 5000;

fn row_amount(id: u32) -> f64 {
    f64::from((id * 7) % 1000)
}

fn replaced(values: &[f64], index: usize, new: f64) -> f64 {
    values
        .iter()
        .enumerate()
        .map(|(i, v)| if i == index { new } else { *v })
        .sum()
}

pub fn specs() -> Vec<Spec> {
    let ledger: Vec<f64> = ledger_amounts().iter().map(|v| v.unwrap_or(0.0)).collect();
    let rows: Vec<f64> = (1..=ROWS).map(row_amount).collect();
    let input_total: f64 = INPUTS.iter().sum();
    let input_after = replaced(&INPUTS, 1, 25.0);
    vec![
        Spec {
            file: "values.xlsx",
            sheet: "Values",
            cell: "B5",
            before: Some(value_count(4)),
            new: 77.0,
            dependents: vec![],
        },
        Spec {
            file: "formulas.xlsx",
            sheet: "Ledger",
            cell: "D2",
            before: None,
            new: 42.0,
            dependents: vec![Dependent {
                sheet: "Ledger",
                cell: "D7",
                before: ledger.iter().sum(),
                after: replaced(&ledger, 0, 42.0),
            }],
        },
        Spec {
            file: "formats.xlsx",
            sheet: "Formats",
            cell: "B3",
            before: Some(format_amount(2)),
            new: 2500.5,
            dependents: vec![],
        },
        Spec {
            file: "merged.xlsx",
            sheet: "Merged",
            cell: "B4",
            before: Some(merged_value(2, 1)),
            new: 99.0,
            dependents: vec![],
        },
        Spec {
            file: "validation.xlsx",
            sheet: "Validation",
            cell: "B3",
            before: Some(2.0),
            new: 7.0,
            dependents: vec![],
        },
        Spec {
            file: "chart.xlsx",
            sheet: "Sales",
            cell: "B3",
            before: Some(SALES[1]),
            new: 640.0,
            dependents: vec![Dependent {
                sheet: "Sales",
                cell: "B9",
                before: SALES.iter().sum(),
                after: replaced(&SALES, 1, 640.0),
            }],
        },
        Spec {
            file: "two-sheets.xlsx",
            sheet: "Input",
            cell: "B3",
            before: Some(INPUTS[1]),
            new: 25.0,
            dependents: vec![
                Dependent {
                    sheet: "Summary",
                    cell: "B1",
                    before: input_total,
                    after: input_after,
                },
                Dependent {
                    sheet: "Summary",
                    cell: "B2",
                    before: input_total * RATE,
                    after: input_after * RATE,
                },
            ],
        },
        Spec {
            file: "rows-5000.xlsx",
            sheet: "Rows",
            cell: "C2501",
            before: Some(row_amount(2500)),
            new: 123456.0,
            dependents: vec![Dependent {
                sheet: "Rows",
                cell: "F1",
                before: rows.iter().sum(),
                after: replaced(&rows, 2499, 123456.0),
            }],
        },
    ]
}

pub fn spec(file: &str) -> Option<Spec> {
    specs().into_iter().find(|spec| spec.file == file)
}

fn format_amount(n: u32) -> f64 {
    f64::from(n * 1000) + 0.25
}

fn merged_value(region: u32, quarter: u32) -> f64 {
    f64::from(region * 100 + quarter * 10)
}

fn result(value: f64) -> String {
    format!("{value}")
}

fn workbook() -> Result<Workbook, XlsxError> {
    let mut workbook = Workbook::new();
    let created = ExcelDateTime::from_ymd(2026, 1, 1)?;
    workbook.set_properties(&DocProperties::new().set_creation_datetime(&created));
    Ok(workbook)
}

fn values() -> Result<Workbook, XlsxError> {
    let mut workbook = workbook()?;
    let sheet = workbook.add_worksheet().set_name("Values")?;
    for (col, header) in ["Item", "Count", "Price", "Paid"].iter().enumerate() {
        sheet.write_string(0, col as u16, *header)?;
    }
    for n in 1..=20u32 {
        sheet.write_string(n, 0, format!("item-{n}"))?;
        sheet.write_number(n, 1, value_count(n))?;
        sheet.write_number(n, 2, f64::from(n) * 1.25)?;
        if n % 3 != 0 {
            sheet.write_boolean(n, 3, n % 2 == 0)?;
        }
    }
    Ok(workbook)
}

fn formulas() -> Result<Workbook, XlsxError> {
    let mut workbook = workbook()?;
    let sheet = workbook.add_worksheet().set_name("Ledger")?;
    for (col, header) in ["Id", "Date", "Payee", "Amount"].iter().enumerate() {
        sheet.write_string(0, col as u16, *header)?;
    }
    for n in 1..=5u32 {
        sheet.write_string(n, 0, format!("INV-{n}"))?;
    }
    let total: f64 = ledger_amounts().iter().map(|v| v.unwrap_or(0.0)).sum();
    sheet.write_formula(6, 3, Formula::new("=SUM(D2:D6)").set_result(result(total)))?;
    Ok(workbook)
}

fn formats() -> Result<Workbook, XlsxError> {
    let mut workbook = workbook()?;
    let header = Format::new()
        .set_bold()
        .set_background_color(Color::RGB(0xDDEEFF));
    let amount = Format::new().set_num_format("#,##0.00");
    let date = Format::new().set_num_format("yyyy-mm-dd");
    let share = Format::new().set_num_format("0.0%").set_italic();
    let sheet = workbook.add_worksheet().set_name("Formats")?;
    for (col, text) in ["Name", "Amount", "Due", "Share"].iter().enumerate() {
        sheet.write_string_with_format(0, col as u16, *text, &header)?;
    }
    for n in 1..=5u32 {
        sheet.write_string(n, 0, format!("row-{n}"))?;
        sheet.write_number_with_format(n, 1, format_amount(n), &amount)?;
        let due = ExcelDateTime::from_ymd(2026, n as u8, 1)?;
        sheet.write_datetime_with_format(n, 2, &due, &date)?;
        sheet.write_number_with_format(n, 3, f64::from(n) * 0.1, &share)?;
    }
    Ok(workbook)
}

fn merged() -> Result<Workbook, XlsxError> {
    let mut workbook = workbook()?;
    let centred = Format::new().set_align(rust_xlsxwriter::FormatAlign::Center);
    let sheet = workbook.add_worksheet().set_name("Merged")?;
    sheet.merge_range(0, 0, 0, 3, "Quarterly summary", &centred)?;
    for (col, text) in ["Region", "Q1", "Q2", "Q3"].iter().enumerate() {
        sheet.write_string(1, col as u16, *text)?;
    }
    for region in 1..=6u32 {
        sheet.write_string(region + 1, 0, format!("region-{region}"))?;
        for quarter in 1..=3u32 {
            sheet.write_number(region + 1, quarter as u16, merged_value(region, quarter))?;
        }
    }
    sheet.merge_range(9, 0, 9, 1, "Notes", &centred)?;
    Ok(workbook)
}

fn validation() -> Result<Workbook, XlsxError> {
    let mut workbook = workbook()?;
    let sheet = workbook.add_worksheet().set_name("Validation")?;
    for (col, text) in ["Task", "Hours", "Done"].iter().enumerate() {
        sheet.write_string(0, col as u16, *text)?;
    }
    for n in 1..=10u32 {
        sheet.write_string(n, 0, format!("task-{n}"))?;
        sheet.write_number(n, 1, f64::from(n))?;
        sheet.write_string(n, 2, if n % 2 == 0 { "yes" } else { "no" })?;
    }
    let hours = DataValidation::new().allow_whole_number(DataValidationRule::Between(0, 24));
    sheet.add_data_validation(1, 1, 10, 1, &hours)?;
    let done = DataValidation::new().allow_list_strings(&["yes", "no"])?;
    sheet.add_data_validation(1, 2, 10, 2, &done)?;
    Ok(workbook)
}

fn chart() -> Result<Workbook, XlsxError> {
    let mut workbook = workbook()?;
    let sheet = workbook.add_worksheet().set_name("Sales")?;
    let columns = [
        TableColumn::new().set_header("Month"),
        TableColumn::new().set_header("Sales"),
    ];
    let table = Table::new().set_name("SalesTable").set_columns(&columns);
    sheet.add_table(0, 0, 6, 1, &table)?;
    for (i, (month, sales)) in MONTHS.iter().zip(SALES).enumerate() {
        sheet.write_string(i as u32 + 1, 0, *month)?;
        sheet.write_number(i as u32 + 1, 1, sales)?;
    }
    sheet.write_string(8, 0, "Total")?;
    let total: f64 = SALES.iter().sum();
    sheet.write_formula(8, 1, Formula::new("=SUM(B2:B7)").set_result(result(total)))?;
    let note = Note::new("Sum of the six months").set_author("fixture");
    sheet.insert_note(8, 0, &note)?;
    let mut chart = Chart::new(ChartType::Column);
    chart
        .add_series()
        .set_categories("Sales!$A$2:$A$7")
        .set_values("Sales!$B$2:$B$7");
    sheet.insert_chart(1, 3, &chart)?;
    Ok(workbook)
}

fn two_sheets() -> Result<Workbook, XlsxError> {
    let mut workbook = workbook()?;
    let input = workbook.add_worksheet().set_name("Input")?;
    input.write_string(0, 0, "Name")?;
    input.write_string(0, 1, "Value")?;
    for (i, (name, value)) in ["north", "south", "east", "west"]
        .iter()
        .zip(INPUTS)
        .enumerate()
    {
        input.write_string(i as u32 + 1, 0, *name)?;
        input.write_number(i as u32 + 1, 1, value)?;
    }
    input.write_string(5, 0, "Rate")?;
    input.write_number(5, 1, RATE)?;
    let total: f64 = INPUTS.iter().sum();
    let summary = workbook.add_worksheet().set_name("Summary")?;
    summary.write_string(0, 0, "Total")?;
    summary.write_formula(
        0,
        1,
        Formula::new("=SUM(Input!B2:B5)").set_result(result(total)),
    )?;
    summary.write_string(1, 0, "Scaled")?;
    summary.write_formula(
        1,
        1,
        Formula::new("=B1*Rate").set_result(result(total * RATE)),
    )?;
    workbook.define_name("Rate", "=Input!$B$6")?;
    Ok(workbook)
}

fn rows_5000() -> Result<Workbook, XlsxError> {
    let mut workbook = workbook()?;
    let sheet = workbook.add_worksheet().set_name("Rows")?;
    for (col, text) in ["Id", "Name", "Amount"].iter().enumerate() {
        sheet.write_string(0, col as u16, *text)?;
    }
    for id in 1..=ROWS {
        sheet.write_number(id, 0, f64::from(id))?;
        sheet.write_string(id, 1, format!("name-{id}"))?;
        sheet.write_number(id, 2, row_amount(id))?;
    }
    let total: f64 = (1..=ROWS).map(row_amount).sum();
    sheet.write_string(0, 4, "Total")?;
    sheet.write_formula(
        0,
        5,
        Formula::new("=SUM(C2:C5001)").set_result(result(total)),
    )?;
    Ok(workbook)
}

pub fn build(file: &str) -> Result<Workbook, XlsxError> {
    match file {
        "values.xlsx" => values(),
        "formulas.xlsx" => formulas(),
        "formats.xlsx" => formats(),
        "merged.xlsx" => merged(),
        "validation.xlsx" => validation(),
        "chart.xlsx" => chart(),
        "two-sheets.xlsx" => two_sheets(),
        "rows-5000.xlsx" => rows_5000(),
        other => Err(XlsxError::ParameterError(format!(
            "no corpus workbook {other}"
        ))),
    }
}

pub fn generate(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    for file in FILES {
        let mut workbook = build(file).map_err(|e| format!("{file}: {e}"))?;
        workbook
            .save(dir.join(file))
            .map_err(|e| format!("{file}: {e}"))?;
    }
    Ok(())
}
