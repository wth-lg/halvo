//! The tree rules of r4 U3.17a over the Rust files of the app, read from the tree: only the ONE function's file
//! names a hosted tier of the supervisor library, and only the start of a run writes the field `hosted` of a
//! `RunSpec`, in ONE line that calls that function.
//!
//! What the reader of the rule does not see: a tier's name that a `use` of the variants or a renamed import hides,
//! and the field written by the shorthand of a struct expression (`RunSpec { hosted, .. }`). It reads a `let` or a
//! parameter named `hosted` with a type as a write of the field and refuses it.
#![cfg(any(target_os = "macos", windows))]

use std::path::{Path, PathBuf};

/// The file of the ONE function, the one file of the app that names a hosted tier of the library.
const ONE_FILE: &str = "crates/halvo-coord-app/src/agreement.rs";
/// The file of the app's start of a run.
const START_FILE: &str = "crates/halvo-coord-app/src/product.rs";
/// The call that the one line which writes the field holds.
const CALL: &str = "agreement::hosted(";
/// The library's names of its two hosted tiers.
const TIER_NAMES: [&str; 2] = ["Hosted::LowCostHosted", "Hosted::Frontier"];

fn sources(dir: &Path, found: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a directory of the app's sources") {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

/// The Rust files under `crates/halvo-coord-app/src` and `src-tauri/src`, each with its path from the root of the
/// repository, its parts joined by `/`, and its text.
fn tree() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths = Vec::new();
    for dir in ["crates/halvo-coord-app/src", "src-tauri/src"] {
        sources(&root.join(dir), &mut paths);
    }
    paths
        .iter()
        .map(|path| {
            let name: Vec<&str> = path
                .strip_prefix(&root)
                .unwrap()
                .components()
                .map(|part| part.as_os_str().to_str().unwrap())
                .collect();
            (name.join("/"), std::fs::read_to_string(path).unwrap())
        })
        .collect()
}

/// The files of `files` that name a hosted tier of the library.
fn naming_files(files: &[(String, String)]) -> Vec<&str> {
    files
        .iter()
        .filter(|(_, text)| TIER_NAMES.iter().any(|name| text.contains(name)))
        .map(|(path, _)| path.as_str())
        .collect()
}

/// Whether `line`, before a `//`, writes the field `hosted`: the word `hosted` followed by one `:` (a field of a
/// struct expression) or, after a `.`, by one `=` (an assignment).
fn writes_hosted(line: &str) -> bool {
    let code = line.split("//").next().unwrap_or_default();
    code.match_indices("hosted").any(|(at, word)| {
        let before = code[..at].chars().next_back();
        let rest = code[at + word.len()..].trim_start();
        let starts = before.is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == ':'));
        let field = rest.starts_with(':') && !rest.starts_with("::");
        let assign = before == Some('.') && rest.starts_with('=') && !rest.starts_with("==");
        starts && (field || assign)
    })
}

/// The lines of `files` that write the field `hosted`, each with its file.
fn field_writes(files: &[(String, String)]) -> Vec<(&str, &str)> {
    files
        .iter()
        .flat_map(|(path, text)| {
            text.lines()
                .filter(|line| writes_hosted(line))
                .map(move |line| (path.as_str(), line.trim()))
        })
        .collect()
}

/// The first rule of the two that `files` break, none where they keep both.
fn refusal(files: &[(String, String)]) -> Option<String> {
    let named = naming_files(files);
    if named != [ONE_FILE] {
        return Some(format!(
            "the library's hosted tiers are named in {named:?}; {ONE_FILE} alone may name them"
        ));
    }
    match field_writes(files).as_slice() {
        [(START_FILE, line)] if line.contains(CALL) => None,
        writes => Some(format!(
            "the field hosted is written by {writes:?}; one line of {START_FILE} alone, which calls {CALL}..), may write it"
        )),
    }
}

/// r4 U3.17a sentence 4: no other place of the app sets a hosted tier. In the Rust files under
/// crates/halvo-coord-app/src and src-tauri/src, the library's names `Hosted::LowCostHosted` and `Hosted::Frontier`
/// stand in the ONE function's file alone, and the field `hosted` of a `RunSpec` is written in ONE line, the start's,
/// which calls that function. On a changed text, a second file with a tier's name and a second line that writes the
/// field are each refused.
#[test]
fn only_the_one_function_names_a_hosted_tier_and_only_the_start_writes_the_field() {
    let files = tree();
    for dir in ["crates/halvo-coord-app/src/", "src-tauri/src/"] {
        assert!(
            files.iter().any(|(path, _)| path.starts_with(dir)),
            "no Rust file read under {dir}"
        );
    }
    assert_eq!(refusal(&files), None);

    let mut named = files.clone();
    named.push((
        "src-tauri/src/second.rs".to_owned(),
        "fn tier() -> Hosted {\n    Hosted::Frontier\n}\n".to_owned(),
    ));
    let refused = refusal(&named);
    assert!(
        refused.as_ref().is_some_and(|r| r.contains("are named in")),
        "{refused:?}"
    );

    let mut written = files.clone();
    let start = written
        .iter_mut()
        .find(|(path, _)| path == START_FILE)
        .unwrap();
    start
        .1
        .push_str("fn second(spec: &mut RunSpec) {\n    spec.hosted = None;\n}\n");
    let refused = refusal(&written);
    assert!(
        refused
            .as_ref()
            .is_some_and(|r| r.contains("is written by")),
        "{refused:?}"
    );
}

/// Whether `origins` name one origin at most: each the same as the first, or none.
fn one_origin<'a>(origins: impl IntoIterator<Item = &'a str>) -> bool {
    let mut origins = origins.into_iter();
    let first = origins.next();
    origins.all(|origin| Some(origin) == first)
}

/// r4 U3.17a sentence 7: a table of hosts whose rows name two origins is refused by the tree: every row of this
/// build's ruling, `halvo_route::release::ruling().rows()`, names the same ONE origin, or there is no row. The rule's
/// reader refuses two origins on plain texts.
#[test]
fn every_row_of_the_table_of_hosts_names_the_same_one_origin() {
    let ruling = halvo_route::release::ruling();
    let origins: Vec<&str> = ruling.rows().iter().map(|row| row.origin()).collect();
    assert!(
        one_origin(origins.iter().copied()),
        "the rows of this build's table of hosts name {origins:?}"
    );
    assert!(one_origin([
        "https://hosted-model.test",
        "https://hosted-model.test"
    ]));
    assert!(
        !one_origin(["https://hosted-model.test", "https://other-model.test"]),
        "two origins"
    );
}
