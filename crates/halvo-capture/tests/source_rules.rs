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
const LISTED: [(&str, &str, &str); 8] = [
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
        "crates/halvo-capture/src/sink.rs",
        "include_str",
        "\"sink.rs\"",
    ),
    (
        "crates/halvo-capture/src/store/segment.rs",
        "include_bytes",
        "\"testdata/segment-v1.golden\"",
    ),
    ("src-tauri/build.rs", "path", "\"src/commands/names.rs\""),
    (
        "src-tauri/src/tests.rs",
        "include_str",
        "\"engine-url-forms.txt\"",
    ),
];

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

/// Every file under the directory `dir` of the repository, tracked or not, by
/// its path from the root, in order.
fn files_under(dir: &str, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(root().join(dir)) else {
        return;
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for name in names {
        let path = format!("{dir}/{name}");
        if root().join(&path).is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
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
    let text = fs::read_to_string(root().join(file))
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
    let mut files = Vec::new();
    files_under("crates/halvo-capture/src/floor", &mut files);
    files_under("crates/halvo-capture/src/daemon", &mut files);
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
    let mut files = Vec::new();
    files_under("crates/halvo-capture", &mut files);
    files_under("src-tauri/src", &mut files);
    files.push("src-tauri/build.rs".to_owned());
    let (mut uses, mut faults) = (Vec::new(), Vec::new());
    for file in files.iter().filter(|f| f.ends_with(".rs")) {
        match read_tokens(file) {
            Err(fault) => faults.push(fault),
            Ok(t) => {
                let (found, wrong) = reach_reading(file, &t);
                uses.extend(found);
                faults.extend(wrong);
            }
        }
    }
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
