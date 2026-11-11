use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{Cursor, Read};

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use crate::book::unreadable;
use crate::error::Error;

/// A part kind of the closed list of contracts.md A13.4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// `chart`: an archive entry under `xl/charts/`.
    Chart,
    /// `drawing`: an archive entry under `xl/drawings/`.
    Drawing,
    /// `pivot`: an archive entry under `xl/pivotTables/` or `xl/pivotCache/`, or a part that
    /// a relationship of a pivot table or a pivot cache links.
    Pivot,
    /// `macro`: the archive entry `xl/vbaProject.bin`, or a part that a relationship of a VBA
    /// project links.
    Macro,
    /// `table`: an archive entry under `xl/tables/`.
    Table,
    /// `comment`: an archive entry `xl/comments*.xml`.
    Comment,
    /// `validation`: a `dataValidations` element with at least one child in a sheet part.
    Validation,
    /// `merged`: a `mergeCells` element with at least one child in a sheet part.
    Merged,
    /// `format`: a `numFmts` element with at least one child in `xl/styles.xml`.
    Format,
    /// `defined_name`: a `definedNames` element with at least one child in `xl/workbook.xml`.
    DefinedName,
}

impl Kind {
    /// The ten kinds in the order of A13.4.
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

    /// The kinds that the verdict of the sheet trial refuses, as its line in
    /// `docs/r3/trials/sheet.md` names them: `verdict sheet=umya refuses=pivot,macro,validation`.
    pub const REFUSED: [Kind; 3] = [Kind::Pivot, Kind::Macro, Kind::Validation];

    /// The kind's name as A13.4 writes it.
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

pub(crate) type Entries = BTreeMap<String, Vec<u8>>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Inventory {
    pub(crate) present: BTreeSet<Kind>,
    pub(crate) instances: BTreeMap<Kind, Vec<String>>,
}

impl Inventory {
    pub(crate) fn refused(&self) -> Vec<Kind> {
        Kind::REFUSED
            .into_iter()
            .filter(|kind| self.present.contains(kind))
            .collect()
    }
}

pub(crate) fn entries(bytes: &[u8]) -> Result<Entries, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| unreadable())?;
    let mut all = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|_| unreadable())?;
        let name = entry.name().map_err(|_| unreadable())?.to_string();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).map_err(|_| unreadable())?;
        if all.insert(name, content).is_some() {
            return Err(unreadable());
        }
    }
    Ok(all)
}

fn lower(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

fn entry_kind(name: &str) -> Option<Kind> {
    let name = lower(name);
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

fn relationship_kind(kind_type: &str) -> Option<Kind> {
    let tail = kind_type.rsplit('/').next().unwrap_or_default();
    match tail {
        "vbaProject" => Some(Kind::Macro),
        "pivotTable" | "pivotCacheDefinition" | "pivotCacheRecords" => Some(Kind::Pivot),
        _ => None,
    }
}

struct ElementRule {
    part: PartSet,
    parent: &'static str,
    child: &'static str,
    key: &'static [&'static str],
    texts: &'static [&'static str],
}

const OWN_TEXT: &str = ".";

#[derive(Clone, Copy, PartialEq, Eq)]
enum PartSet {
    Sheets,
    Styles,
    Workbook,
}

fn element_rule(kind: Kind) -> Option<ElementRule> {
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
            [
                "sqref",
                "type",
                "operator",
                "allowBlank",
                "showErrorMessage",
                "errorStyle",
                "errorTitle",
                "error",
                "promptTitle",
                "prompt",
            ]
            .as_slice(),
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

struct Instance {
    attrs: Vec<String>,
    texts: Vec<String>,
    stack: Vec<String>,
}

fn element_instances(xml: &[u8], rule: &ElementRule) -> Result<Vec<String>, Error> {
    let mut reader = new_reader(xml);
    let mut buf = Vec::new();
    let mut inside = 0usize;
    let mut open: Option<Instance> = None;
    let mut found = Vec::new();
    let attrs_of = |e: &BytesStart<'_>| -> Vec<String> {
        rule.key
            .iter()
            .map(|key| {
                let value = e
                    .attributes()
                    .flatten()
                    .find(|a| local(a.key.as_ref()) == *key)
                    .map(|a| attribute_value(&a));
                with_default(key, value)
            })
            .collect()
    };
    let add = |instance: &mut Instance, text: &str| {
        let slot = match instance.stack.last() {
            None => rule.texts.iter().position(|name| *name == OWN_TEXT),
            Some(name) => rule.texts.iter().position(|text| text == name),
        };
        if let Some(at) = slot {
            instance.texts[at].push_str(text);
        }
    };
    let finish = |instance: Instance| {
        let mut parts = instance.attrs;
        parts.extend(instance.texts);
        parts.join("|")
    };
    loop {
        let event = reader.read_event_into(&mut buf).map_err(|_| unreadable())?;
        match (&event, open.as_mut()) {
            (Event::Eof, _) => break,
            (Event::Start(e), Some(instance)) => {
                instance.stack.push(local(e.name().as_ref()).to_string())
            }
            (Event::Text(t), Some(instance)) => add(instance, &t.xml10_content()),
            (Event::CData(t), Some(instance)) => add(instance, &t.xml10_content()),
            (Event::GeneralRef(r), Some(instance)) => add(instance, &reference_text(r)?),
            (Event::End(_), Some(instance)) => {
                if instance.stack.pop().is_none() {
                    if let Some(done) = open.take() {
                        found.push(finish(done));
                    }
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

fn attribute_value(attribute: &Attribute<'_>) -> String {
    attribute
        .normalized_value(XmlVersion::Implicit1_0)
        .map(|value| value.into_owned())
        .unwrap_or_else(|_| attribute.value.to_string())
}

fn with_default(key: &str, value: Option<String>) -> String {
    match key {
        "allowBlank" | "showErrorMessage" => match value.as_deref().map(str::trim) {
            Some("1") | Some("true") => "1",
            _ => "0",
        }
        .to_string(),
        "errorStyle" => value.unwrap_or_else(|| "stop".to_string()),
        _ => value.unwrap_or_default(),
    }
}

fn reference_text(reference: &BytesRef<'_>) -> Result<String, Error> {
    if let Some(ch) = reference.resolve_char_ref().map_err(|_| unreadable())? {
        return Ok(ch.to_string());
    }
    let name: &str = reference.as_ref();
    Ok(match resolve_predefined_entity(name) {
        Some(text) => text.to_string(),
        None => format!("&{name};"),
    })
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

fn source_of(rels: &str) -> Option<String> {
    let (dir, base) = rels.rsplit_once("_rels/").unwrap_or(("", rels));
    let base = base.strip_suffix(".rels")?;
    Some(format!("{dir}{base}"))
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

type Attrs = BTreeMap<String, String>;

fn element_attributes(xml: &[u8]) -> Vec<(String, Attrs)> {
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
                    .map(|a| (a.key.as_ref().to_string(), attribute_value(&a)))
                    .collect();
                found.push((local(e.name().as_ref()).to_string(), attrs));
            }
            Ok(_) => {}
        }
        buf.clear();
    }
    found
}

fn relationships(rels: &[u8]) -> Vec<Attrs> {
    element_attributes(rels)
        .into_iter()
        .filter(|(element, _)| element == "Relationship")
        .map(|(_, attrs)| attrs)
        .collect()
}

fn referenced_ids(xml: &[u8]) -> BTreeSet<String> {
    element_attributes(xml)
        .into_iter()
        .flat_map(|(_, attrs)| attrs)
        .filter(|(key, _)| key.contains(':') && !key.starts_with("xmlns"))
        .map(|(_, value)| value)
        .collect()
}

fn linked_parts(entries: &Entries) -> BTreeSet<String> {
    let by_lower: BTreeMap<String, &String> =
        entries.keys().map(|name| (lower(name), name)).collect();
    let entry = |name: &str| by_lower.get(&lower(name)).copied();
    let mut live = BTreeSet::new();
    let mut queue = VecDeque::from([String::new()]);
    while let Some(source) = queue.pop_front() {
        let rels_name = if source.is_empty() {
            "_rels/.rels".to_string()
        } else {
            rels_of(&source)
        };
        let Some(rels) = entry(&rels_name).and_then(|name| entries.get(name)) else {
            continue;
        };
        let used: BTreeSet<String> = entries
            .get(&source)
            .map(|xml| referenced_ids(xml))
            .unwrap_or_default();
        for rel in relationships(rels) {
            let get = |k: &str| rel.get(k).cloned().unwrap_or_default();
            if get("TargetMode") == "External" {
                continue;
            }
            let referenced = source.is_empty()
                || used.contains(&get("Id"))
                || IMPLICIT.iter().any(|tail| get("Type").ends_with(tail));
            let Some(target) = entry(&resolve(&source, &get("Target"))) else {
                continue;
            };
            if referenced && live.insert(target.clone()) {
                queue.push_back(target.clone());
            }
        }
    }
    live
}

struct Linked {
    sheets: BTreeSet<String>,
    kinds: BTreeSet<Kind>,
}

fn by_relationship(entries: &Entries) -> Linked {
    let mut linked = Linked {
        sheets: BTreeSet::new(),
        kinds: BTreeSet::new(),
    };
    for (name, xml) in entries {
        let name = name.replace('\\', "/");
        if !lower(&name).ends_with(".rels") {
            continue;
        }
        let source = source_of(&name).unwrap_or_default();
        for rel in relationships(xml) {
            let kind_type = rel.get("Type").cloned().unwrap_or_default();
            linked.kinds.extend(relationship_kind(&kind_type));
            if kind_type.rsplit('/').next() == Some("worksheet") {
                let target = rel.get("Target").cloned().unwrap_or_default();
                linked.sheets.insert(lower(&resolve(&source, &target)));
            }
        }
    }
    linked
}

fn holds(set: PartSet, name: &str, sheets: &BTreeSet<String>) -> bool {
    let name = lower(name);
    match set {
        PartSet::Sheets => {
            let by_path = name.starts_with("xl/worksheets/")
                && name.ends_with(".xml")
                && !name["xl/worksheets/".len()..].contains('/');
            by_path || sheets.contains(&name)
        }
        PartSet::Styles => name == "xl/styles.xml",
        PartSet::Workbook => name == "xl/workbook.xml",
    }
}

pub(crate) fn inventory(entries: &Entries) -> Result<Inventory, Error> {
    let live = linked_parts(entries);
    let linked = by_relationship(entries);
    let mut found = Inventory::default();
    found.present.extend(linked.kinds.iter().copied());
    for (name, xml) in entries {
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
            if !holds(rule.part, name, &linked.sheets) {
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

pub(crate) fn lost(original: &Inventory, saved: &Inventory) -> Vec<Kind> {
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

fn local(name: &str) -> &str {
    match name.rfind(':') {
        Some(at) => &name[at + 1..],
        None => name,
    }
}

fn new_reader(xml: &[u8]) -> Reader<&[u8]> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    reader
}

fn sheet_parts(entries: &Entries) -> Vec<(String, String)> {
    let empty = Vec::new();
    let workbook = entries.get("xl/workbook.xml").unwrap_or(&empty);
    let rels = entries.get("xl/_rels/workbook.xml.rels").unwrap_or(&empty);
    let targets: BTreeMap<String, String> = relationships(rels)
        .into_iter()
        .filter_map(|rel| Some((rel.get("Id")?.clone(), rel.get("Target")?.clone())))
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

pub(crate) type Array = (String, String, String);

pub(crate) fn array_formulas(entries: &Entries) -> Result<BTreeSet<Array>, Error> {
    let mut found = BTreeSet::new();
    for (sheet, part) in sheet_parts(entries) {
        let Some(xml) = entries.get(&part) else {
            continue;
        };
        let mut reader = new_reader(xml);
        let mut buf = Vec::new();
        let mut cell = String::new();
        loop {
            match reader.read_event_into(&mut buf).map_err(|_| unreadable())? {
                Event::Eof => break,
                Event::Start(e) | Event::Empty(e) => {
                    let attr = |key: &str| {
                        e.attributes()
                            .flatten()
                            .find(|a| a.key.as_ref() == key)
                            .map(|a| attribute_value(&a))
                    };
                    match local(e.name().as_ref()) {
                        "c" => cell = attr("r").unwrap_or_default(),
                        "f" if attr("t").as_deref() == Some("array") => {
                            found.insert((
                                sheet.clone(),
                                cell.clone(),
                                attr("ref").unwrap_or_default(),
                            ));
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            buf.clear();
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Refusal;
    use crate::testing::{corpus, edit, zipped};
    use std::path::Path;

    fn names(kinds: impl IntoIterator<Item = Kind>) -> Vec<&'static str> {
        kinds.into_iter().map(Kind::name).collect()
    }

    fn present(all: &Entries) -> Vec<&'static str> {
        names(inventory(all).expect("an inventory").present)
    }

    #[test]
    fn each_corpus_workbook_holds_the_kinds_of_the_trial_table() {
        let table: [(&str, &[&str]); 8] = [
            ("values.xlsx", &[]),
            ("formulas.xlsx", &[]),
            ("formats.xlsx", &["format"]),
            ("merged.xlsx", &["merged"]),
            ("validation.xlsx", &["validation"]),
            ("chart.xlsx", &["chart", "drawing", "table", "comment"]),
            ("two-sheets.xlsx", &["defined_name"]),
            ("rows-5000.xlsx", &[]),
        ];
        for (file, want) in table {
            let all = corpus(file);
            assert_eq!(present(&all), want.to_vec(), "{file}");
            let refused = inventory(&all).expect("an inventory").refused();
            let want_refused: &[&str] = if file == "validation.xlsx" {
                &["validation"]
            } else {
                &[]
            };
            assert_eq!(names(refused), want_refused.to_vec(), "{file}");
        }
    }

    #[test]
    fn the_refused_kinds_are_the_verdict_line_of_the_sheet_trial() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/r3/trials/sheet.md");
        let text = std::fs::read_to_string(path).expect("read the trial's result");
        let verdict = text
            .lines()
            .rfind(|line| line.starts_with("verdict "))
            .expect("a verdict line");
        assert!(verdict.starts_with("verdict sheet=umya "), "{verdict}");
        let refuses = verdict
            .split(' ')
            .find_map(|pair| pair.strip_prefix("refuses="))
            .expect("refuses=");
        assert_eq!(refuses.split(',').collect::<Vec<_>>(), names(Kind::REFUSED));
    }

    #[test]
    fn a_pivot_or_a_macro_part_is_found_by_its_name_or_by_its_relationship() {
        let cases: [(&str, &str); 5] = [
            ("xl/vbaProject.bin", "macro"),
            ("XL/VBAPROJECT.BIN", "macro"),
            ("xl\\vbaProject.bin", "macro"),
            ("xl/pivotTables/pivotTable1.xml", "pivot"),
            ("xl/PivotCache/pivotCacheRecords1.xml", "pivot"),
        ];
        for (entry, kind) in cases {
            let mut all = corpus("values.xlsx");
            all.insert(entry.to_string(), b"<x/>".to_vec());
            let refused = inventory(&all).expect("an inventory").refused();
            assert_eq!(names(refused), vec![kind], "{entry}");
        }
        let types = [
            ("http://schemas.microsoft.com/office/2006/relationships/vbaProject", "macro"),
            ("http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotTable", "pivot"),
            ("http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheDefinition", "pivot"),
            ("http://schemas.openxmlformats.org/officeDocument/2006/relationships/pivotCacheRecords", "pivot"),
        ];
        for (kind_type, kind) in types {
            let mut all = corpus("values.xlsx");
            all.insert("xl/other/part1.bin".to_string(), b"<x/>".to_vec());
            let rel = format!(
                r#"<Relationship Id="rId99" Type="{kind_type}" Target="other/part1.bin"/></Relationships>"#
            );
            edit(
                &mut all,
                "xl/_rels/workbook.xml.rels",
                "</Relationships>",
                &rel,
            );
            let refused = inventory(&all).expect("an inventory").refused();
            assert_eq!(names(refused), vec![kind], "{kind_type}");
        }
    }

    #[test]
    fn a_validation_in_a_sheet_part_of_another_name_is_found() {
        let mut all = corpus("validation.xlsx");
        let sheet = all.remove("xl/worksheets/sheet1.xml").expect("the sheet");
        all.insert("xl/data/table1.xml".to_string(), sheet);
        edit(
            &mut all,
            "xl/_rels/workbook.xml.rels",
            "worksheets/sheet1.xml",
            "data/table1.xml",
        );
        let found = inventory(&all).expect("an inventory");
        assert_eq!(names(found.refused()), vec!["validation"]);
        assert_eq!(
            found.instances.get(&Kind::Validation).map(Vec::len),
            Some(2)
        );
    }

    #[test]
    fn a_data_validations_element_with_no_child_is_no_validation() {
        let mut all = corpus("values.xlsx");
        edit(
            &mut all,
            "xl/worksheets/sheet1.xml",
            "</sheetData>",
            r#"</sheetData><dataValidations count="0"/>"#,
        );
        assert_eq!(present(&all), Vec::<&str>::new());
        edit(
            &mut all,
            "xl/worksheets/sheet1.xml",
            r#"<dataValidations count="0"/>"#,
            r#"<x:dataValidations xmlns:x="y"><x:dataValidation sqref="A1"/></x:dataValidations>"#,
        );
        assert_eq!(present(&all), vec!["validation"]);
    }

    #[test]
    fn a_kind_is_lost_when_the_saved_file_keeps_less_of_it() {
        let cases: [(&str, &str, &str, &str, &[&str]); 4] = [
            (
                "validation.xlsx",
                "xl/worksheets/sheet1.xml",
                "<formula2>24</formula2>",
                "",
                &["validation"],
            ),
            (
                "merged.xlsx",
                "xl/worksheets/sheet1.xml",
                r#"ref="A10:B10""#,
                r#"ref="A10:C10""#,
                &["merged"],
            ),
            (
                "chart.xlsx",
                "xl/worksheets/sheet1.xml",
                r#"<drawing r:id="rId1"/>"#,
                "",
                &["chart", "drawing"],
            ),
            (
                "two-sheets.xlsx",
                "xl/workbook.xml",
                "Input!$B$6",
                "Input!$B$7",
                &["defined_name"],
            ),
        ];
        for (file, part, from, to, want) in cases {
            let original = corpus(file);
            let mut saved = original.clone();
            edit(&mut saved, part, from, to);
            let before = inventory(&original).expect("an inventory");
            let after = inventory(&saved).expect("an inventory");
            assert_eq!(names(lost(&before, &after)), want.to_vec(), "{file}");
            assert_eq!(lost(&before, &before), Vec::new(), "{file}");
        }
    }

    #[test]
    fn a_file_that_is_no_zip_or_holds_a_broken_part_is_unreadable() {
        let refusal = Error::Workbook(Refusal::Unreadable);
        assert_eq!(entries(b"secret,1\n").unwrap_err(), refusal);
        let mut all = corpus("values.xlsx");
        edit(
            &mut all,
            "xl/worksheets/sheet1.xml",
            "</sheetData>",
            "</sheetDat>",
        );
        let bytes = zipped(&all);
        let reread = entries(&bytes).expect("a zip");
        assert_eq!(inventory(&reread).unwrap_err(), refusal);
    }
}
