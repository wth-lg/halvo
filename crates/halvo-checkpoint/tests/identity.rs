//! The ruling CP-129 (1): every place of halvo-checkpoint and halvo-sheet that compares two files
//! for identity goes through `identity.rs` of halvo-checkpoint. This test reads the sources of
//! both crates and fails when a file other than that module reads what names a file on the
//! system or defines a compare of its own.

use std::fs;
use std::path::Path;

/// The functions and fields that read what names a file on the system, matched by their own
/// name wherever it is called or named: as a method (`meta.ino()`), through the trait's path
/// (`MetadataExt::ino(&meta)`) or as a value (`.map(MetadataExt::ino)`).
const READS: [&str; 10] = [
    "ino",
    "dev",
    "mtime",
    "mtime_nsec",
    "creation_time",
    "last_write_time",
    "file_index",
    "volume_serial_number",
    "st_ino",
    "st_dev",
];

/// The Windows call and type of the file id, and a compare under the old name.
const WORDS: [&str; 4] = [
    "GetFileInformationByHandle(",
    "FileIdInfo",
    "FILE_ID_INFO",
    "fn same_file",
];

/// The names of `READS` that `text` calls or names: an identifier of the list that follows a
/// `.` or a `::`, or that a `(` follows.
fn reads(text: &str) -> Vec<&'static str> {
    let bytes = text.as_bytes();
    let word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if !word(bytes[at]) || (at > 0 && word(bytes[at - 1])) {
            at += 1;
            continue;
        }
        let end = (at..bytes.len())
            .find(|&i| !word(bytes[i]))
            .unwrap_or(bytes.len());
        if let Some(name) = READS.iter().find(|name| **name == &text[at..end]) {
            let before = text[..at].trim_end();
            let after = text[end..].trim_start();
            if before.ends_with('.') || before.ends_with("::") || after.starts_with('(') {
                found.push(*name);
            }
        }
        at = end;
    }
    found
}

fn sources(dir: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).expect("list the sources").flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(sources(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let text = fs::read_to_string(&path).expect("read a source");
            found.push((path.to_string_lossy().into_owned(), text));
        }
    }
    found
}

/// The places that read one of the words, as `file: word`, outside `identity.rs`.
pub fn around(roots: &[&Path]) -> Vec<String> {
    let mut places = Vec::new();
    for root in roots {
        for (path, text) in sources(root) {
            if path.ends_with("identity.rs") {
                continue;
            }
            let name = Path::new(&path).file_name().unwrap_or_default();
            let name = name.to_string_lossy();
            for word in WORDS {
                if text.contains(word) {
                    places.push(format!("{name}: {word}"));
                }
            }
            for read in reads(&text) {
                places.push(format!("{name}: {read}"));
            }
        }
    }
    places
}

#[test]
fn no_source_of_either_crate_compares_two_files_outside_the_identity_module() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let roots = [here.join("src"), here.join("../halvo-sheet/src")];
    let roots: Vec<&Path> = roots.iter().map(|root| root.as_path()).collect();
    let places = around(&roots);
    let read = sources(roots[0]).len() + sources(roots[1]).len();
    println!(
        "cp129 identity sources_read={read} places_around={}",
        places.len()
    );
    assert!(read > 20, "the sources of both crates were read: {read}");
    assert_eq!(
        places,
        Vec::<String>::new(),
        "a place compares files around identity.rs"
    );
    let module = fs::read_to_string(here.join("src/identity.rs")).expect("read identity.rs");
    assert!(module.contains("pub fn same_file(a: &FileId, b: &FileId) -> bool"));
}

/// CP-134 H1: `around` reports a source outside `identity.rs` that reads through the trait's
/// path or names a read as a value, and skips a file named `identity.rs`.
#[test]
fn around_reports_a_source_that_reads_through_the_traits_path() {
    let dir =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cp134-scan-{}", std::process::id()));
    fs::create_dir_all(&dir).expect("make the directory");
    fs::write(
        dir.join("probe.rs"),
        "fn f(m: &Metadata) -> u64 { MetadataExt::ino(m) }\nfn g(v: &[Metadata]) { v.iter().map(MetadataExt::dev); }\n",
    )
    .expect("write the probe");
    fs::write(
        dir.join("identity.rs"),
        "fn h(m: &Metadata) -> u64 { m.ino() }\n",
    )
    .expect("write the module's stand-in");
    let places = around(&[dir.as_path()]);
    assert_eq!(places, vec!["probe.rs: ino", "probe.rs: dev"]);
}

/// CP-134 H1: the scan sees a read written through the trait's path or named as a value, not
/// only as a method, and passes a word of the list used as a plain name.
#[test]
fn the_scan_sees_a_read_through_the_traits_path_or_as_a_value() {
    assert_eq!(reads("let a = meta.ino();"), vec!["ino"]);
    assert_eq!(
        reads("MetadataExt::ino(&a) == MetadataExt::ino(&b)"),
        vec!["ino", "ino"]
    );
    assert_eq!(reads("metas.iter().map(MetadataExt::dev)"), vec!["dev"]);
    assert_eq!(reads("creation_time (&m)"), vec!["creation_time"]);
    assert_eq!(
        reads("let (dev, inode) = id.numbers(); let ino = 3;"),
        Vec::<&str>::new()
    );
    assert_eq!(reads("casino(1); device(2); stat.st_ino"), vec!["st_ino"]);
}
