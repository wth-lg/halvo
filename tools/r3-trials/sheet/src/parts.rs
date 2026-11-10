use std::collections::{BTreeMap, BTreeSet, VecDeque};
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
    pub texts: &'static [&'static str],
}

pub const OWN_TEXT: &str = ".";

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
    let rule = |part, parent, child, key, texts| {
        Some(ElementRule {
            part,
            parent,
            child,
            key,
            texts,
        })
    };
    match kind {
        Kind::Validation => rule(
            PartSet::Sheets,
            "dataValidations",
            "dataValidation",
            ["sqref", "type", "operator"].as_slice(),
            ["formula1", "formula2"].as_slice(),
        ),
        Kind::Merged => rule(
            PartSet::Sheets,
            "mergeCells",
            "mergeCell",
            ["ref"].as_slice(),
            [].as_slice(),
        ),
        Kind::Format => rule(
            PartSet::Styles,
            "numFmts",
            "numFmt",
            ["formatCode"].as_slice(),
            [].as_slice(),
        ),
        Kind::DefinedName => rule(
            PartSet::Workbook,
            "definedNames",
            "definedName",
            ["name", "localSheetId"].as_slice(),
            [OWN_TEXT].as_slice(),
        ),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inventory {
    pub present: BTreeSet<Kind>,
    pub instances: BTreeMap<Kind, Vec<String>>,
}

impl Inventory {
    pub fn kinds(&self) -> Vec<Kind> {
        self.present.iter().copied().collect()
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.instances.get(&kind).map_or(0, Vec::len)
    }
}

struct Instance {
    attrs: Vec<String>,
    texts: Vec<String>,
    stack: Vec<String>,
}

pub fn element_instances(xml: &[u8], rule: &ElementRule) -> Result<Vec<String>, String> {
    let mut reader = new_reader(xml);
    let mut buf = Vec::new();
    let mut inside = 0usize;
    let mut open: Option<Instance> = None;
    let mut found = Vec::new();
    let attrs_of = |e: &quick_xml::events::BytesStart<'_>| -> Vec<String> {
        rule.key
            .iter()
            .map(|key| {
                e.attributes()
                    .flatten()
                    .find(|a| local(a.key.as_ref()) == *key)
                    .map(|a| a.value.to_string())
                    .unwrap_or_default()
            })
            .collect()
    };
    let finish = |instance: Instance| {
        let mut parts = instance.attrs;
        parts.extend(instance.texts);
        parts.join("|")
    };
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| format!("xml: {e}"))?;
        match (&event, open.as_mut()) {
            (Event::Eof, _) => break,
            (Event::Start(e), Some(instance)) => {
                instance.stack.push(local(e.name().as_ref()).to_string())
            }
            (Event::Text(t), Some(instance)) => {
                let text: &str = t.as_ref();
                let slot = match instance.stack.last() {
                    None => rule.texts.iter().position(|name| *name == OWN_TEXT),
                    Some(name) => rule.texts.iter().position(|text| text == name),
                };
                if let Some(at) = slot {
                    instance.texts[at].push_str(text);
                }
            }
            (Event::End(_), Some(instance)) => {
                if instance.stack.pop().is_none() {
                    found.push(finish(open.take().expect("open instance")));
                }
            }
            (Event::Start(e), None) if local(e.name().as_ref()) == rule.parent => inside += 1,
            (Event::End(e), None) if local(e.name().as_ref()) == rule.parent => {
                inside = inside.saturating_sub(1)
            }
            (Event::Start(e), None) if inside > 0 && local(e.name().as_ref()) == rule.child => {
                open = Some(Instance {
                    attrs: attrs_of(e),
                    texts: vec![String::new(); rule.texts.len()],
                    stack: Vec::new(),
                });
            }
            (Event::Empty(e), None) if inside > 0 && local(e.name().as_ref()) == rule.child => {
                found.push(finish(Instance {
                    attrs: attrs_of(e),
                    texts: vec![String::new(); rule.texts.len()],
                    stack: Vec::new(),
                }));
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(found)
}

const IMPLICIT: [&str; 10] = [
    "/comments",
    "/styles",
    "/theme",
    "/sharedStrings",
    "/calcChain",
    "/vbaProject",
    "/printerSettings",
    "/chartStyle",
    "/chartColorStyle",
    "/threadedComment",
];

fn rels_of(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, base)) => format!("{dir}/_rels/{base}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

fn resolve(source: &str, target: &str) -> String {
    let mut path: Vec<&str> = match target.strip_prefix('/') {
        Some(_) => Vec::new(),
        None => source
            .rsplit_once('/')
            .map_or(Vec::new(), |(dir, _)| dir.split('/').collect()),
    };
    for piece in target.trim_start_matches('/').split('/') {
        match piece {
            ".." => {
                path.pop();
            }
            "." | "" => {}
            other => path.push(other),
        }
    }
    path.join("/")
}

fn element_attributes(xml: &[u8]) -> Vec<(String, BTreeMap<String, String>)> {
    let mut reader = new_reader(xml);
    let mut buf = Vec::new();
    let mut found = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) | Err(_) => break,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let attrs = e
                    .attributes()
                    .flatten()
                    .map(|a| (a.key.as_ref().to_string(), a.value.to_string()))
                    .collect();
                found.push((local(e.name().as_ref()).to_string(), attrs));
            }
            Ok(_) => {}
        }
        buf.clear();
    }
    found
}

fn referenced_ids(xml: &[u8]) -> BTreeSet<String> {
    element_attributes(xml)
        .into_iter()
        .flat_map(|(_, attrs)| attrs)
        .filter(|(key, _)| key.contains(':') && !key.starts_with("xmlns"))
        .map(|(_, value)| value)
        .collect()
}

pub fn linked_parts(entries: &BTreeMap<String, Vec<u8>>) -> BTreeSet<String> {
    let mut live = BTreeSet::new();
    let mut queue = VecDeque::from([String::new()]);
    while let Some(source) = queue.pop_front() {
        let rels_name = if source.is_empty() {
            "_rels/.rels".to_string()
        } else {
            rels_of(&source)
        };
        let Some(rels) = entries.get(&rels_name) else {
            continue;
        };
        let used: BTreeSet<String> = entries
            .get(&source)
            .map(|xml| referenced_ids(xml))
            .unwrap_or_default();
        for (element, rel) in element_attributes(rels) {
            if element != "Relationship" {
                continue;
            }
            let get = |k: &str| rel.get(k).cloned().unwrap_or_default();
            if get("TargetMode") == "External" {
                continue;
            }
            let referenced = source.is_empty()
                || used.contains(&get("Id"))
                || IMPLICIT.iter().any(|tail| get("Type").ends_with(tail));
            let target = resolve(&source, &get("Target"));
            if referenced && entries.contains_key(&target) && live.insert(target.clone()) {
                queue.push_back(target);
            }
        }
    }
    live
}

pub fn read_entries(path: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("zip: {e}"))?;
    let mut all = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| format!("zip: {e}"))?;
        let name = entry
            .name()
            .map_err(|e| format!("zip name: {e}"))?
            .to_string();
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| format!("read {name}: {e}"))?;
        all.insert(name, bytes);
    }
    Ok(all)
}

pub fn inventory(path: &Path) -> Result<Inventory, String> {
    let entries = read_entries(path)?;
    let live = linked_parts(&entries);
    let mut found = Inventory::default();
    for (name, xml) in &entries {
        if let Some(kind) = entry_kind(name) {
            found.present.insert(kind);
            if live.contains(name) {
                found
                    .instances
                    .entry(kind)
                    .or_default()
                    .push("entry".into());
            }
        }
        for kind in Kind::ALL {
            let Some(rule) = element_rule(kind) else {
                continue;
            };
            if !rule.part.holds(name) {
                continue;
            }
            let keys = element_instances(xml, &rule)?;
            if !keys.is_empty() {
                found.present.insert(kind);
            }
            if live.contains(name) {
                found.instances.entry(kind).or_default().extend(keys);
            }
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
            let mut left = saved.instances.get(kind).cloned().unwrap_or_default();
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

pub fn sheet_parts(entries: &BTreeMap<String, Vec<u8>>) -> Vec<(String, String)> {
    let empty = Vec::new();
    let workbook = entries.get("xl/workbook.xml").unwrap_or(&empty);
    let rels = entries.get("xl/_rels/workbook.xml.rels").unwrap_or(&empty);
    let targets: BTreeMap<String, String> = element_attributes(rels)
        .into_iter()
        .filter(|(element, _)| element == "Relationship")
        .filter_map(|(_, rel)| Some((rel.get("Id")?.clone(), rel.get("Target")?.clone())))
        .collect();
    element_attributes(workbook)
        .into_iter()
        .filter(|(element, _)| element == "sheet")
        .filter_map(|(_, attrs)| {
            let name = attrs.get("name")?.clone();
            let id = attrs
                .iter()
                .find(|(k, _)| local(k) == "id" && k.contains(':'))?
                .1;
            Some((name, resolve("xl/workbook.xml", targets.get(id)?)))
        })
        .collect()
}
