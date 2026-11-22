//! Rules on the source that the compiler reads, read as tokens with proc-macro2
//! so that how a line is spelled changes no count: the implementors of the text
//! filter point and of the secure-input probe, a rename of either with `as`,
//! either inside an attribute or a macro, and the uses of `#[path]` and of the
//! include macros. scripts/r1/ci.d/30-core-boundary-proofs.sh runs these tests
//! by name and prints the lines that they print. This file builds the two trait
//! names from parts, because the name searches of that check read the text of
//! every tracked file.

use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

const POINT: &str = concat!("TextFilter", "Point");
const PROBE: &str = concat!("SecureInput", "Probe");

/// The files of the secure_input module tree, the one place of a probe.
const PROBE_TREE: [&str; 3] = [
    "crates/halvo-capture/src/secure_input.rs",
    "crates/halvo-capture/src/secure_input_macos.rs",
    "crates/halvo-capture/src/windows/secure_input.rs",
];
const PROOF_FILE: &str = "crates/halvo-capture/src/floor/boundary_proofs.rs";
const PROBE_USER: &str = "crates/halvo-capture/src/floor/exclude.rs";
const TEST_POINTS: &str = "crates/halvo-capture/src/floor/cases.rs";
const DAEMON: &str = "crates/halvo-capture/src/daemon/";

/// Every use of `#[path]` and of the include macros that the crate and the app
/// may hold: the file from the root of the repository, the name, and the
/// literal that it reads.
const LISTED: [(&str, &str, &str); 14] = [
    (
        "crates/halvo-capture/src/secure_input.rs",
        "path",
        "\"secure_input_macos.rs\"",
    ),
    (
        "crates/halvo-capture/src/secure_input.rs",
        "path",
        "\"windows/secure_input.rs\"",
    ),
    (
        "crates/halvo-capture/src/secure_input_macos.rs",
        "path",
        "\"secure_input_macos_hold.rs\"",
    ),
    (
        "crates/halvo-capture/src/store/mod.rs",
        "path",
        "\"test_support/pages.rs\"",
    ),
    (
        "crates/halvo-capture/src/store/mod.rs",
        "path",
        "\"test_support/roots.rs\"",
    ),
    (
        "crates/halvo-capture/src/store/mod.rs",
        "path",
        "\"../../../halvo-sqlcipher/tests/page_gap/rules.rs\"",
    ),
    (
        "crates/halvo-capture/src/sink.rs",
        "include_str",
        "\"sink.rs\"",
    ),
    (
        "crates/halvo-capture/src/store/segment.rs",
        "include_bytes",
        "\"testdata/segment-v1.golden\"",
    ),
    (
        "crates/halvo-capture/src/store/segment.rs",
        "include_bytes",
        "\"testdata/segment-v2.golden\"",
    ),
    (
        "crates/halvo-capture/src/daemon/r2/owner_slot.rs",
        "path",
        "\"owner_slot_macos.rs\"",
    ),
    (
        "crates/halvo-capture/src/daemon/r2/owner_slot.rs",
        "path",
        "\"owner_slot_windows.rs\"",
    ),
    ("src-tauri/build.rs", "path", "\"src/commands/names.rs\""),
    (
        "src-tauri/src/tests.rs",
        "include_str",
        "\"engine-url-forms.txt\"",
    ),
    (
        "crates/halvo-capture/tests/cold_evidence.rs",
        "path",
        "\"browser/evidence.rs\"",
    ),
];

/// The roots whose files the tests of this file read.
const ROOTS: [&str; 2] = ["crates/halvo-capture/", "src-tauri/src/"];

/// The files of other crates that a listed use may reach, which these rules
/// read as Rust beside the files under [`ROOTS`] (ruling PR-30 PR7).
const READ_BESIDE_THE_ROOTS: [&str; 1] = ["crates/halvo-sqlcipher/tests/page_gap/rules.rs"];

/// The names of LISTED that bring code to the compiler; `include_str` and
/// `include_bytes` bring a text or bytes.
const CODE: [&str; 2] = ["path", "include"];

/// The file that a use reaches: its literal from the directory of `file`,
/// with each `.` segment taken out.
fn reached(file: &str, literal: &str) -> String {
    let dir = file.rsplit_once('/').map_or("", |(dir, _)| dir);
    let mut parts: Vec<&str> = dir.split('/').filter(|part| !part.is_empty()).collect();
    for part in literal.trim_matches('"').split('/') {
        match part {
            "." => {}
            ".." if parts.last().is_some_and(|last| *last != "..") => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

/// Each use of `listed` that brings code and reaches a file that these rules
/// do not read as Rust: a literal that is absolute, holds a backslash, starts
/// with a drive or climbs with `..`, or a file under no root of [`ROOTS`] or
/// on [`DATA_FILES`].
fn code_reach_faults(listed: &[(&str, &str, &str)]) -> Vec<String> {
    listed
        .iter()
        .filter(|(_, name, _)| CODE.contains(name))
        .filter_map(|(file, name, literal)| {
            let text = literal.trim_matches('"');
            let target = reached(file, literal);
            let drive = text.as_bytes().get(1) == Some(&b':');
            let beside = READ_BESIDE_THE_ROOTS.contains(&target.as_str());
            let climbs = text.split('/').any(|part| part == "..");
            let ok = !text.starts_with('/')
                && !text.contains('\\')
                && !drive
                && (!climbs || beside)
                && !target.split('/').any(|part| part == "..")
                && (ROOTS.iter().any(|root| target.starts_with(root)) || beside)
                && !DATA_FILES.contains(&target.as_str());
            (!ok).then(|| {
                format!("{file}: the listed {name} reaches {target}, a file that these rules do not read as Rust")
            })
        })
        .collect()
}

const KEYWORDS: [&str; 38] = [
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

const INCLUDE: [&str; 3] = ["include", "include_str", "include_bytes"];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The files under the directories that these rules read that hold no code,
/// each by its path from the root, byte for byte. The rules read every other
/// file there as Rust, whatever its name: the compiler opens a module file by
/// a path, and a volume that folds case gives `x.RS` for `x.rs`.
const DATA_FILES: [&str; 11] = [
    "crates/halvo-capture/Cargo.toml",
    "crates/halvo-capture/src/custody/mutants.rows",
    "crates/halvo-capture/src/daemon/mutants.rows",
    "crates/halvo-capture/src/macos/mutants.rows",
    "crates/halvo-capture/src/mutants.rows",
    "crates/halvo-capture/src/store/mutants.rows",
    "crates/halvo-capture/src/store/testdata/crashpoint.c",
    "crates/halvo-capture/src/store/testdata/segment-v1.golden",
    "crates/halvo-capture/src/store/testdata/segment-v2.golden",
    "crates/halvo-capture/src/windows/mutants.rows",
    "src-tauri/src/engine-url-forms.txt",
];

/// Every file under the directory `dir` of the tree at `base`, tracked or not,
/// by its path from `base`, in order, but the paths of [`DATA_FILES`]. A
/// symbolic link is read through: a link to a directory is walked, and any
/// other link is a file of the list, which a dangling link fails to read. A
/// directory that cannot be listed, and an entry that cannot be read, is a
/// fault in `faults` that names its path.
fn files_under(base: &Path, dir: &str, out: &mut Vec<String>, faults: &mut Vec<String>) {
    let entries = match fs::read_dir(base.join(dir)) {
        Ok(entries) => entries,
        Err(e) => {
            faults.push(format!("{dir} cannot be listed: {e}"));
            return;
        }
    };
    let mut names = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => names.push(entry.file_name().to_string_lossy().into_owned()),
            Err(e) => faults.push(format!("an entry of {dir} cannot be read: {e}")),
        }
    }
    names.sort();
    for name in names {
        let path = format!("{dir}/{name}");
        if base.join(&path).is_dir() {
            files_under(base, &path, out, faults);
        } else if !DATA_FILES.contains(&path.as_str()) {
            out.push(path);
        }
    }
}

/// A new directory under the target's directory for the temporary files of
/// tests, for one tree of a test.
fn temp_tree(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let base = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "source-rules-{name}-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&base).unwrap();
    base
}

/// Writes `text` at `path` under `base`, with the directories above it.
fn plant(base: &Path, path: &str, text: &str) {
    let file = base.join(path);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

/// A link at `path` under `base` to the file `target` beside it; where the
/// system makes no link, a copy of the target.
fn link(base: &Path, target: &str, path: &str) -> &'static str {
    let at = base.join(path);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, &at).unwrap();
        "link"
    }
    #[cfg(not(unix))]
    {
        let from = at.parent().unwrap().join(target);
        match std::os::windows::fs::symlink_file(&from, &at) {
            Ok(()) => "link",
            Err(_) => {
                fs::copy(&from, &at).unwrap();
                "copy"
            }
        }
    }
}

/// The files that the rule of the text filter point reads under `base`, and
/// the faults of their listing.
fn point_files(base: &Path) -> (Vec<String>, Vec<String>) {
    let (mut files, mut faults) = (Vec::new(), Vec::new());
    files_under(
        base,
        "crates/halvo-capture/src/floor",
        &mut files,
        &mut faults,
    );
    files_under(
        base,
        "crates/halvo-capture/src/daemon",
        &mut files,
        &mut faults,
    );
    (files, faults)
}

/// The uses of path attributes and include macros in the files that the rule
/// of the listed uses reads under `base`, and the faults of the reading.
fn reach_uses(base: &Path) -> (Vec<Use>, Vec<String>) {
    let (mut files, mut faults) = (Vec::new(), Vec::new());
    files_under(base, "crates/halvo-capture", &mut files, &mut faults);
    files_under(base, "src-tauri/src", &mut files, &mut faults);
    files.push("src-tauri/build.rs".to_owned());
    files.extend(
        READ_BESIDE_THE_ROOTS
            .iter()
            .filter(|file| base.join(file).is_file())
            .map(|file| (*file).to_owned()),
    );
    let mut uses = Vec::new();
    for file in &files {
        match read_tokens_at(base, file) {
            Err(fault) => faults.push(fault),
            Ok(t) => {
                let (found, wrong) = reach_reading(file, &t);
                uses.extend(found);
                faults.extend(wrong);
            }
        }
    }
    (uses, faults)
}

fn flatten(stream: TokenStream, out: &mut Vec<String>) {
    for tree in stream {
        match tree {
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::None => ("", ""),
                };
                if !open.is_empty() {
                    out.push(open.to_owned());
                }
                flatten(group.stream(), out);
                if !close.is_empty() {
                    out.push(close.to_owned());
                }
            }
            TokenTree::Ident(ident) => out.push(ident.to_string()),
            TokenTree::Punct(punct) => out.push(punct.as_char().to_string()),
            TokenTree::Literal(literal) => out.push(literal.to_string()),
        }
    }
}

fn lex(text: &str) -> Result<Vec<String>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if let Some(rest) = text.strip_prefix("#!") {
        if !rest.trim_start().starts_with('[') {
            return Err("its first line is a shebang line, which the compiler skips".to_owned());
        }
    }
    let stream = TokenStream::from_str(text).map_err(|e| format!("the lexer refuses it: {e}"))?;
    let mut out = Vec::new();
    flatten(stream, &mut out);
    Ok(out)
}

fn read_tokens(file: &str) -> Result<Vec<String>, String> {
    read_tokens_at(&root(), file)
}

fn read_tokens_at(base: &Path, file: &str) -> Result<Vec<String>, String> {
    let text = fs::read_to_string(base.join(file))
        .map_err(|e| format!("{file} cannot be read as UTF-8 text: {e}"))?;
    lex(&text).map_err(|cause| format!("{file} cannot be read as the compiler reads it: {cause}"))
}

fn bare(token: &str) -> &str {
    token.strip_prefix("r#").unwrap_or(token)
}

fn is_ident(token: &str) -> bool {
    token
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && !token.contains(['"', '\''])
}

fn is_name(token: &str) -> bool {
    is_ident(token) && (token.starts_with("r#") || !KEYWORDS.contains(&token))
}

fn is_open(token: Option<&String>) -> bool {
    token.is_some_and(|t| matches!(t.as_str(), "(" | "[" | "{"))
}

fn matching(tokens: &[String], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, t) in tokens.iter().enumerate().skip(open) {
        match t.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => match depth.checked_sub(1) {
                Some(0) | None => return i,
                Some(d) => depth = d,
            },
            _ => {}
        }
    }
    tokens.len()
}

/// For each token: whether it stands inside an attribute, and whether it stands
/// inside the definition or the invocation of a macro.
fn regions(t: &[String]) -> (Vec<bool>, Vec<bool>) {
    let mut attribute = vec![false; t.len()];
    let mut in_macro = vec![false; t.len()];
    for i in 0..t.len() {
        let next = |k: usize, s: &str| t.get(i + k).is_some_and(|x| x == s);
        let (open, flags) = if t[i] == "#" && next(1, "[") {
            (i + 1, &mut attribute)
        } else if t[i] == "#" && next(1, "!") && next(2, "[") {
            (i + 2, &mut attribute)
        } else if t[i] == "!" && i > 0 && is_name(&t[i - 1]) && is_open(t.get(i + 1)) {
            (i + 1, &mut in_macro)
        } else if bare(&t[i]) == "macro_rules" && next(1, "!") && is_open(t.get(i + 3)) {
            (i + 3, &mut in_macro)
        } else {
            continue;
        };
        let close = matching(t, open).min(t.len() - 1);
        flags[open..=close].fill(true);
    }
    (attribute, in_macro)
}

/// The index after the generic arguments that open at `open`. An angle bracket
/// inside braces is part of an expression and is not counted.
fn after_generic_args(t: &[String], open: usize) -> usize {
    let (mut angle, mut braces) = (0usize, 0usize);
    for i in open..t.len() {
        match t[i].as_str() {
            "{" => braces += 1,
            "}" => braces = braces.saturating_sub(1),
            "<" if braces == 0 => angle += 1,
            ">" if braces == 0 && t[i - 1] != "-" => {
                angle = angle.saturating_sub(1);
                if angle == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    t.len()
}

/// Whether the trait name at `at` is the trait of an impl: `for` follows it,
/// after its generic arguments and any closing parenthesis.
fn followed_by_for(t: &[String], at: usize) -> bool {
    let mut i = at + 1;
    if t.get(i).is_some_and(|x| x == "<") {
        i = after_generic_args(t, i);
    }
    while t.get(i).is_some_and(|x| x == ")") {
        i += 1;
    }
    t.get(i).is_some_and(|x| x == "for")
}

/// The implementors of the trait `name` in the tokens of one file, and every
/// form that could hide one: the name renamed with `as`, or the name inside an
/// attribute or a macro.
fn trait_reading(file: &str, t: &[String], name: &str) -> (usize, Vec<String>) {
    let (attribute, in_macro) = regions(t);
    let mut implementors = 0;
    let mut faults = Vec::new();
    for i in 0..t.len() {
        if !(is_ident(&t[i]) && bare(&t[i]) == name) {
            continue;
        }
        if attribute[i] || in_macro[i] {
            faults.push(format!(
                "{file}: {name} stands inside an attribute or a macro, which can write an implementor"
            ));
        } else if t.get(i + 1).is_some_and(|x| x == "as") {
            faults.push(format!("{file}: {name} is renamed with as"));
        } else if followed_by_for(t, i) {
            implementors += 1;
        }
    }
    (implementors, faults)
}

type Use = (String, String, String);

/// The uses of `#[path]` and of the include macros in the tokens of one file,
/// as (file, name, literal), and every form that no listed use can name.
fn reach_reading(file: &str, t: &[String]) -> (Vec<Use>, Vec<String>) {
    let (attribute, in_macro) = regions(t);
    let mut uses = Vec::new();
    let mut faults = Vec::new();
    for i in 0..t.len() {
        let at = |k: usize| t.get(i + k).map(String::as_str);
        let literal = |k: usize| at(k).filter(|x| x.starts_with('"'));
        if t[i] == "$" && attribute[i] {
            faults.push(format!(
                "{file}: an attribute holds a macro variable, which can stand for path"
            ));
            continue;
        }
        if !is_ident(&t[i]) {
            continue;
        }
        let name = bare(&t[i]);
        if name == "path" && attribute[i] {
            match (at(1), literal(2)) {
                (Some("="), Some(lit)) => {
                    uses.push((file.to_owned(), name.to_owned(), lit.to_owned()))
                }
                _ => faults.push(format!(
                    "{file}: path stands in an attribute in another form"
                )),
            }
        } else if name == "path" && in_macro[i] && at(1) == Some("=") {
            faults.push(format!(
                "{file}: path = stands inside a macro, which can write a path attribute"
            ));
        } else if INCLUDE.contains(&name) {
            match (at(1), at(2), literal(3), at(4)) {
                (Some("!"), Some("("), Some(lit), Some(")")) if !attribute[i] && !in_macro[i] => {
                    uses.push((file.to_owned(), name.to_owned(), lit.to_owned()));
                }
                _ => faults.push(format!("{file}: {name} stands in another form")),
            }
        }
    }
    (uses, faults)
}

#[test]
fn the_reader_counts_an_implementor_however_it_is_spelled() {
    let read = |text: &str| {
        let t = lex(&text.replace("NAME", POINT)).unwrap();
        trait_reading("f.rs", &t, POINT)
    };
    let none = Vec::<String>::new();
    for text in [
        "impl NAME for X {}",
        "impl<T: Clone>\n    crate::floor::text::NAME\n    for T {}",
        "impl\nNAME\nfor\nX {}",
        "impl super::text::NAME<A> for X {}",
        "impl NAME<(Vec<u8>, fn() -> u8)> for X {}",
        "impl<T: Foo<{ 1 < 2 }>> NAME for T {}",
        "impl NAME<{ 1 > 2 }> for X {}",
        "unsafe impl NAME for X {}",
        "impl (NAME) for X {}",
        "impl r#NAME for X {}",
        "#[cfg(any())]\nimpl NAME for X {}",
        "mod inner { impl super::NAME for X {} }",
        "fn f() { struct Y; impl NAME for Y {} }",
    ] {
        assert_eq!(read(text), (1, none.clone()), "{text}");
    }
    assert_eq!(
        read("impl NAME for X {} impl NAME for Y {}"),
        (2, none.clone())
    );
    for text in [
        "impl<P: NAME> Texts for Through<P> {}",
        "impl<T> Foo for T where T: NAME {}",
        "impl Foo<dyn NAME> for X {}",
        "pub trait NAME { fn f(&self); }",
        "fn encode<P: NAME>(p: &P) {}",
        "fn f(x: &dyn NAME) { <X as NAME>::g(x) }",
        "use super::text::NAME;",
        "use super::text::{self, NAME};",
        "/// impl NAME for X {}\nfn f() {}",
        "// impl NAME for X {}\nfn f() {}",
        "/* impl NAME for X {} */\nfn f() {}",
        "const S: &str = \"impl NAME for X {}\";",
        "const S: &str = r#\"impl NAME for X {}\"#;",
    ] {
        assert_eq!(read(text), (0, none.clone()), "{text}");
    }
    let renamed = vec![format!("f.rs: {POINT} is renamed with as")];
    for text in [
        "use super::text::NAME as Stored;\nimpl Stored for X {}",
        "use super::text::{TextField, NAME\n    as Point};",
        "use super::text::NAME as _;",
    ] {
        assert_eq!(read(text), (0, renamed.clone()), "{text}");
    }
    let inside = vec![format!(
        "f.rs: {POINT} stands inside an attribute or a macro, which can write an implementor"
    )];
    for text in [
        "macro_rules! point { ($t:ident) => { impl NAME for $t {} }; }\npoint!(X);",
        "macro_rules! point { ($p:path, $t:ident) => { impl $p for $t {} }; }\npoint!(NAME, X);",
        "make! { impl NAME for X {} }",
        "#[derive(NAME)]\nstruct X;",
        "#[cfg_attr(test, NAME)]\nstruct X;",
    ] {
        assert_eq!(read(text), (0, inside.clone()), "{text}");
    }
    assert!(lex("impl NAME for X { \"unclosed }").is_err());
    assert!(lex("#!/bin/sh\nimpl NAME for X {}").is_err());
    assert!(lex("#![allow(dead_code)]\nimpl NAME for X {}").is_ok());
}

#[test]
fn the_reader_finds_a_path_attribute_and_an_include_however_they_are_spelled() {
    let read = |text: &str| reach_reading("f.rs", &lex(text).unwrap());
    let used = |name: &str, lit: &str| {
        let one = ("f.rs".to_owned(), name.to_owned(), lit.to_owned());
        (vec![one], Vec::<String>::new())
    };
    assert_eq!(read("#[path = \"a.rs\"]\nmod a;"), used("path", "\"a.rs\""));
    assert_eq!(
        read("#[\n    path\n    =\n    \"a.txt\"\n]\nmod a;"),
        used("path", "\"a.txt\"")
    );
    assert_eq!(
        read("#[cfg_attr(windows, path = \"w.rs\")]\nmod a;"),
        used("path", "\"w.rs\"")
    );
    assert_eq!(read("#![path = \"a.rs\"]"), used("path", "\"a.rs\""));
    assert_eq!(
        read("const S: &str = include_str!(\"a.txt\");"),
        used("include_str", "\"a.txt\"")
    );
    assert_eq!(
        read("const B: &[u8] = std::include_bytes!(\"a.bin\");"),
        used("include_bytes", "\"a.bin\"")
    );
    assert_eq!(read("include!(\"a.rs\");"), used("include", "\"a.rs\""));
    assert_eq!(read("r#include!(\"a.rs\");"), used("include", "\"a.rs\""));
    for text in [
        "pub fn path(p: &str) -> &str { p }",
        "fn tell(path: &str) -> usize { let path = path.len(); path }",
        "fn f(path: &Path) { assert!(path.exists(), \"{}\", path.display()); }",
        "/// #[path = \"more.rs\"] include!(\"x\")\npub fn f() {}",
        "const S: &str = \"#[path = \\\"x\\\"] include!(\\\"x\\\")\";",
        "macro_rules! m { ($p:path) => { $p() }; }",
    ] {
        assert_eq!(read(text), (vec![], vec![]), "{text}");
    }
    let fault = |text: &str| read(text).1;
    let another = |name: &str| vec![format!("f.rs: {name} stands in another form")];
    assert_eq!(
        fault("include!(concat!(\"a\", \".rs\"));"),
        another("include")
    );
    assert_eq!(
        fault("macro_rules! inc { ($f:literal) => { include!($f) }; }"),
        another("include")
    );
    assert_eq!(fault("m!(include_str);"), another("include_str"));
    assert_eq!(
        fault("#[doc = include_str!(\"a.md\")]\nfn f() {}"),
        another("include_str")
    );
    assert_eq!(
        fault("#[path = concat!(\"a\", \".txt\")]\nmod a;"),
        ["f.rs: path stands in an attribute in another form"]
    );
    assert_eq!(
        fault("#[path]\nmod a;"),
        ["f.rs: path stands in an attribute in another form"]
    );
    assert_eq!(
        fault("macro_rules! m { ($p:ident) => { #[$p = \"a.txt\"] mod a; }; }\nm!(path);"),
        ["f.rs: an attribute holds a macro variable, which can stand for path"]
    );
    assert_eq!(
        fault("m!(path = \"a.txt\");"),
        ["f.rs: path = stands inside a macro, which can write a path attribute"]
    );
}

#[test]
fn the_text_filter_point_has_one_implementor_at_most_and_it_is_under_daemon() {
    let tree = temp_tree("point");
    let daemon = "crates/halvo-capture/src/daemon";
    for (name, text) in [
        ("owner.rs", "pub struct Owner;"),
        ("evil.RS", "pub struct Evil;"),
        ("noext", "pub struct NoExtension;"),
        ("mutants.rows", "a\tb"),
        ("outside.txt", "pub struct Linked;"),
        ("long.r\u{17f}", "pub struct LongS;"),
    ] {
        plant(&tree, &format!("{daemon}/{name}"), text);
    }
    plant(&tree, "crates/halvo-capture/src/floor/Mutants.rows", "a\tb");
    let made = link(&tree, "outside.txt", &format!("{daemon}/linked_tests.rs"));
    println!("the fixture of a link is a {made}");
    let (mut read, listing) = point_files(&tree);
    assert_eq!(listing, Vec::<String>::new());
    read.sort();
    let mut want: Vec<String> = [
        "crates/halvo-capture/src/floor/Mutants.rows",
        "crates/halvo-capture/src/daemon/evil.RS",
        "crates/halvo-capture/src/daemon/long.r\u{17f}",
        "crates/halvo-capture/src/daemon/linked_tests.rs",
        "crates/halvo-capture/src/daemon/noext",
        "crates/halvo-capture/src/daemon/outside.txt",
        "crates/halvo-capture/src/daemon/owner.rs",
    ]
    .map(str::to_owned)
    .to_vec();
    want.sort();
    assert_eq!(
        read, want,
        "the rule leaves out a file but the listed data file"
    );
    assert!(read_tokens_at(&tree, &format!("{daemon}/linked_tests.rs")).is_ok());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        struct Guard(PathBuf);
        impl Drop for Guard {
            fn drop(&mut self) {
                let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o755));
            }
        }
        let locked = tree.join(daemon).join("locked");
        fs::create_dir_all(&locked).unwrap();
        fs::write(locked.join("mod.rs"), "pub struct Locked;").unwrap();
        let _guard = Guard(locked.clone());
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o311)).unwrap();
        assert!(
            fs::read_dir(&locked).is_err(),
            "this account lists a directory of mode 311, as the system's first user does, so the fixture cannot be made"
        );
        let (_, listing) = point_files(&tree);
        assert_eq!(listing.len(), 1, "{listing:?}");
        assert!(
            listing[0].starts_with("crates/halvo-capture/src/daemon/locked cannot be listed"),
            "{listing:?}"
        );
    }
    let (files, listing) = point_files(&root());
    assert_eq!(listing, Vec::<String>::new());
    assert!(
        files
            .iter()
            .any(|f| f == "crates/halvo-capture/src/floor/text.rs"),
        "the reader finds no floor/text.rs: {files:?}"
    );
    let (mut tests, mut product, mut faults) = (0, Vec::new(), Vec::new());
    for file in &files {
        match read_tokens(file) {
            Err(fault) => faults.push(fault),
            Ok(t) => {
                let (n, found) = trait_reading(file, &t, POINT);
                faults.extend(found);
                if file == TEST_POINTS {
                    tests += n;
                } else if n > 0 {
                    product.push((file.clone(), n));
                }
            }
        }
    }
    let count: usize = product.iter().map(|(_, n)| n).sum();
    println!("implementors of {POINT} outside floor/cases.rs: {count}");
    assert_eq!(faults, Vec::<String>::new());
    assert!(
        tests > 0,
        "the reader finds no implementor of {POINT} in {TEST_POINTS}"
    );
    let outside: Vec<_> = product
        .iter()
        .filter(|(f, _)| !f.starts_with(DAEMON))
        .collect();
    assert!(
        outside.is_empty(),
        "{POINT} has an implementor outside src/daemon/: {outside:?}"
    );
    assert!(
        count <= 1,
        "{POINT} has {count} implementors outside floor/cases.rs: {product:?}"
    );
}

#[test]
fn the_secure_input_probe_is_implemented_only_in_the_secure_input_tree() {
    let (mut total, mut outside, mut faults) = (0, Vec::new(), Vec::new());
    for file in PROBE_TREE.into_iter().chain([PROOF_FILE, PROBE_USER]) {
        match read_tokens(file) {
            Err(fault) => faults.push(fault),
            Ok(t) => {
                let (n, found) = trait_reading(file, &t, PROBE);
                faults.extend(found);
                total += n;
                if n > 0 && !PROBE_TREE.contains(&file) && file != PROOF_FILE {
                    outside.push((file, n));
                }
            }
        }
    }
    println!("implementors of {PROBE}: {total}");
    assert_eq!(faults, Vec::<String>::new());
    assert!(total > 0, "the reader finds no implementor of {PROBE}");
    assert!(
        outside.is_empty(),
        "{PROBE} has an implementor outside the secure_input tree: {outside:?}"
    );
}

#[test]
fn path_attributes_and_include_macros_stand_only_at_the_listed_uses() {
    assert_eq!(code_reach_faults(&LISTED), Vec::<String>::new());
    let refused = [
        (
            "crates/halvo-capture/src/daemon/mod.rs",
            "path",
            "\"mutants.rows\"",
        ),
        (
            "crates/halvo-capture/src/lib.rs",
            "include",
            "\"mutants.rows\"",
        ),
        (
            "crates/halvo-capture/src/daemon/mod.rs",
            "path",
            "\"../../../../other/x.rs\"",
        ),
        (
            "crates/halvo-capture/src/daemon/mod.rs",
            "path",
            "\"/private/tmp/outside/x.rs\"",
        ),
        (
            "crates/halvo-capture/src/daemon/mod.rs",
            "path",
            "\"..\\\\..\\\\..\\\\..\\\\other\\\\x.rs\"",
        ),
        (
            "crates/halvo-capture/src/daemon/mod.rs",
            "path",
            "\"C:\\\\other\\\\x.rs\"",
        ),
        (
            "crates/halvo-capture/src/daemon/mod.rs",
            "path",
            "\"C:/other/x.rs\"",
        ),
        (
            "crates/halvo-capture/src/lib.rs",
            "path",
            "\"../../other/src/x.rs\"",
        ),
        ("src-tauri/build.rs", "path", "\"build/x.rs\""),
    ];
    for case in refused {
        assert_eq!(code_reach_faults(&[case]).len(), 1, "{case:?}");
    }
    let kept = [
        ("src-tauri/build.rs", "path", "\"src/commands/names.rs\""),
        (
            "crates/halvo-capture/src/secure_input.rs",
            "path",
            "\"./secure_input_macos.rs\"",
        ),
        (
            "crates/halvo-capture/src/a.rs",
            "include_str",
            "\"notes.txt\"",
        ),
        (
            "crates/halvo-capture/src/a.rs",
            "include_bytes",
            "\"data.golden\"",
        ),
    ];
    for case in kept {
        assert_eq!(code_reach_faults(&[case]), Vec::<String>::new(), "{case:?}");
    }
    let tree = temp_tree("reach");
    plant(&tree, "src-tauri/build.rs", "fn main() {}");
    plant(&tree, "src-tauri/src/main.rs", "fn main() {}");
    plant(&tree, "crates/halvo-capture/Cargo.toml", "[package]");
    plant(
        &tree,
        "crates/halvo-capture/src/daemon/evil.RS",
        "#[path = \"y.rs\"]\nmod y;",
    );
    let (found, wrong) = reach_uses(&tree);
    assert_eq!(wrong, Vec::<String>::new());
    assert_eq!(
        found,
        [(
            "crates/halvo-capture/src/daemon/evil.RS".to_owned(),
            "path".to_owned(),
            "\"y.rs\"".to_owned()
        )],
        "a path attribute in a file whose name ends in .RS is not read"
    );
    let (mut uses, faults) = reach_uses(&root());
    println!(
        "uses of path attributes and include macros: {}, listed: {}",
        uses.len(),
        LISTED.len()
    );
    assert_eq!(faults, Vec::<String>::new());
    let mut listed: Vec<Use> = LISTED
        .iter()
        .map(|(f, n, l)| ((*f).to_owned(), (*n).to_owned(), (*l).to_owned()))
        .collect();
    listed.sort();
    uses.sort();
    assert_eq!(
        uses, listed,
        "a path attribute or an include macro stands at a use that LISTED does not name, or a listed use is gone"
    );
}

/// Ruling DM-12 G9 (units/daemon-1.md line 173): the macOS entry hands the
/// signal flag that the binary installs to the timed run, as the fifth
/// argument of its one call of `run_timed`. A process test of a signal needs
/// the main run loop and the sources' grants, so on this Mac the rule is held
/// over the source text; the process test is daemon-3's tests/cli.rs.
#[test]
fn the_macos_timed_run_hands_the_signal_flag_to_the_run() {
    let file = "crates/halvo-capture/src/macos/mod.rs";
    let tokens = read_tokens(file).unwrap();
    let calls: Vec<usize> = (0..tokens.len().saturating_sub(1))
        .filter(|&i| tokens[i] == "run_timed" && tokens[i + 1] == "(")
        .collect();
    assert_eq!(
        calls.len(),
        1,
        "{file} holds {} calls of run_timed",
        calls.len()
    );
    let open = calls[0] + 1;
    let close = matching(&tokens, open);
    let mut args: Vec<Vec<&str>> = vec![Vec::new()];
    let mut depth = 0usize;
    for token in &tokens[open + 1..close] {
        match token.as_str() {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            "," if depth == 0 => {
                args.push(Vec::new());
                continue;
            }
            _ => {}
        }
        args.last_mut().unwrap().push(token);
    }
    let flag = [
        "crate", ":", ":", "daemon", ":", ":", "signal", ":", ":", "flag", "(", ")",
    ];
    assert_eq!(
        args.get(4).map(Vec::as_slice),
        Some(&flag[..]),
        "the fifth argument of run_timed in {file} is not the binary's signal flag"
    );
}

/// The body of each match arm in `tokens` that opens with `head` followed by
/// `=>`: its tokens up to the comma or the closing delimiter at its own depth,
/// without a comma that stands just before a closing delimiter, which the
/// formatter adds to a call that it breaks over lines.
fn arm_bodies(tokens: &[String], head: &[&str]) -> Vec<Vec<String>> {
    let mut bodies = Vec::new();
    for at in 0..tokens.len() {
        let end = at + head.len();
        if end + 2 > tokens.len()
            || tokens[at..end].iter().zip(head).any(|(t, h)| t != h)
            || tokens[end] != "="
            || tokens[end + 1] != ">"
        {
            continue;
        }
        let mut body = Vec::new();
        let mut depth = 0usize;
        for token in &tokens[end + 2..] {
            match token.as_str() {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" if depth == 0 => break,
                ")" | "]" | "}" => {
                    depth -= 1;
                    if body.last().is_some_and(|last: &String| last == ",") {
                        body.pop();
                    }
                }
                "," if depth == 0 => break,
                _ => {}
            }
            body.push(token.clone());
        }
        bodies.push(body);
    }
    bodies
}

/// Ruling DM-15 (1), the binary's own arm (G3): the usage arm of the binary
/// returns the status that `usage_answer` gives, and its run arm the status
/// that `outcome_answer` gives, each arm once. Both answers are tested in the
/// lib; a process test of the binary's exit code is daemon-3's tests/cli.rs,
/// so the two calls are held over the source text.
#[test]
fn the_binary_returns_what_usage_answer_and_outcome_answer_give() {
    let file = "crates/halvo-capture/src/bin/halvo-capture.rs";
    let tokens = read_tokens(file).unwrap();
    let arms = [
        (
            &["Err", "(", "usage", ")"][..],
            "status(daemon::usage_answer(&usage, &mut std::io::stderr()))",
        ),
        (
            &["Ok", "(", "outcome", ")"][..],
            "status(daemon::outcome_answer(&outcome, &mut std::io::stdout().lock(), &mut std::io::stderr()))",
        ),
    ];
    for (head, body) in arms {
        let want = lex(body).unwrap();
        assert_eq!(
            arm_bodies(&tokens, head),
            [want],
            "the arm {} of {file} does not return {body} alone, once",
            head.concat()
        );
    }
}

const DATA_DIR_MAKER: &str = "for_current_user";
const ACCOUNT_MAKER: &str = "for_account";
const SYSTEM_ACCOUNT: &str = "SystemAccount";

const MAKER_USES: [(&str, usize, &str); 4] = [
    (
        "crates/halvo-agent-gate/src/task.rs",
        1,
        "the product's link to the owner without --data-dir",
    ),
    (
        "crates/halvo-control/src/ctl_text.rs",
        3,
        "the product's data_dir without --data-dir, and two tests that claim it as the default",
    ),
    (
        "crates/halvo-control/src/paths.rs",
        7,
        "the two makers, their two calls of for_account, and three tests that claim the default path itself",
    ),
    (
        "src-tauri/src/main.rs",
        2,
        "the app's data directory and login item without --data-dir",
    ),
];

fn maker_uses(t: &[String]) -> usize {
    (0..t.len())
        .filter(|&i| is_ident(&t[i]))
        .filter(|&i| match bare(&t[i]) {
            DATA_DIR_MAKER => true,
            ACCOUNT_MAKER if t.get(i + 1).is_some_and(|x| x == "(") => t[i + 1..matching(t, i + 1)]
                .iter()
                .any(|x| is_ident(x) && bare(x) == SYSTEM_ACCOUNT),
            _ => false,
        })
        .count()
}

fn rust_files(base: &Path, dir: &str, out: &mut Vec<String>, faults: &mut Vec<String>) {
    let entries = match fs::read_dir(base.join(dir)) {
        Ok(entries) => entries,
        Err(e) => {
            faults.push(format!("the directory '{dir}' cannot be listed: {e}"));
            return;
        }
    };
    let mut found = Vec::new();
    for entry in entries {
        let read = entry.and_then(|entry| {
            let kind = entry.file_type()?;
            Ok((entry.file_name().to_string_lossy().into_owned(), kind))
        });
        match read {
            Ok(one) => found.push(one),
            Err(e) => faults.push(format!("an entry of '{dir}' cannot be read: {e}")),
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, kind) in found {
        let path = if dir.is_empty() {
            name.clone()
        } else {
            format!("{dir}/{name}")
        };
        if kind.is_dir() {
            let skipped = name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || base.join(&path).join("CACHEDIR.TAG").exists();
            if !skipped {
                rust_files(base, &path, out, faults);
            }
        } else if name.to_ascii_lowercase().ends_with(".rs") {
            out.push(path);
        }
    }
}

fn maker_reading(base: &Path) -> (Vec<(String, usize)>, Vec<String>) {
    let (mut files, mut faults) = (Vec::new(), Vec::new());
    rust_files(base, "", &mut files, &mut faults);
    let mut counts = Vec::new();
    for file in files {
        let text = match fs::read_to_string(base.join(&file)) {
            Ok(text) => text,
            Err(e) => {
                faults.push(format!("{file} cannot be read as UTF-8 text: {e}"));
                continue;
            }
        };
        if !text.contains(DATA_DIR_MAKER) && !text.contains(SYSTEM_ACCOUNT) {
            continue;
        }
        match lex(&text) {
            Err(cause) => faults.push(format!(
                "{file} cannot be read as the compiler reads it: {cause}"
            )),
            Ok(t) => match maker_uses(&t) {
                0 => {}
                n => counts.push((file, n)),
            },
        }
    }
    (counts, faults)
}

fn lines_naming_a_maker(text: &str) -> String {
    let lines: Vec<String> = text
        .lines()
        .enumerate()
        .filter(|(_, line)| line.contains(DATA_DIR_MAKER) || line.contains(SYSTEM_ACCOUNT))
        .map(|(at, _)| (at + 1).to_string())
        .collect();
    if lines.is_empty() {
        "none".to_owned()
    } else {
        lines.join(", ")
    }
}

fn maker_refusals(
    base: &Path,
    counts: &[(String, usize)],
    listed: &[(&str, usize, &str)],
) -> Vec<String> {
    let mut files: Vec<&str> = counts
        .iter()
        .map(|(file, _)| file.as_str())
        .chain(listed.iter().map(|(file, _, _)| *file))
        .collect();
    files.sort();
    files.dedup();
    files
        .into_iter()
        .filter_map(|file| {
            let found = counts
                .iter()
                .find(|(f, _)| f == file)
                .map_or(0, |(_, n)| *n);
            let (allowed, why) = listed
                .iter()
                .find(|(f, _, _)| *f == file)
                .map_or((0, "the file is not on the list"), |(_, n, why)| (*n, *why));
            (found != allowed).then(|| {
                let lines = fs::read_to_string(base.join(file)).map_or_else(
                    |e| format!("none, as the file cannot be read: {e}"),
                    |text| lines_naming_a_maker(&text),
                );
                format!(
                    "{file}: uses of a maker of the account's own data directory ({DATA_DIR_MAKER}, or {ACCOUNT_MAKER} with {SYSTEM_ACCOUNT}): {found}, where MAKER_USES allows {allowed} ({why}); its lines that hold either name: {lines}; a test takes DataDir::from_flag(<a directory that the test makes>) in their place, and a new product caller goes on MAKER_USES with its count and why"
                )
            })
        })
        .collect()
}

#[test]
fn makers_of_the_accounts_own_data_directory_stand_only_at_the_listed_uses() {
    for (text, want) in [
        (
            "let state = AppState::new(DataDir::for_current_user, no_pane);",
            1,
        ),
        ("let data = DataDir::for_current_user().unwrap();", 1),
        (
            "let agents = halvo_control::paths::LoginItemDir::for_current_user().ok()?;",
            1,
        ),
        (
            "use halvo_control::paths::DataDir as Dir;\nlet data = Dir::r#for_current_user();",
            1,
        ),
        (
            "assert_eq!(plain.data_dir(), DataDir::for_current_user());",
            1,
        ),
        (
            "#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { DataDir::for_current_user().unwrap(); }\n}",
            1,
        ),
        ("DataDir::for_account(&SystemAccount)", 1),
        (
            "DataDir::for_account(\n    &halvo_control::paths::SystemAccount,\n)",
            1,
        ),
        (
            "pub fn for_current_user() -> Result<Self, PathError> {\n    Self::for_account(&SystemAccount)\n}",
            2,
        ),
        (
            "let state = AppState::new(move || DataDir::from_flag(data), no_pane);",
            0,
        ),
        ("let data = DataDir::for_account(&account(BASE)).unwrap();", 0),
        ("let socket = socket_path(&data, &SystemAccount).unwrap();", 0),
        (
            "pub fn for_account(account: &dyn AccountLookup) -> Result<Self, PathError> {}",
            0,
        ),
        ("/// DataDir::for_current_user()\nfn f() {}", 0),
        ("// DataDir::for_current_user()\nfn f() {}", 0),
        ("/* DataDir::for_account(&SystemAccount) */\nfn f() {}", 0),
        ("const S: &str = \"DataDir::for_current_user()\";", 0),
        ("fn for_current_users() {}", 0),
    ] {
        assert_eq!(maker_uses(&lex(text).unwrap()), want, "{text}");
    }

    let tree = temp_tree("makers");
    let held = "crates/halvo-app/src/tests/layer.rs";
    let named = "crates/halvo-control/src/paths.rs";
    let upper = "tools/trial/src/Upper.RS";
    let call = "fn f() { DataDir::for_current_user(); }";
    plant(
        &tree,
        held,
        "fn builds() {\n    let no_pane = || None;\n    let state = AppState::new(DataDir::for_current_user, no_pane);\n}",
    );
    plant(
        &tree,
        "src-tauri/src/tests/layer.rs",
        "let state = AppState::new(move || DataDir::from_flag(data), no_pane);",
    );
    plant(
        &tree,
        named,
        "fn a() { DataDir::for_current_user(); }\nfn b() { DataDir::for_account(&SystemAccount); }",
    );
    plant(&tree, upper, call);
    plant(
        &tree,
        "build-out/CACHEDIR.TAG",
        "Signature: 8a477f597d28d172789f06886806bc55",
    );
    for skipped in [
        "build-out/debug/build/out/made.rs",
        "target/tmp/source-rules-makers/crates/a/tests/held.rs",
        ".claude/worktrees/w/crates/a/src/lib.rs",
        "ui/node_modules/pkg/src/lib.rs",
        "docs/notes.md",
    ] {
        plant(&tree, skipped, call);
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        "../../crates/halvo-app/src",
        tree.join("tools/trial/linked"),
    )
    .unwrap();
    let (counts, faults) = maker_reading(&tree);
    assert_eq!(faults, Vec::<String>::new());
    assert_eq!(
        counts,
        [
            (held.to_owned(), 1),
            (named.to_owned(), 2),
            (upper.to_owned(), 1)
        ]
    );
    let listed = [
        (named, 2, "a named place of the planted tree"),
        (upper, 1, "a product caller of the planted tree"),
    ];
    assert_eq!(
        maker_refusals(&tree, &counts, &listed),
        [format!(
            "{held}: uses of a maker of the account's own data directory (for_current_user, or for_account with SystemAccount): 1, where MAKER_USES allows 0 (the file is not on the list); its lines that hold either name: 3; a test takes DataDir::from_flag(<a directory that the test makes>) in their place, and a new product caller goes on MAKER_USES with its count and why"
        )]
    );
    let refused: Vec<String> = maker_refusals(
        &tree,
        &counts,
        &[
            (named, 3, "a count above the tree's"),
            ("crates/gone.rs", 1, "a file that is gone"),
        ],
    )
    .into_iter()
    .map(|refusal| refusal.split(':').next().unwrap().to_owned())
    .collect();
    assert_eq!(refused, ["crates/gone.rs", held, named, upper]);

    let (counts, faults) = maker_reading(&root());
    let total: usize = counts.iter().map(|(_, n)| n).sum();
    println!(
        "uses of the makers of the account's own data directory: {total} in {} files, listed files: {}",
        counts.len(),
        MAKER_USES.len()
    );
    assert_eq!(faults, Vec::<String>::new());
    assert_eq!(
        maker_refusals(&root(), &counts, &MAKER_USES),
        Vec::<String>::new()
    );
}
