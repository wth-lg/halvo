use std::io::{Cursor, Write};
use std::path::Path;

use crate::parts::{entries, Entries};

pub(crate) fn corpus(name: &str) -> Entries {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/r3-trials/sheet/corpus")
        .join(name);
    entries(&std::fs::read(path).expect("read the corpus file")).expect("a zip")
}

pub(crate) fn zipped(all: &Entries) -> Vec<u8> {
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in all {
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(bytes).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

pub(crate) fn corpus_file(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/r3-trials/sheet/corpus")
        .join(name)
}

pub(crate) fn ordered(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    use std::io::Read;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).expect("a zip");
    (0..archive.len())
        .map(|i| {
            let mut entry = archive.by_index(i).expect("entry");
            let name = entry.name().expect("name").to_string();
            let mut content = Vec::new();
            entry.read_to_end(&mut content).expect("read");
            (name, content)
        })
        .collect()
}

pub(crate) fn zipped_in_order(all: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, bytes) in all {
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(bytes).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

pub(crate) fn spoiled(bytes: &[u8]) -> Vec<u8> {
    let mut all = ordered(bytes);
    let entry = all
        .iter_mut()
        .find(|(name, _)| name == "[Content_Types].xml")
        .expect("a content types part");
    entry.1.push(b' ');
    zipped_in_order(&all)
}

pub(crate) fn scratch(name: &str) -> std::path::PathBuf {
    let exe = std::env::current_exe().expect("the test binary");
    let target = exe
        .ancestors()
        .nth(3)
        .expect("the target directory above deps");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = target
        .join("tmp")
        .join("halvo-sheet-unit")
        .join(format!("{name}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("make the scratch directory");
    dir
}

pub(crate) fn locks() -> halvo_checkpoint::Locks {
    let exe = std::env::current_exe().expect("the test binary");
    let target = exe
        .ancestors()
        .nth(3)
        .expect("the target directory above deps");
    let data = target.join("tmp").join("halvo-sheet-unit-data");
    std::fs::create_dir_all(&data).expect("make the data directory");
    halvo_checkpoint::Locks::open(&data, std::time::Duration::from_secs(10)).expect("the locks")
}

pub(crate) fn edit(all: &mut Entries, name: &str, from: &str, to: &str) {
    let text = String::from_utf8(all.remove(name).expect("the entry")).expect("utf-8");
    assert!(text.contains(from), "{name} lacks {from}");
    all.insert(name.to_string(), text.replacen(from, to, 1).into_bytes());
}
