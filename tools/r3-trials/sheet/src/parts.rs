use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use quick_xml::events::Event;
use quick_xml::Reader;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Chart,
    Drawing,
    Pivot,
    Macro,
    Table,
    Comment,
    Validation,
    Merged,
    Format,
    DefinedName,
}

impl Kind {
    pub const ALL: [Kind; 10] = [
        Kind::Chart,
        Kind::Drawing,
        Kind::Pivot,
        Kind::Macro,
        Kind::Table,
        Kind::Comment,
        Kind::Validation,
        Kind::Merged,
        Kind::Format,
        Kind::DefinedName,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Chart => "chart",
            Kind::Drawing => "drawing",
            Kind::Pivot => "pivot",
            Kind::Macro => "macro",
            Kind::Table => "table",
            Kind::Comment => "comment",
            Kind::Validation => "validation",
            Kind::Merged => "merged",
            Kind::Format => "format",
            Kind::DefinedName => "defined_name",
        }
    }

    pub fn is_entry_kind(self) -> bool {
        element_rule(self).is_none()
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

pub fn list(kinds: &[Kind]) -> String {
    if kinds.is_empty() {
        return "-".to_string();
    }
    let mut sorted = kinds.to_vec();
    sorted.sort();
    sorted.dedup();
    sorted
        .iter()
        .map(|kind| kind.name())
        .collect::<Vec<_>>()
        .join(",")
}

pub fn entry_kind(name: &str) -> Option<Kind> {
    let name = name.to_ascii_lowercase();
    if name.ends_with('/') {
        return None;
    }
    let under = |dir: &str| name.starts_with(dir);
    if under("xl/charts/") {
        Some(Kind::Chart)
    } else if under("xl/drawings/") {
        Some(Kind::Drawing)
    } else if under("xl/pivottables/") || under("xl/pivotcache/") {
        Some(Kind::Pivot)
    } else if name == "xl/vbaproject.bin" {
        Some(Kind::Macro)
    } else if under("xl/tables/") {
        Some(Kind::Table)
    } else if name
        .strip_prefix("xl/comments")
        .is_some_and(|rest| rest.ends_with(".xml") && !rest.contains('/'))
    {
        Some(Kind::Comment)
    } else {
        None
    }
}

pub struct ElementRule {
    pub part: PartSet,
    pub parent: &'static str,
    pub child: &'static str,
    pub key: &'static [&'static str],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PartSet {
    Sheets,
    Styles,
    Workbook,
}

impl PartSet {
    pub fn holds(self, name: &str) -> bool {
        let name = name.to_ascii_lowercase();
        match self {
            PartSet::Sheets => {
                name.starts_with("xl/worksheets/")
                    && name.ends_with(".xml")
                    && !name["xl/worksheets/".len()..].contains('/')
            }
            PartSet::Styles => name == "xl/styles.xml",
            PartSet::Workbook => name == "xl/workbook.xml",
        }
    }
}

pub fn element_rule(kind: Kind) -> Option<ElementRule> {
    let rule = |part, parent, child, key| {
        Some(ElementRule {
            part,
            parent,
            child,
            key,
        })
    };
    match kind {
        Kind::Validation => rule(
            PartSet::Sheets,
            "dataValidations",
            "dataValidation",
            ["sqref", "type"].as_slice(),
        ),
        Kind::Merged => rule(
            PartSet::Sheets,
            "mergeCells",
            "mergeCell",
            ["ref"].as_slice(),
        ),
        Kind::Format => rule(
            PartSet::Styles,
            "numFmts",
            "numFmt",
            ["formatCode"].as_slice(),
        ),
        Kind::DefinedName => rule(
            PartSet::Workbook,
            "definedNames",
            "definedName",
            ["name", "localSheetId"].as_slice(),
        ),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    pub instances: BTreeMap<Kind, Vec<String>>,
}

impl Inventory {
    pub fn kinds(&self) -> Vec<Kind> {
        self.instances
            .iter()
            .filter(|(_, found)| !found.is_empty())
            .map(|(kind, _)| *kind)
            .collect()
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.instances.get(&kind).map_or(0, Vec::len)
    }
}

pub fn element_instances(xml: &[u8], rule: &ElementRule) -> Result<Vec<String>, String> {
    let mut reader = new_reader(xml);
    let mut buf = Vec::new();
    let mut inside = 0usize;
    let mut found = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| format!("xml: {e}"))?;
        match &event {
            Event::Eof => break,
            Event::Start(e) if local(e.name().as_ref()) == rule.parent => inside += 1,
            Event::End(e) if local(e.name().as_ref()) == rule.parent => {
                inside = inside.saturating_sub(1)
            }
            Event::Start(e) | Event::Empty(e)
                if inside > 0 && local(e.name().as_ref()) == rule.child =>
            {
                let values: Vec<String> = rule
                    .key
                    .iter()
                    .map(|key| {
                        e.attributes()
                            .flatten()
                            .find(|a| local(a.key.as_ref()) == *key)
                            .map(|a| a.value.to_string())
                            .unwrap_or_default()
                    })
                    .collect();
                found.push(values.join("|"));
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(found)
}

pub fn inventory(path: &Path) -> Result<Inventory, String> {
    let file = File::open(path).map_err(|e| format!("open: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("zip: {e}"))?;
    let mut found = Inventory::default();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| format!("zip: {e}"))?;
        let name = entry
            .name()
            .map_err(|e| format!("zip name: {e}"))?
            .to_string();
        if let Some(kind) = entry_kind(&name) {
            found.instances.entry(kind).or_default().push(name.clone());
        }
        let rules: Vec<(Kind, ElementRule)> = Kind::ALL
            .into_iter()
            .filter_map(|kind| element_rule(kind).map(|rule| (kind, rule)))
            .filter(|(_, rule)| rule.part.holds(&name))
            .collect();
        if rules.is_empty() {
            continue;
        }
        let mut xml = Vec::new();
        entry
            .read_to_end(&mut xml)
            .map_err(|e| format!("read {name}: {e}"))?;
        for (kind, rule) in rules {
            let keys = element_instances(&xml, &rule)?;
            found.instances.entry(kind).or_default().extend(keys);
        }
    }
    for keys in found.instances.values_mut() {
        keys.sort();
    }
    Ok(found)
}

pub fn lost(original: &Inventory, saved: &Inventory) -> Vec<Kind> {
    Kind::ALL
        .into_iter()
        .filter(|kind| {
            let had = original.instances.get(kind).map_or(&[][..], Vec::as_slice);
            let has = saved.instances.get(kind).map_or(&[][..], Vec::as_slice);
            if had.is_empty() {
                return false;
            }
            if kind.is_entry_kind() {
                return has.len() < had.len();
            }
            let mut left = has.to_vec();
            had.iter()
                .any(|key| match left.iter().position(|k| k == key) {
                    Some(at) => {
                        left.swap_remove(at);
                        false
                    }
                    None => true,
                })
        })
        .collect()
}

pub fn local(name: &str) -> &str {
    match name.rfind(':') {
        Some(at) => &name[at + 1..],
        None => name,
    }
}

pub fn new_reader(xml: &[u8]) -> Reader<&[u8]> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    reader
}
