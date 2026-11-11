use std::path::{Path, PathBuf};
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

fn source(file: &str) -> String {
    let path = format!("{}/src/{file}", env!("CARGO_MANIFEST_DIR"));
    match std::fs::read_to_string(&path) {
        Ok(text) => text.replace("\r\n", "\n"),
        Err(e) => panic!("{path}: {e}"),
    }
}

fn starts_with_a_shebang_line(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("#!") else {
        return false;
    };
    let first = TokenStream::from_str(rest)
        .ok()
        .and_then(|stream| stream.into_iter().next());
    !matches!(first, Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Bracket)
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

const STD_MACROS: [&str; 7] = [
    "concat",
    "format",
    "matches",
    "stringify",
    "vec",
    "write",
    "forward_to_deserialize_any",
];

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

fn macro_names(t: &[String]) -> Vec<String> {
    (0..t.len())
        .filter(|&i| bare(&t[i]) == "macro_rules" && t.get(i + 1).is_some_and(|x| x == "!"))
        .filter_map(|i| t.get(i + 2).map(|name| bare(name).to_owned()))
        .collect()
}

fn locked_macros() -> Vec<String> {
    LOCKED
        .iter()
        .flat_map(|file| macro_names(&tokens(&source(file))))
        .collect()
}

fn root_names() -> Vec<String> {
    let mut names: Vec<String> = LOCKED
        .iter()
        .filter(|file| **file != "lib.rs")
        .map(|file| file.trim_end_matches(".rs").to_owned())
        .chain(LATER.iter().map(|module| (*module).to_owned()))
        .collect();
    let t = tokens(&source("lib.rs"));
    for at in find(&t, &["use"]) {
        for token in t[at + 1..].iter().take_while(|x| *x != ";") {
            if is_name(token) {
                names.push(bare(token).to_owned());
            }
        }
    }
    names
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

fn reach_faults(file: &str, t: &[String]) -> Vec<String> {
    let inside = in_an_attribute_or_a_macro(t);
    let mut faults = Vec::new();
    for (i, token) in t.iter().enumerate() {
        if !is_ident(token) {
            continue;
        }
        let name = bare(token);
        let fragment = i >= 3 && t[i - 1] == ":" && is_ident(&t[i - 2]) && t[i - 3] == "$";
        if name == "path" && inside[i] && !fragment {
            faults.push(format!("{file} holds path in an attribute or a macro"));
        }
        if INCLUDE.contains(&name) {
            faults.push(format!("{file} names {name}"));
        }
    }
    faults
}

fn called_through_an_argument(t: &[String], name_at: usize) -> bool {
    let mut j = name_at;
    while j >= 3 && t[j - 1] == ":" && t[j - 2] == ":" && is_ident(&t[j - 3]) {
        j -= 3;
    }
    j >= 1 && t[j - 1] == "$" && bare(&t[j]) != "crate"
}

fn lock_faults(file: &str, text: &str, macros: &[String], root: &[String]) -> Vec<String> {
    let t = match lex(text) {
        Ok(t) => t,
        Err(cause) => return vec![unread(file, &cause)],
    };
    let own = macro_names(&t);
    let mut faults = reach_faults(file, &t);
    for (i, token) in t.iter().enumerate() {
        let name = bare(token);
        let next = t.get(i + 1).map(String::as_str);
        if is_ident(token) && name == "macro_use" {
            faults.push(format!("{file} holds macro_use"));
        }
        let call = is_name(token) && next == Some("!") && is_open(t.get(i + 2));
        if call
            && !INCLUDE.contains(&name)
            && !macros.iter().chain(&own).any(|m| m == name)
            && !STD_MACROS.contains(&name)
        {
            faults.push(format!(
                "{file} calls the macro {name}, which no locked file defines"
            ));
        }
        if call && called_through_an_argument(&t, i) {
            faults.push(format!("{file} calls a macro through a macro's argument"));
        }
        if is_ident(token)
            && matches!(name, "crate" | "super")
            && next == Some(":")
            && t.get(i + 2).is_some_and(|x| x == ":")
        {
            let items: Vec<String> = match t.get(i + 3).map(String::as_str) {
                Some("{") => split_at_commas(&t[i + 4..matching(&t, i + 3)])
                    .into_iter()
                    .map(|part| bare(&part[0]).to_owned())
                    .collect(),
                Some(item) => vec![bare(item).to_owned()],
                None => Vec::new(),
            };
            for item in items {
                if item != "self" && !root.contains(&item) {
                    faults.push(format!(
                        "{file} names {name}::{item}, which is no module of the crate and no re-export of lib.rs"
                    ));
                }
            }
        }
        if file == "lib.rs" && is_ident(token) && name == "use" {
            let from = t.get(i + 1).map_or("", |x| bare(x));
            if !LOCKED.contains(&format!("{from}.rs").as_str()) {
                faults.push(format!("lib.rs re-exports from {from}"));
            }
        }
    }
    faults
}

#[test]
fn the_lock_test_refuses_a_path_attribute_include_macro_use_and_a_macro_that_no_locked_file_defines(
) {
    let macros = locked_macros();
    let root = root_names();
    for name in ["name_enum", "strict_struct", "string_type", "coarse_types"] {
        assert!(macros.iter().any(|m| m == name), "{name}");
    }
    for name in ["ids", "kinds", "learning", "Coarse", "RowRead", "SiteValue"] {
        assert!(root.iter().any(|m| m == name), "{name}");
    }
    for (text, fault) in [
        (
            "#[path = \"learning.rs\"]\nmod sneak_path;\n",
            "site.rs holds path in an attribute or a macro",
        ),
        (
            "#[cfg_attr(all(), path = \"learning.rs\")]\nmod sneak_path;\n",
            "site.rs holds path in an attribute or a macro",
        ),
        (
            "mod sneak {\n    include!(\"learning.rs\");\n}\n",
            "site.rs names include",
        ),
        (
            "mod sneak {\n    use core::include as grab;\n    grab!(\"executable.rs\");\n}\n",
            "site.rs names include",
        ),
        (
            "fn sneak_len() -> usize {\n    crate::sneak_macro!()\n}\n",
            "site.rs calls the macro sneak_macro",
        ),
        (
            "fn sneak_len() -> usize {\n    crate::r#sneak!()\n}\n",
            "site.rs calls the macro sneak",
        ),
        (
            "fn sneak_len() -> usize {\n    sneak_macro![]\n}\n",
            "site.rs calls the macro sneak_macro",
        ),
        (
            "fn sneak_len() -> usize {\n    hidden!()\n}\n",
            "site.rs calls the macro hidden",
        ),
        (
            "use crate::{ids, sneak_macro as m};\n",
            "site.rs names crate::sneak_macro",
        ),
        (
            "use crate::sneak_macro as format;\nfn f() -> String {\n    format!(\"x\")\n}\n",
            "site.rs names crate::sneak_macro",
        ),
        (
            "fn sneak_len() -> usize {\n    super::sneak!()\n}\n",
            "site.rs names super::sneak",
        ),
        (
            "#[macro_use]\npub mod learning;\n",
            "site.rs holds macro_use",
        ),
    ] {
        let faults = lock_faults("site.rs", text, &macros, &root);
        assert!(
            faults.iter().any(|f| f.starts_with(fault)),
            "{text}: {faults:?}"
        );
    }
    assert_eq!(
        lock_faults("lib.rs", "pub use sneak::Hidden;\n", &macros, &root),
        ["lib.rs re-exports from sneak"]
    );
    for twin in [
        "let path = 1;\n",
        "fn path(p: &str) -> &str {\n    p\n}\n",
        "/// #[path = \"learning.rs\"] include!(\"learning.rs\") sneak_macro!() #[macro_use]\nfn f() {}\n",
        "let sneak_macro = \"sneak_macro!()\";\n",
        "macro_rules! own {\n    () => {};\n}\nown!();\n",
        "fn f() -> String {\n    format!(\"{}\", concat!(\"a\", \"b\"))\n}\n",
        "use crate::ids::{name_enum, ParseError};\nuse crate::{kinds, record::Summary};\npub(crate) use name_enum;\n",
        "fn f(_: &dyn crate::RowRead) -> bool {\n    !(1 == 2) && matches!(1, 1)\n}\n",
        "fn g(a: bool) -> bool {\n    if !(a) {\n        return !(a);\n    }\n    a\n}\n",
        "macro_rules! typed {\n    ($check:path) => {};\n}\n",
    ] {
        assert_eq!(
            lock_faults("site.rs", twin, &macros, &root),
            Vec::<String>::new(),
            "{twin}"
        );
    }
}

#[test]
fn the_lock_test_refuses_path_among_the_tokens_of_a_macro_and_a_macro_called_through_an_argument() {
    let macros = locked_macros();
    let root = root_names();
    for (text, fault) in [
        (
            "macro_rules! own_mod {\n    ($m:meta) => {\n        #[$m]\n        mod sneak;\n    };\n}\nown_mod! {path = \"learning.rs\"}\n",
            "site.rs holds path in an attribute or a macro",
        ),
        (
            "macro_rules! own_mod {\n    ($a:ident $b:literal) => {\n        #[$a = $b]\n        mod sneak;\n    };\n}\nown_mod!(r#path \"learning.rs\");\n",
            "site.rs holds path in an attribute or a macro",
        ),
        (
            "macro_rules! own_mod {\n    () => {\n        #[path = \"learning.rs\"]\n        mod sneak;\n    };\n}\nown_mod!();\n",
            "site.rs holds path in an attribute or a macro",
        ),
        (
            "macro_rules! call {\n    ($vec:ident, $f:literal) => {\n        $vec!($f)\n    };\n}\npub const LATER_TEXT: &str = call!(vec, \"learning.rs\");\n",
            "site.rs calls a macro through a macro's argument",
        ),
        (
            "macro_rules! call {\n    ($m:ident) => {\n        $m::sneak![]\n    };\n}\n",
            "site.rs calls a macro through a macro's argument",
        ),
        (
            "pub const LATER_TEXT: &str = include_str!(\"learning.rs\");\n",
            "site.rs names include_str",
        ),
        (
            "pub const LATER_BYTES: &[u8] = core::include_bytes!(\"learning.rs\");\n",
            "site.rs names include_bytes",
        ),
    ] {
        let faults = lock_faults("site.rs", text, &macros, &root);
        assert!(faults.iter().any(|f| f == fault), "{text}: {faults:?}");
    }
    assert_eq!(
        lock_faults(
            "ids.rs",
            "macro_rules! own {\n    () => {\n        $crate::ids::own_too!()\n    };\n}\nmacro_rules! own_too {\n    () => {};\n}\n",
            &macros,
            &root
        ),
        Vec::<String>::new()
    );
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
        } else if path.extension().is_some_and(|x| x == "rs") {
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
            if LOCKED.contains(&name.as_str()) {
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

fn later_faults(file: &str, text: &str) -> Vec<String> {
    let t = match lex(text) {
        Ok(t) => t,
        Err(cause) => return vec![unread(file, &cause)],
    };
    let mut faults = reach_faults(file, &t);
    faults.extend(
        t.iter()
            .filter(|token| is_ident(token) && bare(token) == "macro_export")
            .map(|_| format!("{file} holds macro_export")),
    );
    faults
}

#[test]
fn no_file_of_a_later_run_holds_macro_export() {
    let files = later_files();
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    for module in LATER {
        assert!(
            names.contains(&format!("{module}.rs").as_str()),
            "{names:?}"
        );
    }
    for (file, text) in &files {
        assert_eq!(later_faults(file, text), Vec::<String>::new());
    }
}

#[test]
fn the_walk_reads_every_rust_file_of_src_outside_the_lock_at_any_depth() {
    let src = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("walk")
        .join("src");
    let written = [
        ("lib.rs", ""),
        ("ids.rs", ""),
        ("category.rs", ""),
        ("site.rs", ""),
        ("record.rs", ""),
        ("request.rs", ""),
        ("kinds.rs", ""),
        ("kinds/inner.rs", "pub fn shown() {}\n"),
        ("learning.rs", "pub const KINDS: &[u8] = &[];\n"),
        ("learning/more.rs", "pub fn shown() {}\n"),
        (
            "learning/deeper/most.rs",
            "#[macro_export]\nmacro_rules! later {\n    () => {};\n}\n",
        ),
        ("other.rs", "pub fn shown() {}\n"),
        ("bin/tool.rs", "fn main() {}\n"),
        ("learning/macros.inc", "#[macro_export]\n"),
        ("routing/notes.txt", "notes\n"),
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
            "kinds/inner.rs",
            "learning.rs",
            "learning/deeper/most.rs",
            "learning/more.rs",
            "other.rs"
        ]
    );
    let faults: Vec<String> = files
        .iter()
        .flat_map(|(name, text)| later_faults(name, text))
        .collect();
    assert_eq!(faults, ["learning/deeper/most.rs holds macro_export"]);
}

#[test]
fn the_test_of_the_later_files_refuses_macro_export_in_every_form() {
    for text in [
        "#[macro_export]\nmacro_rules! name_enum {\n    () => {};\n}\n",
        "#[cfg_attr(all(), macro_export)]\nmacro_rules! own {\n    () => {};\n}\n",
        "macro_rules! make {\n    ($n:ident) => {\n        #[macro_export]\n        macro_rules! $n {\n            () => {};\n        }\n    };\n}\nmake!(hidden);\n",
        "#[r#macro_export]\nmacro_rules! own {\n    () => {};\n}\n",
    ] {
        assert_eq!(
            later_faults("learning.rs", text),
            ["learning.rs holds macro_export"],
            "{text}"
        );
    }
    for twin in [
        "/// #[macro_export]\nfn f() {}\n",
        "let s = \"#[macro_export]\";\n",
        "macro_rules! own {\n    () => {};\n}\nown!();\n",
        "#[macro_use]\nmod inner {}\n",
    ] {
        assert_eq!(
            later_faults("learning.rs", twin),
            Vec::<String>::new(),
            "{twin}"
        );
    }
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
        "learning.rs holds macro_export",
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
    let macros = locked_macros();
    let root = root_names();
    let first_line =
        "its first line is a shebang line, which the compiler skips and the lexer reads";
    for text in [shebang, backtick, &format!("\u{feff}{shebang}")] {
        assert_eq!(
            later_faults("learning.rs", text),
            [unread("learning.rs", first_line)]
        );
        assert_eq!(
            lock_faults("site.rs", text, &macros, &root),
            [unread("site.rs", first_line)]
        );
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
        let (want, form) = match file {
            "kinds.rs" => (1, vec!["crate", ":", ":", module, ":", ":", "KINDS"]),
            "lib.rs" => (1, vec!["pub", "mod", module, ";"]),
            _ => (0, Vec::new()),
        };
        if named != want {
            faults.push(format!("{file} names {module} {named} times"));
        }
        if want == 1 && find(&t, &form).len() != 1 {
            faults.push(format!(
                "{file} does not name {module} as `{}` once",
                form.join(" ")
            ));
        }
    }
    faults
}

#[test]
fn no_locked_file_names_an_item_of_the_files_of_later_runs_but_their_kind_lists() {
    let macros = locked_macros();
    let root = root_names();
    for file in LOCKED {
        let text = source(file);
        assert_eq!(
            lock_faults(file, &text, &macros, &root),
            Vec::<String>::new()
        );
        assert_eq!(later_names_faults(file, &text), Vec::<String>::new());
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
