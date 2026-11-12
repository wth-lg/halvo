#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct Scratch {
    pub dir: PathBuf,
}

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let n = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("halvo-sheet")
            .join(format!("{name}-{}-{nanos}-{n}", std::process::id()));
        fs::create_dir_all(&dir).expect("make the scratch directory");
        Scratch { dir }
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    pub fn write(&self, rel: &str, bytes: &[u8]) -> PathBuf {
        let path = self.dir.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("make the parent directory");
        }
        fs::write(&path, bytes).expect("write the file");
        path
    }

    pub fn names(&self, rel: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.dir.join(rel))
            .expect("list the directory")
            .map(|entry| {
                entry
                    .expect("read the entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
}

pub const RUN: &str = "0123456789abcdef0123456789abcdef";

pub fn locks() -> halvo_sheet::Locks {
    locks_waiting(std::time::Duration::from_secs(10))
}

pub fn locks_waiting(wait: std::time::Duration) -> halvo_sheet::Locks {
    let data = Path::new(env!("CARGO_TARGET_TMPDIR")).join("halvo-sheet-data");
    fs::create_dir_all(&data).expect("make the data directory");
    halvo_sheet::Locks::open(&data, wait).expect("the locks open")
}

pub struct Change {
    pub file: &'static str,
    pub sheet: &'static str,
    pub cell: &'static str,
    pub pos: (u32, u32),
    pub new: f64,
}

pub const CARRIED: [Change; 7] = [
    Change {
        file: "values.xlsx",
        sheet: "Values",
        cell: "B5",
        pos: (4, 1),
        new: 77.0,
    },
    Change {
        file: "formulas.xlsx",
        sheet: "Ledger",
        cell: "D2",
        pos: (1, 3),
        new: 42.0,
    },
    Change {
        file: "formats.xlsx",
        sheet: "Formats",
        cell: "B3",
        pos: (2, 1),
        new: 2500.5,
    },
    Change {
        file: "merged.xlsx",
        sheet: "Merged",
        cell: "B4",
        pos: (3, 1),
        new: 99.0,
    },
    Change {
        file: "chart.xlsx",
        sheet: "Sales",
        cell: "B3",
        pos: (2, 1),
        new: 640.0,
    },
    Change {
        file: "two-sheets.xlsx",
        sheet: "Input",
        cell: "B3",
        pos: (2, 1),
        new: 25.0,
    },
    Change {
        file: "rows-5000.xlsx",
        sheet: "Rows",
        cell: "C2501",
        pos: (2500, 2),
        new: 123456.0,
    },
];

pub const REFUSED: Change = Change {
    file: "validation.xlsx",
    sheet: "Validation",
    cell: "B3",
    pos: (2, 1),
    new: 7.0,
};

pub type Cells = std::collections::BTreeMap<(u32, u32), (calamine::Data, Option<String>)>;

pub fn book(path: &Path) -> Vec<(String, Cells)> {
    use calamine::Reader;
    let mut xlsx: calamine::Xlsx<_> = calamine::open_workbook(path).expect("calamine opens it");
    let mut sheets = Vec::new();
    for name in xlsx.sheet_names() {
        let mut cells = Cells::new();
        let values = xlsx.worksheet_range(&name).expect("values");
        if let Some((r0, c0)) = values.start() {
            for (r, c, value) in values.used_cells() {
                let empty = matches!(value, calamine::Data::Empty)
                    || matches!(value, calamine::Data::String(s) if s.is_empty());
                if !empty {
                    cells.insert((r0 + r as u32, c0 + c as u32), (value.clone(), None));
                }
            }
        }
        let formulas = xlsx.worksheet_formula(&name).expect("formulas");
        if let Some((r0, c0)) = formulas.start() {
            for (r, c, text) in formulas.used_cells() {
                if !text.is_empty() {
                    cells
                        .entry((r0 + r as u32, c0 + c as u32))
                        .or_insert((calamine::Data::Empty, None))
                        .1 = Some(text.clone());
                }
            }
        }
        sheets.push((name, cells));
    }
    sheets
}

pub fn same_but(
    original: &[(String, Cells)],
    saved: &[(String, Cells)],
    sheet: &str,
    targets: &[(u32, u32)],
) -> Vec<String> {
    let mut wrong = Vec::new();
    let names = |book: &[(String, Cells)]| book.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    if names(original) != names(saved) {
        wrong.push("sheet names differ".to_string());
        return wrong;
    }
    for ((name, before), (_, after)) in original.iter().zip(saved) {
        let mut positions: Vec<&(u32, u32)> = before.keys().chain(after.keys()).collect();
        positions.sort();
        positions.dedup();
        for pos in positions {
            if name == sheet && targets.contains(pos) {
                continue;
            }
            let same = match (before.get(pos), after.get(pos)) {
                (Some((_, Some(f))), Some((_, g))) => g.as_ref() == Some(f),
                (Some((v, None)), Some((w, None))) => v == w,
                _ => false,
            };
            if !same {
                wrong.push(format!("{name}!{pos:?} differs"));
            }
        }
    }
    wrong
}

pub fn number_at(book: &[(String, Cells)], sheet: &str, pos: (u32, u32)) -> Option<f64> {
    let (_, cells) = book.iter().find(|(n, _)| n == sheet)?;
    match cells.get(&pos)? {
        (calamine::Data::Float(x), None) => Some(*x),
        (calamine::Data::Int(x), None) => Some(*x as f64),
        _ => None,
    }
}

pub fn element_counts(path: &Path) -> Vec<(String, usize)> {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(fs::File::open(path).expect("open")).expect("a zip");
    let mut text = String::new();
    let mut names = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("entry");
        names.push(entry.name().expect("name").to_ascii_lowercase());
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read");
        text.push_str(&String::from_utf8_lossy(&bytes));
    }
    let mut counts: Vec<(String, usize)> = [
        "<mergeCell ",
        "<numFmt ",
        "<definedName ",
        "<dataValidation ",
    ]
    .iter()
    .map(|tag| (tag.to_string(), text.matches(tag).count()))
    .collect();
    for dir in ["xl/charts/", "xl/drawings/", "xl/tables/", "xl/comments"] {
        let n = names
            .iter()
            .filter(|n| n.starts_with(dir) && !n.ends_with('/') && !n.contains("/_rels/"))
            .count();
        counts.push((dir.to_string(), n));
    }
    counts
}

pub fn rewrite(source: &Path, target: &Path, edits: &[(&str, &str, &str)]) {
    use std::io::{Read, Write};
    let mut archive = zip::ZipArchive::new(fs::File::open(source).expect("open")).expect("a zip");
    let mut out = zip::ZipWriter::new(fs::File::create(target).expect("create"));
    let options = zip::write::SimpleFileOptions::default();
    let mut used = vec![false; edits.len()];
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("entry");
        let name = entry.name().expect("name").to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read");
        for (at, (part, from, to)) in edits.iter().enumerate() {
            if *part == name {
                let text = String::from_utf8(bytes).expect("utf-8");
                assert_eq!(text.matches(from).count(), 1, "{name} holds {from} once");
                bytes = text.replacen(from, to, 1).into_bytes();
                used[at] = true;
            }
        }
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(&bytes).expect("write");
    }
    out.finish().expect("finish");
    assert!(used.iter().all(|u| *u), "an edit found no entry");
}

pub type Repack<'a> = &'a dyn Fn(&str, Vec<u8>) -> Option<(String, Vec<u8>)>;

pub fn repack(source: &Path, target: &Path, each: Repack<'_>, extra: &[(&str, &[u8])]) {
    use std::io::{Read, Write};
    let mut archive = zip::ZipArchive::new(fs::File::open(source).expect("open")).expect("a zip");
    let mut out = zip::ZipWriter::new(fs::File::create(target).expect("create"));
    let options = zip::write::SimpleFileOptions::default();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("entry");
        let name = entry.name().expect("name").to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read");
        if let Some((name, bytes)) = each(&name, bytes) {
            out.start_file(name.as_str(), options).expect("start");
            out.write_all(&bytes).expect("write");
        }
    }
    for (name, bytes) in extra {
        out.start_file(*name, options).expect("start");
        out.write_all(bytes).expect("write");
    }
    out.finish().expect("finish");
}

pub fn replaced(bytes: Vec<u8>, from: &str, to: &str) -> Vec<u8> {
    let text = String::from_utf8(bytes).expect("utf-8");
    assert_eq!(text.matches(from).count(), 1, "the part holds {from} once");
    text.replacen(from, to, 1).into_bytes()
}

pub fn rewrite_bytes(source: &Path, target: &Path, part: &str, edit: &dyn Fn(Vec<u8>) -> Vec<u8>) {
    use std::io::{Read, Write};
    let mut archive = zip::ZipArchive::new(fs::File::open(source).expect("open")).expect("a zip");
    let mut out = zip::ZipWriter::new(fs::File::create(target).expect("create"));
    let options = zip::write::SimpleFileOptions::default();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("entry");
        let name = entry.name().expect("name").to_string();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read");
        if name == part {
            bytes = edit(bytes);
        }
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(&bytes).expect("write");
    }
    out.finish().expect("finish");
}

pub fn entry(path: &Path, name: &str) -> Vec<u8> {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(fs::File::open(path).expect("open")).expect("a zip");
    let mut entry = archive.by_name(name).expect("the entry");
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).expect("read");
    bytes
}

pub fn entry_names(path: &Path) -> Vec<String> {
    let archive = zip::ZipArchive::new(fs::File::open(path).expect("open")).expect("a zip");
    let mut names: Vec<String> = archive
        .file_names()
        .map(|name| name.expect("a name").to_string())
        .collect();
    names.sort();
    names
}

pub fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/r3-trials/sheet/corpus")
        .join(name)
}

#[cfg(unix)]
pub fn link_file(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("make the link");
}

#[cfg(windows)]
pub fn link_file(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_file(target, link).expect("make the link");
}

#[cfg(unix)]
pub fn link_dir(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("make the link");
}

#[cfg(windows)]
pub fn link_dir(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).expect("make the link");
}
