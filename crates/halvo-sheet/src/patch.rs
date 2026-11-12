use std::io::{Cursor, Write};
use std::ops::RangeInclusive;

use quick_xml::events::Event;
use quick_xml::name::{Namespace, ResolveResult};
use quick_xml::reader::NsReader;
use quick_xml::XmlVersion;

use crate::book::unreadable;
use crate::error::{Error, Refusal};
use crate::parts::{self, Entries};
use crate::range::Range;
use crate::source::Value;
use crate::xml::{self, form, Item, Pos, MAIN};

struct Edit {
    at: usize,
    end: usize,
    text: String,
}

fn refused(why: Refusal) -> Error {
    Error::Workbook(why)
}

pub(crate) fn text(bytes: &[u8]) -> Result<&str, Error> {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(form());
    }
    std::str::from_utf8(bytes).map_err(|_| form())
}

fn column_name(mut col: u32) -> String {
    let mut letters = Vec::new();
    col += 1;
    while col > 0 {
        letters.push(b'A' + ((col - 1) % 26) as u8);
        col = (col - 1) / 26;
    }
    letters.reverse();
    String::from_utf8(letters).unwrap_or_default()
}

fn reference((row, col): Pos) -> String {
    format!("{}{}", column_name(col), row + 1)
}

fn range_text(range: Range) -> String {
    if range.first == range.last {
        reference(range.first)
    } else {
        format!("{}:{}", reference(range.first), reference(range.last))
    }
}

fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for (at, c) in text.char_indices() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("_x000D_"),
            '_' if escape_form(&text[at..]) => out.push_str("_x005F_"),
            c => out.push(c),
        }
    }
    out
}

fn escape_form(rest: &str) -> bool {
    let bytes = rest.as_bytes();
    bytes.len() >= 7
        && bytes[1] == b'x'
        && bytes[2..6].iter().all(u8::is_ascii_hexdigit)
        && bytes[6] == b'_'
}

fn qualified(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}:{name}")
    }
}

fn cell_element(prefix: &str, pos: Pos, style: Option<&str>, value: &Value) -> String {
    let c = qualified(prefix, "c");
    let quoted = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('"', "&quot;")
    };
    let style = style.map_or(String::new(), |s| format!(" s=\"{}\"", quoted(s)));
    let r = reference(pos);
    match value {
        Value::Number(x) => {
            let v = qualified(prefix, "v");
            format!("<{c} r=\"{r}\"{style}><{v}>{x}</{v}></{c}>")
        }
        Value::Text(text) => {
            let is = qualified(prefix, "is");
            let t = qualified(prefix, "t");
            let body = escaped(text);
            format!("<{c} r=\"{r}\"{style} t=\"inlineStr\"><{is}><{t} xml:space=\"preserve\">{body}</{t}></{is}></{c}>")
        }
    }
}

/// The pairs of a `spans` value, `a:b` separated by white space; refused when it has another
/// form.
pub(crate) fn span_pairs(spans: &str) -> Result<Vec<(u32, u32)>, Error> {
    let mut pairs = Vec::new();
    for pair in spans.split_whitespace() {
        let (a, b) = pair.split_once(':').ok_or_else(form)?;
        let number = |text: &str| -> Result<u32, Error> {
            if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                return Err(form());
            }
            text.parse().map_err(|_| form())
        };
        pairs.push((number(a)?, number(b)?));
    }
    Ok(pairs)
}

/// Whether the union of `pairs` holds every column from `low` to `high` (counted from 1).
pub(crate) fn covered(pairs: &[(u32, u32)], low: u32, high: u32) -> bool {
    let mut sorted = pairs.to_vec();
    sorted.sort_unstable();
    let mut reach = low;
    for (a, b) in sorted {
        if a > reach {
            break;
        }
        if b >= reach {
            if b >= high {
                return true;
            }
            reach = b + 1;
        }
    }
    false
}

/// The `spans` of a row that holds the columns `cols` (counted from 0): `None` when it holds
/// them already, else one pair over the old pairs and the columns.
fn spans_with(spans: &str, cols: &RangeInclusive<u32>) -> Result<Option<String>, Error> {
    let pairs = span_pairs(spans)?;
    let (low, high) = (cols.start() + 1, cols.end() + 1);
    if covered(&pairs, low, high) {
        return Ok(None);
    }
    let first = pairs.iter().map(|p| p.0).fold(low, u32::min);
    let last = pairs.iter().map(|p| p.1).fold(high, u32::max);
    Ok(Some(format!("{first}:{last}")))
}

struct Fill<'v> {
    range: Range,
    values: &'v [Vec<Value>],
}

impl Fill<'_> {
    fn value(&self, (row, col): Pos) -> &Value {
        &self.values[(row - self.range.first.0) as usize][(col - self.range.first.1) as usize]
    }

    fn cols(&self) -> RangeInclusive<u32> {
        self.range.first.1..=self.range.last.1
    }

    fn holds_row(&self, row: u32) -> bool {
        (self.range.first.0..=self.range.last.0).contains(&row)
    }

    fn cells(&self, prefix: &str, row: u32, cols: RangeInclusive<u32>) -> String {
        cols.map(|col| cell_element(prefix, (row, col), None, self.value((row, col))))
            .collect()
    }

    fn rows(&self, prefix: &str, rows: std::ops::Range<u32>) -> String {
        let name = qualified(prefix, "row");
        rows.map(|row| {
            let cells = self.cells(prefix, row, self.cols());
            format!("<{name} r=\"{}\">{cells}</{name}>", row + 1)
        })
        .collect()
    }
}

struct Open {
    prefix: String,
    number: u32,
    next_col: u32,
}

fn overlaps(a: &Range, b: &Range) -> bool {
    a.first.0 <= b.last.0 && b.first.0 <= a.last.0 && a.first.1 <= b.last.1 && b.first.1 <= a.last.1
}

/// The worksheet part with the cells of `range` written: each cell's element replaced, or
/// inserted in column order, a row that does not exist inserted in row order, and the `spans`
/// of a row and the `ref` of `dimension` grown where a cell lies outside them.
pub(crate) fn sheet_part(xml: &str, range: Range, values: &[Vec<Value>]) -> Result<String, Error> {
    let fill = Fill { range, values };
    let (last_row, last_col) = range.last;
    let mut edits = Vec::new();
    let mut data_prefix = "";
    let mut next_row = range.first.0;
    let mut row: Option<Open> = None;
    let mut shared = false;
    let mut arrays = Vec::new();
    let insert = |edits: &mut Vec<Edit>, at: usize, text: String| {
        edits.push(Edit { at, end: at, text });
    };
    xml::walk(xml, |item| {
        match item {
            Item::Dimension(tag) => {
                let place = tag.get("ref").ok_or_else(form)?;
                let old = Range::parse(&place.value).map_err(|_| form())?;
                let grown = Range {
                    first: (
                        old.first.0.min(range.first.0),
                        old.first.1.min(range.first.1),
                    ),
                    last: (old.last.0.max(last_row), old.last.1.max(last_col)),
                };
                if grown != old {
                    edits.push(Edit {
                        at: place.place.at,
                        end: place.place.end,
                        text: range_text(grown),
                    });
                }
            }
            Item::Data(tag) => {
                data_prefix = tag.prefix;
                if tag.empty {
                    let rows = fill.rows(tag.prefix, next_row..last_row + 1);
                    next_row = last_row + 1;
                    edits.push(Edit {
                        at: tag.span.end - 2,
                        end: tag.span.end,
                        text: format!(">{rows}</{}>", tag.named("sheetData")),
                    });
                }
            }
            Item::Row(tag, number) => {
                if next_row < number && next_row <= last_row {
                    let upto = number.min(last_row + 1);
                    insert(
                        &mut edits,
                        tag.span.at,
                        fill.rows(data_prefix, next_row..upto),
                    );
                    next_row = upto;
                }
                if fill.holds_row(number) {
                    next_row = number + 1;
                    if let Some(spans) = tag.get("spans") {
                        if let Some(text) = spans_with(&spans.value, &fill.cols())? {
                            edits.push(Edit {
                                at: spans.place.at,
                                end: spans.place.end,
                                text,
                            });
                        }
                    }
                    if tag.empty {
                        let cells = fill.cells(tag.prefix, number, fill.cols());
                        edits.push(Edit {
                            at: tag.span.end - 2,
                            end: tag.span.end,
                            text: format!(">{cells}</{}>", tag.named("row")),
                        });
                    } else {
                        row = Some(Open {
                            prefix: tag.prefix.to_string(),
                            number,
                            next_col: range.first.1,
                        });
                    }
                }
            }
            Item::Cell(cell) => {
                arrays.extend(cell.array);
                if let Some(open) = row.as_mut() {
                    let col = cell.pos.1;
                    if open.next_col < col && open.next_col <= last_col {
                        let upto = col.min(last_col + 1);
                        let cells = fill.cells(&open.prefix, open.number, open.next_col..=upto - 1);
                        insert(&mut edits, cell.span.at, cells);
                        open.next_col = upto;
                    }
                    if range.holds(cell.pos) {
                        shared |= cell.shared;
                        let style = cell.tag.get("s").map(|s| s.value.to_string());
                        let value = fill.value(cell.pos);
                        edits.push(Edit {
                            at: cell.span.at,
                            end: cell.span.end,
                            text: cell_element(&open.prefix, cell.pos, style.as_deref(), value),
                        });
                        open.next_col = col + 1;
                    }
                }
            }
            Item::RowRest(at) => {
                if let Some(open) = row.as_mut() {
                    if open.next_col <= last_col {
                        let cells = fill.cells(&open.prefix, open.number, open.next_col..=last_col);
                        insert(&mut edits, at, cells);
                        open.next_col = last_col + 1;
                    }
                }
            }
            Item::RowEnd(span) => {
                if let Some(open) = row.take() {
                    if open.next_col <= last_col {
                        let cells = fill.cells(&open.prefix, open.number, open.next_col..=last_col);
                        insert(&mut edits, span.at, cells);
                    }
                }
            }
            Item::DataEnd(span) => {
                if next_row <= last_row {
                    insert(
                        &mut edits,
                        span.at,
                        fill.rows(data_prefix, next_row..last_row + 1),
                    );
                    next_row = last_row + 1;
                }
            }
        }
        Ok(())
    })?;
    if shared {
        return Err(refused(Refusal::SharedFormula));
    }
    if arrays.iter().any(|array| overlaps(array, &range)) {
        return Err(refused(Refusal::ArrayFormula));
    }
    Ok(applied(xml, edits))
}

fn applied(xml: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|edit| (edit.at, edit.end));
    let extra: usize = edits.iter().map(|edit| edit.text.len()).sum();
    let mut out = String::with_capacity(xml.len() + extra);
    let mut done = 0;
    for edit in edits {
        out.push_str(&xml[done..edit.at]);
        out.push_str(&edit.text);
        done = edit.end;
    }
    out.push_str(&xml[done..]);
    out
}

/// The workbook part with the calculation flag set: `None` where `calcPr` has it at `1` or
/// `true` already, else the part with the flag's value made `1`, the attribute added after the
/// element's name, or the element added before the first child that the schema puts after it.
pub(crate) fn workbook_part(xml: &str) -> Result<Option<String>, Error> {
    let book = xml::book(xml)?;
    let edit = match &book.calc {
        Some((calc, _)) if xml::flag_set(calc) => return Ok(None),
        Some((calc, _)) => match calc.get("fullCalcOnLoad") {
            Some(flag) => Edit {
                at: flag.place.at,
                end: flag.place.end,
                text: "1".to_string(),
            },
            None => {
                let at = calc.name_end("calcPr");
                Edit {
                    at,
                    end: at,
                    text: " fullCalcOnLoad=\"1\"".to_string(),
                }
            }
        },
        None => {
            let at = book.later.unwrap_or(book.end);
            Edit {
                at,
                end: at,
                text: format!("<{} fullCalcOnLoad=\"1\"/>", book.root.named("calcPr")),
            }
        }
    };
    Ok(Some(applied(xml, vec![edit])))
}

/// The header rows of the tables that the sheet's relationships link: the first row of each
/// table's `ref` unless its `headerRowCount` is `0`.
pub(crate) fn table_headers(entries: &Entries, part: &str) -> Result<Vec<Range>, Error> {
    let mut headers = Vec::new();
    for target in parts::linked(entries, part, "table")? {
        let xml = text(entries.get(&target).ok_or_else(unreadable)?)?;
        let mut reader = NsReader::from_str(xml);
        loop {
            let (ns, event) = reader.read_resolved_event().map_err(|_| form())?;
            let main = matches!(ns, ResolveResult::Bound(Namespace(n)) if n == MAIN);
            match event {
                Event::Start(e) | Event::Empty(e) => {
                    if !(main && xml::local(e.name().as_ref()) == "table") {
                        return Err(form());
                    }
                    let mut reference = None;
                    let mut header_rows = None;
                    for attribute in e.attributes() {
                        let attribute = attribute.map_err(|_| form())?;
                        let value = attribute
                            .normalized_value(XmlVersion::Implicit1_0)
                            .map_err(|_| form())?
                            .into_owned();
                        match attribute.key.as_ref() {
                            "ref" => reference = Some(value),
                            "headerRowCount" => header_rows = Some(value),
                            _ => {}
                        }
                    }
                    let range = Range::parse(&reference.ok_or_else(form)?).map_err(|_| form())?;
                    if header_rows.as_deref() != Some("0") {
                        headers.push(Range {
                            first: range.first,
                            last: (range.first.0, range.last.1),
                        });
                    }
                    break;
                }
                Event::Eof => return Err(form()),
                _ => {}
            }
        }
    }
    Ok(headers)
}

pub(crate) fn package(
    original: &[u8],
    (part, sheet): (&str, &str),
    (workbook, book): (&str, Option<&str>),
) -> Result<Vec<u8>, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(original)).map_err(|_| unreadable())?;
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|_| unreadable())?;
        let name = entry.name().map_err(|_| unreadable())?.to_string();
        let replaced = if name == part {
            Some(sheet)
        } else if name == workbook {
            book
        } else {
            None
        };
        match replaced {
            Some(bytes) => {
                let options = zip::write::SimpleFileOptions::default()
                    .compression_method(entry.compression())
                    .last_modified_time(entry.last_modified().unwrap_or_default());
                drop(entry);
                out.start_file(name.as_str(), options)
                    .map_err(|_| unreadable())?;
                out.write_all(bytes.as_bytes()).map_err(|_| unreadable())?;
            }
            None => out.raw_copy_file(entry).map_err(|_| unreadable())?,
        }
    }
    let written = out.finish().map_err(|_| unreadable())?;
    Ok(written.into_inner())
}

pub(crate) fn write(
    original: &[u8],
    entries: &Entries,
    part: &str,
    range: Range,
    values: &[Vec<Value>],
) -> Result<Vec<u8>, Error> {
    for header in table_headers(entries, part)? {
        if overlaps(&header, &range) {
            return Err(refused(Refusal::TableHeader));
        }
    }
    let source = text(entries.get(part).ok_or_else(unreadable)?)?;
    let patched = sheet_part(source, range, values)?;
    let workbook = parts::workbook_name(entries)?;
    let book = workbook_part(text(entries.get(&workbook).ok_or_else(unreadable)?)?)?;
    package(original, (part, &patched), (&workbook, book.as_deref()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::corpus;

    const FILES: [&str; 8] = [
        "values.xlsx",
        "formulas.xlsx",
        "formats.xlsx",
        "merged.xlsx",
        "validation.xlsx",
        "chart.xlsx",
        "two-sheets.xlsx",
        "rows-5000.xlsx",
    ];

    const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;

    fn sheet1(file: &str) -> String {
        let all = corpus(file);
        String::from_utf8(all["xl/worksheets/sheet1.xml"].clone()).expect("utf-8")
    }

    fn one(cell: &str, value: Value) -> (Range, Vec<Vec<Value>>) {
        (Range::parse(cell).expect("a cell"), vec![vec![value]])
    }

    fn patched(file: &str, cell: &str, value: Value) -> String {
        let (range, values) = one(cell, value);
        sheet_part(&sheet1(file), range, &values).expect("patched")
    }

    fn sheet(data: &str) -> String {
        format!(r#"<worksheet {NS}>{data}</worksheet>"#)
    }

    #[test]
    fn every_sheet_part_of_the_corpus_is_in_the_writers_form_and_every_flag_is_set() {
        for file in FILES {
            let all = corpus(file);
            for part in parts::sheet_parts(&all) {
                let source = String::from_utf8(all[&part].clone()).expect("utf-8");
                assert_eq!(xml::walk(&source, |_| Ok(())), Ok(()), "{file} {part}");
            }
            let book = String::from_utf8(all[parts::WORKBOOK].clone()).expect("utf-8");
            assert_eq!(workbook_part(&book).expect("read"), None, "{file}");
        }
    }

    #[test]
    fn a_named_cell_replaces_its_element_or_goes_in_column_and_row_order() {
        let n = Value::Number;
        let cases: [(&str, &str, Value, &str, &str); 5] = [
            (
                "values.xlsx",
                "B5",
                n(77.0),
                r#"<c r="B5"><v>12</v></c>"#,
                r#"<c r="B5"><v>77</v></c>"#,
            ),
            (
                "formulas.xlsx",
                "C2",
                n(1.5),
                r#"<c r="A2" t="s"><v>4</v></c></row>"#,
                r#"<c r="A2" t="s"><v>4</v></c><c r="C2"><v>1.5</v></c></row>"#,
            ),
            (
                "chart.xlsx",
                "A8",
                n(8.0),
                r#"</row><row r="9" spans="1:2">"#,
                r#"</row><row r="8"><c r="A8"><v>8</v></c></row><row r="9" spans="1:2">"#,
            ),
            (
                "merged.xlsx",
                "B10",
                n(3.0),
                r#"<c r="B10" s="1"/>"#,
                r#"<c r="B10" s="1"><v>3</v></c>"#,
            ),
            (
                "formulas.xlsx",
                "D7",
                n(5.0),
                r#"<c r="D7"><f>SUM(D2:D6)</f><v>0</v></c>"#,
                r#"<c r="D7"><v>5</v></c>"#,
            ),
        ];
        for (file, cell, value, from, to) in cases {
            let source = sheet1(file);
            assert_eq!(source.matches(from).count(), 1, "{file} holds {from} once");
            let want = source.replacen(from, to, 1);
            assert_eq!(patched(file, cell, value), want, "{file} {cell}");
        }
    }

    #[test]
    fn a_cell_outside_the_spans_and_the_dimension_grows_them_and_a_text_is_inline() {
        let got = patched("values.xlsx", "F2", Value::Text("a & b".to_string()));
        let source = sheet1("values.xlsx");
        let want = source
            .replacen(r#"<dimension ref="A1:D21"/>"#, r#"<dimension ref="A1:F21"/>"#, 1)
            .replacen(r#"<row r="2" spans="1:4">"#, r#"<row r="2" spans="1:6">"#, 1)
            .replacen(
                r#"<c r="D2" t="b"><v>0</v></c></row>"#,
                r#"<c r="D2" t="b"><v>0</v></c><c r="F2" t="inlineStr"><is><t xml:space="preserve">a &amp; b</t></is></c></row>"#,
                1,
            );
        assert_eq!(got, want);
    }

    #[test]
    fn a_row_below_the_dimension_grows_it_and_goes_after_the_last_row() {
        let got = patched("values.xlsx", "A30", Value::Number(1.0));
        let want = sheet1("values.xlsx")
            .replacen(
                r#"<dimension ref="A1:D21"/>"#,
                r#"<dimension ref="A1:D30"/>"#,
                1,
            )
            .replacen(
                "</row></sheetData>",
                r#"</row><row r="30"><c r="A30"><v>1</v></c></row></sheetData>"#,
                1,
            );
        assert_eq!(got, want);
    }

    #[test]
    fn a_text_is_escaped_so_that_it_reads_back_as_written() {
        let cases = [
            ("a<b>&c", "a&lt;b&gt;&amp;c"),
            ("line\rbreak", "line_x000D_break"),
            ("_x0041_", "_x005F_x0041_"),
            ("_x00E9_ and _x00e9_", "_x005F_x00E9_ and _x005F_x00e9_"),
            (
                "_x41_ and _xZZZZ_ and _X0041_",
                "_x41_ and _xZZZZ_ and _X0041_",
            ),
            ("", ""),
        ];
        for (text, want) in cases {
            assert_eq!(escaped(text), want, "{text:?}");
        }
    }

    #[test]
    fn a_written_cell_keeps_its_style_and_an_inserted_cell_takes_none() {
        let xml = sheet(
            r#"<sheetData><row r="1"><c r="A1" s="3"><v>1</v></c><c r="B1" s="4" t="s"><v>0</v></c><c r="D1" s="5"/></row></sheetData>"#,
        );
        let range = Range::parse("A1:D1").expect("a range");
        let values = vec![vec![
            Value::Text("t".to_string()),
            Value::Number(2.0),
            Value::Number(3.0),
            Value::Text("u".to_string()),
        ]];
        let want = sheet(concat!(
            r#"<sheetData><row r="1">"#,
            r#"<c r="A1" s="3" t="inlineStr"><is><t xml:space="preserve">t</t></is></c>"#,
            r#"<c r="B1" s="4"><v>2</v></c>"#,
            r#"<c r="C1"><v>3</v></c>"#,
            r#"<c r="D1" s="5" t="inlineStr"><is><t xml:space="preserve">u</t></is></c>"#,
            r#"</row></sheetData>"#
        ));
        assert_eq!(sheet_part(&xml, range, &values).expect("patched"), want);
    }

    #[test]
    fn spans_grow_for_a_cell_one_column_past_them_and_stay_where_they_hold_it() {
        let row = |spans: &str| {
            sheet(&format!(
                r#"<sheetData><row r="1" spans="{spans}"><c r="A1"/></row></sheetData>"#
            ))
        };
        let cases = [
            ("1:4", "E1", Some("1:5")),
            ("1:4", "D1", None),
            ("1:2 4:6", "E1", None),
            ("1:2 4:6", "C1", Some("1:6")),
            ("2:3", "A1", Some("1:3")),
        ];
        for (spans, cell, grown) in cases {
            let (range, values) = one(cell, Value::Number(1.0));
            let got = sheet_part(&row(spans), range, &values).expect("patched");
            let want = format!(r#"spans="{}""#, grown.unwrap_or(spans));
            assert!(got.contains(&want), "{spans} {cell}: {got}");
        }
    }

    #[test]
    fn the_dimension_grows_to_hold_the_range_on_every_side() {
        let data = r#"<sheetData><row r="3"><c r="C3"/></row></sheetData>"#;
        let cases = [
            ("B2:D4", "A3", "A2:D4"),
            ("B2:D4", "C1", "B1:D4"),
            ("B2:D4", "E3", "B2:E4"),
            ("B2:D4", "C5", "B2:D5"),
            ("C3", "C3", "C3"),
            ("C3", "A1", "A1:C3"),
        ];
        for (old, cell, new) in cases {
            let xml = sheet(&format!(r#"<dimension ref="{old}"/>{data}"#));
            let (range, values) = one(cell, Value::Number(1.0));
            let got = sheet_part(&xml, range, &values).expect("patched");
            assert!(
                got.contains(&format!(r#"<dimension ref="{new}"/>"#)),
                "{old} {cell}: {got}"
            );
        }
    }

    #[test]
    fn a_row_element_with_no_cells_and_an_empty_sheet_data_get_their_named_cells() {
        let xml = sheet(r#"<sheetData><row r="2" spans="1:1"/></sheetData>"#);
        let range = Range::parse("B2:B3").expect("a range");
        let values = vec![vec![Value::Number(1.0)], vec![Value::Number(2.0)]];
        let want = sheet(
            r#"<sheetData><row r="2" spans="1:2"><c r="B2"><v>1</v></c></row><row r="3"><c r="B3"><v>2</v></c></row></sheetData>"#,
        );
        assert_eq!(sheet_part(&xml, range, &values).expect("patched"), want);
        let empty = sheet(r#"<sheetData />"#);
        let want = sheet(
            r#"<sheetData ><row r="2"><c r="B2"><v>1</v></c></row><row r="3"><c r="B3"><v>2</v></c></row></sheetData>"#,
        );
        assert_eq!(sheet_part(&empty, range, &values).expect("patched"), want);
    }

    #[test]
    fn new_cells_go_before_the_extension_list_of_a_row() {
        let xml =
            sheet(r#"<sheetData><row r="1"><c r="A1"/><extLst><ext/></extLst></row></sheetData>"#);
        let (range, values) = one("C1", Value::Number(1.0));
        let want = sheet(
            r#"<sheetData><row r="1"><c r="A1"/><c r="C1"><v>1</v></c><extLst><ext/></extLst></row></sheetData>"#,
        );
        assert_eq!(sheet_part(&xml, range, &values).expect("patched"), want);
    }

    #[test]
    fn each_spelling_of_an_attribute_is_written_as_the_plain_spelling_is() {
        let spell = |sep: &str, eq: &str, q: char, spans: &str| {
            sheet(&format!(
                r#"<dimension{sep}ref{eq}{q}A1:B2{q}/><sheetData><row{sep}r{eq}{q}1{q}{sep}spans{eq}{q}{spans}{q}><c{sep}r{eq}{q}A1{q}/></row></sheetData>"#
            ))
        };
        let cases = [
            (" ", "=", '"', "1:2"),
            ("\t", "=", '"', "1:2"),
            ("\n", "=", '"', "1:2"),
            (" ", " = ", '"', "1:2"),
            (" ", "=", '\'', "1:2"),
            (" ", "=", '"', "1:&#50;"),
        ];
        for (sep, eq, q, spans) in cases {
            let (range, values) = one("D1", Value::Number(1.0));
            let got = sheet_part(&spell(sep, eq, q, spans), range, &values).expect("patched");
            let want = spell(sep, eq, q, "1:4")
                .replacen("A1:B2", "A1:D2", 1)
                .replacen("</row>", r#"<c r="D1"><v>1</v></c></row>"#, 1);
            assert_eq!(got, want, "{sep:?} {eq:?} {q:?} {spans}");
        }
        let prefixed = r#"<x:worksheet xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><x:sheetData><x:row r="1"><x:c r="A1"/></x:row></x:sheetData></x:worksheet>"#;
        let range = Range::parse("A1:B2").expect("a range");
        let values = vec![vec![Value::Number(1.0); 2]; 2];
        let want = r#"<x:worksheet xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><x:sheetData><x:row r="1"><x:c r="A1"><x:v>1</x:v></x:c><x:c r="B1"><x:v>1</x:v></x:c></x:row><x:row r="2"><x:c r="A2"><x:v>1</x:v></x:c><x:c r="B2"><x:v>1</x:v></x:c></x:row></x:sheetData></x:worksheet>"#;
        assert_eq!(sheet_part(prefixed, range, &values).expect("patched"), want);
    }

    #[test]
    fn a_dimension_or_spans_in_a_form_that_the_writer_does_not_take_is_refused() {
        for dimension in [
            r#"<dimension ref="$A$1:$D$7"/>"#,
            r#"<dimension/>"#,
            r#"<dimension ref="A1:"/>"#,
        ] {
            let xml = sheet(&format!(r#"{dimension}<sheetData/>"#));
            let (range, values) = one("B2", Value::Number(1.0));
            assert_eq!(sheet_part(&xml, range, &values), Err(form()), "{dimension}");
        }
        let xml = sheet(r#"<sheetData><row r="2" spans="a:b"/></sheetData>"#);
        let (range, values) = one("B2", Value::Number(1.0));
        assert_eq!(sheet_part(&xml, range, &values), Err(form()));
    }

    #[test]
    fn an_array_range_that_touches_the_range_or_a_shared_member_in_it_is_refused() {
        let xml = sheet(
            r#"<sheetData><row r="1"><c r="A1"><f t="array" ref="A1:B3">1</f></c><c r="D1"><f t="shared" si="0" ref="D1:D2">1</f></c></row><row r="2"><c r="D2"><f t="shared" si="0"/></c></row></sheetData>"#,
        );
        let cases = [
            ("B3", Some(Refusal::ArrayFormula)),
            ("A1:C1", Some(Refusal::ArrayFormula)),
            ("C1:C3", None),
            ("D2", Some(Refusal::SharedFormula)),
            ("E1:E2", None),
        ];
        for (cells, want) in cases {
            let range = Range::parse(cells).expect("a range");
            let values =
                vec![vec![Value::Number(1.0); range.cols() as usize]; range.rows() as usize];
            let got = sheet_part(&xml, range, &values);
            assert_eq!(got.err(), want.map(refused), "{cells}");
        }
    }

    #[test]
    fn the_calculation_flag_is_set_where_it_was_absent_or_zero_and_added_in_its_place() {
        let head = format!(r#"<workbook {NS}><sheets><sheet name="A" sheetId="1"/></sheets>"#);
        let cases = [
            (format!(r#"{head}<calcPr calcId="1"/></workbook>"#), Some(format!(r#"{head}<calcPr fullCalcOnLoad="1" calcId="1"/></workbook>"#))),
            (format!(r#"{head}<calcPr calcId="1" fullCalcOnLoad="0"/></workbook>"#), Some(format!(r#"{head}<calcPr calcId="1" fullCalcOnLoad="1"/></workbook>"#))),
            (format!(r#"{head}<calcPr fullCalcOnLoad='false'></calcPr></workbook>"#), Some(format!(r#"{head}<calcPr fullCalcOnLoad='1'></calcPr></workbook>"#))),
            (format!(r#"{head}<calcPr fullCalcOnLoad="1"/></workbook>"#), None),
            (format!(r#"{head}<calcPr fullCalcOnLoad="true"/></workbook>"#), None),
            (format!(r#"{head}<calcPr fullCalcOnLoad = 'true' /></workbook>"#), None),
            (format!(r#"{head}</workbook>"#), Some(format!(r#"{head}<calcPr fullCalcOnLoad="1"/></workbook>"#))),
            (
                format!(r#"{head}<definedNames><definedName name="R">A!$B$6</definedName></definedNames><extLst/></workbook>"#),
                Some(format!(r#"{head}<definedNames><definedName name="R">A!$B$6</definedName></definedNames><calcPr fullCalcOnLoad="1"/><extLst/></workbook>"#)),
            ),
            (
                format!(r#"{head}<customWorkbookViews/><extLst/></workbook>"#),
                Some(format!(r#"{head}<calcPr fullCalcOnLoad="1"/><customWorkbookViews/><extLst/></workbook>"#)),
            ),
            (
                r#"<x:workbook xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><x:sheets/><x:oleSize ref="A1"/></x:workbook>"#.to_string(),
                Some(r#"<x:workbook xmlns:x="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><x:sheets/><x:calcPr fullCalcOnLoad="1"/><x:oleSize ref="A1"/></x:workbook>"#.to_string()),
            ),
        ];
        for (source, want) in cases {
            assert_eq!(workbook_part(&source).expect("read"), want, "{source}");
        }
    }
}
