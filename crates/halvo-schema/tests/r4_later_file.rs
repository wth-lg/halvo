use std::path::Path;
use std::str::FromStr;

use proc_macro2::{Delimiter, TokenStream, TokenTree};

fn flatten(stream: TokenStream, out: &mut Vec<TokenTree>) {
    for tree in stream {
        if let TokenTree::Group(group) = &tree {
            let inner = group.stream();
            out.push(tree.clone());
            flatten(inner, out);
        } else {
            out.push(tree);
        }
    }
}

fn is_ident(tree: &TokenTree, word: &str) -> bool {
    matches!(tree, TokenTree::Ident(i) if i == word)
}

fn is_punct(tree: &TokenTree, c: char) -> bool {
    matches!(tree, TokenTree::Punct(p) if p.as_char() == c)
}

fn declared(tokens: &[TokenTree]) -> Vec<String> {
    tokens
        .windows(2)
        .filter(|w| is_ident(&w[0], "struct") || is_ident(&w[0], "enum"))
        .filter_map(|w| match &w[1] {
            TokenTree::Ident(i) => Some(i.to_string()),
            _ => None,
        })
        .collect()
}

fn self_types(tokens: &[TokenTree]) -> Vec<String> {
    let mut found = Vec::new();
    for (at, tree) in tokens.iter().enumerate() {
        if !is_ident(tree, "impl") {
            continue;
        }
        let mut path: Vec<&TokenTree> = Vec::new();
        let mut depth = 0i32;
        for next in &tokens[at + 1..] {
            if matches!(next, TokenTree::Group(g) if g.delimiter() == Delimiter::Brace) {
                break;
            }
            if is_punct(next, '<') {
                depth += 1;
            } else if is_punct(next, '>') {
                depth -= 1;
            } else if depth == 0 && is_ident(next, "for") {
                path.clear();
            } else if depth == 0 && is_ident(next, "where") {
                break;
            } else if depth == 0 {
                path.push(next);
            }
        }
        let name = path.iter().rev().find_map(|t| match t {
            TokenTree::Ident(i) => Some(i.to_string()),
            _ => None,
        });
        let metavariable = path.iter().rev().nth(1).is_some_and(|t| is_punct(t, '$'));
        if let Some(name) = name.filter(|_| !metavariable) {
            found.push(name);
        }
    }
    found
}

fn faults(files: &[(String, String)]) -> Vec<String> {
    let mut tokens = Vec::new();
    for (name, text) in files {
        let stream = TokenStream::from_str(text).unwrap_or_else(|e| panic!("{name}: {e}"));
        flatten(stream, &mut tokens);
    }
    let own = declared(&tokens);
    self_types(&tokens)
        .into_iter()
        .filter(|name| !own.contains(name))
        .collect()
}

fn unit_files() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = vec![src.join("coordinator.rs")];
    let mut inner: Vec<_> = std::fs::read_dir(src.join("coordinator"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    inner.sort();
    files.extend(inner);
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read_to_string(&p).unwrap())
        })
        .collect()
}

#[test]
fn r4_later_file_implements_only_types_that_it_declares() {
    let files = unit_files();
    assert!(
        files.len() >= 5,
        "the coordinator file and its four modules"
    );
    assert_eq!(
        faults(&files),
        Vec::<String>::new(),
        "the interface run's file implements only its own types: ruling S-50 bars an impl for a type of another later file, and the file needs none of the impls that S-47 allows for a core type"
    );
}

#[test]
fn r4_later_file_check_names_an_impl_for_a_type_of_another_file() {
    let fixture = |text: &str| faults(&[("fixture.rs".to_owned(), text.to_owned())]);
    assert_eq!(fixture("struct Note;\nimpl Note {}\nimpl<'de> Deserialize<'de> for Note {}\nimpl Serialize for Note {}\n"), Vec::<String>::new());
    assert_eq!(
        fixture("struct Note;\nimpl WebOrigin {\n    pub fn host(&self) {}\n}\n"),
        ["WebOrigin"]
    );
    assert_eq!(fixture("struct Note;\nimpl From<Note> for crate::record::Instant {\n    fn from(_: Note) -> Self { todo!() }\n}\n"), ["Instant"]);
    assert_eq!(
        fixture(
            "enum Note { A }\nimpl<'a> std::fmt::Display for Wrapper<'a> where Note: Copy {}\n"
        ),
        ["Wrapper"]
    );
    assert_eq!(
        fixture("macro_rules! m { ($t:ident) => { impl $t {} } }\nstruct Note;\n"),
        Vec::<String>::new()
    );
}
