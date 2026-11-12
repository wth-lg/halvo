use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

const LOCKED: [&str; 7] = [
    "lib.rs",
    "ids.rs",
    "category.rs",
    "site.rs",
    "record.rs",
    "request.rs",
    "kinds.rs",
];

const LATER: [&str; 4] = ["executable", "coordinator", "learning", "routing"];

fn read(path: &Path) -> String {
    match std::fs::read_to_string(path) {
        Ok(text) => text.replace("\r\n", "\n"),
        Err(e) => panic!("{}: {e}", path.display()),
    }
}

fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn core_dir() -> PathBuf {
    schema_dir().join("..").join("halvo-schema-core")
}

fn source(file: &str) -> String {
    read(&core_dir().join("src").join(file))
}

const RUSTC_WHITESPACE: [char; 11] = [
    ' ', '\t', '\n', '\r', '\u{b}', '\u{c}', '\u{85}', '\u{200e}', '\u{200f}', '\u{2028}',
    '\u{2029}',
];

fn after_a_block_comment(s: &str) -> &str {
    let mut depth = 0usize;
    let mut i = 0;
    let b = s.as_bytes();
    while i + 1 < b.len() {
        if b[i] == b'/' && b[i + 1] == b'*' {
            depth += 1;
            i += 2;
        } else if b[i] == b'*' && b[i + 1] == b'/' {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return &s[i..];
            }
        } else {
            i += 1;
        }
    }
    ""
}

fn starts_with_a_shebang_line(text: &str) -> bool {
    let Some(mut rest) = text.strip_prefix("#!") else {
        return false;
    };
    loop {
        rest = rest.trim_start_matches(RUSTC_WHITESPACE);
        let line_doc =
            (rest.starts_with("///") && !rest.starts_with("////")) || rest.starts_with("//!");
        let block_doc =
            (rest.starts_with("/**") && !rest.starts_with("/**/") && !rest.starts_with("/***"))
                || rest.starts_with("/*!");
        if rest.starts_with("//") && !line_doc {
            rest = rest.find('\n').map_or("", |n| &rest[n..]);
        } else if rest.starts_with("/*") && !block_doc {
            rest = after_a_block_comment(rest);
        } else {
            break;
        }
    }
    !rest.starts_with('[')
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
    if starts_with_a_shebang_line(text) {
        return Err(
            "its first line is a shebang line, which the compiler skips and the lexer reads"
                .to_owned(),
        );
    }
    let stream = TokenStream::from_str(text).map_err(|e| format!("the lexer refuses it: {e}"))?;
    let mut out = Vec::new();
    flatten(stream, &mut out);
    Ok(out)
}

fn unread(file: &str, cause: &str) -> String {
    format!("{file} cannot be read as the compiler reads it: {cause}")
}

fn tokens(text: &str) -> Vec<String> {
    lex(text).unwrap_or_else(|cause| panic!("{}", unread("the text", &cause)))
}

fn bare(name: &str) -> &str {
    name.strip_prefix("r#").unwrap_or(name)
}

fn is_ident(token: &str) -> bool {
    token
        .chars()
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_')
        && !token.contains(['"', '\''])
}

fn identifiers(text: &str, name: &str) -> usize {
    tokens(text)
        .iter()
        .filter(|t| is_ident(t) && bare(t) == name)
        .count()
}

fn find(tokens: &[String], wanted: &[&str]) -> Vec<usize> {
    (0..tokens.len())
        .filter(|&i| {
            tokens.len() >= i + wanted.len()
                && wanted.iter().zip(&tokens[i..]).all(|(w, t)| *w == t)
        })
        .collect()
}

fn closing(tokens: &[String], open: usize) -> usize {
    let mut depth = 0usize;
    for (i, t) in tokens.iter().enumerate().skip(open) {
        match t.as_str() {
            "{" => depth += 1,
            "}" => {
                depth -= 1;
                if depth == 0 {
                    return i;
                }
            }
            _ => {}
        }
    }
    tokens.len()
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

fn is_open(token: Option<&String>) -> bool {
    token.is_some_and(|t| matches!(t.as_str(), "(" | "[" | "{"))
}

fn outside_macros(tokens: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if bare(&tokens[i]) == "macro_rules" && tokens.get(i + 1).is_some_and(|x| x == "!") {
            let open = i + 3;
            i = if is_open(tokens.get(open)) {
                matching(tokens, open) + 1
            } else {
                open
            };
            if tokens.get(i).is_some_and(|x| x == ";") {
                i += 1;
            }
            continue;
        }
        out.push(tokens[i].clone());
        i += 1;
    }
    out
}

fn split_at_commas(tokens: &[String]) -> Vec<&[String]> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, t) in tokens.iter().enumerate() {
        match t.as_str() {
            "(" | "[" | "{" | "<" => depth += 1,
            ")" | "]" | "}" | ">" => depth -= 1,
            "," if depth == 0 => {
                parts.push(&tokens[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&tokens[start..]);
    parts.into_iter().filter(|p| !p.is_empty()).collect()
}

const COARSE_RECORDS: [&str; 7] = [
    "Summary",
    "Support",
    "FullWorkflow",
    "DismissedWorkflow",
    "Recurrence",
    "KeyCounts",
    "PointerCounts",
];

const COARSE_IMPORT: [&str; 13] = [
    "use",
    "crate",
    ":",
    ":",
    "kinds",
    ":",
    ":",
    "{",
    "assert_coarse",
    ",",
    "Coarse",
    "}",
    ";",
];

const COARSE_FN: [&str; 23] = [
    "fn",
    "coarse",
    "<",
    "T",
    ":",
    "Coarse",
    ">",
    "(",
    "_",
    ":",
    "&",
    "T",
    ")",
    "{",
    "assert_coarse",
    ":",
    ":",
    "<",
    "T",
    ">",
    "(",
    ")",
    "}",
];

fn declared_fields(t: &[String], record: &str) -> Result<Vec<String>, String> {
    let at = find(t, &["struct", record]);
    if at.len() != 1 {
        return Err(format!(
            "src/record.rs declares struct {record} {} times",
            at.len()
        ));
    }
    let open = (at[0]..t.len()).find(|&i| t[i] == "{").unwrap_or(t.len());
    let body = &t[(open + 1).min(t.len())..closing(t, open)];
    Ok(split_at_commas(body)
        .into_iter()
        .filter_map(|field| {
            let colon = field.iter().position(|x| x == ":")?;
            Some(field[colon.checked_sub(1)?].clone())
        })
        .collect())
}

fn asserted_fields(t: &[String], record: &str) -> Vec<String> {
    let at = find(t, &["let", record, "{"]);
    if at.len() != 1 {
        return Vec::new();
    }
    let open = at[0] + 2;
    let close = closing(t, open);
    let pattern: Vec<String> = split_at_commas(&t[open + 1..close])
        .into_iter()
        .filter(|p| p.len() == 1)
        .map(|p| p[0].clone())
        .collect();
    let mut i = close + 1;
    if !(t.get(i).is_some_and(|x| x == "=") && t.get(i + 2).is_some_and(|x| x == ";")) {
        return Vec::new();
    }
    i += 3;
    let mut called = Vec::new();
    while t.get(i).is_some_and(|x| x == "coarse")
        && t.get(i + 1).is_some_and(|x| x == "(")
        && t.get(i + 3).is_some_and(|x| x == ")")
        && t.get(i + 4).is_some_and(|x| x == ";")
    {
        called.push(t[i + 2].clone());
        i += 5;
    }
    pattern.into_iter().filter(|f| called.contains(f)).collect()
}

fn coarse_assertion_faults(text: &str) -> Vec<String> {
    let t = match lex(text) {
        Ok(t) => outside_macros(&t),
        Err(cause) => return vec![unread("src/record.rs", &cause)],
    };
    let mut faults = Vec::new();
    let count = |name: &str| t.iter().filter(|x| is_ident(x) && bare(x) == name).count();
    if find(&t, &COARSE_IMPORT).len() != 1 || count("Coarse") != 2 || count("assert_coarse") != 2 {
        faults.push(
            "src/record.rs: Coarse and assert_coarse must come from `use crate::kinds::{assert_coarse, Coarse};` once and stand only there and in fn coarse".to_owned(),
        );
    }
    if find(&t, &COARSE_FN).len() != 1 || find(&t, &["fn", "coarse"]).len() != 1 {
        faults.push(format!(
            "src/record.rs: `fn coarse<T: Coarse>(_: &T) {{ assert_coarse::<T>() }}` does not stand once, so no field of {} keeps its assertion",
            COARSE_RECORDS.join(", ")
        ));
    }
    if count("cfg") + count("cfg_attr") > 0 {
        faults.push("src/record.rs: a cfg attribute can switch the assertion off".to_owned());
    }
    let mut fields = 0;
    for record in COARSE_RECORDS {
        let declared = match declared_fields(&t, record) {
            Ok(declared) => declared,
            Err(fault) => {
                faults.push(fault);
                continue;
            }
        };
        fields += declared.len();
        let asserted = asserted_fields(&t, record);
        for field in &declared {
            if !asserted.contains(field) {
                faults.push(format!(
                    "src/record.rs: the field {field} of {record} lost its assertion: no `coarse({field});` after the one pattern `let {record} {{ .. }} = ..;`"
                ));
            }
        }
    }
    let named = count("coarse");
    if named != fields + 1 {
        faults.push(format!(
            "src/record.rs names coarse {named} times; the assertion needs {}: the fn and one call per field",
            fields + 1
        ));
    }
    faults
}

#[test]
fn the_assertion_of_record_rs_requires_coarse_of_every_field_of_the_summary_support_and_workflow_records(
) {
    assert_eq!(
        coarse_assertion_faults(&source("record.rs")),
        Vec::<String>::new()
    );
}

#[test]
fn the_check_of_the_coarse_assertion_names_the_field_that_lost_its_assertion() {
    let text = source("record.rs");
    let changed = |old: &str, new: &str| {
        assert_eq!(text.matches(old).count(), 1, "{old}");
        coarse_assertion_faults(&text.replacen(old, new, 1))
    };
    let unbound = changed(
        "fn coarse<T: Coarse>(_: &T) {\n    assert_coarse::<T>()\n}",
        "fn coarse<T>(_: &T) {}",
    );
    assert!(
        unbound.iter().any(|f| f.contains("does not stand once")),
        "{unbound:?}"
    );
    let day = changed(
        "        coarse(workflow);\n        coarse(summary);\n        coarse(day);\n",
        "        coarse(workflow);\n        coarse(summary);\n",
    );
    assert!(
        day.iter()
            .any(|f| f.contains("the field day of Support lost its assertion")),
        "{day:?}"
    );
    let support = changed(
        "        let Support {\n            workflow,\n            summary,\n            day,\n        } = support;\n        coarse(workflow);\n        coarse(summary);\n        coarse(day);\n",
        "        let _ = support;\n",
    );
    for field in ["workflow", "summary", "day"] {
        assert!(
            support
                .iter()
                .any(|f| f.contains(&format!("the field {field} of Support lost its assertion"))),
            "{support:?}"
        );
    }
    let shadowed = changed(
        "        let Summary {\n",
        "        let coarse = |_: &u32| {};\n        let Summary {\n",
    );
    assert!(
        shadowed.iter().any(|f| f.contains("names coarse")),
        "{shadowed:?}"
    );
    let aliased = changed(
        "use crate::kinds::{assert_coarse, Coarse};\n",
        "use self::nothing as assert_coarse;\nuse std::marker::Sized as Coarse;\nconst fn nothing<T: ?Sized>() {}\n",
    );
    assert!(
        aliased.iter().any(|f| f.contains("must come from")),
        "{aliased:?}"
    );
    let open = "const _: fn(&KeyCounts, &PointerCounts) = |keys, pointer| {\n";
    let end = "    coarse(r#move);\n};\n";
    assert_eq!(text.matches(open).count(), 1);
    assert_eq!(text.matches(end).count(), 1);
    let in_a_macro = coarse_assertion_faults(
        &text
            .replacen(
                open,
                &format!("macro_rules! kept {{\n    () => {{\n{open}"),
                1,
            )
            .replacen(end, &format!("{end}    }};\n}}\n"), 1),
    );
    assert!(
        in_a_macro
            .iter()
            .any(|f| f.contains("the field printable of KeyCounts lost its assertion")),
        "{in_a_macro:?}"
    );
}

const KEYWORDS: [&str; 38] = [
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

const INCLUDE: [&str; 3] = ["include", "include_str", "include_bytes"];

fn is_name(token: &str) -> bool {
    is_ident(token) && (token.starts_with("r#") || !KEYWORDS.contains(&token))
}

fn in_an_attribute_or_a_macro(t: &[String]) -> Vec<bool> {
    let mut inside = vec![false; t.len()];
    for i in 0..t.len() {
        let open = if t[i] == "#" && t.get(i + 1).is_some_and(|x| x == "[") {
            Some(i + 1)
        } else if t[i] == "#"
            && t.get(i + 1).is_some_and(|x| x == "!")
            && t.get(i + 2).is_some_and(|x| x == "[")
        {
            Some(i + 2)
        } else if t[i] == "!" && i > 0 && is_name(&t[i - 1]) && is_open(t.get(i + 1)) {
            Some(i + 1)
        } else if bare(&t[i]) == "macro_rules"
            && t.get(i + 1).is_some_and(|x| x == "!")
            && is_open(t.get(i + 3))
        {
            Some(i + 3)
        } else {
            None
        };
        if let Some(open) = open {
            let close = matching(t, open).min(t.len() - 1);
            for flag in &mut inside[open..=close] {
                *flag = true;
            }
        }
    }
    inside
}

fn in_a_matcher(t: &[String]) -> Vec<bool> {
    let mut inside = vec![false; t.len()];
    for i in 0..t.len() {
        if !(bare(&t[i]) == "macro_rules"
            && t.get(i + 1).is_some_and(|x| x == "!")
            && is_open(t.get(i + 3)))
        {
            continue;
        }
        let close = matching(t, i + 3);
        let mut j = i + 4;
        while j < close {
            if !is_open(t.get(j)) {
                j += 1;
                continue;
            }
            let end = matching(t, j);
            if t.get(end + 1).is_some_and(|x| x == "=") && t.get(end + 2).is_some_and(|x| x == ">")
            {
                for flag in &mut inside[j..=end.min(t.len() - 1)] {
                    *flag = true;
                }
            }
            j = end + 1;
        }
    }
    inside
}

fn reach_faults(file: &str, t: &[String]) -> Vec<String> {
    let inside = in_an_attribute_or_a_macro(t);
    let matcher = in_a_matcher(t);
    let mut faults = Vec::new();
    for (i, token) in t.iter().enumerate() {
        if !is_ident(token) {
            continue;
        }
        let name = bare(token);
        let fragment =
            matcher[i] && i >= 3 && t[i - 1] == ":" && is_ident(&t[i - 2]) && t[i - 3] == "$";
        if name == "path" && inside[i] && !fragment {
            faults.push(format!("{file} holds path in an attribute or a macro"));
        }
        if INCLUDE.contains(&name) {
            faults.push(format!("{file} names {name}"));
        }
    }
    faults
}

fn later_faults(file: &str, text: &str) -> Vec<String> {
    match lex(text) {
        Ok(t) => reach_faults(file, &t),
        Err(cause) => vec![unread(file, &cause)],
    }
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => panic!("{}: {e}", dir.display()),
    };
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("rs"))
        {
            out.push(path);
        }
    }
}

fn files_outside_the_lock(src: &Path) -> Vec<(String, String)> {
    let mut paths = Vec::new();
    rust_files(src, &mut paths);
    let mut files: Vec<(String, String)> = paths
        .into_iter()
        .filter_map(|path| {
            let name = path
                .strip_prefix(src)
                .expect("a path under src")
                .to_string_lossy()
                .replace('\\', "/");
            if name == "lib.rs" {
                return None;
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => Some((name, text.replace("\r\n", "\n"))),
                Err(e) => panic!("{name}: {e}"),
            }
        })
        .collect();
    files.sort();
    files
}

fn later_files() -> Vec<(String, String)> {
    files_outside_the_lock(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
}

#[test]
fn the_test_of_the_later_files_refuses_a_path_attribute_and_the_include_macros() {
    for (text, fault) in [
        (
            "#[path = \"../more.rs\"]\nmod more;\n",
            "learning.rs holds path in an attribute or a macro",
        ),
        (
            "#[cfg_attr(all(), path = \"more.rs\")]\nmod more;\n",
            "learning.rs holds path in an attribute or a macro",
        ),
        (
            "macro_rules! own_mod {\n    ($m:meta) => {\n        #[$m]\n        mod more;\n    };\n}\nown_mod! {path = \"more.rs\"}\n",
            "learning.rs holds path in an attribute or a macro",
        ),
        (
            "pub mod more {\n    #![cfg_attr(all(), path = \"elsewhere\")]\n    pub mod deeper;\n}\n",
            "learning.rs holds path in an attribute or a macro",
        ),
        (
            "include!(\"learning/macros.inc\");\n",
            "learning.rs names include",
        ),
        (
            "r#include!(\"learning/macros.inc\");\n",
            "learning.rs names include",
        ),
        (
            "use core::include as grab;\ngrab!(\"learning/macros.inc\");\n",
            "learning.rs names include",
        ),
        (
            "pub const T: &str = include_str!(\"request.rs\");\n",
            "learning.rs names include_str",
        ),
        (
            "pub const B: &[u8] = std::include_bytes!(\"request.rs\");\n",
            "learning.rs names include_bytes",
        ),
    ] {
        assert_eq!(later_faults("learning.rs", text), [fault], "{text}");
    }
    for twin in [
        "pub fn path(p: &str) -> &str {\n    p\n}\n",
        "pub fn tell(path: &str) -> String {\n    let path = path.len();\n    path.to_string()\n}\n",
        "/// #[path = \"more.rs\"] include!(\"x\")\npub fn f() {}\n",
        "macro_rules! typed {\n    ($p:path) => {};\n}\n",
    ] {
        assert_eq!(
            later_faults("learning.rs", twin),
            Vec::<String>::new(),
            "{twin}"
        );
    }
}

#[test]
fn the_reader_reads_strings_chars_and_comments_as_the_compiler_does() {
    let words = "#[macro_export]\nmacro_rules! m {\n    () => {};\n}\n#[path = \"more.rs\"]\nmod more;\npub const T: &str = include_str!(\"request.rs\");\n";
    let all = [
        "learning.rs holds path in an attribute or a macro",
        "learning.rs names include_str",
    ];
    for before in [
        "pub const C: &core::ffi::CStr = cr#\"\\\"#;\n",
        "pub const TWO: [char; 2] = ['\u{e9}', '\"'];\n",
        "pub const TWO: [char; 2] = ['\u{e9}','\"'];\n",
        "pub fn f<'a>(x: &'a str) -> char {\n    let _ = x;\n    'a'\n}\n",
        "/// a \"quote\npub fn g() {}\n",
        "/* /* nested */ \" */\n",
        "pub const Q: char = '\\'';\npub const B: u8 = b'\"';\n",
        "pub const R: &[u8] = br##\"x\"#\"##;\n",
        "pub const E: char = '\\u{1F600}';\n",
    ] {
        let mut faults = later_faults("learning.rs", &format!("{before}{words}"));
        faults.sort();
        let mut want = all.to_vec();
        want.sort();
        assert_eq!(faults, want, "{before}");
    }
    for quiet in [
        "/// #[macro_export] #[path = \"x\"] include!(\"x\") \"\npub fn f() {}\n",
        "//! macro_export path include\n",
        "/** macro_export \" path include */\npub fn f() {}\n",
        "/* /* macro_export */ \" include path */\npub fn f() {}\n",
        "pub const S: &str = \"#[macro_export] #[path = \\\"x\\\"] include!(\\\"x\\\")\";\n",
        "pub const R: &str = r#\"macro_export \" path include\"#;\n",
        "pub const B: &[u8] = br##\"include!(\"x\")\"#\"##;\n",
        "pub const C: &core::ffi::CStr = c\"macro_export path include\";\n",
        "pub const D: &core::ffi::CStr = cr#\"include!(\"x\") \\\"#;\n",
        "pub const Q: char = '\"';\npub const P: u8 = b'\"';\n",
    ] {
        assert_eq!(
            later_faults("learning.rs", quiet),
            Vec::<String>::new(),
            "{quiet}"
        );
    }
    assert_eq!(
        later_faults("learning.rs", "pub fn r#include() {}\n"),
        ["learning.rs names include"]
    );
}

#[test]
fn the_reader_refuses_a_file_that_it_cannot_read_as_the_compiler_does() {
    let shebang = "#!/bin/sh \"\npub const KINDS: &[crate::kinds::KindEntry] = &[];\n#[macro_export]\nmacro_rules! hidden {\n    () => {\n        3\n    };\n}\npub const H: u8 = crate::hidden!();\n// \"\n";
    let backtick = "#!/usr/bin/env `run`\npub const KINDS: &[crate::kinds::KindEntry] = &[];\n";
    assert!(TokenStream::from_str(backtick).is_err());
    assert!(TokenStream::from_str(shebang).is_ok());
    let first_line =
        "its first line is a shebang line, which the compiler skips and the lexer reads";
    let nbsp = "#!\u{a0}[allow(dead_code)] /*\npub const KINDS: &[crate::kinds::KindEntry] = &[];\n#[macro_export]\nmacro_rules! hidden {\n    () => {};\n}\n// */\n";
    let word = "#! x /*\npub const KINDS: &[crate::kinds::KindEntry] = &[];\n#[macro_export]\nmacro_rules! hidden {\n    () => {};\n}\n// */\n";
    let paren = "#!(x) /*\npub const KINDS: &[crate::kinds::KindEntry] = &[];\n#[macro_export]\nmacro_rules! hidden {\n    () => {};\n}\n// */\n";
    for text in [
        shebang,
        backtick,
        &format!("\u{feff}{shebang}"),
        nbsp,
        word,
        paren,
    ] {
        assert_eq!(
            later_faults("learning.rs", text),
            [unread("learning.rs", first_line)]
        );
        assert_eq!(list_file(text).err(), Some(FIRST_LINE.to_owned()));
        assert_eq!(
            coarse_assertion_faults(text),
            [unread("src/record.rs", first_line)]
        );
    }
    for text in [
        "pub fn f() {\n    `\n}\n",
        "pub fn f() {\n",
        "pub const S: &str = \"open;\n",
    ] {
        let faults = later_faults("learning.rs", text);
        assert_eq!(faults.len(), 1, "{text}");
        assert!(
            faults[0].starts_with(
                "learning.rs cannot be read as the compiler reads it: the lexer refuses it"
            ),
            "{faults:?}"
        );
    }
    for text in [
        "#![allow(dead_code)]\npub fn shown() {}\n",
        "#! /* a comment */ [allow(dead_code)]\npub fn shown() {}\n",
        "#! // a comment\n[allow(dead_code)]\npub fn shown() {}\n",
        "\u{feff}pub fn shown() {}\n",
    ] {
        assert_eq!(
            later_faults("learning.rs", text),
            Vec::<String>::new(),
            "{text}"
        );
    }
}

fn later_names_faults(file: &str, text: &str) -> Vec<String> {
    let t = match lex(text) {
        Ok(t) => t,
        Err(cause) => return vec![unread(file, &cause)],
    };
    let mut faults = Vec::new();
    for module in LATER {
        let named = t
            .iter()
            .filter(|x| is_ident(x) && bare(x) == module)
            .count();
        let forms = match file {
            "kinds.rs" => vec![
                vec!["crate", ":", ":", module, ":", ":", "KINDS"],
                vec!["crate", ":", ":", module, ":", ":", "ERROR_NAMES"],
            ],
            "lib.rs" => vec![vec!["pub", "mod", module, ";"]],
            _ => Vec::new(),
        };
        if named != forms.len() {
            faults.push(format!("{file} names {module} {named} times"));
        }
        for form in forms {
            if find(&t, &form).len() != 1 {
                faults.push(format!(
                    "{file} does not name {module} as `{}` once",
                    form.join(" ")
                ));
            }
        }
    }
    faults
}

#[test]
fn no_locked_file_names_an_item_of_the_files_of_later_runs_but_their_kind_lists() {
    for file in LOCKED {
        assert_eq!(
            later_names_faults(file, &source(file)),
            Vec::<String>::new(),
            "{file}"
        );
    }
}

#[test]
fn the_reading_of_identifiers_leaves_out_comments_strings_and_chars() {
    let text = "/// learning\n// routing\nlet a = \"learning\"; let b = r#\"routing \"x\"\"#; /* executable */\nlet c = b'\"'; let d = '\\''; let e = learning::KINDS; fn f<'a>(x: &'a str) {}\nuse crate::{routing as r};\n";
    assert_eq!(identifiers(text, "learning"), 1);
    assert_eq!(identifiers(text, "routing"), 1);
    assert_eq!(identifiers(text, "executable"), 0);
    assert_eq!(identifiers(text, "coordinator"), 0);
}

fn fn_body(text: &str, name: &str) -> Option<Vec<String>> {
    let t = outside_macros(&tokens(text));
    let at = find(&t, &["fn", name, "("]);
    if at.len() != 1 {
        return None;
    }
    let open = (at[0]..t.len()).find(|&i| t[i] == "{")?;
    Some(t[open + 1..closing(&t, open)].to_vec())
}

const REGISTRY: &str = "KINDS.iter().chain(crate::executable::KINDS).chain(crate::coordinator::KINDS).chain(crate::learning::KINDS).chain(crate::routing::KINDS)";
const CHECK_REGISTRY: &str =
    "let entries: Vec<&KindEntry> = registry().collect(); check_entries(&entries)";

#[test]
fn registry_chains_the_kinds_of_this_run_and_the_four_lists_of_later_runs_in_the_order_of_the_contract(
) {
    assert_eq!(
        fn_body(&source("kinds.rs"), "registry"),
        Some(tokens(REGISTRY)),
        "src/kinds.rs: registry() must chain kinds::KINDS, executable::KINDS, coordinator::KINDS, learning::KINDS and routing::KINDS in that order"
    );
}

const DECLARED_ERROR_NAMES: &str = "let lists: [&'static [&'static str]; 4] = [crate::executable::ERROR_NAMES, crate::coordinator::ERROR_NAMES, crate::learning::ERROR_NAMES, crate::routing::ERROR_NAMES,]; lists.into_iter().flatten().copied()";

fn is_punct(token: Option<&TokenTree>, c: char) -> bool {
    matches!(token, Some(TokenTree::Punct(p)) if p.as_char() == c)
}

fn is_word(token: Option<&TokenTree>, word: &str) -> bool {
    matches!(token, Some(TokenTree::Ident(i)) if i == word)
}

#[test]
fn declared_error_names_chains_the_four_lists_of_later_runs_in_the_order_of_the_contract() {
    assert_eq!(
        fn_body(&source("kinds.rs"), "declared_error_names"),
        Some(tokens(DECLARED_ERROR_NAMES)),
        "src/kinds.rs: declared_error_names() must chain executable::ERROR_NAMES, coordinator::ERROR_NAMES, learning::ERROR_NAMES and routing::ERROR_NAMES in that order"
    );
}

#[test]
fn check_registry_reads_the_whole_registry() {
    assert_eq!(
        fn_body(&source("kinds.rs"), "check_registry"),
        Some(tokens(CHECK_REGISTRY)),
        "src/kinds.rs: check_registry() must call check_entries over registry() and nothing else"
    );
}

#[test]
fn the_tests_of_the_chain_read_the_code_outside_macros() {
    let text = source("kinds.rs");
    let real = "pub fn registry() -> impl Iterator<Item = &'static KindEntry> {\n";
    assert_eq!(text.matches(real).count(), 1);
    let hidden = text.replacen(
        real,
        &format!(
            "macro_rules! shown {{\n    () => {{\n        fn registry() {{\n            {REGISTRY}\n        }}\n        fn check_registry() {{\n            {CHECK_REGISTRY}\n        }}\n    }};\n}}\n{real}"
        ),
        1,
    );
    assert_eq!(fn_body(&hidden, "registry"), fn_body(&text, "registry"));
    let made = hidden
        .replacen("pub fn registry()", "pub fn registry_made()", 1)
        .replacen("pub fn check_registry()", "pub fn check_registry_made()", 1);
    assert_eq!(fn_body(&made, "registry"), None);
    assert_eq!(fn_body(&made, "check_registry"), None);
}

#[test]
fn each_predicate_of_the_textual_tests_has_a_case_of_its_own() {
    assert_eq!(
        later_faults("site.rs", "r#include!(\"learning.rs\");\n"),
        ["site.rs names include"]
    );
    let record = source("record.rs");
    let open = "const _: fn(&KeyCounts, &PointerCounts) = |keys, pointer| {\n";
    assert_eq!(record.matches(open).count(), 1);
    for attribute in [
        "#[cfg(any())]\n",
        "#[cfg_attr(any(), allow(unused))]\n",
        "#[r#cfg(any())]\n",
    ] {
        let faults =
            coarse_assertion_faults(&record.replacen(open, &format!("{attribute}{open}"), 1));
        assert_eq!(
            faults,
            ["src/record.rs: a cfg attribute can switch the assertion off"],
            "{attribute}"
        );
    }
    let kinds = source("kinds.rs");
    assert_eq!(
        later_names_faults(
            "kinds.rs",
            &format!("{kinds}pub fn twice() -> usize {{\n    crate::learning::KINDS.len()\n}}\n")
        ),
        [
            "kinds.rs names learning 3 times",
            "kinds.rs does not name learning as `crate : : learning : : KINDS` once"
        ]
    );
    assert_eq!(
        later_names_faults(
            "kinds.rs",
            &format!(
                "{kinds}pub fn twice() -> usize {{\n    crate::learning::ERROR_NAMES.len()\n}}\n"
            )
        ),
        [
            "kinds.rs names learning 3 times",
            "kinds.rs does not name learning as `crate : : learning : : ERROR_NAMES` once"
        ]
    );
    assert_eq!(
        later_names_faults(
            "kinds.rs",
            &kinds.replacen("crate::learning::ERROR_NAMES", "crate::learning::KINDS", 1)
        ),
        [
            "kinds.rs does not name learning as `crate : : learning : : KINDS` once",
            "kinds.rs does not name learning as `crate : : learning : : ERROR_NAMES` once"
        ]
    );
    let lib = source("lib.rs");
    assert_eq!(
        later_names_faults(
            "lib.rs",
            &format!("{lib}pub use learning::KINDS as LEARNING;\n")
        ),
        ["lib.rs names learning 2 times"]
    );
}

fn core_files() -> Vec<(String, String)> {
    let src = core_dir().join("src");
    let mut paths = Vec::new();
    rust_files(&src, &mut paths);
    let mut files: Vec<(String, String)> = paths
        .into_iter()
        .map(|path| {
            let name = path
                .strip_prefix(&src)
                .expect("a path under src")
                .to_string_lossy()
                .replace('\\', "/");
            let text = read(&path);
            (name, text)
        })
        .collect();
    files.sort();
    files
}

#[test]
fn no_file_of_either_crate_holds_a_path_attribute_or_an_include_macro() {
    let later = later_files();
    let core = core_files();
    let names = |files: &[(String, String)]| -> Vec<String> {
        files.iter().map(|(name, _)| name.clone()).collect()
    };
    for module in LATER {
        assert!(names(&later).contains(&format!("{module}.rs")), "{later:?}");
        assert!(names(&core).contains(&format!("{module}.rs")), "{core:?}");
    }
    for file in LOCKED {
        assert!(names(&core).contains(&file.to_owned()), "{file}");
    }
    assert!(!names(&later).contains(&"lib.rs".to_owned()));
    let faults: Vec<String> = later
        .iter()
        .chain(&core)
        .flat_map(|(name, text)| later_faults(name, text))
        .collect();
    assert_eq!(faults, Vec::<String>::new());
}

#[test]
fn the_test_of_the_core_files_refuses_a_path_attribute_and_the_include_macros() {
    for (file, text, fault) in [
        (
            "kinds.rs",
            "#[path = \"../../halvo-schema/src/learning.rs\"]\nmod later;\n",
            "kinds.rs holds path in an attribute or a macro",
        ),
        (
            "kinds.rs",
            "macro_rules! own_mod {\n    ($m:meta) => {\n        #[$m]\n        mod later;\n    };\n}\nown_mod! {path = \"../../halvo-schema/src/learning.rs\"}\n",
            "kinds.rs holds path in an attribute or a macro",
        ),
        (
            "learning.rs",
            "include!(\"../../halvo-schema/src/learning.rs\");\n",
            "learning.rs names include",
        ),
        (
            "site.rs",
            "pub const LATER_TEXT: &str = include_str!(\"../../halvo-schema/src/learning.rs\");\n",
            "site.rs names include_str",
        ),
    ] {
        let changed = format!("{}{text}", source(file));
        assert_eq!(later_faults(file, &changed), [fault], "{file}: {text}");
    }
}

#[test]
fn the_path_rule_reads_a_raw_identifier_and_exempts_a_fragment_only_inside_a_matcher() {
    for text in [
        "macro_rules! own_mod {\n    ($a:ident $b:literal) => {\n        #[$a = $b]\n        mod sneak;\n    };\n}\nown_mod!(r#path \"learning.rs\");\n",
        "macro_rules! own_mod {\n    ($d:tt $a:ident : $($m:tt)*) => {\n        #[$($m)*]\n        mod sneak;\n    };\n}\nown_mod! {$ a: path = \"learning.rs\"}\n",
    ] {
        assert_eq!(
            later_faults("learning.rs", text),
            ["learning.rs holds path in an attribute or a macro"],
            "{text}"
        );
        assert_eq!(
            later_faults("kinds.rs", &format!("{}{text}", source("kinds.rs"))),
            ["kinds.rs holds path in an attribute or a macro"],
            "{text}"
        );
    }
}

#[test]
fn the_walk_reads_every_rust_file_of_src_outside_the_lock_at_any_depth() {
    let src = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("walk-outside-lib")
        .join("src");
    let written = [
        ("lib.rs", "#[path = \"other.rs\"]\nmod again;\n"),
        (
            "learning.rs",
            "pub use halvo_schema_core::learning::{ERROR_NAMES, KINDS};\n",
        ),
        ("learning/more.rs", "pub fn shown() {}\n"),
        (
            "learning/deeper/most.rs",
            "#[path = \"../../other.rs\"]\nmod again;\n",
        ),
        ("other.rs", "pub fn shown() {}\n"),
        ("learning/upper.RS", "include!(\"macros.inc\");\n"),
        ("bin/tool.rs", "fn main() {}\n"),
        ("learning/macros.inc", "#[path = \"x.rs\"]\n"),
        ("routing/notes.txt", "include!(\"x.rs\")\n"),
    ];
    for (name, text) in written {
        let path = src.join(name);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
        std::fs::write(&path, text).expect("a fixture file");
    }
    let files = files_outside_the_lock(&src);
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "bin/tool.rs",
            "learning.rs",
            "learning/deeper/most.rs",
            "learning/more.rs",
            "learning/upper.RS",
            "other.rs"
        ]
    );
    let faults: Vec<String> = files
        .iter()
        .flat_map(|(name, text)| later_faults(name, text))
        .collect();
    assert_eq!(
        faults,
        [
            "learning/deeper/most.rs holds path in an attribute or a macro",
            "learning/upper.RS names include"
        ]
    );
}

const SIX: [&str; 6] = ["category", "ids", "kinds", "record", "request", "site"];

const CORE_HEAD: &str = "#![forbid(unsafe_code)]\npub mod category;\n#[allow(unused_imports)]\npub mod coordinator;\n#[allow(unused_imports)]\npub mod executable;\npub mod ids;\npub mod kinds;\n#[allow(unused_imports)]\npub mod learning;\npub mod record;\npub mod request;\n#[allow(unused_imports)]\npub mod routing;\npub mod site;\n";

const SCHEMA_HEAD: &str = "#![forbid(unsafe_code)]\npub mod coordinator;\npub mod executable;\npub mod learning;\npub mod routing;\npub use halvo_schema_core::{category, ids, kinds, record, request, site};\n";

fn re_exports(t: &[String]) -> Result<(), String> {
    let mut i = 0;
    while i < t.len() {
        let end = (i..t.len()).find(|&k| t[k] == ";").unwrap_or(t.len());
        let item = &t[i..end];
        let form = item.len() > 3
            && item[0] == "pub"
            && item[1] == "use"
            && SIX.contains(&item[2].as_str())
            && item[3..].iter().all(|x| {
                matches!(x.as_str(), ":" | "{" | "}" | ",")
                    || (is_ident(x) && !x.starts_with("r#") && !KEYWORDS.contains(&x.as_str()))
            });
        if !form || end == t.len() {
            return Err(format!(
                "holds `{}` where only a re-export of a locked module stands",
                item.join(" ")
            ));
        }
        i = end + 1;
    }
    Ok(())
}

fn lib_faults(core: &str, schema: &str) -> Vec<String> {
    let core = match lex(core) {
        Ok(t) => t,
        Err(cause) => return vec![unread("halvo-schema-core/src/lib.rs", &cause)],
    };
    let head = tokens(CORE_HEAD);
    if !core.starts_with(&head) {
        return vec!["halvo-schema-core/src/lib.rs does not start with the line that forbids unsafe code and the module lines of its ten modules".to_owned()];
    }
    let uses = &core[head.len()..];
    if uses.is_empty() {
        return vec!["halvo-schema-core/src/lib.rs re-exports nothing".to_owned()];
    }
    if let Err(fault) = re_exports(uses) {
        return vec![format!("halvo-schema-core/src/lib.rs {fault}")];
    }
    let schema = match lex(schema) {
        Ok(t) => t,
        Err(cause) => return vec![unread("halvo-schema/src/lib.rs", &cause)],
    };
    if schema != [tokens(SCHEMA_HEAD).as_slice(), uses].concat() {
        return vec!["halvo-schema/src/lib.rs holds other than the line that forbids unsafe code, the module lines of the four later files and the re-exports of the core crate".to_owned()];
    }
    Vec::new()
}

#[test]
fn each_lib_rs_holds_its_form() {
    let core = source("lib.rs");
    let schema = read(&schema_dir().join("src").join("lib.rs"));
    assert_eq!(lib_faults(&core, &schema), Vec::<String>::new());
    let one = |text: &str, old: &str, new: &str| {
        assert_eq!(text.matches(old).count(), 1, "{old}");
        text.replacen(old, new, 1)
    };
    let not_schema = ["halvo-schema/src/lib.rs holds other than the line that forbids unsafe code, the module lines of the four later files and the re-exports of the core crate"];
    for (old, new) in [
        ("#![forbid(unsafe_code)]\n", ""),
        ("#![forbid(unsafe_code)]", "#![deny(unsafe_code)]"),
        ("pub mod learning;", "#[macro_use]\npub mod learning;"),
        (
            "pub mod learning;",
            "#[path = \"learning.rs\"]\npub mod learning;",
        ),
        ("pub mod routing;\n", "pub mod routing;\npub mod later;\n"),
        ("pub mod routing;\n", ""),
        (
            "pub use halvo_schema_core::{category, ids, kinds, record, request, site};",
            "pub use halvo_schema_core::{category, ids, kinds, record, request};",
        ),
        (
            "pub use category::{Category, SeedCategory};",
            "pub use category::Category;",
        ),
        (
            "pub use category::{Category, SeedCategory};",
            "pub use category::{Category, SeedCategory};\npub use learning::KINDS;",
        ),
    ] {
        assert_eq!(
            lib_faults(&core, &one(&schema, old, new)),
            not_schema,
            "{new}"
        );
    }
    let not_core = ["halvo-schema-core/src/lib.rs does not start with the line that forbids unsafe code and the module lines of its ten modules"];
    for (old, new) in [
        ("#![forbid(unsafe_code)]\n", ""),
        (
            "pub mod learning;",
            "#[path = \"../../halvo-schema/src/learning.rs\"]\npub mod learning;",
        ),
        (
            "#[allow(unused_imports)]\npub mod routing;",
            "pub mod routing;",
        ),
    ] {
        assert_eq!(
            lib_faults(&one(&core, old, new), &schema),
            not_core,
            "{new}"
        );
    }
    for (new, item) in [
        ("pub fn later() {}\n", "pub fn later ( ) { }"),
        ("pub mod later;\n", "pub mod later"),
        (
            "pub use kinds::KindEntry as Entry;\n",
            "pub use kinds : : KindEntry as Entry",
        ),
        ("pub use kinds::{self};\n", "pub use kinds : : { self }"),
        (
            "pub use crate::learning::KINDS;\n",
            "pub use crate : : learning : : KINDS",
        ),
        ("pub use learning::*;\n", "pub use learning : : *"),
        ("pub use kinds::*;\n", "pub use kinds : : *"),
    ] {
        assert_eq!(
            lib_faults(&format!("{core}{new}"), &format!("{schema}{new}")),
            [format!("halvo-schema-core/src/lib.rs holds `{item}` where only a re-export of a locked module stands")],
            "{new}"
        );
    }
    assert_eq!(
        lib_faults(
            &core.replace("\n\n", "\n// a comment\n\n"),
            &schema.replace("\n\n", "\n/* a comment */\n\n")
        ),
        Vec::<String>::new()
    );
}

const CORE_FILES: [&str; 15] = [
    ".gitattributes",
    "Cargo.toml",
    "NOTICE",
    "src/category.rs",
    "src/coordinator.rs",
    "src/executable.rs",
    "src/ids.rs",
    "src/kinds.rs",
    "src/learning.rs",
    "src/lib.rs",
    "src/record.rs",
    "src/request.rs",
    "src/routing.rs",
    "src/site.rs",
    "tests/notice.rs",
];

fn files_of(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            files_of(root, &path, out);
        } else {
            out.push(
                path.strip_prefix(root)
                    .expect("a path under the root")
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
}

fn set_faults(dir: &Path) -> Vec<String> {
    let mut have = Vec::new();
    files_of(dir, dir, &mut have);
    have.sort();
    let mut faults: Vec<String> = have
        .iter()
        .filter(|file| !CORE_FILES.contains(&file.as_str()))
        .map(|file| format!("the core crate holds {file}, which is none of its files"))
        .collect();
    faults.extend(
        CORE_FILES
            .iter()
            .filter(|file| !have.iter().any(|h| h == *file))
            .map(|file| format!("the core crate lacks {file}")),
    );
    faults
}

fn fixture_dir(name: &str, files: &[&str]) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("crate-files")
        .join(name);
    for file in files {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("a directory");
        std::fs::write(&path, "").expect("a fixture file");
    }
    dir
}

#[test]
fn the_core_crate_holds_its_set_of_files() {
    assert_eq!(set_faults(&core_dir()), Vec::<String>::new());
    assert_eq!(
        set_faults(&fixture_dir("whole", &CORE_FILES)),
        Vec::<String>::new()
    );
    let more = [CORE_FILES.as_slice(), &["src/learning/more.rs"]].concat();
    assert_eq!(
        set_faults(&fixture_dir("more", &more)),
        ["the core crate holds src/learning/more.rs, which is none of its files"]
    );
    let built = [CORE_FILES.as_slice(), &["build.rs"]].concat();
    assert_eq!(
        set_faults(&fixture_dir("built", &built)),
        ["the core crate holds build.rs, which is none of its files"]
    );
    let less: Vec<&str> = CORE_FILES
        .iter()
        .copied()
        .filter(|file| *file != "src/routing.rs")
        .collect();
    assert_eq!(
        set_faults(&fixture_dir("less", &less)),
        ["the core crate lacks src/routing.rs"]
    );
}

fn build_script_faults(name: &str, dir: &Path) -> Vec<String> {
    if dir.join("build.rs").exists() {
        vec![format!("{name} has a build script")]
    } else {
        Vec::new()
    }
}

#[test]
fn neither_crate_has_a_build_script() {
    assert_eq!(
        build_script_faults("halvo-schema", &schema_dir()),
        Vec::<String>::new()
    );
    assert_eq!(
        build_script_faults("halvo-schema-core", &core_dir()),
        Vec::<String>::new()
    );
    assert_eq!(
        build_script_faults("a crate", &fixture_dir("built", &["build.rs"])),
        ["a crate has a build script"]
    );
    assert_eq!(
        build_script_faults("a crate", &fixture_dir("whole", &CORE_FILES)),
        Vec::<String>::new()
    );
}

const CORE_MANIFEST: &str = "[package]\nname = \"halvo-schema-core\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[dependencies]\npsl = \"=2.1.246\"\nserde = { version = \"1\", features = [\"derive\"] }\nserde_json = { version = \"1\", features = [\"raw_value\"] }\n";

const SCHEMA_MANIFEST: &str = "[package]\nname = \"halvo-schema\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n\n[dependencies]\nhalvo-schema-core = { path = \"../halvo-schema-core\" }\nserde = { version = \"1\", features = [\"derive\"] }\nserde_json = { version = \"1\", features = [\"raw_value\"] }\n\n[dev-dependencies]\nproc-macro2 = { version = \"1.0.107\", default-features = false }\n";

fn manifest_faults(name: &str, text: &str, want: &str) -> Vec<String> {
    if text == want {
        Vec::new()
    } else {
        vec![format!("the manifest of {name} is not its fixed file")]
    }
}

#[test]
fn both_manifests_are_the_fixed_files() {
    let core = read(&core_dir().join("Cargo.toml"));
    let schema = read(&schema_dir().join("Cargo.toml"));
    assert_eq!(
        manifest_faults("halvo-schema-core", &core, CORE_MANIFEST),
        Vec::<String>::new()
    );
    assert_eq!(
        manifest_faults("halvo-schema", &schema, SCHEMA_MANIFEST),
        Vec::<String>::new()
    );
    for (text, want) in [
        (format!("{core}\n[features]\nlater = []\n"), CORE_MANIFEST),
        (
            core.replace(
                "psl = \"=2.1.246\"\n",
                "psl = \"=2.1.246\"\nlater = { path = \"../later\", optional = true }\n",
            ),
            CORE_MANIFEST,
        ),
        (
            core.replace(
                "[dependencies]\n",
                "build = \"later.rs\"\n\n[dependencies]\n",
            ),
            CORE_MANIFEST,
        ),
        (
            schema.replace(
                "features = [\"raw_value\"]",
                "features = [\"raw_value\", \"arbitrary_precision\"]",
            ),
            SCHEMA_MANIFEST,
        ),
        (format!("{schema}later = \"1\"\n"), SCHEMA_MANIFEST),
    ] {
        assert_eq!(
            manifest_faults("a crate", &text, want),
            ["the manifest of a crate is not its fixed file"],
            "{text}"
        );
    }
}

const USE_LINE: &str = "use crate::kinds::{FieldType, KindEntry, Origin, RetentionClass};";

const KINDS_HEADER: &str = "pub const KINDS: &[KindEntry] =";

const NAMES_HEADER: &str = "pub const ERROR_NAMES: &[&str] =";

const ENTRY_FIELDS: [&str; 7] = [
    "name",
    "class",
    "origin",
    "family",
    "workflow_key",
    "fields",
    "example",
];

const FIELD_TYPES: [&str; 14] = [
    "Version", "Id", "Name", "App", "Domain", "Hash", "Site", "Count", "Day", "Instant", "Ordinal",
    "Token", "Text", "OwnText",
];

const ENTRY_HEAD: &str = "holds an entry that is no literal `KindEntry { .. }`";

const FIRST_LINE: &str =
    "its first line is neither the use line nor a doc comment in the three-slash form";

#[derive(Debug, PartialEq)]
struct Listed {
    kinds: Vec<String>,
    error_names: Vec<String>,
}

fn trees(text: &str) -> Vec<TokenTree> {
    TokenStream::from_str(text)
        .expect("a text that the lexer reads")
        .into_iter()
        .collect()
}

fn flat(trees: &[TokenTree]) -> Vec<String> {
    let mut out = Vec::new();
    flatten(trees.iter().cloned().collect(), &mut out);
    out
}

fn outside(trees: &[TokenTree]) -> String {
    format!(
        "holds `{}` where the grammar of a list file has no place for it",
        flat(trees).join(" ")
    )
}

fn inner(tree: &TokenTree, delimiter: Delimiter) -> Option<Vec<TokenTree>> {
    match tree {
        TokenTree::Group(g) if g.delimiter() == delimiter => Some(g.stream().into_iter().collect()),
        _ => None,
    }
}

fn string_value(tree: &TokenTree) -> Result<String, String> {
    let TokenTree::Literal(literal) = tree else {
        return Err(outside(std::slice::from_ref(tree)));
    };
    let text = literal.to_string();
    if let Some(rest) = text.strip_prefix('r') {
        let hashes = rest.len() - rest.trim_start_matches('#').len();
        return rest[hashes..]
            .strip_prefix('"')
            .and_then(|body| body.strip_suffix(&"#".repeat(hashes)))
            .and_then(|body| body.strip_suffix('"'))
            .map(str::to_owned)
            .ok_or_else(|| outside(std::slice::from_ref(tree)));
    }
    let Some(body) = text.strip_prefix('"').and_then(|b| b.strip_suffix('"')) else {
        return Err(outside(std::slice::from_ref(tree)));
    };
    if body.contains('\\') {
        return Err(format!(
            "holds `{text}`, a string literal with an escape or a line continuation"
        ));
    }
    Ok(body.to_owned())
}

fn parts(trees: &[TokenTree]) -> Result<Vec<&[TokenTree]>, String> {
    let mut parts = Vec::new();
    let mut start = 0;
    for (i, tree) in trees.iter().enumerate() {
        if is_punct(Some(tree), ',') {
            if i == start {
                return Err(outside(&trees[i..=i]));
            }
            parts.push(&trees[start..i]);
            start = i + 1;
        }
    }
    if start < trees.len() {
        parts.push(&trees[start..]);
    }
    Ok(parts)
}

fn path_is(trees: &[TokenTree], head: &str, names: &[&str]) -> bool {
    let t = flat(trees);
    t.len() == 4 && t[0] == head && t[1] == ":" && t[2] == ":" && names.contains(&t[3].as_str())
}

fn days(trees: &[TokenTree]) -> bool {
    let n = if is_punct(trees.get(3), ',') { 4 } else { 3 };
    trees.len() == n
        && is_word(trees.first(), "days")
        && is_punct(trees.get(1), ':')
        && matches!(&trees[2], TokenTree::Literal(l) if l.to_string().bytes().all(|b| b.is_ascii_digit()))
}

fn field_pair(pair: &[TokenTree]) -> Result<(), String> {
    let tuple = match pair {
        [tree] => inner(tree, Delimiter::Parenthesis),
        _ => None,
    }
    .ok_or_else(|| outside(pair))?;
    if !is_punct(tuple.get(1), ',') {
        return Err(outside(pair));
    }
    string_value(&tuple[0])?;
    if path_is(&tuple[2..], "FieldType", &FIELD_TYPES) {
        Ok(())
    } else {
        Err(outside(pair))
    }
}

fn entry_value(field: &str, value: &[TokenTree]) -> Result<(), String> {
    let form = match field {
        "name" | "family" | "example" => {
            if value.len() != 1 {
                return Err(outside(value));
            }
            string_value(&value[0])?;
            true
        }
        "class" => {
            path_is(value, "RetentionClass", &["Window", "Kept"])
                || (value.len() == 5
                    && path_is(&value[..4], "RetentionClass", &["Bounded"])
                    && inner(&value[4], Delimiter::Brace).is_some_and(|t| days(&t)))
        }
        "origin" => path_is(value, "Origin", &["Capture", "Own"]),
        "workflow_key" => match value {
            [none] => is_word(Some(none), "None"),
            [some, group] if is_word(Some(some), "Some") => {
                match inner(group, Delimiter::Parenthesis).as_deref() {
                    Some([key]) => {
                        string_value(key)?;
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        },
        "fields" => match value {
            [and, group] if is_punct(Some(and), '&') => match inner(group, Delimiter::Bracket) {
                Some(pairs) => {
                    for pair in parts(&pairs)? {
                        field_pair(pair)?;
                    }
                    true
                }
                None => false,
            },
            _ => false,
        },
        _ => false,
    };
    if form {
        Ok(())
    } else {
        Err(outside(value))
    }
}

fn entry(trees: &[TokenTree]) -> Result<String, String> {
    let body = match trees {
        [kind, body] if is_word(Some(kind), "KindEntry") => inner(body, Delimiter::Brace),
        _ => None,
    }
    .ok_or_else(|| ENTRY_HEAD.to_owned())?;
    let mut seen: Vec<String> = Vec::new();
    let mut name = String::new();
    for field in parts(&body)? {
        let key = match field {
            [TokenTree::Ident(key), colon, ..] if is_punct(Some(colon), ':') => key.to_string(),
            _ => return Err(outside(field)),
        };
        if !ENTRY_FIELDS.contains(&key.as_str()) || seen.contains(&key) {
            return Err(outside(field));
        }
        entry_value(&key, &field[2..])?;
        if key == "name" {
            name = string_value(&field[2])?;
        }
        seen.push(key);
    }
    if seen.len() != ENTRY_FIELDS.len() {
        return Err(format!(
            "holds a KindEntry without each of its {} fields",
            ENTRY_FIELDS.len()
        ));
    }
    Ok(name)
}

fn is_doc(attribute: &TokenTree) -> bool {
    inner(attribute, Delimiter::Bracket).is_some_and(|t| {
        t.len() == 3
            && is_word(t.first(), "doc")
            && is_punct(t.get(1), '=')
            && matches!(&t[2], TokenTree::Literal(l) if l.to_string().starts_with('"'))
    })
}

fn without_three_slash_lines(text: &str) -> String {
    text.split('\n')
        .map(|line| {
            let start = line.trim_start();
            if start.starts_with("///") && !start.starts_with("////") {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn three_slash_docs(text: &str) -> Option<Vec<String>> {
    let mut docs = Vec::new();
    for line in text.split('\n') {
        let start = line.trim_start();
        if !start.starts_with("///") || start.starts_with("////") {
            continue;
        }
        match TokenStream::from_str(start)
            .ok()?
            .into_iter()
            .collect::<Vec<_>>()[..]
        {
            [ref hash, ref doc] if is_punct(Some(hash), '#') && is_doc(doc) => {
                docs.push(doc.to_string())
            }
            _ => return None,
        }
    }
    Some(docs)
}

fn list_file(text: &str) -> Result<Listed, String> {
    let first = text.split('\n').next().unwrap_or_default();
    if first != USE_LINE && !(first.starts_with("///") && !first.starts_with("////")) {
        return Err(FIRST_LINE.to_owned());
    }
    let top: Vec<TokenTree> = TokenStream::from_str(text)
        .map_err(|e| format!("the lexer refuses it: {e}"))?
        .into_iter()
        .collect();
    let use_line = trees(USE_LINE);
    let kinds_header = trees(KINDS_HEADER);
    let names_header = trees(NAMES_HEADER);
    let mut listed = Listed {
        kinds: Vec::new(),
        error_names: Vec::new(),
    };
    let mut kept: Vec<TokenTree> = Vec::new();
    let mut docs: Vec<String> = Vec::new();
    let (mut items, mut kinds, mut names) = (0, 0, 0);
    let mut i = 0;
    while i < top.len() {
        while is_punct(top.get(i), '#') {
            match top.get(i + 1) {
                Some(attribute) if is_doc(attribute) => {
                    docs.push(attribute.to_string());
                    i += 2;
                }
                _ => return Err("holds an attribute other than a doc comment".to_owned()),
            }
        }
        let rest = &top[i..];
        if items == 0 {
            if rest.len() < use_line.len() || flat(&rest[..use_line.len()]) != flat(&use_line) {
                return Err("does not start with the use line".to_owned());
            }
            kept.extend_from_slice(&rest[..use_line.len()]);
            i += use_line.len();
            items += 1;
            continue;
        }
        let header = [(&kinds_header, "KINDS"), (&names_header, "ERROR_NAMES")]
            .into_iter()
            .find(|(header, _)| {
                rest.len() >= header.len() && flat(&rest[..header.len()]) == flat(header)
            });
        let Some((header, list)) = header else {
            return Err(outside(&rest[..rest.len().min(4)]));
        };
        let n = header.len();
        let Some(values) = (is_punct(rest.get(n), '&'))
            .then(|| rest.get(n + 1).and_then(|t| inner(t, Delimiter::Bracket)))
            .flatten()
        else {
            return Err(outside(&rest[..rest.len().min(n + 2)]));
        };
        if !is_punct(rest.get(n + 2), ';') {
            let after = rest.get(n + 2).map_or("nothing".to_owned(), |t| {
                format!("`{}`", flat(std::slice::from_ref(t)).join(" "))
            });
            return Err(format!(
                "holds {after} after the bracket of its list {list}, where only `;` stands"
            ));
        }
        for part in parts(&values)? {
            if list == "KINDS" {
                listed.kinds.push(entry(part)?);
            } else {
                match part {
                    [name] => listed.error_names.push(string_value(name)?),
                    _ => return Err(outside(part)),
                }
            }
        }
        if list == "KINDS" {
            kinds += 1;
        } else {
            names += 1;
        }
        kept.extend_from_slice(&rest[..n + 3]);
        i += n + 3;
        items += 1;
    }
    if items == 0 {
        return Err("does not start with the use line".to_owned());
    }
    if kinds != 1 || names != 1 {
        return Err(format!(
            "holds {kinds} lists KINDS and {names} lists ERROR_NAMES, not one of each"
        ));
    }
    let stripped = TokenStream::from_str(&without_three_slash_lines(text))
        .map(|stream| {
            let mut out = Vec::new();
            flatten(stream, &mut out);
            out
        })
        .unwrap_or_default();
    if stripped != flat(&kept) || three_slash_docs(text) != Some(docs) {
        return Err("holds a doc comment that is no line of the three-slash form".to_owned());
    }
    Ok(listed)
}

const LIST_FILE: &str = r####"/// The lists of a later run.
use crate::kinds::{FieldType, KindEntry, Origin, RetentionClass};

/// The error names.
pub const ERROR_NAMES: &[&str] = &["binding", r#"playbook"#];

/// The kinds.
pub const KINDS: &[KindEntry] = &[
    KindEntry {
        name: "later_run",
        class: RetentionClass::Bounded { days: 93 },
        origin: Origin::Capture,
        family: "later",
        workflow_key: Some("workflow"),
        fields: &[
            ("v", FieldType::Version),
            ("workflow", FieldType::Id),
            ("day", FieldType::Day),
        ],
        example: r#"{"v":1,"workflow":"30000000000000000000000000000001","day":20768}"#,
    },
    KindEntry {
        name: "later_config",
        class: RetentionClass::Kept,
        origin: Origin::Own,
        family: "later",
        workflow_key: None,
        fields: &[("v", FieldType::Version)],
        example: r#"{"v":1}"#,
    },
];
"####;

fn changed(old: &str, new: &str) -> String {
    assert_eq!(LIST_FILE.matches(old).count(), 1, "{old}");
    LIST_FILE.replacen(old, new, 1)
}

#[test]
fn each_file_of_a_later_run_lists_its_error_names_as_string_literals() {
    let mut kinds = Vec::new();
    for module in LATER {
        let listed = list_file(&source(&format!("{module}.rs")))
            .unwrap_or_else(|fault| panic!("halvo-schema-core/src/{module}.rs {fault}"));
        kinds.extend(listed.kinds);
    }
    let registered: Vec<String> = halvo_schema::kinds::registry()
        .skip(halvo_schema::kinds::KINDS.len())
        .map(|entry| entry.name.to_owned())
        .collect();
    assert_eq!(kinds, registered);
    let want = |kinds: &[&str], names: &[&str]| {
        Ok(Listed {
            kinds: kinds.iter().map(|k| (*k).to_owned()).collect(),
            error_names: names.iter().map(|n| (*n).to_owned()).collect(),
        })
    };
    assert_eq!(
        list_file(LIST_FILE),
        want(&["later_run", "later_config"], &["binding", "playbook"])
    );
    for text in [
        LIST_FILE.replacen("/// The lists of a later run.\n", "", 1),
        changed("/// The kinds.\n", ""),
        changed(
            "/// The error names.\n",
            "/// The error names.\n//// Not a doc comment.\n/* Nor this. */\n",
        ),
        changed(
            "RetentionClass::Bounded { days: 93 }",
            "RetentionClass::Bounded { days: 93, }",
        ),
        changed(
            "&[\"binding\", r#\"playbook\"#];",
            "&[\n    \"binding\",\n    r#\"playbook\"#,\n];",
        ),
        changed(
            "fields: &[(\"v\", FieldType::Version)],",
            "fields: &[(\"v\", FieldType::Version),],",
        ),
        LIST_FILE.replacen("    },\n];", "    }\n];", 1),
    ] {
        assert_eq!(
            list_file(&text),
            want(&["later_run", "later_config"], &["binding", "playbook"]),
            "{text}"
        );
    }
    let turned = changed(
        "/// The error names.\npub const ERROR_NAMES: &[&str] = &[\"binding\", r#\"playbook\"#];\n\n",
        "",
    ) + "\npub const ERROR_NAMES: &[&str] = &[r##\"play\"#book\"##];\n";
    assert_eq!(
        list_file(&turned),
        want(&["later_run", "later_config"], &["play\"#book"])
    );
    let empty = format!(
        "{USE_LINE}\n\npub const KINDS: &[KindEntry] = &[];\npub const ERROR_NAMES: &[&str] = &[];\n"
    );
    assert_eq!(list_file(&empty), want(&[], &[]));
}

#[test]
fn the_grammar_of_a_list_file_refuses_each_form_outside_it() {
    let names = "&[\"binding\", r#\"playbook\"#]";
    let start = "does not start with the use line";
    let attribute = "holds an attribute other than a doc comment";
    let three = "holds a doc comment that is no line of the three-slash form";
    for (text, fault) in [
        (
            format!("#! x pub const ERROR_NAMES: &[&str] = &[\"shown\"];\n{LIST_FILE}"),
            FIRST_LINE.to_owned(),
        ),
        (format!("\u{feff}{LIST_FILE}"), FIRST_LINE.to_owned()),
        (format!("\n{LIST_FILE}"), FIRST_LINE.to_owned()),
        (
            format!("//! The lists.\n{LIST_FILE}"),
            FIRST_LINE.to_owned(),
        ),
        (format!("// The lists.\n{LIST_FILE}"), FIRST_LINE.to_owned()),
        (
            format!("//// The lists.\n{LIST_FILE}"),
            FIRST_LINE.to_owned(),
        ),
        (
            changed(&format!("{names};"), &format!("{names} as &[&str];")),
            "holds `as` after the bracket of its list ERROR_NAMES, where only `;` stands"
                .to_owned(),
        ),
        (
            changed(&format!("{names};"), names),
            "holds `#` after the bracket of its list ERROR_NAMES, where only `;` stands".to_owned(),
        ),
        (
            LIST_FILE.replacen("    },\n];\n", "    },\n].as_slice();\n", 1),
            "holds `.` after the bracket of its list KINDS, where only `;` stands".to_owned(),
        ),
        (
            LIST_FILE.replacen("    },\n];\n", "    },\n]\n", 1),
            "holds nothing after the bracket of its list KINDS, where only `;` stands".to_owned(),
        ),
        (
            changed("\"binding\"", "\"bind\\x69ng\""),
            "holds `\"bind\\x69ng\"`, a string literal with an escape or a line continuation"
                .to_owned(),
        ),
        (
            changed("\"binding\"", "\"\\u{62}inding\""),
            "holds `\"\\u{62}inding\"`, a string literal with an escape or a line continuation"
                .to_owned(),
        ),
        (
            changed("\"binding\"", "\"bind\\\n    ing\""),
            "holds `\"bind\\\n    ing\"`, a string literal with an escape or a line continuation"
                .to_owned(),
        ),
        (
            changed(
                "family: \"later\",\n        workflow_key: Some",
                "family: \"lat\\x65r\",\n        workflow_key: Some",
            ),
            "holds `\"lat\\x65r\"`, a string literal with an escape or a line continuation"
                .to_owned(),
        ),
        (changed(USE_LINE, "use crate::kinds::*;"), start.to_owned()),
        (
            changed(
                USE_LINE,
                "use crate::kinds::{FieldType, KindEntry, Origin};",
            ),
            start.to_owned(),
        ),
        (
            changed(
                USE_LINE,
                "/// The error names.\npub const ERROR_NAMES: &[&str] = &[];",
            ),
            start.to_owned(),
        ),
        (
            changed("/// The kinds.\n", &format!("{USE_LINE}\n/// The kinds.\n")),
            outside(&trees("use crate ::")),
        ),
        (
            changed(
                "pub const ERROR_NAMES: &[&str] =",
                "pub const ERROR_NAMES: &'static [&str] =",
            ),
            outside(&trees("pub const ERROR_NAMES :")),
        ),
        (
            changed(
                "pub const KINDS: &[KindEntry] =",
                "pub static KINDS: &[KindEntry] =",
            ),
            outside(&trees("pub static KINDS :")),
        ),
        (
            changed(
                "pub const KINDS: &[KindEntry] =",
                "pub(crate) const KINDS: &[KindEntry] =",
            ),
            outside(&trees("pub (crate) const KINDS")),
        ),
        (
            changed(
                &format!("{names};"),
                &format!("{names};\npub const ERROR_NAMES: &[&str] = &[];"),
            ),
            "holds 1 lists KINDS and 2 lists ERROR_NAMES, not one of each".to_owned(),
        ),
        (
            LIST_FILE
                .split("\n/// The kinds.\n")
                .next()
                .unwrap_or_default()
                .to_owned(),
            "holds 0 lists KINDS and 1 lists ERROR_NAMES, not one of each".to_owned(),
        ),
        (
            changed(&format!("= {names};"), "= OTHER;"),
            outside(&trees("pub const ERROR_NAMES: &[&str] = OTHER;")),
        ),
        (
            changed("\"binding\"", "b\"binding\""),
            outside(&trees("b\"binding\"")),
        ),
        (
            changed("\"binding\"", "c\"binding\""),
            outside(&trees("c\"binding\"")),
        ),
        (
            changed("\"binding\"", "\"binding\"x"),
            outside(&trees("\"binding\"x")),
        ),
        (
            changed("\"binding\"", "BINDING"),
            outside(&trees("BINDING")),
        ),
        (
            changed("\"binding\"", "r#BINDING"),
            outside(&trees("r#BINDING")),
        ),
        (
            changed("\"binding\"", "concat!(\"bind\", \"ing\")"),
            outside(&trees("concat ! (\"bind\" , \"ing\")")),
        ),
        (
            changed("\"binding\"", "{ \"binding\" }"),
            outside(&trees("{ \"binding\" }")),
        ),
        (
            changed("\"binding\"", "const { \"binding\" }"),
            outside(&trees("const { \"binding\" }")),
        ),
        (
            changed("\"binding\"", "\"binding\".trim_ascii()"),
            outside(&trees("\"binding\" . trim_ascii ()")),
        ),
        (
            changed("\"binding\"", "\"binding\" as &str"),
            outside(&trees("\"binding\" as & str")),
        ),
        (
            changed(names, "&[\"binding\"; 2]"),
            outside(&trees("\"binding\" ; 2")),
        ),
        (
            changed("\"binding\", r#", "\"binding\",, r#"),
            outside(&trees(",")),
        ),
        (
            changed("days: 93", "days: 93u16"),
            outside(&trees("RetentionClass :: Bounded { days : 93u16 }")),
        ),
        (
            changed("days: 93", "days: 9_3"),
            outside(&trees("RetentionClass :: Bounded { days : 9_3 }")),
        ),
        (
            changed("days: 93", "days: 0x5d"),
            outside(&trees("RetentionClass :: Bounded { days : 0x5d }")),
        ),
        (
            changed("days: 93", "days: 90 + 3"),
            outside(&trees("RetentionClass :: Bounded { days : 90 + 3 }")),
        ),
        (
            changed("days: 93", "days: DAYS"),
            outside(&trees("RetentionClass :: Bounded { days : DAYS }")),
        ),
        (
            changed("name: \"later_run\"", "name: \"later_run\".trim_ascii()"),
            outside(&trees("\"later_run\" . trim_ascii ()")),
        ),
        (
            changed("workflow_key: None,", "workflow_key: NONE,"),
            outside(&trees("NONE")),
        ),
        (
            changed("Some(\"workflow\")", "Some(WORKFLOW)"),
            outside(&trees("WORKFLOW")),
        ),
        (
            changed("Some(\"workflow\")", "Some(\"workflow\").or(None)"),
            outside(&trees("Some (\"workflow\") . or (None)")),
        ),
        (
            changed("Some(\"workflow\")", "Some(\"workflow\", )"),
            outside(&trees("Some (\"workflow\" ,)")),
        ),
        (
            changed(
                "example: r#\"{\"v\":1}\"#,\n",
                "example: r#\"{\"v\":1}\"#,\n        ..OTHER\n",
            ),
            outside(&trees(". . OTHER")),
        ),
        (
            changed(
                "        family: \"later\",\n        workflow_key: None,",
                "        workflow_key: None,",
            ),
            "holds a KindEntry without each of its 7 fields".to_owned(),
        ),
        (
            changed(
                "family: \"later\",\n        workflow_key: None,",
                "family: \"later\",\n        family: \"later\",\n        workflow_key: None,",
            ),
            outside(&trees("family : \"later\"")),
        ),
        (
            changed(
                "family: \"later\",\n        workflow_key: None,",
                "kind: \"later\",\n        workflow_key: None,",
            ),
            outside(&trees("kind : \"later\"")),
        ),
        (
            changed(
                "family: \"later\",\n        workflow_key: None,",
                "r#family: \"later\",\n        workflow_key: None,",
            ),
            outside(&trees("r#family : \"later\"")),
        ),
        (
            changed(
                "    KindEntry {\n        name: \"later_config\"",
                "    crate::kinds::KindEntry {\n        name: \"later_config\"",
            ),
            ENTRY_HEAD.to_owned(),
        ),
        (
            changed(
                "class: RetentionClass::Kept",
                "class: crate::kinds::RetentionClass::Kept",
            ),
            outside(&trees("crate :: kinds :: RetentionClass :: Kept")),
        ),
        (
            changed("origin: Origin::Own", "origin: Origins::Own"),
            outside(&trees("Origins :: Own")),
        ),
        (
            changed("class: RetentionClass::Kept", "class: Origin::Kept"),
            outside(&trees("Origin :: Kept")),
        ),
        (
            changed("(\"v\", FieldType::Version)]", "(\"v\", Origin::Version)]"),
            outside(&trees("(\"v\" , Origin :: Version)")),
        ),
        (
            changed("origin: Origin::Own", "origin: Origin::Own.clone()"),
            outside(&trees("Origin :: Own . clone ()")),
        ),
        (
            changed("origin: Origin::Own", "origin: crate::kinds::Origin::Own"),
            outside(&trees("crate :: kinds :: Origin :: Own")),
        ),
        (
            changed(
                "(\"v\", FieldType::Version)]",
                "(\"v\", FieldType::Version, )]",
            ),
            outside(&trees("(\"v\" , FieldType :: Version ,)")),
        ),
        (
            changed("(\"v\", FieldType::Version)]", "(V, FieldType::Version)]"),
            outside(&trees("V")),
        ),
        (
            changed(
                "(\"v\", FieldType::Version)]",
                "(\"v\", self::FieldType::Version)]",
            ),
            outside(&trees("(\"v\" , self :: FieldType :: Version)")),
        ),
        (
            changed(
                "fields: &[(\"v\", FieldType::Version)]",
                "fields: &[(\"v\", FieldType::Version); 1]",
            ),
            outside(&trees("(\"v\" , FieldType :: Version) ; 1")),
        ),
        (
            changed("/// The error names.\n", "#[doc = \"The error names.\"]\n"),
            three.to_owned(),
        ),
        (
            changed(
                "/// The error names.\n",
                "/*\n/// */ #[doc = \"The error names: \\x41.\"] /*\n*/\n",
            ),
            three.to_owned(),
        ),
        (
            changed(
                "/// The error names.\n",
                "/*\n/// */ /** The error names. */ /*\n*/\n",
            ),
            three.to_owned(),
        ),
        (
            changed("/// The error names.\n", "/*\n/// The error names.\n*/\n"),
            three.to_owned(),
        ),
        (
            format!("{USE_LINE}\n\n/*\n/// X\n*/\n/** X*/\npub const KINDS: &[KindEntry] = &[];\npub const ERROR_NAMES: &[&str] = &[];\n"),
            three.to_owned(),
        ),
        (
            changed("/// The error names.\n", "/** The error names. */\n"),
            three.to_owned(),
        ),
        (
            changed(&format!("{names};"), &format!("{names}; /// Trailing.")),
            three.to_owned(),
        ),
        (
            changed("/// The error names.\n", "#[cfg(all())]\n"),
            attribute.to_owned(),
        ),
        (
            changed(
                "/// The error names.\n",
                "#[doc = include_str!(\"names.txt\")]\n",
            ),
            attribute.to_owned(),
        ),
        (
            changed("/// The error names.\n", "#![allow(unused)]\n"),
            attribute.to_owned(),
        ),
        (
            changed("/// The error names.\n", "//! The error names.\n"),
            attribute.to_owned(),
        ),
        (
            format!("{LIST_FILE}pub fn later() {{}}\n"),
            outside(&trees("pub fn later ()")),
        ),
        (
            format!("{LIST_FILE}mod names;\npub use names::*;\n"),
            outside(&trees("mod names ; pub")),
        ),
        (
            format!("{LIST_FILE}macro_rules! listed {{\n    () => {{}};\n}}\n"),
            outside(&trees("macro_rules ! listed { () => { } ; }")),
        ),
        (
            changed(
                &format!("pub const ERROR_NAMES: &[&str] = {names};"),
                "listed! {\n    pub const ERROR_NAMES: &[&str] = &[\"learned\"];\n}",
            ),
            outside(&trees(
                "listed ! { pub const ERROR_NAMES : & [& str] = & [\"learned\"] ; } #",
            )),
        ),
    ] {
        assert_eq!(list_file(&text).err(), Some(fault), "{text}");
    }
}

const FIXTURE_LISTS: [(&str, &str); 4] = [
    ("executable", "&[\"fixture_a\", r#\"fixture_b\"#]"),
    ("coordinator", "&[\"fixture_c\"]"),
    ("learning", "&[\"fixture_d\", \"fixture_e\"]"),
    ("routing", "&[r\"fixture_f\"]"),
];

fn list_text(names: &str) -> String {
    format!("{USE_LINE}\n\npub const KINDS: &[KindEntry] = &[];\npub const ERROR_NAMES: &[&str] = {names};\n")
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap_or_else(|e| panic!("{}: {e}", to.display()));
    let entries = std::fs::read_dir(from).unwrap_or_else(|e| panic!("{}: {e}", from.display()));
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        let target = to.join(path.file_name().expect("a file name"));
        if path.is_dir() {
            copy_tree(&path, &target);
        } else {
            std::fs::copy(&path, &target).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }
}

fn names_read_by_a_build(lists: &[(String, String)]) -> Vec<String> {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("declared-names");
    let dir = root.join("crate");
    copy_tree(&core_dir().join("src"), &dir.join("src"));
    for (module, text) in lists {
        std::fs::write(dir.join("src").join(format!("{module}.rs")), text).expect("a list file");
    }
    let manifest = read(&core_dir().join("Cargo.toml"));
    assert_eq!(manifest.matches("name = \"halvo-schema-core\"").count(), 1);
    let manifest = manifest.replacen(
        "name = \"halvo-schema-core\"",
        "name = \"declared-names\"",
        1,
    ) + "\n[workspace]\n";
    std::fs::write(dir.join("Cargo.toml"), manifest).expect("the manifest of the fixture");
    std::fs::copy(
        schema_dir().join("../../Cargo.lock"),
        dir.join("Cargo.lock"),
    )
    .expect("the root Cargo.lock");
    std::fs::create_dir_all(dir.join("examples")).expect("a directory");
    std::fs::write(
        dir.join("examples").join("names.rs"),
        "fn main() {\n    for name in declared_names::kinds::declared_error_names() {\n        println!(\"{name}\");\n    }\n}\n",
    )
    .expect("the example");
    let out = Command::new(env!("CARGO"))
        .args(["run", "--offline", "--quiet", "--example", "names"])
        .current_dir(&dir)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_BUILD_RUSTFLAGS")
        .output()
        .expect("cargo starts");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

fn names_faults(read: &[String], lists: &[(String, String)]) -> Vec<String> {
    let mut listed = Vec::new();
    for (module, text) in lists {
        match list_file(text) {
            Ok(file) => listed.extend(file.error_names),
            Err(fault) => return vec![format!("{module}.rs {fault}")],
        }
    }
    if listed == read {
        Vec::new()
    } else {
        vec![format!(
            "the crate reads the names {read:?}, and the four list files hold {listed:?}"
        )]
    }
}

#[test]
fn declared_error_names_yields_the_literals_that_the_four_later_files_list_in_order() {
    let files: Vec<(String, String)> = LATER
        .iter()
        .map(|module| ((*module).to_owned(), source(&format!("{module}.rs"))))
        .collect();
    let read: Vec<String> = halvo_schema::kinds::declared_error_names()
        .map(str::to_owned)
        .collect();
    assert_eq!(names_faults(&read, &files), Vec::<String>::new());
    let lists: Vec<(String, String)> = FIXTURE_LISTS
        .iter()
        .map(|(module, names)| ((*module).to_owned(), list_text(names)))
        .collect();
    let read = names_read_by_a_build(&lists);
    assert_eq!(
        read,
        [
            "fixture_a",
            "fixture_b",
            "fixture_c",
            "fixture_d",
            "fixture_e",
            "fixture_f"
        ]
    );
    assert_eq!(names_faults(&read, &lists), Vec::<String>::new());
    let with = |module: &str, old: &str, new: &str| -> Vec<(String, String)> {
        lists
            .iter()
            .map(|(m, text)| {
                if m == module {
                    assert_eq!(text.matches(old).count(), 1, "{old}");
                    (m.clone(), text.replacen(old, new, 1))
                } else {
                    (m.clone(), text.clone())
                }
            })
            .collect()
    };
    let another_value = with("coordinator", "\"fixture_c\"", "\"fixture_x\"");
    assert_eq!(
        names_faults(&read, &another_value),
        [format!("the crate reads the names {read:?}, and the four list files hold [\"fixture_a\", \"fixture_b\", \"fixture_x\", \"fixture_d\", \"fixture_e\", \"fixture_f\"]")]
    );
    let another_order = with(
        "learning",
        "\"fixture_d\", \"fixture_e\"",
        "\"fixture_e\", \"fixture_d\"",
    );
    assert_eq!(
        names_faults(&read, &another_order),
        [format!("the crate reads the names {read:?}, and the four list files hold [\"fixture_a\", \"fixture_b\", \"fixture_c\", \"fixture_e\", \"fixture_d\", \"fixture_f\"]")]
    );
}

#[test]
fn each_later_file_re_exports_its_two_lists_at_their_old_paths() {
    assert_eq!(
        halvo_schema::executable::KINDS,
        halvo_schema_core::executable::KINDS
    );
    assert_eq!(
        halvo_schema::executable::ERROR_NAMES,
        halvo_schema_core::executable::ERROR_NAMES
    );
    assert_eq!(
        halvo_schema::coordinator::KINDS,
        halvo_schema_core::coordinator::KINDS
    );
    assert_eq!(
        halvo_schema::coordinator::ERROR_NAMES,
        halvo_schema_core::coordinator::ERROR_NAMES
    );
    assert_eq!(
        halvo_schema::learning::KINDS,
        halvo_schema_core::learning::KINDS
    );
    assert_eq!(
        halvo_schema::learning::ERROR_NAMES,
        halvo_schema_core::learning::ERROR_NAMES
    );
    assert_eq!(
        halvo_schema::routing::KINDS,
        halvo_schema_core::routing::KINDS
    );
    assert_eq!(
        halvo_schema::routing::ERROR_NAMES,
        halvo_schema_core::routing::ERROR_NAMES
    );
    for module in LATER {
        let text = read(&schema_dir().join("src").join(format!("{module}.rs")));
        assert_eq!(old_path_faults(module, &text), Vec::<String>::new());
    }
    let line = "pub use ::halvo_schema_core::learning::{ERROR_NAMES, KINDS};\n";
    let old = "pub use halvo_schema_core::learning::{ERROR_NAMES, KINDS};\n";
    let fake = "pub mod fake;\nmod halvo_schema_core {\n    pub use super::fake as learning;\n}\n";
    let own =
        "pub const KINDS: &[crate::KindEntry] = &[];\npub const ERROR_NAMES: &[&str] = &[];\n";
    let file = "halvo-schema/src/learning.rs";
    let no_line = format!(
        "{file} does not hold the line `{}` once at the top level",
        line.trim_end()
    );
    let attribute = format!("{file} puts an attribute on the line `{}`", line.trim_end());
    let named = format!(
        "{file} names KINDS or ERROR_NAMES outside the line `{}`",
        line.trim_end()
    );
    for (text, faults) in [
        (own.to_owned(), vec![no_line.clone()]),
        (String::new(), vec![no_line.clone(), named.clone()]),
        (format!("#[cfg(any())]\n{line}"), vec![attribute.clone()]),
        (format!("#[cfg(any())]\n{line}{own}"), vec![attribute.clone(), named.clone()]),
        (
            format!("#[cfg(any())]\n{line}pub const KINDS: &[crate::KindEntry; 0] = &[];\npub const ERROR_NAMES: &[&str; 0] = &[];\n"),
            vec![attribute.clone(), named.clone()],
        ),
        (
            format!("mod inner {{\n    {line}}}\npub use inner::*;\n"),
            vec![no_line.clone()],
        ),
        (
            format!("{line}mod other {{\n    pub const KINDS: u8 = 0;\n}}\n"),
            vec![named.clone()],
        ),
        (
            format!("{line}pub use crate::kinds::KindEntry as r#ERROR_NAMES;\n"),
            vec![named.clone()],
        ),
        (format!("{line}{line}"), vec![no_line.clone(), named.clone()]),
        (old.to_owned(), vec![no_line.clone()]),
        (
            format!("{old}mod halvo_schema_core {{\n    pub use ::halvo_schema_core::routing as learning;\n}}\n"),
            vec![no_line.clone()],
        ),
        (format!("{old}{fake}"), vec![no_line.clone()]),
    ] {
        assert_eq!(old_path_faults("learning", &text), faults, "{text}");
    }
    for twin in [
        format!(
            "/// The lists of the learning run.\npub mod more {{}}\n{line}pub fn shown() {{}}\n"
        ),
        format!("#![allow(unused)]\n{line}"),
        format!("{line}{fake}"),
    ] {
        assert_eq!(
            old_path_faults("learning", &twin),
            Vec::<String>::new(),
            "{twin}"
        );
    }
}

fn old_path_faults(module: &str, text: &str) -> Vec<String> {
    let file = format!("halvo-schema/src/{module}.rs");
    let top: Vec<TokenTree> = match TokenStream::from_str(text) {
        Ok(stream) => stream.into_iter().collect(),
        Err(cause) => return vec![unread(&file, &cause.to_string())],
    };
    let line = format!("pub use ::halvo_schema_core::{module}::{{ERROR_NAMES, KINDS}};");
    let want = trees(&line);
    let at: Vec<usize> = (0..top.len())
        .filter(|&i| top.len() >= i + want.len() && flat(&top[i..i + want.len()]) == flat(&want))
        .collect();
    let mut faults = Vec::new();
    if at.len() != 1 {
        faults.push(format!(
            "{file} does not hold the line `{line}` once at the top level"
        ));
    }
    if at.iter().any(|&i| {
        i >= 2 && is_punct(top.get(i - 2), '#') && inner(&top[i - 1], Delimiter::Bracket).is_some()
    }) {
        faults.push(format!("{file} puts an attribute on the line `{line}`"));
    }
    let all = flat(&top);
    let named = |name: &str| all.iter().filter(|t| bare(t) == name).count();
    if named("KINDS") != 1 || named("ERROR_NAMES") != 1 {
        faults.push(format!(
            "{file} names KINDS or ERROR_NAMES outside the line `{line}`"
        ));
    }
    faults
}
