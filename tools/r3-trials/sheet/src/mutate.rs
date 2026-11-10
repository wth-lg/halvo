use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::parts::{self, Kind};

#[derive(Clone, Debug, PartialEq)]
pub enum Mutation {
    RemoveKind(Kind),
    ReplaceCell {
        sheet: String,
        cell: String,
    },
    SetCached {
        sheet: String,
        cell: String,
        value: Option<String>,
    },
    SetFormula {
        sheet: String,
        cell: String,
        text: String,
    },
}

impl Mutation {
    pub fn describe(&self) -> String {
        match self {
            Mutation::RemoveKind(kind) => format!("remove={kind}"),
            Mutation::ReplaceCell { sheet, cell } => format!("replace_cell={sheet}!{cell}"),
            Mutation::SetCached { sheet, cell, value } => format!(
                "cached={sheet}!{cell}:{}",
                value.as_deref().unwrap_or("removed")
            ),
            Mutation::SetFormula { sheet, cell, .. } => format!("formula_text={sheet}!{cell}"),
        }
    }
}

pub fn entries(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("zip: {e}"))?;
    let mut all = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| format!("zip: {e}"))?;
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("zip read: {e}"))?;
        all.push((
            entry
                .name()
                .map_err(|e| format!("zip name: {e}"))?
                .to_string(),
            bytes,
        ));
    }
    Ok(all)
}

fn write_entries(path: &Path, all: &[(String, Vec<u8>)]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in all {
        zip.start_file(name.as_str(), options)
            .map_err(|e| format!("zip write: {e}"))?;
        zip.write_all(bytes)
            .map_err(|e| format!("zip write: {e}"))?;
    }
    zip.finish().map_err(|e| format!("zip finish: {e}"))?;
    Ok(())
}

fn attr(start: &BytesStart<'_>, key: &str) -> Option<String> {
    start
        .attributes()
        .flatten()
        .find(|a| parts::local(a.key.as_ref()) == key)
        .map(|a| a.value.to_string())
}

pub fn sheet_part(all: &[(String, Vec<u8>)], sheet: &str) -> Result<String, String> {
    let entries = all.iter().cloned().collect();
    parts::sheet_parts(&entries)
        .into_iter()
        .find(|(name, _)| name == sheet)
        .map(|(_, part)| part)
        .ok_or_else(|| format!("no sheet {sheet}"))
}

fn owned(event: Event<'_>) -> Event<'static> {
    event.into_owned()
}

fn remove_element(xml: &[u8], element: &str) -> Result<Vec<u8>, String> {
    let mut reader = parts::new_reader(xml);
    let mut writer = Writer::new(Vec::new());
    let mut buf = Vec::new();
    let mut skip = 0usize;
    loop {
        let event = owned(
            reader
                .read_event_into(&mut buf)
                .map_err(|e| format!("xml: {e}"))?,
        );
        buf.clear();
        if matches!(event, Event::Eof) {
            break;
        }
        if skip > 0 {
            match event {
                Event::Start(_) => skip += 1,
                Event::End(_) => skip -= 1,
                _ => {}
            }
            continue;
        }
        match &event {
            Event::Start(e) if parts::local(e.name().as_ref()) == element => {
                skip = 1;
                continue;
            }
            Event::Empty(e) if parts::local(e.name().as_ref()) == element => continue,
            _ => {}
        }
        writer
            .write_event(event)
            .map_err(|e| format!("xml write: {e}"))?;
    }
    Ok(writer.into_inner())
}

fn simple(writer: &mut Writer<Vec<u8>>, name: &str, text: &str) -> Result<(), String> {
    let err = |e: std::io::Error| format!("xml write: {e}");
    writer
        .write_event(Event::Start(BytesStart::new(name)))
        .map_err(err)?;
    writer
        .write_event(Event::Text(BytesText::new(text)))
        .map_err(err)?;
    writer
        .write_event(Event::End(BytesEnd::new(name)))
        .map_err(err)?;
    Ok(())
}

fn edit_cell(xml: &[u8], cell: &str, mutation: &Mutation) -> Result<(Vec<u8>, bool), String> {
    let mut reader = parts::new_reader(xml);
    let mut writer = Writer::new(Vec::new());
    let mut buf = Vec::new();
    let mut skip = 0usize;
    let mut inside = false;
    let mut wrote_value = false;
    let mut hit = false;
    let err = |e: std::io::Error| format!("xml write: {e}");
    loop {
        let event = owned(
            reader
                .read_event_into(&mut buf)
                .map_err(|e| format!("xml: {e}"))?,
        );
        buf.clear();
        if matches!(event, Event::Eof) {
            break;
        }
        if skip > 0 {
            match event {
                Event::Start(_) => skip += 1,
                Event::End(_) => skip -= 1,
                _ => {}
            }
            continue;
        }
        let name = match &event {
            Event::Start(e) | Event::Empty(e) => Some(parts::local(e.name().as_ref()).to_string()),
            Event::End(e) => Some(parts::local(e.name().as_ref()).to_string()),
            _ => None,
        };
        let is_target = match &event {
            Event::Start(e) | Event::Empty(e) => {
                name.as_deref() == Some("c") && attr(e, "r").as_deref() == Some(cell)
            }
            _ => false,
        };
        if is_target {
            hit = true;
            if let Mutation::ReplaceCell { .. } = mutation {
                let mut start = BytesStart::new("c");
                start.push_attribute(("r", cell));
                start.push_attribute(("t", "inlineStr"));
                writer.write_event(Event::Start(start)).map_err(err)?;
                writer
                    .write_event(Event::Start(BytesStart::new("is")))
                    .map_err(err)?;
                simple(&mut writer, "t", "mutant-cell")?;
                writer
                    .write_event(Event::End(BytesEnd::new("is")))
                    .map_err(err)?;
                writer
                    .write_event(Event::End(BytesEnd::new("c")))
                    .map_err(err)?;
                if matches!(event, Event::Start(_)) {
                    skip = 1;
                }
                continue;
            }
            if matches!(event, Event::Start(_)) {
                inside = true;
                wrote_value = false;
            }
        } else if inside {
            match (&event, mutation, name.as_deref()) {
                (Event::Start(_), Mutation::SetCached { value, .. }, Some("v")) => {
                    skip = 1;
                    if let Some(value) = value {
                        simple(&mut writer, "v", value)?;
                    }
                    wrote_value = true;
                    continue;
                }
                (Event::Empty(_), Mutation::SetCached { value, .. }, Some("v")) => {
                    if let Some(value) = value {
                        simple(&mut writer, "v", value)?;
                    }
                    wrote_value = true;
                    continue;
                }
                (Event::Start(e), Mutation::SetFormula { text, .. }, Some("f")) => {
                    writer.write_event(Event::Start(e.clone())).map_err(err)?;
                    writer
                        .write_event(Event::Text(BytesText::new(text)))
                        .map_err(err)?;
                    writer
                        .write_event(Event::End(BytesEnd::new(e.name().as_ref().to_string())))
                        .map_err(err)?;
                    skip = 1;
                    continue;
                }
                (
                    Event::End(_),
                    Mutation::SetCached {
                        value: Some(value), ..
                    },
                    Some("c"),
                ) => {
                    if !wrote_value {
                        simple(&mut writer, "v", value)?;
                    }
                    inside = false;
                }
                (Event::End(_), _, Some("c")) => inside = false,
                _ => {}
            }
        }
        writer.write_event(event).map_err(err)?;
    }
    Ok((writer.into_inner(), hit))
}

pub fn apply(src: &Path, dst: &Path, mutation: &Mutation) -> Result<(), String> {
    let mut all = entries(src)?;
    match mutation {
        Mutation::RemoveKind(kind) => match parts::element_rule(*kind) {
            None => all.retain(|(name, _)| parts::entry_kind(name) != Some(*kind)),
            Some(rule) => {
                for (name, bytes) in all.iter_mut() {
                    if rule.part.holds(name) {
                        *bytes = remove_element(bytes, rule.parent)?;
                    }
                }
            }
        },
        Mutation::ReplaceCell { sheet, cell }
        | Mutation::SetCached { sheet, cell, .. }
        | Mutation::SetFormula { sheet, cell, .. } => {
            let part = sheet_part(&all, sheet)?;
            let (_, bytes) = all
                .iter_mut()
                .find(|(name, _)| *name == part)
                .ok_or_else(|| format!("no entry {part}"))?;
            let (edited, hit) = edit_cell(bytes, cell, mutation)?;
            if !hit {
                return Err(format!("no cell {sheet}!{cell} in {part}"));
            }
            *bytes = edited;
        }
    }
    write_entries(dst, &all)
}
