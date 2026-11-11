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

fn ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn code(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let next = b.get(i + 1).copied();
        if b[i] == b'/' && next == Some(b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i] == b'/' && next == Some(b'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            out.push(b' ');
        } else if b[i] == b'r'
            && (i == 0 || !ident(b[i - 1]) || (b[i - 1] == b'b' && (i < 2 || !ident(b[i - 2]))))
            && matches!(next, Some(b'"' | b'#'))
        {
            let mut j = i + 1;
            while j < b.len() && b[j] == b'#' {
                j += 1;
            }
            if j < b.len() && b[j] == b'"' {
                let close = format!("\"{}", "#".repeat(j - i - 1));
                let rest = &text[j + 1..];
                i = j + 1 + rest.find(&close).map_or(rest.len(), |k| k + close.len());
                out.push(b' ');
            } else {
                out.push(b[i]);
                i += 1;
            }
        } else if b[i] == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' {
                if b[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            out.push(b' ');
        } else if b[i] == b'\'' && next == Some(b'\\') {
            i += 2;
            while i < b.len() && b[i] != b'\'' {
                i += 1;
            }
            i += 1;
            out.push(b' ');
        } else if b[i] == b'\'' && b.get(i + 2) == Some(&b'\'') {
            i += 3;
            out.push(b' ');
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn identifiers(text: &str, name: &str) -> usize {
    code(text)
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| *word == name)
        .count()
}

fn tokens(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut word = String::new();
    for c in code(text).chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c);
            continue;
        }
        if !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
        if !c.is_whitespace() {
            out.push(c.to_string());
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    let mut merged: Vec<String> = Vec::new();
    let mut i = 0;
    while i < out.len() {
        let raw = out[i] == "r"
            && out.get(i + 1).is_some_and(|t| t == "#")
            && out
                .get(i + 2)
                .is_some_and(|t| t.bytes().all(ident) && !t.is_empty());
        if raw {
            merged.push(format!("r#{}", out[i + 2]));
            i += 3;
        } else {
            merged.push(out[i].clone());
            i += 1;
        }
    }
    merged
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

fn outside_macros(tokens: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == "macro_rules" && tokens.get(i + 1).is_some_and(|x| x == "!") {
            let open = i + 3;
            i = match tokens.get(open).map(String::as_str) {
                Some("{" | "(" | "[") => matching(tokens, open) + 1,
                _ => open,
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
    let t = outside_macros(&tokens(text));
    let mut faults = Vec::new();
    let count = |name: &str| t.iter().filter(|x| *x == name).count();
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
    if t.iter().any(|x| x == "cfg" || x == "cfg_attr") {
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
    let named = t.iter().filter(|x| *x == "coarse").count();
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

fn bare(name: &str) -> &str {
    name.strip_prefix("r#").unwrap_or(name)
}

fn is_name(token: &str) -> bool {
    let name = bare(token);
    !name.is_empty() && name.bytes().all(ident) && !KEYWORDS.contains(&name)
}

fn macro_names(text: &str) -> Vec<String> {
    let t = tokens(text);
    find(&t, &["macro_rules", "!"])
        .into_iter()
        .filter_map(|i| t.get(i + 2).map(|name| bare(name).to_owned()))
        .collect()
}

fn locked_macros() -> Vec<String> {
    LOCKED
        .iter()
        .flat_map(|file| macro_names(&source(file)))
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

fn lock_faults(file: &str, text: &str, macros: &[String], root: &[String]) -> Vec<String> {
    let t = tokens(text);
    let own = macro_names(text);
    let mut faults = Vec::new();
    for (i, token) in t.iter().enumerate() {
        let name = bare(token);
        let next = t.get(i + 1).map(String::as_str);
        let before = i.checked_sub(1).map(|j| t[j].as_str());
        if name == "path" && next == Some("=") && matches!(before, Some("[" | "," | "(")) {
            faults.push(format!("{file} holds a path attribute"));
        }
        if name == "include" {
            faults.push(format!("{file} names include"));
        }
        if name == "macro_use" {
            faults.push(format!("{file} holds macro_use"));
        }
        let call =
            next == Some("!") && matches!(t.get(i + 2).map(String::as_str), Some("(" | "[" | "{"));
        if call
            && is_name(token)
            && name != "include"
            && !macros.iter().chain(&own).any(|m| m == name)
            && !STD_MACROS.contains(&name)
        {
            faults.push(format!(
                "{file} calls the macro {name}, which no locked file defines"
            ));
        }
        if matches!(name, "crate" | "super")
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
        if file == "lib.rs" && name == "use" {
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
            "site.rs holds a path attribute",
        ),
        (
            "#[cfg_attr(all(), path = \"learning.rs\")]\nmod sneak_path;\n",
            "site.rs holds a path attribute",
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
    ] {
        assert_eq!(
            lock_faults("site.rs", twin, &macros, &root),
            Vec::<String>::new(),
            "{twin}"
        );
    }
}

fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
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

fn later_files() -> Vec<(String, String)> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<std::path::PathBuf> = LATER
        .iter()
        .map(|module| src.join(format!("{module}.rs")))
        .collect();
    for module in LATER {
        let dir = src.join(module);
        if dir.is_dir() {
            rust_files(&dir, &mut files);
        }
    }
    files
        .into_iter()
        .map(|path| {
            let name = path
                .strip_prefix(&src)
                .map_or_else(|_| path.display().to_string(), |p| p.display().to_string());
            match std::fs::read_to_string(&path) {
                Ok(text) => (name, text.replace("\r\n", "\n")),
                Err(e) => panic!("{name}: {e}"),
            }
        })
        .collect()
}

fn later_faults(file: &str, text: &str) -> Vec<String> {
    tokens(text)
        .iter()
        .filter(|token| bare(token) == "macro_export")
        .map(|_| format!("{file} holds macro_export"))
        .collect()
}

#[test]
fn no_file_of_a_later_run_holds_macro_export() {
    let files = later_files();
    assert!(files.len() >= 4, "{}", files.len());
    for (file, text) in files {
        assert_eq!(later_faults(&file, &text), Vec::<String>::new());
    }
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
fn no_locked_file_names_an_item_of_the_files_of_later_runs_but_their_kind_lists() {
    let macros = locked_macros();
    let root = root_names();
    for file in LOCKED {
        let text = source(file);
        assert_eq!(
            lock_faults(file, &text, &macros, &root),
            Vec::<String>::new()
        );
        for module in LATER {
            let named = identifiers(&text, module);
            match file {
                "kinds.rs" => {
                    assert_eq!(named, 1, "{file} names {module} {named} times");
                    assert_eq!(
                        code(&text)
                            .matches(&format!("crate::{module}::KINDS"))
                            .count(),
                        1,
                        "{file}"
                    );
                }
                "lib.rs" => {
                    assert_eq!(named, 1, "{file} names {module} {named} times");
                    assert_eq!(
                        code(&text).matches(&format!("pub mod {module};\n")).count(),
                        1,
                        "{file}"
                    );
                }
                _ => assert_eq!(named, 0, "{file} names {module}"),
            }
        }
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
