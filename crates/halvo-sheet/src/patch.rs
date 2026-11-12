use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::book::unreadable;
use crate::error::{Error, Refusal};
use crate::parts::{self, Entries};
use crate::range::Range;
use crate::source::Value;

type Pos = (u32, u32);

struct Edit {
    at: usize,
    end: usize,
    text: String,
}

fn refused(why: Refusal) -> Error {
    Error::Workbook(why)
}

fn form() -> Error {
    refused(Refusal::Part)
}

pub(crate) fn text(bytes: &[u8]) -> Result<&str, Error> {
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        return Err(form());
    }
    std::str::from_utf8(bytes).map_err(|_| form())
}

fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

fn prefix(name: &str) -> &str {
    name.rsplit_once(':').map_or("", |(prefix, _)| prefix)
}

fn attr(e: &BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .map(|a| a.value.to_string())
}

fn cell_pos(text: &str) -> Result<Pos, Error> {
    let range = Range::parse(text).map_err(|_| form())?;
    if range.first != range.last {
        return Err(form());
    }
    Ok(range.first)
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
    let style = style.map_or(String::new(), |s| format!(" s=\"{s}\""));
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

fn with_attribute(tag: &str, key: &str, old: Option<&str>, new: &str) -> String {
    match old {
        Some(old) => {
            for quote in ['"', '\''] {
                let from = format!(" {key}={quote}{old}{quote}");
                if let Some(at) = tag.find(&from) {
                    let mut out = tag.to_string();
                    out.replace_range(at..at + from.len(), &format!(" {key}=\"{new}\""));
                    return out;
                }
            }
            tag.to_string()
        }
        None => {
            let name_end = tag[1..]
                .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
                .map_or(tag.len(), |at| at + 1);
            let mut out = tag.to_string();
            out.insert_str(name_end, &format!(" {key}=\"{new}\""));
            out
        }
    }
}

fn spans_with(spans: &str, cols: &[u32]) -> Option<String> {
    let mut pairs = Vec::new();
    for pair in spans.split_whitespace() {
        let (a, b) = pair.split_once(':')?;
        pairs.push((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?));
    }
    let outside = cols
        .iter()
        .any(|col| !pairs.iter().any(|(a, b)| (*a..=*b).contains(&(col + 1))));
    if !outside {
        return None;
    }
    let low = pairs
        .iter()
        .map(|p| p.0)
        .chain(cols.iter().map(|c| c + 1))
        .min()?;
    let high = pairs
        .iter()
        .map(|p| p.1)
        .chain(cols.iter().map(|c| c + 1))
        .max()?;
    Some(format!("{low}:{high}"))
}

fn dimension_with(dimension: &str, named: &BTreeMap<Pos, &Value>) -> Option<String> {
    let range = Range::parse(dimension).ok()?;
    let (first, last) = (range.first, range.last);
    let low = named
        .keys()
        .fold(first, |(r, c), (nr, nc)| (r.min(*nr), c.min(*nc)));
    let high = named
        .keys()
        .fold(last, |(r, c), (nr, nc)| (r.max(*nr), c.max(*nc)));
    if (low, high) == (first, last) {
        return None;
    }
    Some(format!("{}:{}", reference(low), reference(high)))
}

struct Row {
    number: u32,
    tag: (usize, usize),
    raw: String,
    spans: Option<String>,
    last_col: Option<u32>,
}

struct Walk<'a> {
    named: &'a BTreeMap<Pos, &'a Value>,
    prefix: String,
    edits: Vec<Edit>,
    pending_rows: Vec<u32>,
    last_row: Option<u32>,
    arrays: Vec<Range>,
    shared: Vec<Pos>,
}

impl Walk<'_> {
    fn cells_of(&self, row: u32) -> Vec<(Pos, &Value)> {
        self.named
            .range((row, 0)..=(row, u32::MAX))
            .map(|(pos, value)| (*pos, *value))
            .collect()
    }

    fn new_row(&self, row: u32) -> String {
        let name = qualified(&self.prefix, "row");
        let cells: String = self
            .cells_of(row)
            .into_iter()
            .map(|(pos, value)| cell_element(&self.prefix, pos, None, value))
            .collect();
        format!("<{name} r=\"{}\">{cells}</{name}>", row + 1)
    }

    fn rows_before(&mut self, at: usize, row: Option<u32>) {
        let mut text = String::new();
        while let Some(&next) = self.pending_rows.first() {
            if row.is_some_and(|row| next >= row) {
                break;
            }
            text.push_str(&self.new_row(next));
            self.pending_rows.remove(0);
        }
        if !text.is_empty() {
            self.edits.push(Edit { at, end: at, text });
        }
    }
}

pub(crate) fn sheet_part(xml: &str, named: &BTreeMap<Pos, &Value>) -> Result<String, Error> {
    let mut reader = Reader::from_reader(xml.as_bytes());
    reader.config_mut().trim_text(false);
    let mut walk = Walk {
        named,
        prefix: String::new(),
        edits: Vec::new(),
        pending_rows: named.keys().map(|(row, _)| *row).collect(),
        last_row: None,
        arrays: Vec::new(),
        shared: Vec::new(),
    };
    walk.pending_rows.dedup();
    let mut buf = Vec::new();
    let mut in_data = false;
    let mut row: Option<Row> = None;
    let mut cell: Option<(Pos, usize, Option<String>)> = None;
    let mut seen_data = false;
    loop {
        let at = reader.buffer_position() as usize;
        let event = reader.read_event_into(&mut buf).map_err(|_| form())?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Eof => break,
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(event, Event::Empty(_));
                let name = e.name().as_ref().to_string();
                match local(&name) {
                    "dimension" if !in_data => {
                        if let Some(old) = attr(e, "ref") {
                            if let Some(new) = dimension_with(&old, named) {
                                let tag = &xml[at..end];
                                let text = with_attribute(tag, "ref", Some(&old), &new);
                                walk.edits.push(Edit { at, end, text });
                            }
                        }
                    }
                    "sheetData" if !seen_data => {
                        seen_data = true;
                        walk.prefix = prefix(&name).to_string();
                        if empty {
                            let rows: String =
                                walk.pending_rows.iter().map(|r| walk.new_row(*r)).collect();
                            if !rows.is_empty() {
                                let inner = &xml[at + 1..end - 2];
                                let text = format!("<{inner}>{rows}</{name}>");
                                walk.edits.push(Edit { at, end, text });
                            }
                            walk.pending_rows.clear();
                        } else {
                            in_data = true;
                        }
                    }
                    "row" if in_data && cell.is_none() => {
                        let number = attr(e, "r")
                            .and_then(|r| r.parse::<u32>().ok())
                            .filter(|r| *r >= 1)
                            .ok_or_else(form)?
                            - 1;
                        if walk.last_row.is_some_and(|last| number <= last) {
                            return Err(form());
                        }
                        walk.last_row = Some(number);
                        walk.rows_before(at, Some(number));
                        let named_here = walk.pending_rows.first() == Some(&number);
                        if named_here {
                            walk.pending_rows.remove(0);
                        }
                        let current = Row {
                            number,
                            tag: (at, end),
                            raw: xml[at..end].to_string(),
                            spans: attr(e, "spans"),
                            last_col: None,
                        };
                        if empty {
                            if named_here {
                                let cells: String = walk
                                    .cells_of(number)
                                    .into_iter()
                                    .map(|(pos, value)| {
                                        cell_element(&walk.prefix, pos, None, value)
                                    })
                                    .collect();
                                let open = grown_row_tag(
                                    &current,
                                    &walk,
                                    &current.raw[..current.raw.len() - 2],
                                );
                                let text = format!("{open}>{cells}</{name}>");
                                walk.edits.push(Edit { at, end, text });
                            }
                        } else {
                            row = Some(current);
                        }
                    }
                    "c" if row.is_some() && cell.is_none() => {
                        let current = row.as_mut().ok_or_else(form)?;
                        let pos = cell_pos(&attr(e, "r").ok_or_else(form)?)?;
                        if pos.0 != current.number || current.last_col.is_some_and(|c| pos.1 <= c) {
                            return Err(form());
                        }
                        let missing: Vec<(Pos, &Value)> = walk
                            .cells_of(current.number)
                            .into_iter()
                            .filter(|(p, _)| {
                                p.1 < pos.1 && current.last_col.is_none_or(|c| p.1 > c)
                            })
                            .collect();
                        let text: String = missing
                            .iter()
                            .map(|(p, v)| cell_element(&walk.prefix, *p, None, v))
                            .collect();
                        if !text.is_empty() {
                            walk.edits.push(Edit { at, end: at, text });
                        }
                        current.last_col = Some(pos.1);
                        let style = attr(e, "s");
                        if empty {
                            if let Some(value) = named.get(&pos) {
                                let text = cell_element(&walk.prefix, pos, style.as_deref(), value);
                                walk.edits.push(Edit { at, end, text });
                            }
                        } else {
                            cell = Some((pos, at, style));
                        }
                    }
                    "f" if cell.is_some() => {
                        let (pos, _, _) = cell.as_ref().ok_or_else(form)?;
                        match attr(e, "t").as_deref() {
                            Some("array") | Some("dataTable") => {
                                let range = match attr(e, "ref") {
                                    Some(r) => Range::parse(&r).map_err(|_| form())?,
                                    None => Range {
                                        first: *pos,
                                        last: *pos,
                                    },
                                };
                                walk.arrays.push(range);
                            }
                            Some("shared") => walk.shared.push(*pos),
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Event::End(ref e) => {
                let name = e.name().as_ref().to_string();
                match local(&name) {
                    "c" if cell.is_some() => {
                        if let Some((pos, start, style)) = cell.take() {
                            if let Some(value) = named.get(&pos) {
                                let text = cell_element(&walk.prefix, pos, style.as_deref(), value);
                                walk.edits.push(Edit {
                                    at: start,
                                    end,
                                    text,
                                });
                            }
                        }
                    }
                    "row" if row.is_some() && cell.is_none() => {
                        if let Some(current) = row.take() {
                            let missing: Vec<(Pos, &Value)> = walk
                                .cells_of(current.number)
                                .into_iter()
                                .filter(|(p, _)| current.last_col.is_none_or(|c| p.1 > c))
                                .collect();
                            let text: String = missing
                                .iter()
                                .map(|(p, v)| cell_element(&walk.prefix, *p, None, v))
                                .collect();
                            if !text.is_empty() {
                                walk.edits.push(Edit { at, end: at, text });
                            }
                            let (tag_at, tag_end) = current.tag;
                            let open = grown_row_tag(
                                &current,
                                &walk,
                                &current.raw[..current.raw.len() - 1],
                            );
                            let open = format!("{open}>");
                            if open != current.raw {
                                walk.edits.push(Edit {
                                    at: tag_at,
                                    end: tag_end,
                                    text: open,
                                });
                            }
                        }
                    }
                    "sheetData" if in_data => {
                        in_data = false;
                        walk.rows_before(at, None);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }
    if !seen_data {
        return Err(form());
    }
    for pos in named.keys() {
        if walk.shared.contains(pos) {
            return Err(refused(Refusal::SharedFormula));
        }
        if walk.arrays.iter().any(|range| range.holds(*pos)) {
            return Err(refused(Refusal::ArrayFormula));
        }
    }
    Ok(applied(xml, walk.edits))
}

fn grown_row_tag(row: &Row, walk: &Walk<'_>, open: &str) -> String {
    let cols: Vec<u32> = walk.cells_of(row.number).iter().map(|(p, _)| p.1).collect();
    match row
        .spans
        .as_deref()
        .and_then(|spans| spans_with(spans, &cols).map(|new| (spans, new)))
    {
        Some((old, new)) => with_attribute(open, "spans", Some(old), &new),
        None => open.to_string(),
    }
}

fn applied(xml: &str, mut edits: Vec<Edit>) -> String {
    edits.sort_by_key(|edit| (edit.at, edit.end));
    let mut out = String::with_capacity(xml.len() + 256);
    let mut done = 0;
    for edit in edits {
        out.push_str(&xml[done..edit.at]);
        out.push_str(&edit.text);
        done = edit.end;
    }
    out.push_str(&xml[done..]);
    out
}

const AFTER_CALC: [&str; 9] = [
    "oleSize",
    "customWorkbookViews",
    "pivotCaches",
    "smartTagPr",
    "smartTagTypes",
    "webPublishing",
    "fileRecoveryPr",
    "webPublishObjects",
    "extLst",
];

pub(crate) fn workbook_part(xml: &str) -> Result<Option<String>, Error> {
    let mut reader = Reader::from_reader(xml.as_bytes());
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut depth = 0usize;
    let mut root = String::new();
    loop {
        let at = reader.buffer_position() as usize;
        let event = reader.read_event_into(&mut buf).map_err(|_| form())?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Eof => return Err(form()),
            Event::Start(ref e) | Event::Empty(ref e) => {
                let name = e.name().as_ref().to_string();
                if depth == 1 && local(&name) == "calcPr" {
                    let tag = &xml[at..end];
                    let text = match attr(e, "fullCalcOnLoad").as_deref() {
                        Some("1") | Some("true") => return Ok(None),
                        Some(old) => with_attribute(tag, "fullCalcOnLoad", Some(old), "1"),
                        None => with_attribute(tag, "fullCalcOnLoad", None, "1"),
                    };
                    return Ok(Some(applied(xml, vec![Edit { at, end, text }])));
                }
                if depth == 1 && AFTER_CALC.contains(&local(&name)) {
                    return Ok(Some(with_calc(xml, at, &root)));
                }
                if depth == 0 {
                    root = name;
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
            }
            Event::End(_) => {
                depth -= 1;
                if depth == 0 {
                    return Ok(Some(with_calc(xml, at, &root)));
                }
            }
            _ => {}
        }
        buf.clear();
    }
}

fn with_calc(xml: &str, at: usize, root: &str) -> String {
    let name = qualified(prefix(root), "calcPr");
    let text = format!("<{name} fullCalcOnLoad=\"1\"/>");
    applied(xml, vec![Edit { at, end: at, text }])
}

pub(crate) fn table_headers(entries: &Entries, part: &str) -> Result<Vec<Range>, Error> {
    let mut headers = Vec::new();
    for target in parts::linked(entries, part, "table")? {
        let xml = text(entries.get(&target).ok_or_else(unreadable)?)?;
        let mut reader = Reader::from_reader(xml.as_bytes());
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf).map_err(|_| form())? {
                Event::Start(ref e) | Event::Empty(ref e) => {
                    if local(e.name().as_ref()) == "table" {
                        let range =
                            Range::parse(&attr(e, "ref").ok_or_else(form)?).map_err(|_| form())?;
                        if attr(e, "headerRowCount").as_deref() != Some("0") {
                            let first = range.first;
                            headers.push(Range {
                                first,
                                last: (first.0, range.last.1),
                            });
                        }
                    }
                    break;
                }
                Event::Eof => break,
                _ => {}
            }
            buf.clear();
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
    let named: BTreeMap<Pos, &Value> = range.cells().zip(values.iter().flatten()).collect();
    for header in table_headers(entries, part)? {
        if named.keys().any(|pos| header.holds(*pos)) {
            return Err(refused(Refusal::TableHeader));
        }
    }
    let source = text(entries.get(part).ok_or_else(unreadable)?)?;
    let patched = sheet_part(source, &named)?;
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

    fn sheet1(file: &str) -> String {
        let all = corpus(file);
        String::from_utf8(all["xl/worksheets/sheet1.xml"].clone()).expect("utf-8")
    }

    fn patched(file: &str, cells: &[(&str, Value)]) -> String {
        let named: BTreeMap<Pos, &Value> = cells
            .iter()
            .map(|(r, v)| (cell_pos(r).expect("a cell"), v))
            .collect();
        sheet_part(&sheet1(file), &named).expect("patched")
    }

    #[test]
    fn every_sheet_part_and_workbook_part_of_the_corpus_with_no_cell_written_is_its_source() {
        for file in FILES {
            let all = corpus(file);
            for part in parts::sheet_parts(&all) {
                let source = String::from_utf8(all[&part].clone()).expect("utf-8");
                assert_eq!(
                    sheet_part(&source, &BTreeMap::new()).expect("patched"),
                    source,
                    "{file} {part}"
                );
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
            assert_eq!(patched(file, &[(cell, value)]), want, "{file} {cell}");
        }
    }

    #[test]
    fn a_cell_outside_the_spans_and_the_dimension_grows_them_and_a_text_is_inline() {
        let got = patched("values.xlsx", &[("F2", Value::Text("a & b".to_string()))]);
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
        let got = patched("values.xlsx", &[("A30", Value::Number(1.0))]);
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
            ("_x41_ and _xZZZZ_", "_x41_ and _xZZZZ_"),
            ("", ""),
        ];
        for (text, want) in cases {
            assert_eq!(escaped(text), want, "{text:?}");
        }
    }

    #[test]
    fn the_calculation_flag_is_set_where_it_was_absent_or_zero_and_added_in_its_place() {
        let head = r#"<workbook xmlns="m"><sheets><sheet name="A" sheetId="1"/></sheets>"#;
        let cases = [
            (format!(r#"{head}<calcPr calcId="1"/></workbook>"#), Some(format!(r#"{head}<calcPr fullCalcOnLoad="1" calcId="1"/></workbook>"#))),
            (format!(r#"{head}<calcPr calcId="1" fullCalcOnLoad="0"/></workbook>"#), Some(format!(r#"{head}<calcPr calcId="1" fullCalcOnLoad="1"/></workbook>"#))),
            (format!(r#"{head}<calcPr fullCalcOnLoad='false'></calcPr></workbook>"#), Some(format!(r#"{head}<calcPr fullCalcOnLoad="1"></calcPr></workbook>"#))),
            (format!(r#"{head}<calcPr fullCalcOnLoad="1"/></workbook>"#), None),
            (format!(r#"{head}</workbook>"#), Some(format!(r#"{head}<calcPr fullCalcOnLoad="1"/></workbook>"#))),
            (
                format!(r#"{head}<definedNames><definedName name="R">A!$B$6</definedName></definedNames><extLst/></workbook>"#),
                Some(format!(r#"{head}<definedNames><definedName name="R">A!$B$6</definedName></definedNames><calcPr fullCalcOnLoad="1"/><extLst/></workbook>"#)),
            ),
            (
                r#"<x:workbook xmlns:x="m"><x:sheets/><x:oleSize ref="A1"/></x:workbook>"#.to_string(),
                Some(r#"<x:workbook xmlns:x="m"><x:sheets/><x:calcPr fullCalcOnLoad="1"/><x:oleSize ref="A1"/></x:workbook>"#.to_string()),
            ),
        ];
        for (source, want) in cases {
            assert_eq!(workbook_part(&source).expect("read"), want, "{source}");
        }
    }
}
