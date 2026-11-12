//! The walks over a worksheet part and over the workbook part that the writer and the check
//! share (CP-54), and the pass that finds the rectangle of a sheet's used cells the way the
//! reader places them (CP-53).
//!
//! An element is matched by the spreadsheet namespace under whatever prefix; an element of
//! another namespace is carried as bytes. An attribute is read as the XML reader gives it, and
//! its place in the text comes from the reader's own slices of that text, so neither the writer
//! nor the check searches the text for an attribute. One spelling is refused all the same: an
//! entity in the `ref` of `dimension`, which these walks read as the plain spelling but which
//! the reader of the read-back cannot read, so `prepare` refuses that workbook as unreadable
//! before any write.

use std::borrow::Cow;

use quick_xml::events::{BytesStart, Event};
use quick_xml::name::{Namespace, ResolveResult};
use quick_xml::reader::NsReader;
use quick_xml::{Reader, XmlVersion};

use crate::error::{Error, Refusal};
use crate::range::Range;

/// The spreadsheet namespace.
pub(crate) const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

pub(crate) type Pos = (u32, u32);

pub(crate) fn form() -> Error {
    Error::Workbook(Refusal::Part)
}

pub(crate) fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

/// A place in a text, from `at` up to `end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) at: usize,
    pub(crate) end: usize,
}

/// An attribute as the reader gives it.
pub(crate) struct Attr<'a> {
    pub(crate) key: &'a str,
    /// The value with entities and white space handled as XML 1.0 asks.
    pub(crate) value: Cow<'a, str>,
    /// The place of the value as written, between its quotes.
    pub(crate) place: Span,
    /// The whole attribute, from the white space before its name to its closing quote.
    pub(crate) whole: Span,
}

/// A start tag or an empty tag.
pub(crate) struct Tag<'a> {
    pub(crate) span: Span,
    pub(crate) empty: bool,
    pub(crate) prefix: &'a str,
    pub(crate) attrs: Vec<Attr<'a>>,
}

impl<'a> Tag<'a> {
    pub(crate) fn get(&self, key: &str) -> Option<&Attr<'a>> {
        self.attrs.iter().find(|attr| attr.key == key)
    }

    /// `name` under this tag's prefix, which is bound to the spreadsheet namespace for the
    /// element and for its children.
    pub(crate) fn named(&self, name: &str) -> String {
        if self.prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}:{name}", self.prefix)
        }
    }

    /// The end of the element's name inside the tag: where a new attribute goes.
    pub(crate) fn name_end(&self, name: &str) -> usize {
        self.span.at + 1 + self.named(name).len()
    }
}

/// `part`, a slice of `text`, as a slice of `text` with its place.
fn within<'a>(text: &'a str, part: &str) -> Result<(usize, &'a str), Error> {
    let at = (part.as_ptr() as usize)
        .checked_sub(text.as_ptr() as usize)
        .ok_or_else(form)?;
    let slice = text.get(at..at + part.len()).ok_or_else(form)?;
    Ok((at, slice))
}

fn tag<'a>(xml: &'a str, e: &BytesStart<'_>, span: Span, empty: bool) -> Result<Tag<'a>, Error> {
    let prefix = match e.name().prefix() {
        Some(prefix) => within(xml, prefix.as_ref())?.1,
        None => "",
    };
    let mut attrs = Vec::new();
    for attribute in e.attributes() {
        let attribute = attribute.map_err(|_| form())?;
        let (key_at, key) = within(xml, attribute.key.as_ref())?;
        let Cow::Borrowed(raw) = &attribute.value else {
            return Err(form());
        };
        let (value_at, raw) = within(xml, raw)?;
        let value = match attribute
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|_| form())?
        {
            Cow::Borrowed(_) => Cow::Borrowed(raw),
            Cow::Owned(text) => Cow::Owned(text),
        };
        let place = Span {
            at: value_at,
            end: value_at + raw.len(),
        };
        let start = xml[..key_at]
            .trim_end_matches([' ', '\t', '\r', '\n'])
            .len();
        let whole = Span {
            at: start,
            end: place.end + 1,
        };
        attrs.push(Attr {
            key,
            value,
            place,
            whole,
        });
    }
    Ok(Tag {
        span,
        empty,
        prefix,
        attrs,
    })
}

/// A cell of a row: its tag, its position, the place of the whole element, and what its
/// formula is, if it has one (an element named `f` of any namespace, as the reader reads it).
pub(crate) struct Cell<'a> {
    pub(crate) tag: Tag<'a>,
    pub(crate) pos: Pos,
    pub(crate) span: Span,
    pub(crate) shared: bool,
    pub(crate) array: Option<Range>,
}

/// What the walk of a worksheet part meets, in the order of the text.
pub(crate) enum Item<'a> {
    /// `dimension`: the worksheet's own element before `sheetData`.
    Dimension(Tag<'a>),
    /// The start tag or the empty tag of `sheetData`.
    Data(Tag<'a>),
    /// A row: its tag and its number counted from 0.
    Row(Tag<'a>, u32),
    /// A cell of the current row.
    Cell(Cell<'a>),
    /// The place of the first element of the current row after its cells (`extLst`).
    RowRest(usize),
    /// The end tag of the current row.
    RowEnd(Span),
    /// The end tag of `sheetData`.
    DataEnd(Span),
}

struct Open<'a> {
    tag: Tag<'a>,
    pos: Pos,
    shared: bool,
    array: Option<Range>,
}

struct RowState {
    number: u32,
    last_col: Option<u32>,
    rest: bool,
}

fn number(tag: &Tag<'_>) -> Result<u32, Error> {
    let value = tag.get("r").ok_or_else(form)?;
    let digits = !value.value.is_empty() && value.value.bytes().all(|b| b.is_ascii_digit());
    let number: u32 = value.value.parse().map_err(|_| form())?;
    if !digits || number == 0 {
        return Err(form());
    }
    Ok(number - 1)
}

fn cell_pos(tag: &Tag<'_>) -> Result<Pos, Error> {
    let value = tag.get("r").ok_or_else(form)?;
    let range = Range::parse(&value.value).map_err(|_| form())?;
    if range.first != range.last {
        return Err(form());
    }
    Ok(range.first)
}

/// Walks a worksheet part and hands each item to `each`, refusing a part that is not in the
/// form that [`Refusal::Part`] describes.
pub(crate) fn walk<'a>(
    xml: &'a str,
    mut each: impl FnMut(Item<'a>) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut reader = NsReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut depth = 0usize;
    let mut skip: Option<usize> = None;
    let mut dimension = false;
    let mut data = false;
    let mut in_data = false;
    let mut last_row: Option<u32> = None;
    let mut row: Option<RowState> = None;
    let mut cell: Option<Open<'a>> = None;
    loop {
        let at = reader.buffer_position() as usize;
        let (ns, event) = reader.read_resolved_event().map_err(|_| form())?;
        let main = matches!(ns, ResolveResult::Bound(Namespace(n)) if n == MAIN);
        let end = reader.buffer_position() as usize;
        let span = Span { at, end };
        let (e, empty) = match &event {
            Event::Eof => break,
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or_else(form)?;
                if skip == Some(depth) {
                    skip = None;
                } else if skip.is_some() {
                } else if cell.is_some() && depth == 3 {
                    if let Some(open) = cell.take() {
                        let whole = Span {
                            at: open.tag.span.at,
                            end,
                        };
                        each(Item::Cell(Cell {
                            tag: open.tag,
                            pos: open.pos,
                            span: whole,
                            shared: open.shared,
                            array: open.array,
                        }))?;
                    }
                } else if cell.is_some() {
                } else if row.is_some() && depth == 2 {
                    row = None;
                    each(Item::RowEnd(span))?;
                } else if in_data && depth == 1 {
                    in_data = false;
                    each(Item::DataEnd(span))?;
                }
                continue;
            }
            _ => continue,
        };
        let qname = e.name();
        let name = local(qname.as_ref());
        let read_as_cells = matches!(name, "row" | "c");
        let carried = |depth: usize| if empty { None } else { Some(depth) };
        if name == "sheetData" && (data || depth != 1 || !main || skip.is_some()) {
            return Err(form());
        }
        if skip.is_some() {
            if in_data && read_as_cells {
                return Err(form());
            }
        } else if depth == 0 {
            if !(main && name == "worksheet") {
                return Err(form());
            }
        } else if cell.is_some() {
            if read_as_cells {
                return Err(form());
            }
            if depth == 4 && name == "f" {
                let f = tag(xml, e, span, empty)?;
                if let Some(open) = cell.as_mut() {
                    match f.get("t").map(|t| t.value.as_ref()) {
                        Some("shared") => open.shared = true,
                        Some("array") | Some("dataTable") => {
                            open.array = Some(match f.get("ref") {
                                Some(r) => Range::parse(&r.value).map_err(|_| form())?,
                                None => Range {
                                    first: open.pos,
                                    last: open.pos,
                                },
                            });
                        }
                        _ => {}
                    }
                }
            }
        } else if let Some(current) = row.as_mut().filter(|_| depth == 3) {
            if main && name == "c" {
                let found = tag(xml, e, span, empty)?;
                let pos = cell_pos(&found)?;
                let ordered = current.last_col.is_none_or(|last| pos.1 > last);
                if current.rest || pos.0 != current.number || !ordered {
                    return Err(form());
                }
                current.last_col = Some(pos.1);
                if empty {
                    each(Item::Cell(Cell {
                        tag: found,
                        pos,
                        span,
                        shared: false,
                        array: None,
                    }))?;
                } else {
                    cell = Some(Open {
                        tag: found,
                        pos,
                        shared: false,
                        array: None,
                    });
                }
            } else if read_as_cells || (main && name != "extLst") {
                return Err(form());
            } else {
                if main && !current.rest {
                    current.rest = true;
                    each(Item::RowRest(at))?;
                }
                skip = carried(depth);
            }
        } else if in_data && depth == 2 {
            if main && name == "row" {
                let found = tag(xml, e, span, empty)?;
                let n = number(&found)?;
                if last_row.is_some_and(|last| n <= last) {
                    return Err(form());
                }
                last_row = Some(n);
                each(Item::Row(found, n))?;
                if !empty {
                    row = Some(RowState {
                        number: n,
                        last_col: None,
                        rest: false,
                    });
                }
            } else if main || read_as_cells {
                return Err(form());
            } else {
                skip = carried(depth);
            }
        } else if depth == 1 && main && name == "dimension" && !data {
            if dimension {
                return Err(form());
            }
            dimension = true;
            each(Item::Dimension(tag(xml, e, span, empty)?))?;
            skip = carried(depth);
        } else if depth == 1 && name == "sheetData" {
            data = true;
            each(Item::Data(tag(xml, e, span, empty)?))?;
            in_data = !empty;
        } else {
            skip = carried(depth);
        }
        if !empty {
            depth += 1;
        }
    }
    if depth != 0 || !data {
        return Err(form());
    }
    Ok(())
}

/// The workbook part as the writer and the check read it.
pub(crate) struct Book<'a> {
    /// The start tag of `workbook`.
    pub(crate) root: Tag<'a>,
    /// `calcPr`, a child of `workbook`, with the place of the whole element.
    pub(crate) calc: Option<(Tag<'a>, Span)>,
    /// The place of the first child of `workbook` that the schema puts after `calcPr`.
    pub(crate) later: Option<usize>,
    /// The place of the end tag of `workbook`.
    pub(crate) end: usize,
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

/// Reads a workbook part: its root must be `workbook` of the spreadsheet namespace, with at
/// most one `calcPr` among its children.
pub(crate) fn book(xml: &str) -> Result<Book<'_>, Error> {
    let mut reader = NsReader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut depth = 0usize;
    let mut root = None;
    let mut calc: Option<(Tag<'_>, Span)> = None;
    let mut open_calc = false;
    let mut later = None;
    loop {
        let at = reader.buffer_position() as usize;
        let (ns, event) = reader.read_resolved_event().map_err(|_| form())?;
        let main = matches!(ns, ResolveResult::Bound(Namespace(n)) if n == MAIN);
        let end = reader.buffer_position() as usize;
        let span = Span { at, end };
        match &event {
            Event::Eof => return Err(form()),
            Event::Start(e) | Event::Empty(e) => {
                let empty = matches!(event, Event::Empty(_));
                let qname = e.name();
                let name = local(qname.as_ref());
                if depth == 0 {
                    if !(main && name == "workbook") || empty {
                        return Err(form());
                    }
                    root = Some(tag(xml, e, span, empty)?);
                } else if depth == 1 && main && name == "calcPr" {
                    if calc.is_some() {
                        return Err(form());
                    }
                    calc = Some((tag(xml, e, span, empty)?, span));
                    open_calc = !empty;
                } else if depth == 1 && main && AFTER_CALC.contains(&name) && later.is_none() {
                    later = Some(at);
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::End(_) => {
                depth = depth.checked_sub(1).ok_or_else(form)?;
                if depth == 1 && open_calc {
                    open_calc = false;
                    if let Some((_, whole)) = calc.as_mut() {
                        whole.end = end;
                    }
                }
                if depth == 0 {
                    let root = root.ok_or_else(form)?;
                    return Ok(Book {
                        root,
                        calc,
                        later,
                        end: at,
                    });
                }
            }
            _ => {}
        }
    }
}

/// The value of the calculation flag of a `calcPr` tag, if it is set (`1` or `true`).
pub(crate) fn flag_set(calc: &Tag<'_>) -> bool {
    calc.get("fullCalcOnLoad")
        .is_some_and(|flag| matches!(flag.value.as_ref(), "1" | "true"))
}

/// The rectangle of the cells of a worksheet part, found the way the reader places cells: in
/// the first element named `sheetData`, every element named `c` at its reference, or after the
/// cell before it in its row when it has none, rows counted the same way. `None` when the part
/// holds no cell.
pub(crate) fn extent(xml: &str) -> Result<Option<(Pos, Pos)>, Error> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = true;
    loop {
        match reader.read_event().map_err(|_| form())? {
            Event::Start(e) if local(e.name().as_ref()) == "sheetData" => break,
            Event::Eof => return Ok(None),
            _ => {}
        }
    }
    let raw_r = |e: &BytesStart<'_>| -> Result<Option<String>, Error> {
        for attribute in e.attributes() {
            let attribute = attribute.map_err(|_| form())?;
            if attribute.key.as_ref() == "r" {
                return Ok(Some(attribute.value.to_string()));
            }
        }
        Ok(None)
    };
    let mut row_index = 0u32;
    let mut col_index = 0u32;
    let mut found: Option<(Pos, Pos)> = None;
    loop {
        match reader.read_event().map_err(|_| form())? {
            Event::Start(e) => match local(e.name().as_ref()) {
                "row" => {
                    if let Some(r) = raw_r(&e)? {
                        row_index = reference(&r, false)?.0;
                    }
                }
                "c" => {
                    let pos = match raw_r(&e)? {
                        Some(r) => {
                            let pos = reference(&r, true)?;
                            col_index = pos.1;
                            pos
                        }
                        None => (row_index, col_index),
                    };
                    found = Some(match found {
                        None => (pos, pos),
                        Some((low, high)) => (
                            (low.0.min(pos.0), low.1.min(pos.1)),
                            (high.0.max(pos.0), high.1.max(pos.1)),
                        ),
                    });
                    loop {
                        match reader.read_event().map_err(|_| form())? {
                            Event::End(e) if local(e.name().as_ref()) == "c" => break,
                            Event::Eof => return Err(form()),
                            _ => {}
                        }
                    }
                    col_index = col_index.checked_add(1).ok_or_else(form)?;
                }
                _ => {}
            },
            Event::End(e) => match local(e.name().as_ref()) {
                "row" => {
                    row_index = row_index.checked_add(1).ok_or_else(form)?;
                    col_index = 0;
                }
                "sheetData" => break,
                _ => {}
            },
            Event::Eof => return Err(form()),
            _ => {}
        }
    }
    Ok(found)
}

/// A reference as the reader reads one: letters for the column, then digits for the row,
/// counted from 1. With `column` false the letters may be absent and the column is 0.
fn reference(text: &str, column: bool) -> Result<Pos, Error> {
    let letters = text.bytes().take_while(u8::is_ascii_alphabetic).count();
    let (col_text, row_text) = text.split_at(letters);
    let digits = !row_text.is_empty() && row_text.bytes().all(|b| b.is_ascii_digit());
    if !digits || (column && letters == 0) || letters > 6 {
        return Err(form());
    }
    let col = col_text.bytes().fold(0u32, |acc, b| {
        acc * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1)
    });
    let row: u32 = row_text.parse().map_err(|_| form())?;
    let row = row.checked_sub(1).ok_or_else(form)?;
    Ok((row, col.saturating_sub(1)))
}

/// Whether the rectangle that holds `extent` and `range` holds at most `bound` cells.
pub(crate) fn fits(extent: Option<(Pos, Pos)>, range: Option<Range>, bound: u64) -> bool {
    let parts = [extent, range.map(|r| (r.first, r.last))];
    let mut all = parts.into_iter().flatten();
    let Some(first) = all.next() else {
        return true;
    };
    let (low, high) = all.fold(first, |(low, high), (a, b)| {
        (
            (low.0.min(a.0), low.1.min(a.1)),
            (high.0.max(b.0), high.1.max(b.1)),
        )
    });
    let rows = u64::from(high.0 - low.0) + 1;
    let cols = u64::from(high.1 - low.1) + 1;
    rows.saturating_mul(cols) <= bound
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(xml: &str) -> Result<Vec<String>, Error> {
        let mut seen = Vec::new();
        walk(xml, |item| {
            seen.push(match item {
                Item::Dimension(t) => {
                    format!("dimension {}", t.get("ref").map_or("", |r| &r.value))
                }
                Item::Data(t) => format!("data empty={}", t.empty),
                Item::Row(t, n) => format!("row {n} empty={}", t.empty),
                Item::Cell(c) => format!("cell {:?} {}", c.pos, &xml[c.span.at..c.span.end]),
                Item::RowRest(_) => "rest".to_string(),
                Item::RowEnd(_) => "row end".to_string(),
                Item::DataEnd(_) => "data end".to_string(),
            });
            Ok(())
        })?;
        Ok(seen)
    }

    const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;

    #[test]
    fn the_walk_meets_the_parts_of_the_spreadsheet_namespace_under_any_prefix() {
        let plain = format!(
            r#"<worksheet {NS}><dimension ref="A1:B2"/><sheetData><row r="1"><c r="A1"><v>1</v></c><c r="B1"/></row><row r="2"/></sheetData></worksheet>"#
        );
        let want = vec![
            "dimension A1:B2",
            "data empty=false",
            "row 0 empty=false",
            r#"cell (0, 0) <c r="A1"><v>1</v></c>"#,
            r#"cell (0, 1) <c r="B1"/>"#,
            "row end",
            "row 1 empty=true",
            "data end",
        ];
        assert_eq!(items(&plain).expect("walked"), want);
        let prefix = |text: &str| text.replace('<', "<x:").replace("<x:/", "</x:");
        let prefixed = prefix(&plain).replace(r#"xmlns="#, r#"xmlns:x="#);
        let want: Vec<String> = want.iter().map(|w| prefix(w)).collect();
        assert_eq!(items(&prefixed).expect("walked"), want);
    }

    #[test]
    fn an_element_of_another_namespace_is_carried_and_one_the_reader_would_take_is_refused() {
        let other = format!(
            r#"<worksheet {NS} xmlns:o="urn:o"><sheetData><o:x><o:y/></o:x><row r="1"><o:z/><c r="A1"/><extLst/></row></sheetData><o:row r="9"><o:c r="A9"/></o:row></worksheet>"#
        );
        assert_eq!(
            items(&other).expect("walked"),
            vec![
                "data empty=false",
                "row 0 empty=false",
                r#"cell (0, 0) <c r="A1"/>"#,
                "rest",
                "row end",
                "data end"
            ]
        );
        let refused = [
            format!(
                r#"<worksheet {NS} xmlns:o="urn:o"><sheetData><o:row r="1"/></sheetData></worksheet>"#
            ),
            format!(
                r#"<worksheet {NS} xmlns:o="urn:o"><sheetData><o:x><row r="1"/></o:x></sheetData></worksheet>"#
            ),
            format!(
                r#"<worksheet {NS} xmlns:o="urn:o"><sheetData><row r="1"><o:c r="A1"/></row></sheetData></worksheet>"#
            ),
            format!(r#"<worksheet {NS} xmlns:o="urn:o"><o:sheetData/><sheetData/></worksheet>"#),
            format!(r#"<worksheet {NS} xmlns:o="urn:o"><o:sheetData/></worksheet>"#),
            format!(r#"<worksheet {NS}><sheetData/><sheetData/></worksheet>"#),
            format!(
                r#"<worksheet {NS}><sheetData><row r="1"><c r="A1"/><extLst/><c r="B1"/></row></sheetData></worksheet>"#
            ),
            format!(r#"<worksheet {NS}><sheetData><row r="1"><x/></row></sheetData></worksheet>"#),
            format!(
                r#"<worksheet {NS}><sheetData><row r="1"><c r="A1"><c r="A1"/></c></row></sheetData></worksheet>"#
            ),
            r#"<worksheet xmlns="urn:other"><sheetData/></worksheet>"#.to_string(),
            format!(r#"<chartsheet {NS}/>"#),
            format!(r#"<chartsheet {NS}><sheetData/></chartsheet>"#),
            format!(r#"<worksheet {NS}><sheetData><extLst/></sheetData></worksheet>"#),
            format!(r#"<worksheet {NS}><sheetData/>"#),
            format!(
                r#"<worksheet {NS}><dimension ref="A1"/><dimension ref="A1"/><sheetData/></worksheet>"#
            ),
        ];
        for xml in refused {
            assert_eq!(items(&xml).err(), Some(form()), "{xml}");
        }
    }

    #[test]
    fn rows_and_cells_stand_once_in_order_with_their_own_numbers() {
        let ok = format!(
            r#"<worksheet {NS}><sheetData><row r="1"><c r="A1"/><c r="C1"/></row><row r="3"/></sheetData></worksheet>"#
        );
        assert!(items(&ok).is_ok());
        let refused = [
            r#"<row r="0"/>"#,
            r#"<row r="2"/><row r="2"/>"#,
            r#"<row r="2"/><row r="1"/>"#,
            r#"<row r="1"><c r="A1"/><c r="A1"/></row>"#,
            r#"<row r="1"><c r="B1"/><c r="A1"/></row>"#,
            r#"<row r="1"><c r="A2"/></row>"#,
            r#"<row r="1"><c/></row>"#,
            r#"<row><c r="A1"/></row>"#,
            r#"<row r="+1"/>"#,
            r#"<row r="1"><c r="A1:B1"/></row>"#,
        ];
        for rows in refused {
            let xml = format!(r#"<worksheet {NS}><sheetData>{rows}</sheetData></worksheet>"#);
            assert_eq!(items(&xml).err(), Some(form()), "{rows}");
        }
    }

    #[test]
    fn an_attribute_value_is_found_at_its_place_in_any_spelling() {
        let spellings = [
            "<row r=\"1\" spans=\"1:4\"/>",
            "<row\tr=\"1\"\tspans=\"1:4\"/>",
            "<row\nr=\"1\"\nspans=\"1:4\"/>",
            "<row r = \"1\" spans = \"1:4\"/>",
            "<row r='1' spans='1:4'/>",
            "<row r=\"1\" spans=\"1:&#52;\"/>",
        ];
        for row in spellings {
            let xml = format!(r#"<worksheet {NS}><sheetData>{row}</sheetData></worksheet>"#);
            let mut got = None;
            walk(&xml, |item| {
                if let Item::Row(t, _) = item {
                    let spans = t.get("spans").expect("spans");
                    let whole = &xml[spans.whole.at..spans.whole.end];
                    let place = &xml[spans.place.at..spans.place.end];
                    got = Some((
                        spans.value.to_string(),
                        place.to_string(),
                        whole.to_string(),
                    ));
                }
                Ok(())
            })
            .expect("walked");
            let (value, place, whole) = got.expect("a row");
            assert_eq!(value, "1:4", "{row}");
            let raw = if row.contains("&#52;") {
                "1:&#52;"
            } else {
                "1:4"
            };
            assert_eq!(place, raw, "{row}");
            assert!(whole.starts_with([' ', '\t', '\n']), "{row}: {whole}");
            assert!(whole.trim_start().starts_with("spans"), "{row}: {whole}");
            assert!(
                whole.ends_with('"') || whole.ends_with('\''),
                "{row}: {whole}"
            );
        }
    }

    #[test]
    fn the_rectangle_of_the_used_cells_is_found_as_the_reader_places_them() {
        let cases: [(&str, Option<(Pos, Pos)>); 5] = [
            (r#"<sheetData/>"#, None),
            (
                r#"<sheetData><row r="2"><c r="B2"/><c/></row><row><c/></row></sheetData>"#,
                Some(((1, 0), (2, 2))),
            ),
            (
                r#"<sheetData><row r="1"><c r="A1"/></row><row r="1048576"><c r="XFD1048576"/></row></sheetData>"#,
                Some(((0, 0), (1_048_575, 16_383))),
            ),
            (
                r#"<o:sheetData xmlns:o="urn:o"><o:row r="5"><o:c r="E5"/></o:row></o:sheetData><sheetData><row r="1"><c r="A1"/></row></sheetData>"#,
                Some(((4, 4), (4, 4))),
            ),
            (
                r#"<sheetData><row r="3"><c r="C3"><v>1</v></c></row></sheetData><row r="9"><c r="Z9"/></row>"#,
                Some(((2, 2), (2, 2))),
            ),
        ];
        for (data, want) in cases {
            let xml = format!(r#"<worksheet {NS}>{data}</worksheet>"#);
            assert_eq!(extent(&xml), Ok(want), "{data}");
        }
        let broken = format!(
            r#"<worksheet {NS}><sheetData><row r="1"><c r="1A"/></row></sheetData></worksheet>"#
        );
        assert_eq!(extent(&broken), Err(form()));
        let a1 = Range::parse("A1").ok();
        assert!(fits(Some(((0, 0), (999, 999))), a1, 1_000_000));
        assert!(!fits(Some(((0, 0), (1000, 999))), a1, 1_000_000));
        assert!(!fits(
            Some(((0, 0), (0, 0))),
            Range::parse("ALL1001").ok(),
            1_000_000
        ));
        assert!(fits(None, None, 0));
    }

    #[test]
    fn the_workbook_part_gives_its_calculation_tag_and_the_places_around_it() {
        let ns = NS;
        let xml =
            format!(r#"<workbook {ns}><sheets/><calcPr calcId="1"></calcPr><extLst/></workbook>"#);
        let found = book(&xml).expect("read");
        let (calc, whole) = found.calc.expect("calcPr");
        assert_eq!(&xml[whole.at..whole.end], r#"<calcPr calcId="1"></calcPr>"#);
        assert_eq!(calc.name_end("calcPr"), whole.at + "<calcPr".len());
        assert!(!flag_set(&calc));
        assert_eq!(found.later, Some(xml.find("<extLst").expect("extLst")));
        assert_eq!(found.end, xml.find("</workbook>").expect("end"));
        let other = format!(r#"<workbook {ns} xmlns:o="urn:o"><o:calcPr/><sheets/></workbook>"#);
        assert!(book(&other).expect("read").calc.is_none());
        for refused in [
            r#"<workbook xmlns="urn:x"/>"#.to_string(),
            format!(r#"<workbook {ns}><calcPr/><calcPr/></workbook>"#),
        ] {
            assert!(book(&refused).is_err(), "{refused}");
        }
    }
}
