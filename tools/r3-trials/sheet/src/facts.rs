use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use quick_xml::events::Event;

use crate::corpus::Spec;
use crate::mutate::sheet_part;
use crate::parts;

fn entries(path: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("zip: {e}"))?;
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

fn find<'a>(all: &'a [(String, Vec<u8>)], name: &str) -> Option<&'a [u8]> {
    all.iter()
        .find(|(entry, _)| entry == name)
        .map(|(_, bytes)| bytes.as_slice())
}

fn first_attrs(xml: &[u8], element: &str) -> Option<String> {
    let mut reader = parts::new_reader(xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => return None,
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if parts::local(e.name().as_ref()) == element =>
            {
                let text: Vec<String> = e
                    .attributes()
                    .flatten()
                    .map(|a| format!("{}:{}", a.key.as_ref(), a.value))
                    .collect();
                return Some(if text.is_empty() {
                    "-".into()
                } else {
                    text.join(",")
                });
            }
            Ok(_) => {}
        }
        buf.clear();
    }
}

fn count_attr(xml: &[u8], element: &str) -> String {
    first_attrs(xml, element)
        .and_then(|attrs| {
            attrs
                .split(',')
                .find_map(|pair| pair.strip_prefix("count:").map(str::to_string))
        })
        .unwrap_or_else(|| "absent".into())
}

fn cell_types(xml: &[u8]) -> String {
    let mut reader = parts::new_reader(xml);
    let mut buf = Vec::new();
    let mut seen = BTreeSet::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if parts::local(e.name().as_ref()) == "c" => {
                let t = e
                    .attributes()
                    .flatten()
                    .find(|a| a.key.as_ref() == "t")
                    .map(|a| a.value.to_string())
                    .unwrap_or_else(|| "n".into());
                seen.insert(t);
            }
            Ok(_) => {}
        }
        buf.clear();
    }
    seen.into_iter().collect::<Vec<_>>().join(",")
}

fn cell_xml(xml: &[u8], cell: &str) -> String {
    let text = String::from_utf8_lossy(xml);
    let open = format!("<c r=\"{cell}\"");
    let Some(start) = text.find(&open) else {
        return "absent".into();
    };
    let rest = &text[start..];
    let close = rest.find('>').map_or(rest.len(), |at| at + 1);
    let end = if rest[..close].ends_with("/>") {
        close
    } else {
        rest.find("</c>").map_or(rest.len(), |at| at + 4)
    };
    rest[..end].replace(' ', "_")
}

pub fn facts(original: &Path, saved: &Path, spec: &Spec) -> Result<Vec<String>, String> {
    let before = entries(original)?;
    let after = entries(saved)?;
    let names = |all: &[(String, Vec<u8>)]| -> BTreeSet<String> {
        all.iter().map(|(name, _)| name.clone()).collect()
    };
    let (b, a) = (names(&before), names(&after));
    let list = |set: Vec<&String>| {
        if set.is_empty() {
            "-".to_string()
        } else {
            set.into_iter().cloned().collect::<Vec<_>>().join(",")
        }
    };
    let mut out = vec![
        format!("entries_removed={}", list(b.difference(&a).collect())),
        format!("entries_added={}", list(a.difference(&b).collect())),
    ];
    for (label, all) in [("before", &before), ("after", &after)] {
        if let Some(styles) = find(all, "xl/styles.xml") {
            out.push(format!(
                "styles_{label} fonts={} fills={} borders={} cellXfs={} numFmts={}",
                count_attr(styles, "fonts"),
                count_attr(styles, "fills"),
                count_attr(styles, "borders"),
                count_attr(styles, "cellXfs"),
                count_attr(styles, "numFmts"),
            ));
        }
        out.push(format!(
            "shared_strings_{label}={}",
            find(all, "xl/sharedStrings.xml")
                .and_then(|xml| first_attrs(xml, "sst"))
                .unwrap_or_else(|| "absent".into())
        ));
        out.push(format!(
            "calc_pr_{label}={}",
            find(all, "xl/workbook.xml")
                .and_then(|xml| first_attrs(xml, "calcPr"))
                .unwrap_or_else(|| "absent".into())
        ));
        let part = sheet_part(all, spec.sheet)?;
        if let Some(sheet) = find(all, &part) {
            out.push(format!("cell_types_{label}={}", cell_types(sheet)));
            out.push(format!("target_{label}={}", cell_xml(sheet, spec.cell)));
        }
        for dependent in &spec.dependents {
            let part = sheet_part(all, dependent.sheet)?;
            if let Some(sheet) = find(all, &part) {
                out.push(format!(
                    "dependent_{label} {}!{}={}",
                    dependent.sheet,
                    dependent.cell,
                    cell_xml(sheet, dependent.cell)
                ));
            }
        }
    }
    Ok(out)
}
