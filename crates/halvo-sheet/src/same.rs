use std::collections::BTreeSet;
use std::io::{Cursor, Read};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

type Pos = (u32, u32);

fn listed(bytes: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    let mut all = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).ok()?;
        let name = entry.name().ok()?.to_string();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).ok()?;
        all.push((name, content));
    }
    Some(all)
}

pub(crate) fn package(original: &[u8], copy: &[u8], part: &str, cells: &[Pos]) -> bool {
    let (Some(before), Some(after)) = (listed(original), listed(copy)) else {
        return false;
    };
    if before.len() != after.len() {
        return false;
    }
    before
        .iter()
        .zip(&after)
        .all(|((name, old), (other, new))| {
            name == other
                && if name == part {
                    sheet(old, new, cells)
                } else if name == "xl/workbook.xml" {
                    old == new || workbook(old, new)
                } else {
                    old == new
                }
        })
}

fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

fn value(e: &BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .map(|a| a.value.to_string())
}

fn without(tag: &str, key: &str) -> String {
    let mut out = tag.to_string();
    for quote in ['"', '\''] {
        let start = format!(" {key}={quote}");
        if let Some(at) = out.find(&start) {
            let from = at + start.len();
            if let Some(len) = out[from..].find(quote) {
                out.replace_range(at..from + len + 1, "");
                return out;
            }
        }
    }
    out
}

fn position(text: &str) -> Option<Pos> {
    let letters = text.bytes().take_while(u8::is_ascii_alphabetic).count();
    let (col, row) = text.split_at(letters);
    let col = col.bytes().try_fold(0u32, |acc, b| {
        acc.checked_mul(26)?
            .checked_add(u32::from(b.to_ascii_uppercase() - b'A' + 1))
    })?;
    let row: u32 = row.parse().ok()?;
    Some((row.checked_sub(1)?, col.checked_sub(1)?))
}

fn canonical(
    xml: &str,
    cells: &[Pos],
    known: Option<&BTreeSet<u32>>,
) -> Option<(String, BTreeSet<u32>)> {
    let rows: BTreeSet<u32> = cells.iter().map(|(row, _)| *row).collect();
    let mut reader = Reader::from_reader(xml.as_bytes());
    reader.config_mut().trim_text(false);
    let mut buf = Vec::new();
    let mut out = String::with_capacity(xml.len());
    let mut seen = BTreeSet::new();
    let mut row: Option<(String, String, bool, u32)> = None;
    let mut skipping = 0usize;
    loop {
        let at = reader.buffer_position() as usize;
        let event = reader.read_event_into(&mut buf).ok()?;
        let end = reader.buffer_position() as usize;
        let raw = &xml[at..end];
        if let Event::Eof = event {
            break;
        }
        if skipping > 0 {
            match event {
                Event::Start(_) => skipping += 1,
                Event::End(_) => skipping -= 1,
                _ => {}
            }
            buf.clear();
            continue;
        }
        let mut piece = raw.to_string();
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let empty = matches!(event, Event::Empty(_));
                let name = e.name().as_ref().to_string();
                match local(&name) {
                    "dimension" if row.is_none() => piece = without(raw, "ref"),
                    "sheetData" if empty => piece = format!("{}></{name}>", &raw[..raw.len() - 2]),
                    "row" => {
                        let number = value(e, "r")?.parse::<u32>().ok()?.checked_sub(1)?;
                        seen.insert(number);
                        let mut tag = raw.to_string();
                        if rows.contains(&number) {
                            tag = without(&tag, "spans");
                            if empty {
                                tag = format!("{}>", tag.trim_end_matches("/>"));
                            }
                        }
                        let new = known.is_some_and(|known| !known.contains(&number));
                        if empty && !rows.contains(&number) {
                            out.push_str(&tag);
                        } else if empty {
                            out.push_str(&format!("{tag}</{name}>"));
                        } else {
                            row = Some((tag, String::new(), new, number));
                        }
                        buf.clear();
                        continue;
                    }
                    "c" if row.is_some() => {
                        let pos = position(&value(e, "r")?)?;
                        if cells.contains(&pos) {
                            if !empty {
                                skipping = 1;
                            }
                            buf.clear();
                            continue;
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) if local(e.name().as_ref()) == "row" => {
                if let Some((tag, inner, new, _)) = row.take() {
                    let only_named = inner.trim().is_empty();
                    if !(new && only_named) {
                        out.push_str(&tag);
                        out.push_str(&inner);
                        out.push_str(raw);
                    }
                }
                buf.clear();
                continue;
            }
            _ => {}
        }
        match row.as_mut() {
            Some((_, inner, _, _)) => inner.push_str(&piece),
            None => out.push_str(&piece),
        }
        buf.clear();
    }
    Some((out, seen))
}

fn sheet(old: &[u8], new: &[u8], cells: &[Pos]) -> bool {
    let (Ok(old), Ok(new)) = (std::str::from_utf8(old), std::str::from_utf8(new)) else {
        return false;
    };
    let Some((before, known)) = canonical(old, cells, None) else {
        return false;
    };
    canonical(new, cells, Some(&known)).is_some_and(|(after, _)| after == before)
}

fn calc(xml: &str) -> Option<(Option<String>, String)> {
    let mut reader = Reader::from_reader(xml.as_bytes());
    let mut buf = Vec::new();
    let mut depth = 0usize;
    loop {
        let at = reader.buffer_position() as usize;
        let event = reader.read_event_into(&mut buf).ok()?;
        let end = reader.buffer_position() as usize;
        match &event {
            Event::Eof => return Some((None, xml.to_string())),
            Event::Start(e) | Event::Empty(e) => {
                if depth == 1 && local(e.name().as_ref()) == "calcPr" {
                    let flag = value(e, "fullCalcOnLoad");
                    let tag = without(&xml[at..end], "fullCalcOnLoad");
                    let rest = format!("{}\u{0}{tag}\u{0}{}", &xml[..at], &xml[end..]);
                    return Some((flag, rest));
                }
                if matches!(event, Event::Start(_)) {
                    depth += 1;
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
        buf.clear();
    }
}

fn workbook(old: &[u8], new: &[u8]) -> bool {
    let (Ok(old), Ok(new)) = (std::str::from_utf8(old), std::str::from_utf8(new)) else {
        return false;
    };
    let (Some((_, before)), Some((flag, after))) = (calc(old), calc(new)) else {
        return false;
    };
    if flag.as_deref() != Some("1") {
        return false;
    }
    if before.contains('\u{0}') {
        return before == after;
    }
    let mut pieces = after.splitn(3, '\u{0}');
    let (Some(head), Some(tag), Some(tail)) = (pieces.next(), pieces.next(), pieces.next()) else {
        return false;
    };
    let bare = tag
        .strip_prefix('<')
        .and_then(|t| t.strip_suffix("/>"))
        .is_some_and(|name| local(name) == "calcPr" && !name.contains(' '));
    bare && format!("{head}{tail}") == before
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{corpus_file, ordered, zipped_in_order};

    const SHEET: &str = "xl/worksheets/sheet1.xml";
    const BOOK: &str = "xl/workbook.xml";

    fn source(file: &str) -> Vec<u8> {
        std::fs::read(corpus_file(file)).expect("read the corpus file")
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

    type Case = (
        &'static str,
        Vec<Pos>,
        (&'static str, &'static str, &'static str),
    );

    fn b5() -> Vec<Pos> {
        vec![(4, 1)]
    }

    #[test]
    fn a_package_is_the_same_as_itself_and_with_its_named_cells_changed() {
        for file in [
            "values.xlsx",
            "chart.xlsx",
            "two-sheets.xlsx",
            "rows-5000.xlsx",
        ] {
            assert!(package(&source(file), &source(file), SHEET, &[]), "{file}");
        }
        let b5_written = (
            SHEET,
            r#"<c r="B5"><v>12</v></c>"#,
            r#"<c r="B5" s="2" t="inlineStr"><is><t>x</t></is></c>"#,
        );
        assert!(package(
            &source("values.xlsx"),
            &changed("values.xlsx", &[b5_written]),
            SHEET,
            &b5()
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
            &[(1, 5)]
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
            &[(7, 0)]
        ));
    }

    #[test]
    fn any_other_difference_in_the_sheet_part_is_a_miss() {
        let cases: [Case; 5] = [
            (
                "values.xlsx",
                b5(),
                (
                    SHEET,
                    r#"<c r="B6"><v>15</v></c>"#,
                    r#"<c r="B6"><v>16</v></c>"#,
                ),
            ),
            (
                "chart.xlsx",
                vec![(2, 1)],
                (SHEET, "<f>SUM(B2:B7)</f>", "<f>SUM(B2:B6)</f>"),
            ),
            (
                "values.xlsx",
                b5(),
                (
                    SHEET,
                    r#"<row r="3" spans="1:4">"#,
                    r#"<row r="3" spans="1:5">"#,
                ),
            ),
            (
                "chart.xlsx",
                vec![(7, 0)],
                (
                    SHEET,
                    r#"</row><row r="9" spans="1:2">"#,
                    r#"</row><row r="8"><c r="B8"><v>8</v></c></row><row r="9" spans="1:2">"#,
                ),
            ),
            (
                "merged.xlsx",
                vec![(3, 1)],
                (
                    SHEET,
                    r#"<mergeCell ref="A10:B10"/>"#,
                    r#"<mergeCell ref="A10:C10"/>"#,
                ),
            ),
        ];
        for (file, cells, edit) in cases {
            assert!(
                !package(&source(file), &changed(file, &[edit]), SHEET, &cells),
                "{file} {}",
                edit.1
            );
        }
    }

    #[test]
    fn any_other_difference_in_the_package_is_a_miss() {
        let original = source("values.xlsx");
        let mut all = ordered(&original);
        all.swap(1, 2);
        assert!(!package(&original, &zipped_in_order(&all), SHEET, &b5()));
        let mut all = ordered(&original);
        all.pop();
        assert!(!package(&original, &zipped_in_order(&all), SHEET, &b5()));
        assert!(!package(
            &original,
            &crate::testing::spoiled(&original),
            SHEET,
            &b5()
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
            &[(2, 1)]
        ));
        assert!(!package(&original, b"no zip", SHEET, &b5()));
    }

    #[test]
    fn the_workbook_part_may_differ_only_by_the_calculation_flag() {
        let head = r#"<workbook xmlns="m"><sheets/>"#;
        let cases = [
            (
                r#"<calcPr calcId="1"/>"#,
                r#"<calcPr fullCalcOnLoad="1" calcId="1"/>"#,
                true,
            ),
            (
                r#"<calcPr calcId="1" fullCalcOnLoad="0"/>"#,
                r#"<calcPr calcId="1" fullCalcOnLoad="1"/>"#,
                true,
            ),
            ("", r#"<calcPr fullCalcOnLoad="1"/>"#, true),
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
    }
}
