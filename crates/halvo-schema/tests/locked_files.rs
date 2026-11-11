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
