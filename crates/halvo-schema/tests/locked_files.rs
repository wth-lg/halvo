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

/// The source with every comment, string literal and char literal written as one space.
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

#[test]
fn no_locked_file_names_an_item_of_the_files_of_later_runs_but_their_kind_lists() {
    for file in LOCKED {
        let text = source(file);
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
