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
    let t = tokens(text);
    let mut faults = Vec::new();
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
}

fn macro_names(text: &str) -> Vec<String> {
    let t = tokens(text);
    find(&t, &["macro_rules", "!"])
        .into_iter()
        .filter_map(|i| t.get(i + 2).cloned())
        .collect()
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

fn macros_of_later_files() -> Vec<String> {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&src).expect("src") {
        let path = entry.expect("a directory entry").path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() && name != "bin" {
            rust_files(&path, &mut files);
        } else if path.is_file() && name.ends_with(".rs") && !LOCKED.contains(&name) {
            files.push(path);
        }
    }
    let mut names = Vec::new();
    for file in files {
        match std::fs::read_to_string(&file) {
            Ok(text) => names.extend(macro_names(&text)),
            Err(e) => panic!("{}: {e}", file.display()),
        }
    }
    names
}

fn lock_faults(file: &str, text: &str, later_macros: &[String]) -> Vec<String> {
    let t = tokens(text);
    let mut faults = Vec::new();
    let mut in_use = false;
    for (i, token) in t.iter().enumerate() {
        let next = t.get(i + 1).map(String::as_str);
        let before = i.checked_sub(1).map(|j| t[j].as_str());
        match token.as_str() {
            "use" => in_use = true,
            ";" => in_use = false,
            _ => {}
        }
        if token == "path" && next == Some("=") && matches!(before, Some("[" | "," | "(")) {
            faults.push(format!("{file} holds a path attribute"));
        }
        if token == "include" && next == Some("!") {
            faults.push(format!("{file} holds include!"));
        }
        if later_macros.contains(token)
            && before != Some("macro_rules")
            && (next == Some("!") || in_use)
        {
            faults.push(format!(
                "{file} calls or imports the macro {token}, which a file of a later run defines"
            ));
        }
    }
    faults
}

#[test]
fn the_lock_test_refuses_a_path_attribute_include_and_a_macro_of_a_later_file() {
    let later = macro_names(
        "#[macro_export]\nmacro_rules! sneak_macro {\n    () => {\n        $crate::learning::KINDS.len()\n    };\n}\n",
    );
    assert_eq!(later, ["sneak_macro"]);
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
            "site.rs holds include!",
        ),
        (
            "fn sneak_len() -> usize {\n    crate::sneak_macro!()\n}\n",
            "site.rs calls or imports the macro sneak_macro",
        ),
        (
            "fn sneak_len() -> usize {\n    sneak_macro![]\n}\n",
            "site.rs calls or imports the macro sneak_macro",
        ),
        (
            "use crate::{ids, sneak_macro as m};\n",
            "site.rs calls or imports the macro sneak_macro",
        ),
    ] {
        let faults = lock_faults("site.rs", text, &later);
        assert_eq!(faults.len(), 1, "{text}: {faults:?}");
        assert!(faults[0].starts_with(fault), "{text}: {faults:?}");
    }
    for twin in [
        "let path = 1;\n",
        "fn path(p: &str) -> &str {\n    p\n}\n",
        "let s = include_str!(\"learning.rs\");\n",
        "/// #[path = \"learning.rs\"] include!(\"learning.rs\") sneak_macro!()\nfn f() {}\n",
        "let sneak_macro = \"sneak_macro!()\";\n",
        "macro_rules! own {\n    () => {};\n}\nown!();\n",
    ] {
        assert_eq!(
            lock_faults("site.rs", twin, &later),
            Vec::<String>::new(),
            "{twin}"
        );
    }
}

#[test]
fn no_locked_file_names_an_item_of_the_files_of_later_runs_but_their_kind_lists() {
    let later_macros = macros_of_later_files();
    for file in LOCKED {
        let text = source(file);
        assert_eq!(
            lock_faults(file, &text, &later_macros),
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

fn fn_body(t: &[String], name: &str) -> Vec<String> {
    let at = find(t, &["fn", name, "("]);
    assert_eq!(
        at.len(),
        1,
        "src/kinds.rs declares fn {name} {} times",
        at.len()
    );
    let open = (at[0]..t.len()).find(|&i| t[i] == "{").unwrap_or(t.len());
    t[(open + 1).min(t.len())..closing(t, open)].to_vec()
}

#[test]
fn registry_chains_the_kinds_of_this_run_and_the_four_lists_of_later_runs_in_the_order_of_the_contract(
) {
    let body = fn_body(&tokens(&source("kinds.rs")), "registry");
    assert_eq!(
        body,
        tokens(
            "KINDS.iter().chain(crate::executable::KINDS).chain(crate::coordinator::KINDS).chain(crate::learning::KINDS).chain(crate::routing::KINDS)"
        ),
        "src/kinds.rs: registry() must chain kinds::KINDS, executable::KINDS, coordinator::KINDS, learning::KINDS and routing::KINDS in that order"
    );
}

#[test]
fn check_registry_reads_the_whole_registry() {
    let body = fn_body(&tokens(&source("kinds.rs")), "check_registry");
    assert_eq!(
        body,
        tokens("let entries: Vec<&KindEntry> = registry().collect(); check_entries(&entries)"),
        "src/kinds.rs: check_registry() must call check_entries over registry() and nothing else"
    );
}
