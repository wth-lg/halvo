//! The reader of the fence's manifest test (U2.6 round 1, R3): a manifest's text read as cargo
//! decodes it, so far as its dependencies and its features go. Strings are decoded (the escapes of
//! a basic string, a multi-line string and its line-ending backslash, literal strings), keys are
//! read bare, quoted or dotted, table headers and inline tables are flattened into whole paths, a
//! dependency's package is its key or its `package`, and `workspace = true` takes the package and
//! the features of the workspace's own entry. A text that the reader cannot read is refused.

use std::path::{Path, PathBuf};

/// A value of a manifest, as far as the reader needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Value {
    Str(String),
    Arr(Vec<Value>),
    Bool(bool),
    Other,
}

/// The entries of a manifest's text, each value with its whole path of keys.
pub(crate) type Entries = Vec<(Vec<String>, Value)>;

/// Manifests, each a path and its text.
pub(crate) type Files = Vec<(PathBuf, String)>;

/// The entries of a manifest's text, each value with its whole path of keys: table headers, dotted
/// keys and inline tables flattened, an array of tables numbered by its place (`#0`, `#1`).
pub(crate) fn entries(text: &str) -> Result<Entries, String> {
    let mut p = Parser {
        c: text.chars().collect(),
        i: 0,
    };
    let mut out = Vec::new();
    let mut table: Vec<String> = Vec::new();
    let mut arrays: Vec<(Vec<String>, usize)> = Vec::new();
    loop {
        p.blank();
        if p.peek().is_none() {
            return Ok(out);
        }
        if p.peek() == Some('[') {
            let array = p.peek_at(1) == Some('[');
            p.i += if array { 2 } else { 1 };
            let path = p.key_path()?;
            p.expect(']')?;
            if array {
                p.expect(']')?;
            }
            p.line_end()?;
            table = path.clone();
            if array {
                let n = match arrays.iter_mut().find(|(k, _)| *k == path) {
                    Some(e) => {
                        e.1 += 1;
                        e.1
                    }
                    None => {
                        arrays.push((path, 0));
                        0
                    }
                };
                table.push(format!("#{n}"));
            }
        } else {
            let mut path = table.clone();
            path.extend(p.key_path()?);
            p.spaces();
            p.expect('=')?;
            p.spaces();
            p.value_into(&path, &mut out)?;
            p.line_end()?;
        }
    }
}

struct Parser {
    c: Vec<char>,
    i: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn peek_at(&self, k: usize) -> Option<char> {
        self.c.get(self.i + k).copied()
    }

    fn starts(&self, s: &str) -> bool {
        s.chars()
            .enumerate()
            .all(|(k, ch)| self.peek_at(k) == Some(ch))
    }

    fn spaces(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.i += 1;
        }
    }

    fn comment(&mut self) {
        if self.peek() == Some('#') {
            while !matches!(self.peek(), None | Some('\n')) {
                self.i += 1;
            }
        }
    }

    fn newline(&mut self) -> bool {
        if self.peek() == Some('\n') {
            self.i += 1;
            true
        } else if self.starts("\r\n") {
            self.i += 2;
            true
        } else {
            false
        }
    }

    /// Spaces, comments and line ends.
    fn blank(&mut self) {
        loop {
            self.spaces();
            self.comment();
            if !self.newline() {
                return;
            }
        }
    }

    fn expect(&mut self, ch: char) -> Result<(), String> {
        if self.peek() == Some(ch) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected {ch:?} at {}", self.i))
        }
    }

    fn line_end(&mut self) -> Result<(), String> {
        self.spaces();
        self.comment();
        if self.peek().is_none() || self.newline() {
            Ok(())
        } else {
            Err(format!("unexpected text at {}", self.i))
        }
    }

    fn key_path(&mut self) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        loop {
            self.spaces();
            let part = match self.peek() {
                Some('"') => self.basic(false)?,
                Some('\'') => self.literal(false)?,
                _ => {
                    let s = self.i;
                    while self
                        .peek()
                        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                    {
                        self.i += 1;
                    }
                    if s == self.i {
                        return Err(format!("no key at {s}"));
                    }
                    self.c[s..self.i].iter().collect()
                }
            };
            out.push(part);
            self.spaces();
            if self.peek() == Some('.') {
                self.i += 1;
            } else {
                return Ok(out);
            }
        }
    }

    fn value_into(
        &mut self,
        path: &[String],
        out: &mut Vec<(Vec<String>, Value)>,
    ) -> Result<(), String> {
        if self.peek() != Some('{') {
            let v = self.value()?;
            out.push((path.to_vec(), v));
            return Ok(());
        }
        self.i += 1;
        loop {
            self.blank();
            if self.peek() == Some('}') {
                self.i += 1;
                return Ok(());
            }
            let mut inner = path.to_vec();
            inner.extend(self.key_path()?);
            self.spaces();
            self.expect('=')?;
            self.spaces();
            self.value_into(&inner, out)?;
            self.blank();
            match self.peek() {
                Some(',') => self.i += 1,
                Some('}') => {}
                _ => return Err(format!("bad inline table at {}", self.i)),
            }
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        match self.peek() {
            Some('"') => {
                let multi = self.starts("\"\"\"");
                Ok(Value::Str(self.basic(multi)?))
            }
            Some('\'') => {
                let multi = self.starts("'''");
                Ok(Value::Str(self.literal(multi)?))
            }
            Some('[') => {
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.blank();
                    if self.peek() == Some(']') {
                        self.i += 1;
                        return Ok(Value::Arr(items));
                    }
                    if self.peek() == Some('{') {
                        self.value_into(&[], &mut Vec::new())?;
                        items.push(Value::Other);
                    } else {
                        items.push(self.value()?);
                    }
                    self.blank();
                    match self.peek() {
                        Some(',') => self.i += 1,
                        Some(']') => {}
                        _ => return Err(format!("bad array at {}", self.i)),
                    }
                }
            }
            _ => {
                let s = self.i;
                while !matches!(self.peek(), None | Some(',' | ']' | '}' | '\n' | '#')) {
                    self.i += 1;
                }
                let word: String = self.c[s..self.i].iter().collect();
                match word.trim() {
                    "" => Err(format!("no value at {s}")),
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    _ => Ok(Value::Other),
                }
            }
        }
    }

    /// A basic string (`"` or `"""`) with TOML's escapes; in a multi-line one, a backslash at a
    /// line's end takes the line end and the white space after it.
    fn basic(&mut self, multi: bool) -> Result<String, String> {
        self.i += if multi { 3 } else { 1 };
        if multi {
            self.newline();
        }
        let mut out = String::new();
        loop {
            let Some(ch) = self.peek() else {
                return Err("a string is not closed".into());
            };
            if multi && self.starts("\"\"\"") {
                let mut k = 3;
                while k < 5 && self.peek_at(k) == Some('"') {
                    k += 1;
                }
                out.extend(std::iter::repeat_n('"', k - 3));
                self.i += k;
                return Ok(out);
            }
            if !multi && ch == '"' {
                self.i += 1;
                return Ok(out);
            }
            if !multi && ch == '\n' {
                return Err("a line end in a string".into());
            }
            if ch != '\\' {
                out.push(ch);
                self.i += 1;
                continue;
            }
            self.i += 1;
            let Some(e) = self.peek() else {
                return Err("an escape at the end".into());
            };
            self.i += 1;
            match e {
                'b' => out.push('\u{8}'),
                't' => out.push('\t'),
                'n' => out.push('\n'),
                'f' => out.push('\u{c}'),
                'r' => out.push('\r'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'u' | 'U' => {
                    let n = if e == 'u' { 4 } else { 8 };
                    let hex: String = self
                        .c
                        .get(self.i..self.i + n)
                        .unwrap_or(&[])
                        .iter()
                        .collect();
                    let code = u32::from_str_radix(&hex, 16).map_err(|_| "a bad escape")?;
                    out.push(char::from_u32(code).ok_or("a bad code point")?);
                    self.i += n;
                }
                ' ' | '\t' | '\r' | '\n' if multi => {
                    self.i -= 1;
                    while matches!(self.peek(), Some(' ' | '\t' | '\r' | '\n')) {
                        self.i += 1;
                    }
                }
                _ => return Err(format!("a bad escape at {}", self.i)),
            }
        }
    }

    /// A literal string (`'` or `'''`): no escape.
    fn literal(&mut self, multi: bool) -> Result<String, String> {
        self.i += if multi { 3 } else { 1 };
        if multi {
            self.newline();
        }
        let s = self.i;
        loop {
            match self.peek() {
                None => return Err("a string is not closed".into()),
                Some('\'') if !multi => {
                    let out = self.c[s..self.i].iter().collect();
                    self.i += 1;
                    return Ok(out);
                }
                Some('\'') if self.starts("'''") => {
                    let mut k = 3;
                    while k < 5 && self.peek_at(k) == Some('\'') {
                        k += 1;
                    }
                    let out = self.c[s..self.i + k - 3].iter().collect();
                    self.i += k;
                    return Ok(out);
                }
                Some('\n') if !multi => return Err("a line end in a string".into()),
                _ => self.i += 1,
            }
        }
    }
}

/// The dependency sections of a manifest.
const SECTIONS: [&str; 5] = [
    "dependencies",
    "dev-dependencies",
    "build-dependencies",
    "dev_dependencies",
    "build_dependencies",
];

/// A dependency as a manifest declares it: its key, its `package`, its features and whether it
/// takes the workspace's entry (`workspace = true`).
#[derive(Clone, Debug, Default)]
struct Dep {
    key: String,
    package: Option<String>,
    features: Vec<String>,
    workspace: bool,
}

/// The dependencies of a manifest's entries, those of its sections apart from those of
/// `[workspace.dependencies]`.
fn deps(entries: &[(Vec<String>, Value)]) -> (Vec<Dep>, Vec<Dep>) {
    let mut own: Vec<(Vec<String>, Dep)> = Vec::new();
    let mut root: Vec<(Vec<String>, Dep)> = Vec::new();
    for (path, value) in entries {
        let p: Vec<&str> = path.iter().map(String::as_str).collect();
        let (at, is_root) = match p.as_slice() {
            [s, ..] if SECTIONS.contains(s) => (1, false),
            ["target", _, s, ..] if SECTIONS.contains(s) => (3, false),
            ["workspace", "dependencies", ..] => (2, true),
            _ => continue,
        };
        let Some(key) = p.get(at) else {
            continue;
        };
        let prefix: Vec<String> = path[..=at].to_vec();
        let list = if is_root { &mut root } else { &mut own };
        let i = match list.iter().position(|(k, _)| *k == prefix) {
            Some(i) => i,
            None => {
                list.push((
                    prefix,
                    Dep {
                        key: key.to_string(),
                        ..Dep::default()
                    },
                ));
                list.len() - 1
            }
        };
        let dep = &mut list[i].1;
        match (p.get(at + 1), value) {
            (Some(&"package"), Value::Str(s)) => dep.package = Some(s.clone()),
            (Some(&"features"), Value::Arr(items)) => {
                dep.features.extend(items.iter().filter_map(|v| match v {
                    Value::Str(s) => Some(s.clone()),
                    _ => None,
                }))
            }
            (Some(&"workspace"), Value::Bool(b)) => dep.workspace = *b,
            _ => {}
        }
    }
    (
        own.into_iter().map(|(_, d)| d).collect(),
        root.into_iter().map(|(_, d)| d).collect(),
    )
}

/// The features of a manifest's `[features]` table, each with the names that it turns on.
fn features(entries: &[(Vec<String>, Value)]) -> Vec<(String, Vec<String>)> {
    entries
        .iter()
        .filter_map(|(path, value)| match (path.as_slice(), value) {
            ([f, name], Value::Arr(items)) if f == "features" => Some((
                name.clone(),
                items
                    .iter()
                    .filter_map(|v| match v {
                        Value::Str(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

/// The manifests among `files` (each a path and its text) that turn halvo-web's feature
/// `test-fence` on, each with the reason, read as cargo decodes them: a dependency whose package
/// is halvo-web (its key, or its `package`, or the `package` of the workspace's entry that it
/// takes) with the feature among its features or those of that entry; a feature of the manifest
/// that names `<key>/test-fence` or `<key>?/test-fence` for such a dependency; and in the crate's
/// own manifest `own`, its `default` or a feature that `default` turns on naming `test-fence`. The
/// dev-dependency of the crate on itself is the fence's own and is not counted. A text that the
/// reader cannot read is named, with its fault.
pub(crate) fn naming_the_fence(files: &[(PathBuf, String)], own: &Path) -> Vec<(PathBuf, String)> {
    let mut read: Vec<(PathBuf, Entries)> = Vec::new();
    let mut named = Vec::new();
    for (path, text) in files {
        match entries(text) {
            Ok(e) => read.push((path.clone(), e)),
            Err(fault) => named.push((path.clone(), format!("unread: {fault}"))),
        }
    }
    let roots: Vec<(PathBuf, Vec<Dep>)> = read
        .iter()
        .map(|(path, e)| (path.clone(), deps(e).1))
        .filter(|(_, root)| !root.is_empty())
        .collect();
    let inherited = |file: &Path, key: &str| -> Option<Dep> {
        roots
            .iter()
            .filter(|(root, _)| root.parent().is_some_and(|d| file.starts_with(d)))
            .max_by_key(|(root, _)| root.components().count())
            .and_then(|(_, list)| list.iter().find(|d| d.key == key).cloned())
    };
    for (path, e) in &read {
        if path == own {
            let table = features(e);
            let mut reach = vec!["default".to_string()];
            let mut k = 0;
            while k < reach.len() {
                let name = reach[k].clone();
                k += 1;
                for (_, on) in table.iter().filter(|(n, _)| *n == name) {
                    for f in on {
                        if !reach.contains(f) {
                            reach.push(f.clone());
                        }
                    }
                }
            }
            if reach.iter().any(|f| f == "test-fence") {
                named.push((path.clone(), "default turns test-fence on".into()));
            }
            continue;
        }
        let (own_deps, root_deps) = deps(e);
        let mut fenced_keys = Vec::new();
        for d in own_deps.iter().chain(root_deps.iter()) {
            let from = d.workspace.then(|| inherited(path, &d.key)).flatten();
            let package = d
                .package
                .clone()
                .or_else(|| from.as_ref().and_then(|f| f.package.clone()))
                .unwrap_or_else(|| d.key.clone());
            if package != "halvo-web" {
                continue;
            }
            fenced_keys.push(d.key.clone());
            let on = d
                .features
                .iter()
                .chain(from.iter().flat_map(|f| f.features.iter()));
            if on.clone().any(|f| f == "test-fence") {
                named.push((path.clone(), format!("{} turns test-fence on", d.key)));
            }
        }
        for (name, on) in features(e) {
            for f in on {
                let Some((k, feature)) = f.split_once('/') else {
                    continue;
                };
                if feature == "test-fence"
                    && fenced_keys.iter().any(|d| d == k.trim_end_matches('?'))
                {
                    named.push((path.clone(), format!("feature {name} turns test-fence on")));
                }
            }
        }
    }
    named
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, &str)]) -> Files {
        list.iter()
            .map(|(p, t)| (PathBuf::from(p), t.to_string()))
            .collect()
    }

    fn own() -> PathBuf {
        PathBuf::from("/r/crates/halvo-web/Cargo.toml")
    }

    /// U2.6 round 1, R3, failing first: each form that turns the fence on as cargo decodes it is
    /// named, though the two words stand together in no one file's text: the escaped form, the
    /// multi-line strings in a table under another name, the split between a workspace's root and
    /// a member, a feature of the member that names the dependency's feature, and a `default` of
    /// the crate's own manifest. Its controls are named by no reader.
    #[test]
    fn r3_each_form_that_cargo_decodes_names_the_fence() {
        let escaped = format!(
            "[package]\nname = \"a\"\n\n[dependencies]\n\"halvo{e}u002Dweb\" = {{ path = \"../halvo{e}u002Dweb\", features = [\"test{e}u002Dfence\"] }}\n",
            e = '\\'
        );
        let multiline = "[package]\nname = \"m\"\n\n[dependencies.web]\npackage = \"\"\"halvo-\\\nweb\"\"\"\npath = \"\"\"../halvo-\\\n  web\"\"\"\nfeatures = [\"\"\"test-\\\nfence\"\"\"]\n";
        let root = "[workspace]\nmembers = [\"app\"]\n\n[workspace.dependencies]\nweb = { package = \"halvo-web\", path = \"../crates/halvo-web\" }\n";
        let member = "[package]\nname = \"app\"\n\n[dependencies]\nweb = { workspace = true, features = [\"test-fence\"] }\n";
        let by_feature = "[package]\nname = \"f\"\n\n[features]\nx = [\"web?/test-fence\"]\n\n[target.'cfg(unix)'.dependencies]\nweb = { package = 'halvo-web', path = \"x\", optional = true }\n";
        let default = "[package]\nname = \"halvo-web\"\n\n[features]\ndefault = [\"more\"]\nmore = [\"test-fence\"]\ntest-fence = []\n";
        let cases: Vec<(Files, Vec<&str>)> = vec![
            (
                files(&[("/r/a/Cargo.toml", &escaped)]),
                vec!["/r/a/Cargo.toml"],
            ),
            (
                files(&[("/r/m/Cargo.toml", multiline)]),
                vec!["/r/m/Cargo.toml"],
            ),
            (
                files(&[("/r/w/Cargo.toml", root), ("/r/w/app/Cargo.toml", member)]),
                vec!["/r/w/app/Cargo.toml"],
            ),
            (
                files(&[("/r/f/Cargo.toml", by_feature)]),
                vec!["/r/f/Cargo.toml"],
            ),
            (
                files(&[("/r/crates/halvo-web/Cargo.toml", default)]),
                vec!["/r/crates/halvo-web/Cargo.toml"],
            ),
        ];
        let mut wrong = Vec::new();
        for (k, (input, want)) in cases.into_iter().enumerate() {
            let got: Vec<PathBuf> = naming_the_fence(&input, &own())
                .into_iter()
                .map(|(p, _)| p)
                .collect();
            let want: Vec<PathBuf> = want.iter().map(PathBuf::from).collect();
            if got != want {
                wrong.push(format!("case {k}: named {got:?}, want {want:?}"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
        let controls = files(&[
            (
                "/r/t/Cargo.toml",
                "[package]\nname = \"t\"\n\n[dev-dependencies]\nr3-web = { path = \".\", default-features = false, features = [\"test-fence\"] }\n",
            ),
            (
                "/r/b/Cargo.toml",
                "[package]\nname = \"b\"\n\n[dependencies]\nhalvo-web = { path = \"../crates/halvo-web\" }\n",
            ),
            (
                "/r/crates/halvo-web/Cargo.toml",
                "[package]\nname = \"halvo-web\"\n\n[features]\ntest-fence = []\n\n[dev-dependencies]\nhalvo-web = { path = \".\", features = [\"test-fence\"] }\n",
            ),
        ]);
        assert_eq!(naming_the_fence(&controls, &own()), Vec::new());
    }

    /// The reader's strings and keys: escapes, a multi-line basic string with its line-ending
    /// backslash, literal strings, quoted and dotted keys, an array of tables; a text that it
    /// cannot read is refused.
    #[test]
    fn the_reader_decodes_strings_and_keys_as_toml_does() {
        let e = entries(
            "a.\"b c\" = \"x\\ty\\u0041\"\n[t.'q r']\nk = '''\nlit\\'''\nv = [1, \"s\", { i = 2 }]\n[[bin]]\nname = 'one'\n[[bin]]\nname = \"two\"\nm = \"\"\"a\\\n   b\"\"\"\n",
        )
        .expect("read");
        let s = |p: &[&str]| {
            e.iter()
                .find(|(k, _)| k.iter().map(String::as_str).eq(p.iter().copied()))
                .map(|(_, v)| v.clone())
        };
        assert_eq!(s(&["a", "b c"]), Some(Value::Str("x\tyA".into())));
        assert_eq!(s(&["t", "q r", "k"]), Some(Value::Str("lit\\".into())));
        assert_eq!(
            s(&["t", "q r", "v"]),
            Some(Value::Arr(vec![
                Value::Other,
                Value::Str("s".into()),
                Value::Other
            ]))
        );
        assert_eq!(s(&["bin", "#1", "name"]), Some(Value::Str("two".into())));
        assert_eq!(s(&["bin", "#1", "m"]), Some(Value::Str("ab".into())));
        assert!(entries("a = \"open\n").is_err());
        assert!(entries("a = \"\\q\"\n").is_err());
    }
}
