use std::fs;
use std::path::{Path, PathBuf};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(path: &Path) -> String {
    match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => panic!("cannot read {}: {e}", path.display()),
    }
}

/// The names of the keys of the TOML table `[name]` of `manifest`, in order.
fn table_keys(manifest: &str, name: &str) -> Vec<String> {
    let header = format!("[{name}]");
    let mut inside = false;
    let mut keys = Vec::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == header;
            continue;
        }
        if inside && !line.is_empty() && !line.starts_with('#') {
            let key = line.split('=').next().unwrap_or("").trim();
            keys.push(key.to_owned());
        }
    }
    keys
}

fn tables(manifest: &str) -> Vec<String> {
    manifest
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('['))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_crate_depends_on_halvo_schema_serde_serde_json_and_sha2_and_nothing_else() {
    let manifest = read(&crate_dir().join("Cargo.toml"));
    assert_eq!(
        tables(&manifest),
        ["[package]", "[dependencies]"],
        "the manifest has another table than [package] and [dependencies]"
    );
    assert_eq!(
        table_keys(&manifest, "dependencies"),
        ["halvo-schema", "serde", "serde_json", "sha2"]
    );
    assert_eq!(
        table_keys(&manifest, "package"),
        ["name", "version", "edition", "publish"]
    );
    assert!(manifest.contains("version = \"0.0.0\""));
    assert!(manifest.contains("edition = \"2021\""));
    assert!(manifest.contains("publish = false"));
}

#[test]
fn halvo_schema_does_not_depend_on_halvo_policy() {
    let manifest = read(&crate_dir().join("../halvo-schema/Cargo.toml"));
    assert!(!manifest.contains("halvo-policy"));
    assert!(!manifest.contains("halvo_policy"));
}

const IO: &[&str] = &[
    "std::fs",
    "std::io",
    "std::net",
    "std::process",
    "std::env",
    "std::time",
    "std::thread",
    "std::{",
    "::std",
    "extern crate",
    "print!",
    "println!",
    "eprint!",
    "eprintln!",
    "dbg!",
    "include_str!",
    "include_bytes!",
    "env!",
    "option_env!",
];

/// The library's files: `src/lib.rs` and the files of its module lines that no `#[cfg(test)]` line stands before.
fn library_files() -> Vec<PathBuf> {
    let src = crate_dir().join("src");
    let lib = read(&src.join("lib.rs"));
    let mut files = vec![src.join("lib.rs")];
    let mut test_only = false;
    for line in lib.lines() {
        let line = line.trim();
        if line == "#[cfg(test)]" {
            test_only = true;
            continue;
        }
        let module = line
            .strip_prefix("pub mod ")
            .or_else(|| line.strip_prefix("mod "))
            .and_then(|rest| rest.strip_suffix(';'));
        if let Some(module) = module {
            if !test_only {
                files.push(src.join(format!("{module}.rs")));
            }
        }
        test_only = false;
    }
    files
}

#[test]
fn the_library_reads_no_file_clock_network_or_environment_and_prints_nothing() {
    let files = library_files();
    assert!(files.len() > 1, "no module line was read from src/lib.rs");
    for path in files {
        let text = read(&path);
        for token in IO {
            assert!(
                !text.contains(token),
                "{} holds {token}",
                path.strip_prefix(crate_dir()).unwrap_or(&path).display()
            );
        }
    }
}
