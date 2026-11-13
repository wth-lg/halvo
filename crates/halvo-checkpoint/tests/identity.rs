//! The ruling CP-129 (1): every place of halvo-checkpoint and halvo-sheet that compares two files
//! for identity goes through `identity.rs` of halvo-checkpoint. This test reads the sources of
//! both crates and fails when a file other than that module reads what names a file on the
//! system or defines a compare of its own.

use std::fs;
use std::path::Path;

/// The words that read or compare what names a file: the inode and the device, the times that
/// the old check compared, the Windows file id and its calls, and a function of the old name.
const WORDS: [&str; 11] = [
    ".ino()",
    ".dev()",
    "creation_time()",
    "last_write_time()",
    ".mtime()",
    "file_index",
    "volume_serial_number",
    "GetFileInformationByHandle(",
    "FileIdInfo",
    "FILE_ID_INFO",
    "fn same_file",
];

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
            for word in WORDS {
                if text.contains(word) {
                    let name = Path::new(&path).file_name().unwrap_or_default();
                    places.push(format!("{}: {word}", name.to_string_lossy()));
                }
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
