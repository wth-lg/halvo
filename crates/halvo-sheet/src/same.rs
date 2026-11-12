//! The check of a prepared copy against its source (CP-32, CP-55): the same entry names in the
//! same order, every entry byte for byte but two, the workbook part with the calculation flag
//! as its one change, and the worksheet part byte for byte once the places that the write
//! changes are taken out of both, each of those places holding what the write allows.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use crate::patch::{covered, span_pairs};
use crate::range::Range;
use crate::xml::{self, form, Item, Span, Tag};

/// The bound past which an entry of the copy is not read. The copy's entries are the source's,
/// which `prepare` read within 32 MiB each, and a written sheet part that holds the range.
const ENTRY_MAX: u64 = 64 << 20;

type Archive<'a> = zip::ZipArchive<Cursor<&'a [u8]>>;

fn entry(archive: &mut Archive<'_>, index: usize) -> Option<(String, Vec<u8>)> {
    let entry = archive.by_index(index).ok()?;
    let name = entry.name().ok()?.to_string();
    let mut content = Vec::new();
    entry.take(ENTRY_MAX + 1).read_to_end(&mut content).ok()?;
    (content.len() as u64 <= ENTRY_MAX).then_some((name, content))
}

pub(crate) fn package(
    original: &[u8],
    copy: &[u8],
    (part, book): (&str, &str),
    range: Range,
) -> bool {
    let (Ok(mut before), Ok(mut after)) = (
        zip::ZipArchive::new(Cursor::new(original)),
        zip::ZipArchive::new(Cursor::new(copy)),
    ) else {
        return false;
    };
    if before.len() != after.len() {
        return false;
    }
    for index in 0..before.len() {
        let (Some((name, old)), Some((other, new))) =
            (entry(&mut before, index), entry(&mut after, index))
        else {
            return false;
        };
        let same = name == other
            && if name == part {
                sheet(&old, &new, range)
            } else if name == book {
                workbook(&old, &new)
            } else {
                old == new
            };
        if !same {
            return false;
        }
    }
    true
}

/// A worksheet part with the places that the write changes taken out, and what stood there.
#[derive(Default)]
struct Canonical {
    text: String,
    rows: Vec<u32>,
    spans: BTreeMap<u32, Vec<(u32, u32)>>,
    dimension: Option<Range>,
}

/// A row of the copy that the source does not have: from its start, the end of what it holds
/// so far.
struct Inserted {
    last: usize,
}

fn contains(outer: Range, inner: Range) -> bool {
    outer.first.0 <= inner.first.0
        && outer.first.1 <= inner.first.1
        && inner.last.0 <= outer.last.0
        && inner.last.1 <= outer.last.1
}

/// Reads a worksheet part into its canonical form. With `source` it reads the copy of that
/// source and refuses (with `Err`) a copy whose changed places do not hold what the write
/// allows: an inserted row with any attribute but its number or anything but named cells, a
/// `spans` that does not hold the source's columns and the named ones, a `dimension` whose
/// `ref` does not hold the source's and the range, or a named cell that is missing.
fn canonical(xml: &str, range: Range, source: Option<&Canonical>) -> Result<Canonical, ()> {
    let mut out = Canonical::default();
    let mut text = String::with_capacity(xml.len());
    let mut done = 0usize;
    let mut inserted: Option<Inserted> = None;
    let cols = range.first.1 + 1..=range.last.1 + 1;
    let width = range.cols() as usize;
    let mut seen = vec![false; range.rows() as usize * width];
    let keep = |text: &mut String, done: &mut usize, at: usize| {
        text.push_str(&xml[*done..at]);
        *done = at;
    };
    let mark = |text: &mut String, done: &mut usize, place: Span| {
        text.push_str(&xml[*done..place.at]);
        text.push('\u{0}');
        *done = place.end;
    };
    let close_empty = |text: &mut String, done: &mut usize, tag: &Tag<'_>, name: &str| {
        text.push_str(&xml[*done..tag.span.end - 2]);
        text.push('>');
        text.push_str(&format!("</{}>", tag.named(name)));
        *done = tag.span.end;
    };
    let walked = xml::walk(xml, |item| {
        match item {
            Item::Dimension(tag) => {
                let place = tag.get("ref").ok_or_else(form)?;
                let found = Range::parse(&place.value).map_err(|_| form())?;
                if let Some(source) = source {
                    let old = source.dimension.ok_or_else(form)?;
                    if !(contains(found, old) && contains(found, range)) {
                        return Err(form());
                    }
                }
                out.dimension = Some(found);
                mark(&mut text, &mut done, place.place);
            }
            Item::Data(tag) => {
                if tag.empty {
                    close_empty(&mut text, &mut done, &tag, "sheetData");
                }
            }
            Item::Row(tag, number) => {
                let named = (range.first.0..=range.last.0).contains(&number);
                let known = source.is_none_or(|s| s.rows.binary_search(&number).is_ok());
                if !known {
                    let only_number = tag.attrs.len() == 1 && tag.attrs[0].key == "r";
                    if !named || !only_number || tag.empty {
                        return Err(form());
                    }
                    keep(&mut text, &mut done, tag.span.at);
                    inserted = Some(Inserted { last: tag.span.end });
                    return Ok(());
                }
                out.rows.push(number);
                if named {
                    if let Some(spans) = tag.get("spans") {
                        let pairs = span_pairs(&spans.value)?;
                        if let Some(source) = source {
                            let old = source.spans.get(&number).ok_or_else(form)?;
                            let holds_old = old.iter().all(|(a, b)| covered(&pairs, *a, *b));
                            if !holds_old || !covered(&pairs, *cols.start(), *cols.end()) {
                                return Err(form());
                            }
                        }
                        out.spans.insert(number, pairs);
                        mark(&mut text, &mut done, spans.place);
                    }
                    if tag.empty {
                        close_empty(&mut text, &mut done, &tag, "row");
                    }
                }
            }
            Item::Cell(cell) => {
                let named = range.holds(cell.pos);
                if named && source.is_some() {
                    let at = (cell.pos.0 - range.first.0) as usize * width
                        + (cell.pos.1 - range.first.1) as usize;
                    seen[at] = true;
                }
                if let Some(row) = inserted.as_mut() {
                    if !named || cell.span.at != row.last {
                        return Err(form());
                    }
                    row.last = cell.span.end;
                } else if named {
                    keep(&mut text, &mut done, cell.span.at);
                    done = cell.span.end;
                }
            }
            Item::RowRest(_) => {
                if inserted.is_some() {
                    return Err(form());
                }
            }
            Item::RowEnd(span) => {
                if let Some(row) = inserted.take() {
                    if span.at != row.last {
                        return Err(form());
                    }
                    done = span.end;
                }
            }
            Item::DataEnd(_) => {}
        }
        Ok(())
    });
    walked.map_err(|_| ())?;
    if source.is_some() && !seen.iter().all(|s| *s) {
        return Err(());
    }
    text.push_str(&xml[done..]);
    out.text = text;
    Ok(out)
}

fn sheet(old: &[u8], new: &[u8], range: Range) -> bool {
    let (Ok(old), Ok(new)) = (std::str::from_utf8(old), std::str::from_utf8(new)) else {
        return false;
    };
    let Ok(before) = canonical(old, range, None) else {
        return false;
    };
    canonical(new, range, Some(&before)).is_ok_and(|after| after.text == before.text)
}

/// The text of a tag without its calculation flag, if it has one.
fn without_flag(text: &str, tag: &Tag<'_>) -> String {
    match tag.get("fullCalcOnLoad") {
        Some(flag) => format!(
            "{}{}",
            &text[tag.span.at..flag.whole.at],
            &text[flag.whole.end..tag.span.end]
        ),
        None => text[tag.span.at..tag.span.end].to_string(),
    }
}

/// The copy's workbook part has the calculation flag set (`1` or `true`), and is the source's
/// bytes where the source had it set, else the source's bytes with that one change: the flag's
/// value, the flag added to `calcPr`, or a `calcPr` that holds the flag alone added among the
/// children of `workbook`.
fn workbook(old: &[u8], new: &[u8]) -> bool {
    let (Ok(old), Ok(new)) = (std::str::from_utf8(old), std::str::from_utf8(new)) else {
        return false;
    };
    let (Ok(before), Ok(after)) = (xml::book(old), xml::book(new)) else {
        return false;
    };
    let Some((calc, whole)) = &after.calc else {
        return false;
    };
    if !xml::flag_set(calc) {
        return false;
    }
    match &before.calc {
        Some((old_calc, _)) if xml::flag_set(old_calc) => old == new,
        Some((old_calc, _)) => {
            old[..old_calc.span.at] == new[..calc.span.at]
                && old[old_calc.span.end..] == new[calc.span.end..]
                && without_flag(old, old_calc) == without_flag(new, calc)
        }
        None => {
            let bare = calc.empty && calc.attrs.len() == 1;
            bare && old.len() + (whole.end - whole.at) == new.len()
                && old[..whole.at] == new[..whole.at]
                && old[whole.at..] == new[whole.end..]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus_file, ordered, zipped_in_order};

    const SHEET: &str = "xl/worksheets/sheet1.xml";
    const BOOK: &str = "xl/workbook.xml";
    const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;

    fn source(file: &str) -> Vec<u8> {
        std::fs::read(corpus_file(file)).expect("read the corpus file")
    }

    fn at(cells: &str) -> Range {
        Range::parse(cells).expect("a range")
    }

    fn package(original: &[u8], copy: &[u8], part: &str, cells: &str) -> bool {
        super::package(original, copy, (part, BOOK), at(cells))
    }

    fn changed(file: &str, edits: &[(&str, &str, &str)]) -> Vec<u8> {
        let mut all = ordered(&source(file));
        for (part, from, to) in edits {
            let entry = all.iter_mut().find(|(n, _)| n == part).expect("the part");
            let text = String::from_utf8(entry.1.clone()).expect("utf-8");
            assert_eq!(text.matches(from).count(), 1, "{part} holds {from} once");
            entry.1 = text.replacen(from, to, 1).into_bytes();
        }
        zipped_in_order(&all)
    }

    fn sheet_text(data: &str) -> String {
        format!(r#"<worksheet {NS}>{data}</worksheet>"#)
    }

    fn same_sheet(old: &str, new: &str, cells: &str) -> bool {
        sheet(
            sheet_text(old).as_bytes(),
            sheet_text(new).as_bytes(),
            at(cells),
        )
    }

    #[test]
    fn a_package_is_the_same_as_itself_and_with_its_named_cells_changed() {
        let b5 = (
            SHEET,
            r#"<c r="B5"><v>12</v></c>"#,
            r#"<c r="B5" s="2" t="inlineStr"><is><t>x</t></is></c>"#,
        );
        assert!(package(
            &source("values.xlsx"),
            &changed("values.xlsx", &[b5]),
            SHEET,
            "B5"
        ));
        let grown = [
            (
                SHEET,
                r#"<dimension ref="A1:D21"/>"#,
                r#"<dimension ref="A1:F21"/>"#,
            ),
            (
                SHEET,
                r#"<row r="2" spans="1:4">"#,
                r#"<row r="2" spans="1:6">"#,
            ),
            (
                SHEET,
                r#"<c r="D2" t="b"><v>0</v></c></row>"#,
                r#"<c r="D2" t="b"><v>0</v></c><c r="F2"><v>1</v></c></row>"#,
            ),
        ];
        assert!(package(
            &source("values.xlsx"),
            &changed("values.xlsx", &grown),
            SHEET,
            "F2"
        ));
        let new_row = (
            SHEET,
            r#"</row><row r="9" spans="1:2">"#,
            r#"</row><row r="8"><c r="A8"><v>8</v></c></row><row r="9" spans="1:2">"#,
        );
        assert!(package(
            &source("chart.xlsx"),
            &changed("chart.xlsx", &[new_row]),
            SHEET,
            "A8"
        ));
    }

    #[test]
    fn any_other_difference_in_the_sheet_part_is_a_miss() {
        let cases: [(&str, &str, (&str, &str, &str)); 5] = [
            (
                "values.xlsx",
                "B5",
                (
                    SHEET,
                    r#"<c r="B6"><v>15</v></c>"#,
                    r#"<c r="B6"><v>16</v></c>"#,
                ),
            ),
            (
                "chart.xlsx",
                "B3",
                (SHEET, "<f>SUM(B2:B7)</f>", "<f>SUM(B2:B6)</f>"),
            ),
            (
                "values.xlsx",
                "B5",
                (
                    SHEET,
                    r#"<row r="3" spans="1:4">"#,
                    r#"<row r="3" spans="1:5">"#,
                ),
            ),
            (
                "chart.xlsx",
                "A8",
                (
                    SHEET,
                    r#"</row><row r="9" spans="1:2">"#,
                    r#"</row><row r="8"><c r="B8"><v>8</v></c></row><row r="9" spans="1:2">"#,
                ),
            ),
            (
                "merged.xlsx",
                "B4",
                (
                    SHEET,
                    r#"<mergeCell ref="A10:B10"/>"#,
                    r#"<mergeCell ref="A10:C10"/>"#,
                ),
            ),
        ];
        for (file, cells, edit) in cases {
            assert!(
                !package(&source(file), &changed(file, &[edit]), SHEET, cells),
                "{file} {}",
                edit.1
            );
        }
    }

    #[test]
    fn a_named_cell_out_of_order_in_another_row_twice_or_missing_is_a_miss() {
        let old = r#"<sheetData><row r="1"><c r="A1"/><c r="B1"/><c r="C1"/></row><row r="2"><c r="A2"/></row></sheetData>"#;
        assert!(same_sheet(old, old, "B1"));
        let cases = [
            (
                "order",
                r#"<sheetData><row r="1"><c r="A1"/><c r="C1"/><c r="B1"><v>1</v></c></row><row r="2"><c r="A2"/></row></sheetData>"#,
            ),
            (
                "wrongrow",
                r#"<sheetData><row r="1"><c r="A1"/><c r="C1"/></row><row r="2"><c r="A2"/><c r="B1"><v>1</v></c></row></sheetData>"#,
            ),
            (
                "twice",
                r#"<sheetData><row r="1"><c r="A1"/><c r="B1"><v>1</v></c><c r="B1"><v>1</v></c><c r="C1"/></row><row r="2"><c r="A2"/></row></sheetData>"#,
            ),
            (
                "missing",
                r#"<sheetData><row r="1"><c r="A1"/><c r="C1"/></row><row r="2"><c r="A2"/></row></sheetData>"#,
            ),
        ];
        for (label, new) in cases {
            assert!(!same_sheet(old, new, "B1"), "{label}");
        }
    }

    #[test]
    fn an_inserted_row_holds_its_number_and_named_cells_alone() {
        let old = r#"<sheetData><row r="1"><c r="A1"/></row></sheetData>"#;
        let ok = r#"<sheetData><row r="1"><c r="A1"/></row><row r="3"><c r="A3"><v>1</v></c><c r="B3"><v>2</v></c></row></sheetData>"#;
        assert!(same_sheet(old, ok, "A3:B3"));
        let cases = [
            (
                "rowattr",
                r#"<sheetData><row r="1"><c r="A1"/></row><row r="3" hidden="1"><c r="A3"><v>1</v></c><c r="B3"><v>2</v></c></row></sheetData>"#,
            ),
            (
                "other cell",
                r#"<sheetData><row r="1"><c r="A1"/></row><row r="3"><c r="A3"><v>1</v></c><c r="B3"><v>2</v></c><c r="C3"/></row></sheetData>"#,
            ),
            (
                "text between",
                r#"<sheetData><row r="1"><c r="A1"/></row><row r="3"><c r="A3"><v>1</v></c> <c r="B3"><v>2</v></c></row></sheetData>"#,
            ),
            (
                "extension",
                r#"<sheetData><row r="1"><c r="A1"/></row><row r="3"><c r="A3"><v>1</v></c><c r="B3"><v>2</v></c><extLst/></row></sheetData>"#,
            ),
            (
                "outside the range",
                r#"<sheetData><row r="1"><c r="A1"/></row><row r="2"/><row r="3"><c r="A3"><v>1</v></c><c r="B3"><v>2</v></c></row></sheetData>"#,
            ),
        ];
        for (label, new) in cases {
            assert!(!same_sheet(old, new, "A3:B3"), "{label}");
        }
    }

    #[test]
    fn spans_and_the_dimension_hold_the_sources_and_the_named_cells_and_change_in_their_value_alone(
    ) {
        let old = r#"<dimension ref="A1:B2"/><sheetData><row r="1" spans="1:2"><c r="A1"/></row></sheetData>"#;
        let new = |dimension: &str, spans: &str| {
            format!(
                r#"<dimension {dimension}/><sheetData><row r="1" spans="{spans}"><c r="A1"/><c r="D1"><v>1</v></c></row></sheetData>"#
            )
        };
        assert!(same_sheet(old, &new(r#"ref="A1:D2""#, "1:4"), "D1"));
        assert!(same_sheet(old, &new(r#"ref="A1:E9""#, "1:2 3:5"), "D1"));
        let cases = [
            ("dim", new(r#"ref="A1""#, "1:4")),
            ("dim short of the range", new(r#"ref="A1:C2""#, "1:4")),
            ("dim short of the source", new(r#"ref="A1:D1""#, "1:4")),
            ("dim attribute", new(r#"ref="A1:D2" x="1""#, "1:4")),
            ("spans short", new(r#"ref="A1:D2""#, "1:3")),
            ("spans lose the source", new(r#"ref="A1:D2""#, "3:4")),
            ("spans broken", new(r#"ref="A1:D2""#, "x")),
        ];
        for (label, new) in cases {
            assert!(!same_sheet(old, &new, "D1"), "{label}");
        }
        let dropped = r#"<sheetData><row r="1" spans="1:4"><c r="A1"/><c r="D1"><v>1</v></c></row></sheetData>"#;
        assert!(!same_sheet(old, dropped, "D1"), "the dimension dropped");
    }

    #[test]
    fn single_quotes_and_other_spellings_of_the_changed_places_pass() {
        let old = "<dimension\tref = 'A1:B2'/><sheetData><row\nr='1' spans = '1:&#50;'><c r='A1'/></row><row r=\"2\"/></sheetData>";
        let new = "<dimension\tref = 'A1:D2'/><sheetData><row\nr='1' spans = '1:4'><c r='A1'/><c r=\"D1\"><v>1</v></c></row><row r=\"2\"/></sheetData>";
        assert!(same_sheet(old, new, "D1"));
        let outside = r#"<sheetData><row r="1"><c r="A1"/></row></sheetData><o:row xmlns:o="urn:o" r="1"><o:c r="A1"/></o:row>"#;
        let written = r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData><o:row xmlns:o="urn:o" r="1"><o:c r="A1"/></o:row>"#;
        assert!(same_sheet(outside, written, "A1"));
        let changed = r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData><o:row xmlns:o="urn:o" r="1"><o:c r="A1">x</o:c></o:row>"#;
        assert!(!same_sheet(outside, changed, "A1"));
    }

    #[test]
    fn any_other_difference_in_the_package_is_a_miss() {
        let original = source("values.xlsx");
        let mut all = ordered(&original);
        all.swap(1, 2);
        assert!(!package(&original, &zipped_in_order(&all), SHEET, "B5"));
        let mut all = ordered(&original);
        all.pop();
        assert!(!package(&original, &zipped_in_order(&all), SHEET, "B5"));
        assert!(!package(
            &original,
            &crate::testing::spoiled(&original),
            SHEET,
            "B5"
        ));
        let order = (
            BOOK,
            r#"<sheet name="Input" sheetId="1" r:id="rId1"/><sheet name="Summary" sheetId="2" r:id="rId2"/>"#,
            r#"<sheet name="Summary" sheetId="2" r:id="rId2"/><sheet name="Input" sheetId="1" r:id="rId1"/>"#,
        );
        assert!(!package(
            &source("two-sheets.xlsx"),
            &changed("two-sheets.xlsx", &[order]),
            SHEET,
            "B3"
        ));
        assert!(!package(&original, b"no zip", SHEET, "B5"));
        let mut all = ordered(&original);
        all[1].0 = "_rels/other.rels".to_string();
        assert!(!package(&original, &zipped_in_order(&all), SHEET, "B5"));
    }

    #[test]
    fn the_workbook_part_may_differ_only_by_the_calculation_flag() {
        let head = format!(r#"<workbook {NS}><sheets/>"#);
        let cases = [
            (
                r#"<calcPr calcId="1"/>"#,
                r#"<calcPr fullCalcOnLoad="1" calcId="1"/>"#,
                true,
            ),
            (
                r#"<calcPr calcId="1"/>"#,
                r#"<calcPr calcId="1" fullCalcOnLoad="true"/>"#,
                true,
            ),
            (
                r#"<calcPr calcId="1" fullCalcOnLoad="0"/>"#,
                r#"<calcPr calcId="1" fullCalcOnLoad="1"/>"#,
                true,
            ),
            (
                r#"<calcPr fullCalcOnLoad='false'></calcPr>"#,
                r#"<calcPr fullCalcOnLoad='1'></calcPr>"#,
                true,
            ),
            ("", r#"<calcPr fullCalcOnLoad="1"/>"#, true),
            (
                r#"<calcPr fullCalcOnLoad="1"/>"#,
                r#"<calcPr fullCalcOnLoad="1"/>"#,
                true,
            ),
            (
                r#"<calcPr fullCalcOnLoad="true"/>"#,
                r#"<calcPr fullCalcOnLoad="1"/>"#,
                false,
            ),
            (r#"<calcPr calcId="1"/>"#, r#"<calcPr calcId="1"/>"#, false),
            ("", "", false),
            (
                r#"<calcPr calcId="1"/>"#,
                r#"<calcPr fullCalcOnLoad="1" calcId="2"/>"#,
                false,
            ),
            (
                r#"<calcPr calcId="1" fullCalcOnLoad="0"/>"#,
                r#"<calcPr calcId="1" fullCalcOnLoad="0" x="1"/>"#,
                false,
            ),
            ("", r#"<calcPr fullCalcOnLoad="1" calcId="1"/>"#, false),
            ("", r#"<calcPr fullCalcOnLoad="0"/>"#, false),
            ("", r#"<calcPr fullCalcOnLoad="1"></calcPr>"#, false),
        ];
        for (old, new, want) in cases {
            let old = format!("{head}{old}</workbook>");
            let new = format!("{head}{new}</workbook>");
            assert_eq!(
                workbook(old.as_bytes(), new.as_bytes()),
                want,
                "{old} -> {new}"
            );
        }
        let elsewhere = format!(r#"{head}</workbook><!--x-->"#);
        let moved = format!(r#"{head}<calcPr fullCalcOnLoad="1"/></workbook><!--y-->"#);
        assert!(!workbook(elsewhere.as_bytes(), moved.as_bytes()));
    }
}
