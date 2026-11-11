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

pub(crate) fn edit(all: &mut Entries, name: &str, from: &str, to: &str) {
    let text = String::from_utf8(all.remove(name).expect("the entry")).expect("utf-8");
    assert!(text.contains(from), "{name} lacks {from}");
    all.insert(name.to_string(), text.replacen(from, to, 1).into_bytes());
}
