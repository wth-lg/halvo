use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use quick_xml::escape::{resolve_predefined_entity, unescape};
use quick_xml::events::attributes::Attribute;
use quick_xml::events::{BytesRef, BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use crate::book::unreadable;
use crate::error::{Error, Refusal};

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
    /// `macro`: the archive entry `xl/vbaProject.bin`, a part that a relationship of a VBA
    /// project links, or a sheet that a relationship of an Excel 4 macro sheet links
    /// (`xlMacrosheet` or `xlIntlMacrosheet`).
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

    /// The kinds that the executor refuses before any write, its own list (contracts.md A11.3):
    /// `pivot`, `macro` and `validation`.
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
}

impl Inventory {
    pub(crate) fn refused(&self) -> Vec<Kind> {
        Kind::REFUSED
            .into_iter()
            .filter(|kind| self.present.contains(kind))
            .collect()
    }
}

/// The workbook part, found by the rule of names below.
pub(crate) const WORKBOOK: &str = "xl/workbook.xml";

/// The last segment of each relationship type that the crate or its reader reads. A
/// relationship of such a type names an entry, and a type that equals one of them only when
/// ASCII case is ignored refuses the workbook.
const READ: [&str; 11] = [
    "officeDocument",
    "worksheet",
    "chartsheet",
    "dialogsheet",
    "table",
    "vbaProject",
    "pivotTable",
    "pivotCacheDefinition",
    "pivotCacheRecords",
    "xlMacrosheet",
    "xlIntlMacrosheet",
];

/// The types of [`READ`] whose target is a sheet part that the element rules scan.
const SHEETS: [&str; 5] = [
    "worksheet",
    "chartsheet",
    "dialogsheet",
    "xlMacrosheet",
    "xlIntlMacrosheet",
];

fn relationship() -> Error {
    Error::Workbook(Refusal::Relationship)
}

fn part() -> Error {
    Error::Workbook(Refusal::Part)
}

pub(crate) const FILE_MAX: u64 = halvo_checkpoint::FILE_MAX;
/// The bound of one entry, uncompressed.
pub(crate) const ENTRY_MAX: u64 = 32 << 20;
/// The bound of all entries together, uncompressed.
pub(crate) const ENTRIES_MAX: u64 = 128 << 20;
/// The bound of the rectangle of a sheet's used cells.
pub(crate) const CELLS_MAX: u64 = 1_000_000;
/// The bound of a range.
pub(crate) const RANGE_MAX: u64 = 100_000;

pub(crate) fn size() -> Error {
    Error::Workbook(Refusal::Size)
}

/// The bytes of a workbook file, refused before it is read when it holds more than
/// [`FILE_MAX`], and cut there when it grows while it is read.
pub(crate) fn package_file(path: &Path) -> Result<Vec<u8>, Error> {
    if std::fs::metadata(path)?.len() > FILE_MAX {
        return Err(size());
    }
    package_of(&File::open(path)?)
}

/// The bytes of the workbook that `file` has open, read through that handle under the bound
/// of a file.
pub(crate) fn package_of(file: &File) -> Result<Vec<u8>, Error> {
    if file.metadata()?.len() > FILE_MAX {
        return Err(size());
    }
    let mut bytes = Vec::new();
    file.take(FILE_MAX + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > FILE_MAX {
        return Err(size());
    }
    Ok(bytes)
}

/// Refuses with [`Refusal::Size`] an archive whose entries declare more than [`ENTRY_MAX`] in
/// one or more than [`ENTRIES_MAX`] in all, summed entry by entry before any is inflated.
fn declared_within(archive: &mut zip::ZipArchive<Cursor<&[u8]>>) -> Result<(), Error> {
    let mut declared = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index_raw(index).map_err(|_| unreadable())?;
        declared = declared.saturating_add(entry.size());
        if entry.size() > ENTRY_MAX || declared > ENTRIES_MAX {
            return Err(size());
        }
    }
    Ok(())
}

/// Whether the package `bytes` keeps the bounds that the next read of it keeps: the file and
/// the declared sizes of its entries ([`Refusal::Size`] when not). `prepare` asks it of the copy
/// that it would write, before it writes anything, so that the next call can read the copy.
pub(crate) fn within_bounds(bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() as u64 > FILE_MAX {
        return Err(size());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| unreadable())?;
    declared_within(&mut archive)
}

/// The entries of a package. Refused: no zip archive, an encrypted entry, two entries whose
/// names fold alike, an entry that declares more than [`ENTRY_MAX`] uncompressed or entries
/// that declare more than [`ENTRIES_MAX`] in all (summed entry by entry before any is
/// inflated), and an entry whose data holds more than it may, its read cut at the bound.
pub(crate) fn entries(bytes: &[u8]) -> Result<Entries, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| unreadable())?;
    if archive.has_duplicate_names() {
        return Err(unreadable());
    }
    declared_within(&mut archive)?;
    let mut all = BTreeMap::new();
    let mut folded = BTreeSet::new();
    let mut held = 0u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|_| unreadable())?;
        let name = entry.name().map_err(|_| unreadable())?.to_string();
        if !folded.insert(fold(&name)) {
            return Err(unreadable());
        }
        let room = ENTRY_MAX.min(ENTRIES_MAX - held);
        let mut content = Vec::new();
        entry
            .take(room + 1)
            .read_to_end(&mut content)
            .map_err(|_| unreadable())?;
        if content.len() as u64 > room {
            return Err(size());
        }
        held += content.len() as u64;
        all.insert(name, content);
    }
    Ok(all)
}

/// A name as the package format compares part names: without ASCII case, `\` read as `/`.
pub(crate) fn fold(name: &str) -> String {
    name.replace('\\', "/").to_ascii_lowercase()
}

/// The entries of a package by their folded names. [`entries`] refuses two entries whose names
/// fold alike, so each folded name stands for one entry.
pub(crate) struct Names<'a> {
    folded: BTreeMap<String, (&'a str, &'a [u8])>,
}

impl<'a> Names<'a> {
    pub(crate) fn new(entries: &'a Entries) -> Names<'a> {
        let folded = entries
            .iter()
            .map(|(name, bytes)| (fold(name), (name.as_str(), bytes.as_slice())))
            .collect();
        Names { folded }
    }

    /// The entry that `name` names, with its name as the archive spells it.
    pub(crate) fn find(&self, name: &str) -> Option<(&'a str, &'a [u8])> {
        self.folded.get(&fold(name)).copied()
    }
}

/// A target with each `%` and two hex digits decoded. Refused: a `%` without two hex digits,
/// an encoded `/` or `\`, and bytes that are not UTF-8.
fn decoded(target: &str) -> Result<String, Error> {
    let bytes = target.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] != b'%' {
            out.push(bytes[at]);
            at += 1;
            continue;
        }
        let hex = |i: usize| bytes.get(i).copied().filter(u8::is_ascii_hexdigit);
        let (Some(high), Some(low)) = (hex(at + 1), hex(at + 2)) else {
            return Err(relationship());
        };
        let digit = |b: u8| (b as char).to_digit(16).unwrap_or_default() as u8;
        let byte = digit(high) * 16 + digit(low);
        if byte == b'/' || byte == b'\\' {
            return Err(relationship());
        }
        out.push(byte);
        at += 3;
    }
    String::from_utf8(out).map_err(|_| relationship())
}

fn entry_kind(name: &str) -> Option<Kind> {
    let name = fold(name);
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

fn relationship_kind(tail: &str) -> Option<Kind> {
    match tail {
        "vbaProject" | "xlMacrosheet" | "xlIntlMacrosheet" => Some(Kind::Macro),
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
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|_| Error::Workbook(Refusal::Part))?;
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
    if let Some(ch) = reference
        .resolve_char_ref()
        .map_err(|_| Error::Workbook(Refusal::Part))?
    {
        return Ok(ch.to_string());
    }
    let name: &str = reference.as_ref();
    Ok(match resolve_predefined_entity(name) {
        Some(text) => text.to_string(),
        None => format!("&{name};"),
    })
}

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

/// A relationship as the crate reads it: its id and target as written (the reader keys and
/// joins them so), its target's value, and the last segment of its type.
struct Rel {
    id: String,
    tail: String,
    target: String,
    raw: String,
    /// `TargetMode="External"`: the target is an address outside the package, no part name.
    external: bool,
}

fn tail(kind_type: &str) -> &str {
    kind_type.rsplit('/').next().unwrap_or_default()
}

fn relationships(rels: &[u8]) -> Result<Vec<Rel>, Error> {
    let mut reader = new_reader(rels);
    let mut buf = Vec::new();
    let mut found = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf).map_err(|_| part())?;
        match &event {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) if local(e.name().as_ref()) == "Relationship" => {
                let mut rel = Rel {
                    id: String::new(),
                    tail: String::new(),
                    target: String::new(),
                    raw: String::new(),
                    external: false,
                };
                for attribute in e.attributes() {
                    let attribute = attribute.map_err(|_| part())?;
                    let value = attribute
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map_err(|_| part())?;
                    match attribute.key.as_ref() {
                        "Id" => rel.id = attribute.value.to_string(),
                        "Type" => rel.tail = tail(&value).to_string(),
                        "Target" => {
                            rel.target = value.into_owned();
                            rel.raw = attribute.value.to_string();
                        }
                        "TargetMode" => rel.external = value == "External",
                        _ => {}
                    }
                }
                found.push(rel);
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(found)
}

/// The entry that `target`, decoded and taken from the part `source`, names.
fn named<'a>(names: &Names<'a>, source: &str, target: &str) -> Result<Option<&'a str>, Error> {
    let path = resolve(source, &decoded(target)?);
    Ok(names.find(&path).map(|(name, _)| name))
}

/// The workbook part, as the archive spells its name. Refused: no such entry, no package
/// relationships, no office document relationship, or one whose target as written is not
/// `xl/workbook.xml` (ASCII case ignored, one leading `/` allowed): the reader opens the part
/// that this relationship names.
pub(crate) fn workbook<'a>(names: &Names<'a>) -> Result<(&'a str, &'a [u8]), Error> {
    let found = names.find(WORKBOOK).ok_or_else(unreadable)?;
    let (_, root) = names.find("_rels/.rels").ok_or_else(unreadable)?;
    let documents: Vec<Rel> = relationships(root)?
        .into_iter()
        .filter(|rel| rel.tail == "officeDocument")
        .collect();
    let at_workbook = |rel: &Rel| {
        let raw = rel.raw.strip_prefix('/').unwrap_or(&rel.raw);
        raw.eq_ignore_ascii_case(WORKBOOK)
    };
    if documents.is_empty() || !documents.iter().all(at_workbook) {
        return Err(relationship());
    }
    Ok(found)
}

/// The name of the workbook part as the archive spells it.
pub(crate) fn workbook_name(entries: &Entries) -> Result<String, Error> {
    Ok(workbook(&Names::new(entries))?.0.to_string())
}

struct Sheet<'a> {
    name: String,
    part: Option<&'a str>,
    agrees: bool,
}

/// The sheets that the workbook part names, in its order, read as the reader reads them: the
/// element `sheet` at any depth, its name with entities decoded, and its relationship by the
/// last attribute `id` of any prefix. Each comes with the entry that its relationship names,
/// and with whether the reader, which joins `xl/` and the target as written, finds that entry.
fn sheets<'a>(names: &Names<'a>) -> Result<Vec<Sheet<'a>>, Error> {
    let (book, xml) = workbook(names)?;
    let rels = match names.find(&rels_of(book)) {
        Some((_, rels)) => relationships(rels)?,
        None => Vec::new(),
    };
    let by_id: BTreeMap<&str, &Rel> = rels.iter().map(|rel| (rel.id.as_str(), rel)).collect();
    let mut found = Vec::new();
    for (name, id) in sheet_elements(xml)? {
        let internal = id
            .as_deref()
            .and_then(|id| by_id.get(id))
            .filter(|rel| !rel.external);
        let Some(rel) = internal else {
            found.push(Sheet {
                name,
                part: None,
                agrees: false,
            });
            continue;
        };
        let part = named(names, book, &rel.target)?;
        let read = match rel.raw.strip_prefix('/') {
            Some(path) => path.to_string(),
            None => format!("xl/{}", rel.raw),
        };
        let agrees = part.is_some_and(|part| fold(part) == read.to_ascii_lowercase());
        found.push(Sheet { name, part, agrees });
    }
    Ok(found)
}

fn sheet_elements(xml: &[u8]) -> Result<Vec<(String, Option<String>)>, Error> {
    let mut reader = new_reader(xml);
    let mut buf = Vec::new();
    let mut found = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf).map_err(|_| part())?;
        match &event {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) if local(e.name().as_ref()) == "sheet" => {
                let mut name = String::new();
                let mut id = None;
                for attribute in e.attributes() {
                    let attribute = attribute.map_err(|_| part())?;
                    let key = attribute.key.as_ref();
                    if key == "name" {
                        name = unescape(&attribute.value).map_err(|_| part())?.into_owned();
                    } else if key == "id" || key.ends_with(":id") {
                        id = Some(attribute.value.to_string());
                    }
                }
                found.push((name, id));
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(found)
}

#[derive(Default)]
struct Linked {
    sheets: BTreeSet<String>,
    kinds: BTreeSet<Kind>,
}

fn by_relationship(entries: &Entries, names: &Names<'_>) -> Result<Linked, Error> {
    let mut linked = Linked::default();
    for (name, xml) in entries {
        let folded = fold(name);
        if !folded.ends_with(".rels") {
            continue;
        }
        let source = source_of(&folded).unwrap_or_default();
        for rel in relationships(xml)? {
            if rel.external {
                if READ.iter().any(|read| read.eq_ignore_ascii_case(&rel.tail)) {
                    return Err(relationship());
                }
                continue;
            }
            let target = named(names, &source, &rel.target)?;
            let tail = rel.tail.as_str();
            if !READ.contains(&tail) {
                if READ.iter().any(|read| read.eq_ignore_ascii_case(tail)) {
                    return Err(relationship());
                }
                continue;
            }
            let target = target.ok_or_else(relationship)?;
            linked.kinds.extend(relationship_kind(tail));
            if SHEETS.contains(&tail) {
                linked.sheets.insert(fold(target));
            }
        }
    }
    for sheet in sheets(names)? {
        linked.sheets.extend(sheet.part.map(fold));
    }
    Ok(linked)
}

fn holds(set: PartSet, name: &str, sheets: &BTreeSet<String>) -> bool {
    let name = fold(name);
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
    let names = Names::new(entries);
    let linked = by_relationship(entries, &names)?;
    let mut found = Inventory::default();
    found.present.extend(linked.kinds.iter().copied());
    for (name, xml) in entries {
        if let Some(kind) = entry_kind(name) {
            found.present.insert(kind);
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
        }
    }
    Ok(found)
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

/// The entries that the relationships of type `kind` of the part `part` name, as the archive
/// spells them. A target that names no entry refuses the workbook, whatever its target mode.
pub(crate) fn linked(entries: &Entries, part: &str, kind: &str) -> Result<Vec<String>, Error> {
    let names = Names::new(entries);
    let Some((_, rels)) = names.find(&rels_of(part)) else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    for rel in relationships(rels)? {
        if rel.tail == kind && !rel.external {
            let target = named(&names, part, &rel.target)?.ok_or_else(relationship)?;
            found.push(target.to_string());
        }
    }
    Ok(found)
}

/// The entry of the sheet named `sheet` (compared byte for byte), as the archive spells it.
/// [`Error::NoSheet`] when the workbook names no such sheet. Refused when its relationship is
/// missing, names no entry, or names another entry than the one that the reader reads.
pub(crate) fn part_of(entries: &Entries, sheet: &str) -> Result<String, Error> {
    let names = Names::new(entries);
    let found = sheets(&names)?
        .into_iter()
        .find(|found| found.name == sheet)
        .ok_or(Error::NoSheet)?;
    match found.part {
        Some(part) if found.agrees => Ok(part.to_string()),
        _ => Err(relationship()),
    }
}

#[cfg(test)]
pub(crate) fn sheet_parts(entries: &Entries) -> Vec<String> {
    let names = Names::new(entries);
    let all = sheets(&names).expect("the sheets");
    all.into_iter()
        .filter_map(|sheet| sheet.part.map(str::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Refusal;
    use crate::testing::{corpus, edit, zipped};

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
    fn a_target_is_percent_decoded_and_an_encoded_separator_or_a_broken_escape_is_refused() {
        let cases: [(&str, Option<&str>); 10] = [
            ("../tables/table1.xml", Some("../tables/table1.xml")),
            ("table%31.xml", Some("table1.xml")),
            ("t%61ble%2E%78ml", Some("table.xml")),
            ("%C3%A9.xml", Some("\u{e9}.xml")),
            ("..%2Ftables", None),
            ("..%2ftables", None),
            ("..%5Ctables", None),
            ("table%3.xml", None),
            ("table%+1.xml", None),
            ("%E9.xml", None),
        ];
        for (target, want) in cases {
            let got = decoded(target).ok();
            assert_eq!(got.as_deref(), want, "{target}");
        }
        assert_eq!(decoded("%2F"), Err(Error::Workbook(Refusal::Relationship)));
    }

    /// Why the check of the held size in `entries` cannot be reached through zip: zip 9.5.1
    /// stops an entry of an archive at the size that its header declares, with an error, and
    /// the first pass has already refused a declared size over the room that the read gets.
    #[test]
    fn zip_stops_an_entry_at_its_declared_size_before_the_cut_at_the_bound() {
        use std::io::Write;
        let name = "docProps/held.bin";
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        out.start_file(name, options).expect("start");
        out.write_all(&vec![0u8; 1 << 20]).expect("write");
        let mut bytes = out.finish().expect("finish").into_inner();
        let spots: Vec<usize> = bytes
            .windows(name.len())
            .enumerate()
            .filter(|(_, w)| *w == name.as_bytes())
            .map(|(at, _)| at)
            .collect();
        let (local, central) = (spots[0] - 30, spots[1] - 46);
        bytes[local + 22..local + 26].copy_from_slice(&100u32.to_le_bytes());
        bytes[central + 24..central + 28].copy_from_slice(&100u32.to_le_bytes());
        let mut archive = zip::ZipArchive::new(Cursor::new(&bytes[..])).expect("a zip");
        let entry = archive.by_index(0).expect("the entry");
        assert_eq!(entry.size(), 100);
        let mut content = Vec::new();
        let read = entry.take(ENTRY_MAX + 1).read_to_end(&mut content);
        let error = read.expect_err("zip refuses to read past the declared size");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{error}");
        assert!(
            content.len() <= 100 + 64 * 1024,
            "{} bytes held",
            content.len()
        );
        assert_eq!(entries(&bytes), Err(unreadable()));
    }

    #[test]
    fn the_refused_kinds_are_the_executors_own_list() {
        assert_eq!(names(Kind::REFUSED), vec!["pivot", "macro", "validation"]);
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
    }

    #[test]
    fn a_validation_in_a_sheet_part_that_no_relationship_links_is_found_by_its_name() {
        let mut all = corpus("values.xlsx");
        let orphan = r#"<worksheet><sheetData/><dataValidations count="1"><dataValidation type="whole" sqref="A1"><formula1>0</formula1></dataValidation></dataValidations></worksheet>"#;
        all.insert(
            "xl/worksheets/sheet9.xml".to_string(),
            orphan.as_bytes().to_vec(),
        );
        let found = inventory(&all).expect("an inventory");
        assert_eq!(names(found.refused()), vec!["validation"]);
    }

    #[test]
    fn a_validation_in_a_part_that_the_workbook_names_or_a_worksheet_relationship_links_is_found() {
        let moved = |kind: &str| {
            let mut all = corpus("validation.xlsx");
            let sheet = all.remove("xl/worksheets/sheet1.xml").expect("the sheet");
            all.insert("xl/data/table1.xml".to_string(), sheet);
            edit(
                &mut all,
                "xl/_rels/workbook.xml.rels",
                "worksheets/sheet1.xml",
                "data/table1.xml",
            );
            let od = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
            edit(
                &mut all,
                "xl/_rels/workbook.xml.rels",
                &format!("{od}/worksheet"),
                kind,
            );
            all
        };
        let named_by_the_workbook = moved("http://example.com/relationships/sheet");
        let found = inventory(&named_by_the_workbook).expect("an inventory");
        assert_eq!(names(found.refused()), vec!["validation"]);
        let mut orphan = corpus("values.xlsx");
        let sheet = corpus("validation.xlsx")["xl/worksheets/sheet1.xml"].clone();
        orphan.insert("xl/data/extra.xml".to_string(), sheet);
        let rel = r#"<Relationship Id="rId99" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="data/extra.xml"/></Relationships>"#;
        edit(
            &mut orphan,
            "xl/_rels/workbook.xml.rels",
            "</Relationships>",
            rel,
        );
        let found = inventory(&orphan).expect("an inventory");
        assert_eq!(names(found.refused()), vec!["validation"]);
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
    fn a_file_that_is_no_zip_is_unreadable_and_a_broken_part_is_refused() {
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
        assert_eq!(
            inventory(&reread).unwrap_err(),
            Error::Workbook(Refusal::Part)
        );
    }
}
