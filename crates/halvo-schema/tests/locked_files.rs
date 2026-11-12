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
            "macro_rules! own_mod {\n    ($a:ident : $($m:tt)*) => {\n        #[$($m)*]\n        mod sneak;\n    };\n}\nown_mod! {a: path = \"learning.rs\"}\n",
            "site.rs holds path in an attribute or a macro",
        ),
        (
            "macro_rules! own_mod {\n    ($d:tt $a:ident : $($m:tt)*) => {\n        #[$($m)*]\n        mod sneak;\n    };\n}\nown_mod! {$ a: path = \"learning.rs\"}\n",
            "site.rs holds path in an attribute or a macro",
        ),
        (
            "macro_rules! inner {\n    ($x:ident : $($m:tt)*) => {\n        #[$($m)*]\n        mod sneak;\n    };\n}\nmacro_rules! outer {\n    ($a:ident) => {\n        inner! {$a: path = \"learning.rs\"}\n    };\n}\nouter!(a);\n",
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

const MODULES: [&str; 6] = ["ids", "category", "site", "record", "request", "kinds"];

const WRAPPERS: [&str; 2] = ["Box", "Pin"];

fn macro_bodies(t: &[String]) -> Vec<(String, usize, usize)> {
    (0..t.len())
        .filter(|&i| {
            bare(&t[i]) == "macro_rules"
                && t.get(i + 1).is_some_and(|x| x == "!")
                && t.get(i + 2).is_some_and(|x| is_name(x))
                && is_open(t.get(i + 3))
        })
        .map(|i| (bare(&t[i + 2]).to_owned(), i + 3, matching(t, i + 3)))
        .collect()
}

fn declares_a_fragment(body: &[String]) -> bool {
    (0..body.len()).any(|i| {
        matches!(body[i].as_str(), "struct" | "enum" | "union")
            && body.get(i + 1).is_some_and(|x| x == "$")
    })
}

fn first_name_of_a_call(t: &[String], open: usize) -> Option<String> {
    let close = matching(t, open);
    let mut j = open + 1;
    while j < close {
        if t[j] == "#" && t.get(j + 1).is_some_and(|x| x == "[") {
            j = matching(t, j + 1) + 1;
            continue;
        }
        if is_name(&t[j]) {
            return Some(bare(&t[j]).to_owned());
        }
        j += 1;
    }
    None
}

struct Locked {
    types: Vec<(String, String)>,
    root: Vec<String>,
}

fn locked_types() -> Locked {
    let files: Vec<(&str, Vec<String>)> = LOCKED.iter().map(|f| (*f, tokens(&source(f)))).collect();
    let mut declaring: Vec<String> = Vec::new();
    loop {
        let before = declaring.len();
        for (_, t) in &files {
            for (name, open, close) in macro_bodies(t) {
                let body = &t[open..close];
                let calls_one = (0..body.len()).any(|i| {
                    declaring.iter().any(|d| *d == bare(&body[i]))
                        && body.get(i + 1).is_some_and(|x| x == "!")
                });
                if !declaring.contains(&name) && (declares_a_fragment(body) || calls_one) {
                    declaring.push(name);
                }
            }
        }
        if declaring.len() == before {
            break;
        }
    }
    let mut types = Vec::new();
    let mut root = Vec::new();
    for (file, t) in &files {
        let module = file.trim_end_matches(".rs").to_owned();
        if *file == "lib.rs" {
            for at in find(t, &["use"]) {
                let from = t.get(at + 1).map_or("", |x| bare(x)).to_owned();
                for token in t[at + 2..].iter().take_while(|x| *x != ";") {
                    if is_name(token) {
                        types.push((from.clone(), bare(token).to_owned()));
                        root.push(bare(token).to_owned());
                    }
                }
            }
            continue;
        }
        for i in 0..t.len() {
            if matches!(
                t[i].as_str(),
                "struct" | "enum" | "union" | "type" | "trait"
            ) {
                if let Some(name) = t.get(i + 1).filter(|x| is_name(x)) {
                    types.push((module.clone(), bare(name).to_owned()));
                }
            }
            let call =
                is_name(&t[i]) && t.get(i + 1).is_some_and(|x| x == "!") && is_open(t.get(i + 2));
            if call && declaring.iter().any(|d| *d == bare(&t[i])) {
                if let Some(name) = first_name_of_a_call(t, i + 2) {
                    types.push((module.clone(), name));
                }
            }
        }
    }
    Locked { types, root }
}

fn module_of(file: &str) -> Vec<String> {
    let stem = file.trim_end_matches(".rs").trim_end_matches(".RS");
    let mut parts: Vec<String> = stem.split('/').map(str::to_owned).collect();
    if parts.last().is_some_and(|p| p == "mod") {
        parts.pop();
    }
    parts
}

fn module_paths(t: &[String], inside: &[bool], file_module: &[String]) -> Vec<Vec<String>> {
    let mut paths = vec![Vec::new(); t.len()];
    let mut open: Vec<(String, usize)> = Vec::new();
    for i in 0..t.len() {
        while open.last().is_some_and(|(_, close)| i > *close) {
            open.pop();
        }
        let mut path = file_module.to_vec();
        path.extend(open.iter().map(|(name, _)| name.clone()));
        paths[i] = path;
        if !inside[i]
            && t[i] == "mod"
            && t.get(i + 1).is_some_and(|x| is_name(x))
            && t.get(i + 2).is_some_and(|x| x == "{")
        {
            open.push((bare(&t[i + 1]).to_owned(), matching(t, i + 2)));
        }
    }
    paths
}

type Segments = Vec<String>;

type Leaf = (Option<String>, Segments, bool);

fn type_path(t: &[String]) -> Segments {
    let mut i = 0;
    loop {
        match t.get(i).map(String::as_str) {
            Some("&" | "mut" | "dyn") => i += 1,
            Some("'") => i += 2,
            _ => break,
        }
    }
    let mut path = Vec::new();
    loop {
        match t.get(i).map(String::as_str) {
            Some("$") => {
                path.push("$".to_owned());
                i += 1;
            }
            Some(x) if is_ident(x) => {
                path.push(bare(x).to_owned());
                i += 1;
                if t.get(i).is_some_and(|x| x == ":") && t.get(i + 1).is_some_and(|x| x == ":") {
                    i += 2;
                    continue;
                }
                break;
            }
            _ => break,
        }
    }
    if path.len() == 1 && WRAPPERS.contains(&path[0].as_str()) && t.get(i).is_some_and(|x| x == "<")
    {
        return type_path(&t[i + 1..]);
    }
    path
}

fn use_tree(t: &[String], i: &mut usize, prefix: &[String], out: &mut Vec<Leaf>) {
    let mut path = prefix.to_vec();
    loop {
        match t.get(*i).map(String::as_str) {
            Some("{") => {
                let close = matching(t, *i);
                *i += 1;
                while *i < close {
                    use_tree(t, i, &path, out);
                    if *i < close {
                        *i += 1;
                    }
                }
                *i = close + 1;
                return;
            }
            Some("*") => {
                out.push((None, path, true));
                *i += 1;
                return;
            }
            Some(":") | Some("$") => {
                if t[*i] == "$" {
                    path.push("$".to_owned());
                }
                *i += 1;
            }
            Some(x) if is_ident(x) => {
                path.push(bare(x).to_owned());
                *i += 1;
                if t.get(*i).is_some_and(|x| x == "as") {
                    let alias = t.get(*i + 1).map(|a| bare(a).to_owned());
                    *i += 2;
                    if path.last().is_some_and(|p| p == "self") {
                        path.pop();
                    }
                    out.push((alias, path, false));
                    return;
                }
                if !t.get(*i).is_some_and(|x| x == ":") {
                    if path.last().is_some_and(|p| p == "self") {
                        path.pop();
                    }
                    let alias = path.last().cloned();
                    out.push((alias, path, false));
                    return;
                }
            }
            _ => return,
        }
    }
}

#[derive(Default)]
struct Tree {
    declared: Vec<(Segments, String)>,
    aliases: Vec<(Segments, String, Segments)>,
    globs: Vec<(Segments, Segments, bool)>,
    selves: Vec<String>,
    modules: Vec<(Segments, String)>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Place {
    Locked,
    Own,
    Other,
}

struct Lexed {
    file: String,
    t: Vec<String>,
    matcher: Vec<bool>,
    paths: Vec<Segments>,
    bodies: Vec<(String, usize, usize)>,
}

impl Lexed {
    fn body_of(&self, i: usize) -> Option<&(String, usize, usize)> {
        self.bodies
            .iter()
            .find(|(_, open, close)| *open < i && i < *close)
    }

    fn calls(&self, name: &str) -> Vec<(Segments, usize, usize)> {
        (0..self.t.len())
            .filter(|&i| {
                bare(&self.t[i]) == name
                    && self.t.get(i + 1).is_some_and(|x| x == "!")
                    && is_open(self.t.get(i + 2))
                    && self.body_of(i).is_none()
            })
            .map(|i| (self.paths[i].clone(), i + 2, matching(&self.t, i + 2)))
            .collect()
    }
}

fn uses_in(t: &[String], from: usize, to: usize) -> Vec<(usize, Vec<Leaf>)> {
    let mut out = Vec::new();
    for i in from..to {
        if t[i] == "use" {
            let mut j = i + 1;
            let mut leaves = Vec::new();
            use_tree(t, &mut j, &[], &mut leaves);
            out.push((i, leaves));
        }
    }
    out
}

fn arguments_of(t: &[String], open: usize, close: usize) -> Vec<Segments> {
    let mut out = Vec::new();
    let mut j = open + 1;
    while j < close {
        let starts = is_ident(&t[j]) || (t[j] == "$" && t.get(j + 1).is_some_and(|x| x == "crate"));
        let after_colon = j > 0 && t[j - 1] == ":" && j > 1 && t[j - 2] == ":";
        if starts && !after_colon {
            let path = type_path(&t[j..close]);
            if !path.is_empty() {
                out.push(path);
            }
        }
        j += 1;
    }
    out
}

fn build_tree(lexed: &[Lexed]) -> Tree {
    let mut tree = Tree::default();
    for l in lexed {
        let t = &l.t;
        for i in 0..t.len() {
            if t[i] == "extern"
                && t.get(i + 1).is_some_and(|x| x == "crate")
                && t.get(i + 2).is_some_and(|x| x == "self")
                && t.get(i + 3).is_some_and(|x| x == "as")
            {
                if let Some(alias) = t.get(i + 4) {
                    tree.selves.push(bare(alias).to_owned());
                }
            }
            if l.body_of(i).is_some() {
                continue;
            }
            if t[i] == "mod"
                && t.get(i + 1).is_some_and(|x| is_name(x))
                && t.get(i + 2).is_some_and(|x| x == ";" || x == "{")
            {
                tree.modules
                    .push((l.paths[i].clone(), bare(&t[i + 1]).to_owned()));
            }
            if matches!(t[i].as_str(), "struct" | "enum" | "union" | "trait") {
                if let Some(name) = t.get(i + 1).filter(|x| is_name(x)) {
                    tree.declared
                        .push((l.paths[i].clone(), bare(name).to_owned()));
                }
            }
            if t[i] == "type" && t.get(i + 1).is_some_and(|x| is_name(x)) {
                let end = (i..t.len()).find(|&k| t[k] == ";").unwrap_or(t.len());
                if let Some(eq) = (i..end).find(|&k| t[k] == "=") {
                    tree.aliases.push((
                        l.paths[i].clone(),
                        bare(&t[i + 1]).to_owned(),
                        type_path(&t[eq + 1..end]),
                    ));
                }
            }
        }
        for (at, leaves) in uses_in(t, 0, t.len()) {
            if l.body_of(at).is_some() {
                continue;
            }
            for (alias, path, glob) in leaves {
                if glob {
                    tree.globs
                        .push((l.paths[at].clone(), path, in_a_block(t, at)));
                } else if let Some(alias) = alias.filter(|a| a != "_") {
                    tree.aliases.push((l.paths[at].clone(), alias, path));
                }
            }
        }
    }
    for l in lexed {
        for (name, open, close) in &l.bodies {
            let body = &l.t[*open..*close];
            let declares = declares_a_fragment(body);
            let mut alias_targets = Vec::new();
            let mut literal_aliases = Vec::new();
            let mut glob_targets = Vec::new();
            for (_, leaves) in uses_in(&l.t, *open, *close) {
                for (alias, path, glob) in leaves {
                    match alias {
                        _ if glob => glob_targets.push(path),
                        Some(a) if a == "$" => alias_targets.push(path),
                        Some(a) if a != "_" => literal_aliases.push((a, path)),
                        _ => {}
                    }
                }
            }
            for k in *open..*close {
                if l.t[k] != "type" {
                    continue;
                }
                let end = (k..*close).find(|&m| l.t[m] == ";").unwrap_or(*close);
                let Some(eq) = (k..end).find(|&m| l.t[m] == "=") else {
                    continue;
                };
                let target = type_path(&l.t[eq + 1..end]);
                match l.t.get(k + 1) {
                    Some(x) if x == "$" => alias_targets.push(target),
                    Some(x) if is_name(x) => literal_aliases.push((bare(x).to_owned(), target)),
                    _ => {}
                }
            }
            for caller in lexed {
                for (module, c_open, c_close) in caller.calls(name) {
                    for k in c_open + 1..c_close {
                        if declares && matches!(caller.t[k].as_str(), "struct" | "enum" | "union") {
                            if let Some(n) = caller.t.get(k + 1).filter(|x| is_name(x)) {
                                tree.declared.push((module.clone(), bare(n).to_owned()));
                            }
                        }
                    }
                    if declares {
                        if let Some(n) = first_name_of_a_call(&caller.t, c_open) {
                            tree.declared.push((module.clone(), n));
                        }
                    }
                    for target in &alias_targets {
                        for k in c_open + 1..c_close {
                            if is_name(&caller.t[k]) {
                                tree.aliases.push((
                                    module.clone(),
                                    bare(&caller.t[k]).to_owned(),
                                    target.clone(),
                                ));
                            }
                        }
                    }
                    for target in &glob_targets {
                        tree.globs.push((
                            module.clone(),
                            target.clone(),
                            in_a_block(&caller.t, c_open),
                        ));
                    }
                    for (alias, target) in &literal_aliases {
                        tree.aliases
                            .push((module.clone(), alias.clone(), target.clone()));
                    }
                }
            }
        }
    }
    tree
}

fn absolute(tree: &Tree, module: &[String], path: &[String], depth: usize) -> Option<Segments> {
    if depth > 8 || path.is_empty() {
        return None;
    }
    let first = path[0].as_str();
    if first == "crate" {
        return Some(path[1..].to_vec());
    }
    if first == "$" && path.get(1).is_some_and(|x| x == "crate") {
        return Some(path[2..].to_vec());
    }
    if first == "self" {
        return Some(
            module
                .iter()
                .cloned()
                .chain(path[1..].iter().cloned())
                .collect(),
        );
    }
    if first == "super" {
        let ups = path.iter().take_while(|p| *p == "super").count();
        let keep = module.len().checked_sub(ups)?;
        return Some(
            module[..keep]
                .iter()
                .cloned()
                .chain(path[ups..].iter().cloned())
                .collect(),
        );
    }
    if tree.selves.iter().any(|s| s == first) {
        return Some(path[1..].to_vec());
    }
    if tree.modules.iter().any(|(m, n)| m == module && n == first) {
        return Some(module.iter().cloned().chain(path.iter().cloned()).collect());
    }
    let target = tree
        .aliases
        .iter()
        .find(|(m, alias, _)| m == module && alias == first)
        .map(|(_, _, target)| target.clone())?;
    let joined: Segments = target
        .into_iter()
        .chain(path[1..].iter().cloned())
        .collect();
    absolute(tree, module, &joined, depth + 1)
}

fn place(tree: &Tree, locked: &Locked, module: &[String], path: &[String], depth: usize) -> Place {
    if depth > 8 || path.is_empty() {
        return Place::Other;
    }
    if path.len() == 1 && path[0] != "$" {
        return place_name(tree, locked, module, &path[0], depth);
    }
    match absolute(tree, module, path, depth) {
        Some(abs) => place_absolute(tree, locked, &abs, depth),
        None => Place::Other,
    }
}

fn place_absolute(tree: &Tree, locked: &Locked, abs: &[String], depth: usize) -> Place {
    match abs {
        [] => Place::Other,
        [first, ..] if MODULES.contains(&first.as_str()) => Place::Locked,
        [only] => {
            if locked.root.contains(only) {
                Place::Locked
            } else {
                Place::Other
            }
        }
        [modules @ .., last] => place_name(tree, locked, modules, last, depth + 1),
    }
}

fn place_name(tree: &Tree, locked: &Locked, module: &[String], name: &str, depth: usize) -> Place {
    if depth > 8 {
        return Place::Other;
    }
    let mut own = false;
    for (m, alias, target) in &tree.aliases {
        if m == module
            && alias == name
            && target.last().is_none_or(|t| t != name || target.len() > 1)
        {
            match place(tree, locked, module, target, depth + 1) {
                Place::Locked => return Place::Locked,
                Place::Own => own = true,
                Place::Other => {}
            }
        }
    }
    if own {
        return Place::Own;
    }
    if glob_reaches_a_locked_type(tree, locked, module, name, depth, true) {
        return Place::Locked;
    }
    if tree.declared.iter().any(|(m, n)| m == module && n == name) {
        return Place::Own;
    }
    if glob_reaches_a_locked_type(tree, locked, module, name, depth, false) {
        return Place::Locked;
    }
    Place::Other
}

fn in_a_block(t: &[String], at: usize) -> bool {
    let mut open: Vec<bool> = Vec::new();
    for i in 0..at.min(t.len()) {
        match t[i].as_str() {
            "{" => open.push(!(i >= 2 && t[i - 2] == "mod")),
            "(" | "[" => open.push(false),
            "}" | ")" | "]" => {
                open.pop();
            }
            _ => {}
        }
    }
    open.contains(&true)
}

fn glob_reaches_a_locked_type(
    tree: &Tree,
    locked: &Locked,
    module: &[String],
    name: &str,
    depth: usize,
    in_block: bool,
) -> bool {
    for (m, target, block) in &tree.globs {
        if m != module || *block != in_block {
            continue;
        }
        match absolute(tree, module, target, depth + 1) {
            Some(abs) if abs.is_empty() => {
                if locked.root.iter().any(|r| r == name) {
                    return true;
                }
            }
            Some(abs) if MODULES.contains(&abs[0].as_str()) => {
                if abs.len() == 1 && locked.types.iter().any(|(m, n)| *m == abs[0] && n == name) {
                    return true;
                }
            }
            Some(abs) if place_name(tree, locked, &abs, name, depth + 1) == Place::Locked => {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn type_position(before: Option<&str>) -> bool {
    matches!(
        before,
        Some(":" | "," | "<" | ">" | "&" | "mut" | "=" | "(" | "const" | "+" | "dyn" | "*" | "[")
    )
}

fn impl_header(t: &[String], at: usize) -> (Vec<String>, Vec<String>) {
    let mut j = at + 1;
    let mut generics = Vec::new();
    if t.get(j).is_some_and(|x| x == "<") {
        let mut depth = 0i32;
        while j < t.len() {
            match t[j].as_str() {
                "<" => depth += 1,
                ">" if t[j - 1] != "-" => {
                    depth -= 1;
                    if depth == 0 {
                        j += 1;
                        break;
                    }
                }
                x if is_ident(x) => generics.push(bare(x).to_owned()),
                _ => {}
            }
            j += 1;
        }
    }
    let mut path = Vec::new();
    let mut self_type: Option<Vec<String>> = None;
    let mut depth = 0i32;
    while j < t.len() {
        let x = t[j].as_str();
        if depth == 0 && (x == "{" || x == ";" || x == "where") {
            break;
        }
        if depth == 0 && x == "for" && self_type.is_none() && t.get(j + 1).is_none_or(|n| n != "<")
        {
            self_type = Some(Vec::new());
            j += 1;
            continue;
        }
        match x {
            "<" | "(" | "[" => depth += 1,
            ">" if t[j - 1] != "-" => depth -= 1,
            ")" | "]" => depth -= 1,
            _ => {}
        }
        match &mut self_type {
            Some(s) => s.push(x.to_owned()),
            None => path.push(x.to_owned()),
        }
        j += 1;
    }
    (generics, self_type.unwrap_or(path))
}

fn impl_faults(lexed: &[Lexed], tree: &Tree, locked: &Locked) -> Vec<String> {
    let mut faults = Vec::new();
    for l in lexed {
        let t = &l.t;
        for i in 0..t.len() {
            if t[i] != "impl"
                || l.matcher[i]
                || type_position(i.checked_sub(1).map(|j| t[j].as_str()))
            {
                continue;
            }
            let (generics, self_type) = impl_header(t, i);
            let path = type_path(&self_type);
            let shown = self_type.join(" ");
            if path.len() == 1 && generics.contains(&path[0]) {
                continue;
            }
            if let Some((name, open, close)) = l.body_of(i) {
                if path.first().is_some_and(|p| p == "$")
                    && path.get(1).is_some_and(|p| p != "crate")
                {
                    let fragment = &path[1];
                    let body = &t[*open..*close];
                    let declared = (0..body.len()).any(|k| {
                        matches!(body[k].as_str(), "struct" | "enum" | "union" | "type")
                            && body.get(k + 1).is_some_and(|x| x == "$")
                            && body.get(k + 2).is_some_and(|x| x == fragment)
                    });
                    if declared {
                        continue;
                    }
                    for caller in lexed {
                        for (module, c_open, c_close) in caller.calls(name) {
                            for argument in arguments_of(&caller.t, c_open, c_close) {
                                if place(tree, locked, &module, &argument, 0) == Place::Locked {
                                    faults.push(format!(
                                        "{} passes `{}` to the macro {name} of {}, which implements its argument, and it is a type of a locked module",
                                        caller.file,
                                        argument.join("::"),
                                        l.file
                                    ));
                                }
                            }
                        }
                    }
                    continue;
                }
            }
            if place(tree, locked, &l.paths[i], &path, 0) == Place::Locked {
                faults.push(format!(
                    "{} holds an impl for `{shown}`, which is a type of a locked module",
                    l.file
                ));
            }
        }
    }
    faults
}

fn outside_the_lock_faults(files: &[(String, String)]) -> Vec<String> {
    let locked = locked_types();
    let mut faults = Vec::new();
    let mut lexed = Vec::new();
    for (file, text) in files {
        match lex(text) {
            Ok(t) => {
                faults.extend(reach_faults(file, &t));
                faults.extend(
                    t.iter()
                        .filter(|token| is_ident(token) && bare(token) == "macro_export")
                        .map(|_| format!("{file} holds macro_export")),
                );
                if file.starts_with("bin/") {
                    continue;
                }
                let inside = in_an_attribute_or_a_macro(&t);
                let matcher = in_a_matcher(&t);
                let paths = module_paths(&t, &inside, &module_of(file));
                let bodies = macro_bodies(&t);
                lexed.push(Lexed {
                    file: file.clone(),
                    t,
                    matcher,
                    paths,
                    bodies,
                });
            }
            Err(cause) => faults.push(unread(file, &cause)),
        }
    }
    let tree = build_tree(&lexed);
    faults.extend(impl_faults(&lexed, &tree, &locked));
    faults
}

fn later_faults(file: &str, text: &str) -> Vec<String> {
    outside_the_lock_faults(&[(file.to_owned(), text.to_owned())])
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
    assert_eq!(outside_the_lock_faults(&files), Vec::<String>::new());
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
        (
            "learning/upper.RS",
            "#[macro_export]\nmacro_rules! upper {\n    () => {};\n}\n",
        ),
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
            "learning/deeper/most.rs holds macro_export",
            "learning/upper.RS holds macro_export"
        ]
    );
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

#[test]
fn the_test_of_the_later_files_refuses_an_impl_for_a_type_of_a_locked_module() {
    let cases = [
        (
            "crate::kinds::KindEntry",
            "kinds",
            "KindEntry",
            "impl @ {\n    pub(crate) fn later_says_ok(&self) -> bool {\n        true\n    }\n}\n",
        ),
        (
            "crate::kinds::KindEntry",
            "kinds",
            "KindEntry",
            "impl<T: AsRef<str>> PartialEq<T> for @ {\n    fn eq(&self, other: &T) -> bool {\n        self.family == other.as_ref()\n    }\n}\n",
        ),
        (
            "crate::site::AppId",
            "site",
            "AppId",
            "impl @ {\n    pub(crate) const LATER_MAX: usize = 4096;\n}\n",
        ),
        (
            "crate::record::TypedRecord",
            "record",
            "TypedRecord",
            "impl std::fmt::Display for @ {\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        f.write_str(&self.json)\n    }\n}\n",
        ),
    ];
    for (path, module, name, body) in cases {
        let forms = [
            body.replace('@', path),
            format!("use {path} as LaterAlias;\n{}", body.replace('@', "LaterAlias")),
            format!("use crate::{module}::*;\n{}", body.replace('@', name)),
            format!("pub type LaterAlias = {path};\n{}", body.replace('@', "LaterAlias")),
            format!(
                "macro_rules! give {{\n    ($t:ty) => {{\n        {}\n    }};\n}}\ngive!({path});\n",
                body.replace('@', "$t")
            ),
        ];
        for text in forms {
            let faults = later_faults("learning.rs", &text);
            assert!(
                faults
                    .iter()
                    .any(|f| f.ends_with("a type of a locked module")),
                "{text}: {faults:?}"
            );
        }
    }
    for text in [
        "use crate::{kinds::KindEntry};\nimpl KindEntry {}\n",
        "use crate::kinds as k;\nimpl k::KindEntry {}\n",
        "use crate::kinds::{self};\nimpl kinds::KindEntry {}\n",
        "impl super::kinds::KindEntry {}\n",
        "extern crate self as me;\nimpl me::kinds::KindEntry {}\n",
        "impl std::fmt::Display for crate::TypedRecord {\n    fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        Ok(())\n    }\n}\n",
        "pub trait LaterTrait {}\nimpl LaterTrait for &crate::record::TypedRecord {}\n",
        "pub trait LaterTrait {}\nimpl LaterTrait for Box<crate::record::TypedRecord> {}\n",
        "impl dyn crate::kinds::Coarse {\n    pub fn later(&self) {}\n}\n",
        "pub struct KindEntry;\npub fn f() {\n    use crate::kinds::KindEntry;\n    impl KindEntry {}\n}\n",
        "macro_rules! name_it {\n    ($n:ident) => {\n        use crate::kinds::KindEntry as $n;\n    };\n}\nname_it!(LaterAlias);\nimpl LaterAlias {}\n",
        "macro_rules! open_kinds {\n    () => {\n        use $crate::kinds::*;\n    };\n}\nopen_kinds!();\nimpl KindEntry {}\n",
        "macro_rules! alias_it {\n    () => {\n        type LaterAlias = crate::kinds::KindEntry;\n    };\n}\nalias_it!();\nimpl LaterAlias {}\n",
        "macro_rules! own_impl {\n    () => {\n        impl $crate::kinds::KindEntry {}\n    };\n}\nown_impl!();\n",
        "macro_rules! pass {\n    ($($t:tt)*) => {\n        $($t)*\n    };\n}\npass! {\n    impl crate::kinds::KindEntry {}\n}\n",
        "use crate::kinds::KindEntry;\nmacro_rules! give {\n    ($t:ty) => {\n        impl $t {}\n    };\n}\ngive!(KindEntry);\n",
    ] {
        let faults = later_faults("learning.rs", text);
        assert!(
            faults.iter().any(|f| f.ends_with("a type of a locked module")),
            "{text}: {faults:?}"
        );
    }
    let across = |more: &str, learning: &str| {
        outside_the_lock_faults(&[
            (
                "learning.rs".to_owned(),
                format!("pub mod more;\n{learning}"),
            ),
            ("learning/more.rs".to_owned(), more.to_owned()),
        ])
    };
    for (more, learning) in [
        (
            "pub use crate::kinds::KindEntry;\n",
            "use self::more::*;\nimpl KindEntry {}\n",
        ),
        (
            "pub type Entry = crate::kinds::KindEntry;\n",
            "use self::more::Entry;\nimpl Entry {}\n",
        ),
        ("pub use crate::kinds::*;\n", "impl more::KindEntry {}\n"),
        ("impl super::super::kinds::KindEntry {}\n", ""),
        (
            "pub use crate::kinds::KindEntry as LaterTwin;\n",
            "pub struct LaterTwin;\npub fn f() {\n    use self::more::*;\n    impl LaterTwin {}\n}\n",
        ),
    ] {
        let faults = across(more, learning);
        assert!(
            faults
                .iter()
                .any(|f| f.ends_with("a type of a locked module")),
            "{more} {learning}: {faults:?}"
        );
    }
    for twin in [
        "pub trait FromJson {}\nimpl FromJson for u32 {}\nimpl FromJson for std::num::NonZeroU32 {}\nuse std::num::NonZeroU64;\nimpl FromJson for NonZeroU64 {}\nimpl<T: FromJson> FromJson for Vec<T> {}\nimpl<T: FromJson> FromJson for Option<T> {}\n",
        "pub trait LaterTrait {}\nimpl<T> LaterTrait for T {}\nimpl<KindEntry> LaterTrait for Box<KindEntry> {}\n",
        "pub struct OwnText(String);\nimpl OwnText {\n    pub fn get(&self) -> &str {\n        &self.0\n    }\n}\npub struct Category;\nimpl Category {}\n",
        "macro_rules! names {\n    ($(#[$m:meta])* pub enum $name:ident { $($v:ident = $s:literal),+ $(,)? }) => {\n        $(#[$m])*\n        pub enum $name { $($v),+ }\n        impl $name {\n            pub fn all() -> usize {\n                0\n            }\n        }\n        impl std::fmt::Display for $name {\n            fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n                Ok(())\n            }\n        }\n    };\n}\nnames! {\n    pub enum Mood {\n        Calm = \"calm\",\n    }\n}\n",
        "macro_rules! with_fmt {\n    () => {\n        use std::fmt;\n    };\n}\nwith_fmt!();\n",
        "pub struct LaterNote;\nmacro_rules! give {\n    ($t:ty) => {\n        impl $t {}\n    };\n}\ngive!(LaterNote);\n",
        "pub struct LaterBody {\n    pub n: u32,\n}\nimpl crate::request::RunnerBody for LaterBody {\n    type Reply = u32;\n}\n",
        "pub fn f() -> impl Iterator<Item = u8> {\n    0..3\n}\npub fn g(x: impl AsRef<str>) -> usize {\n    x.as_ref().len()\n}\n",
    ] {
        assert_eq!(
            later_faults("learning.rs", twin),
            Vec::<String>::new(),
            "{twin}"
        );
    }
    for (more, learning) in [
        ("pub struct OwnText;\n", "pub trait FromJson {}\nuse self::more::OwnText;\nimpl FromJson for OwnText {}\n"),
        ("pub struct OwnText;\n", "pub trait FromJson {}\nuse self::more::*;\nimpl FromJson for OwnText {}\n"),
        (
            "pub use crate::kinds::KindEntry as LaterTwin;\n",
            "pub struct LaterTwin;\nuse self::more::*;\nimpl LaterTwin {}\n",
        ),
        ("pub enum Cause {\n    Late,\n}\n", "pub trait FromJson {}\nuse crate::learning::more::{self, Cause};\nimpl FromJson for Cause {}\nimpl FromJson for more::Cause {}\n"),
    ] {
        assert_eq!(across(more, learning), Vec::<String>::new(), "{more} {learning}");
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

#[test]
fn each_predicate_of_the_textual_tests_has_a_case_of_its_own() {
    let macros = locked_macros();
    let root = root_names();
    let faults = lock_faults("site.rs", "r#include!(\"learning.rs\");\n", &macros, &root);
    assert_eq!(faults, ["site.rs names include"]);
    for (text, fault) in [
        (
            "fn f() -> Vec<u8> {\n    vec_sneak!()\n}\n",
            "site.rs calls the macro vec_sneak, which no locked file defines",
        ),
        (
            "fn f() -> String {\n    format_args!(\"x\").to_string()\n}\n",
            "site.rs calls the macro format_args, which no locked file defines",
        ),
    ] {
        assert_eq!(
            lock_faults("site.rs", text, &macros, &root),
            [fault],
            "{text}"
        );
    }
    assert_eq!(
        lock_faults("lib.rs", "use sneak::Hidden;\n", &macros, &root),
        ["lib.rs re-exports from sneak"]
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
            "kinds.rs names learning 2 times",
            "kinds.rs does not name learning as `crate : : learning : : KINDS` once"
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
