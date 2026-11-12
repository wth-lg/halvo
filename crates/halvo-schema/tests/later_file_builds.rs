use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const LISTS: &str = "pub use ::halvo_schema_core::learning::{ERROR_NAMES, KINDS};\n";

const LEAK: &str =
    "fn leak(text: &str) -> &'static str {\n    Box::leak(text.to_owned().into_boxed_str())\n}\n";

const RECURRENCE: &str =
    "fn broken() -> crate::record::Recurrence {\n    crate::record::Recurrence { count: 3, period: None, first: 9, last: 1 }\n}\n";

struct Case {
    name: &'static str,
    later: &'static [&'static str],
    codes: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        name: "transmute",
        later: &["pub fn forge(s: String) -> crate::site::AppId {\n    unsafe { core::mem::transmute::<String, crate::site::AppId>(s) }\n}\n"],
        codes: &["unsafe_code"],
    },
    Case {
        name: "no-mangle",
        later: &["#[no_mangle]\npub extern \"Rust\" fn later_rule(family: &str) -> bool {\n    family == \"never\"\n}\n"],
        codes: &["unsafe_code"],
    },
    Case {
        name: "parse-error-new",
        later: &[LEAK, "pub fn carry(text: &str) -> crate::ids::ParseError {\n    crate::ids::ParseError::new(\"AppId\", leak(text))\n}\n"],
        codes: &["E0061", "E0277", "E0624"],
    },
    Case {
        name: "parse-error-new-with-a-locked-key",
        later: &[LEAK, "pub fn carry(text: &str) -> crate::ids::ParseError {\n    crate::ids::ParseError::new(crate::record::Key(()), \"AppId\", leak(text))\n}\n"],
        codes: &["E0603", "E0624"],
    },
    Case {
        name: "parse-error-new-with-a-key-of-its-own",
        later: &[LEAK, "pub struct Key(());\npub fn carry(text: &str) -> crate::ids::ParseError {\n    crate::ids::ParseError::new(Key(()), \"AppId\", leak(text))\n}\n"],
        codes: &["E0277", "E0624"],
    },
    Case {
        name: "unchecked-of-ids",
        later: &[RECURRENCE, "pub fn written() -> String {\n    serde_json::to_string(&crate::ids::Unchecked(&broken())).unwrap_or_default()\n}\n"],
        codes: &["E0425"],
    },
    Case {
        name: "unchecked-of-request",
        later: &[RECURRENCE, "pub fn written() -> String {\n    serde_json::to_string(&crate::request::Unchecked(&broken())).unwrap_or_default()\n}\n"],
        codes: &["E0603"],
    },
    Case {
        name: "write-fields-with-a-locked-key",
        later: &[RECURRENCE, "pub fn written() -> String {\n    let mut out = Vec::new();\n    let _ = crate::ids::Strict::write_fields(&broken(), &mut serde_json::Serializer::new(&mut out), crate::request::Key(()));\n    String::from_utf8(out).unwrap_or_default()\n}\n"],
        codes: &["E0603"],
    },
    Case {
        name: "write-fields-with-a-key-of-its-own",
        later: &[RECURRENCE, "pub struct Key(());\npub fn written() -> String {\n    let mut out = Vec::new();\n    let _ = crate::ids::Strict::write_fields(&broken(), &mut serde_json::Serializer::new(&mut out), Key(()));\n    String::from_utf8(out).unwrap_or_default()\n}\n"],
        codes: &["E0277", "E0603"],
    },
    Case {
        name: "string-type",
        later: &[LEAK, "pub struct Key(());\nfn any(s: &str) -> Result<(), &'static str> {\n    Err(leak(s))\n}\ncrate::ids::string_type! {\n    Note [Debug] = any\n}\n"],
        codes: &["E0603"],
    },
    Case {
        name: "name-enum",
        later: &["pub struct Key(());\ncrate::ids::name_enum! {\n    pub enum Mood {\n        Calm = \"calm\",\n    }\n}\n"],
        codes: &["E0603"],
    },
    Case {
        name: "parse-error-new-with-the-key-of-ids",
        later: &[LEAK, "pub fn carry(text: &str) -> crate::ids::ParseError {\n    crate::ids::ParseError::new(crate::ids::Key(()), \"AppId\", leak(text))\n}\n"],
        codes: &["E0603", "E0624"],
    },
    Case {
        name: "parse-error-new-with-the-key-of-category",
        later: &[LEAK, "pub fn carry(text: &str) -> crate::ids::ParseError {\n    crate::ids::ParseError::new(crate::category::Key(()), \"AppId\", leak(text))\n}\n"],
        codes: &["E0603", "E0624"],
    },
    Case {
        name: "parse-error-new-with-the-key-of-site",
        later: &[LEAK, "pub fn carry(text: &str) -> crate::ids::ParseError {\n    crate::ids::ParseError::new(crate::site::Key(()), \"AppId\", leak(text))\n}\n"],
        codes: &["E0603", "E0624"],
    },
    Case {
        name: "parse-error-new-with-a-key-that-the-later-file-seals",
        later: &[LEAK, "pub struct Key(());\nimpl crate::ids::key::Sealed for Key {}\npub fn carry(text: &str) -> crate::ids::ParseError {\n    crate::ids::ParseError::new(Key(()), \"AppId\", leak(text))\n}\n"],
        codes: &["E0603", "E0624"],
    },
    Case {
        name: "transmute-under-allow",
        later: &["#[allow(unsafe_code)]\npub fn forge(s: String) -> crate::site::AppId {\n    unsafe { core::mem::transmute::<String, crate::site::AppId>(s) }\n}\n"],
        codes: &["E0453", "unsafe_code"],
    },
    Case {
        name: "seal-by-its-module",
        later: &["pub struct LaterNote;\nimpl crate::kinds::sealed::Sealed for LaterNote {}\nimpl crate::kinds::Coarse for LaterNote {}\n"],
        codes: &["E0603"],
    },
    Case {
        name: "seal-by-option",
        later: &["pub struct LaterNote;\nimpl crate::kinds::Coarse for Option<LaterNote> {}\n"],
        codes: &["E0117"],
    },
    Case {
        name: "strict-struct",
        later: &["pub struct Key(());\ncrate::ids::strict_struct! {\n    pub struct Note {\n        pub n: u32,\n    }\n    rules(_n) {\n        Ok(())\n    }\n}\n"],
        codes: &["E0603"],
    },
    Case {
        name: "declared-name-by-hand",
        later: &["pub fn forged() -> crate::request::ErrorName {\n    crate::request::ErrorName::Declared(crate::request::DeclaredName(\"nobody_declares_this\"))\n}\n"],
        codes: &["E0603"],
    },
];

const BOUNDARY: &[Case] = &[
    Case {
        name: "foreign-trait-default-for-a-core-type",
        later: &["impl Default for crate::site::AppId {\n    fn default() -> Self {\n        loop {}\n    }\n}\n"],
        codes: &["E0117"],
    },
    Case {
        name: "foreign-trait-drop-for-a-core-type",
        later: &["impl Drop for crate::request::InterfaceDecision {\n    fn drop(&mut self) {}\n}\n"],
        codes: &["E0117", "E0120"],
    },
    Case {
        name: "foreign-trait-serialize-for-a-request",
        later: &["impl serde::Serialize for crate::request::InterfaceDecision {\n    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {\n        s.serialize_unit()\n    }\n}\n"],
        codes: &["E0117"],
    },
    Case {
        name: "core-trait-for-a-core-type",
        later: &["impl crate::request::RunnerBody for crate::site::AppId {\n    type Reply = ();\n}\n"],
        codes: &["E0117"],
    },
    Case {
        name: "foreign-trait-by-a-macro",
        later: &["macro_rules! give {\n    ($t:ty) => {\n        impl Default for $t {\n            fn default() -> Self {\n                loop {}\n            }\n        }\n    };\n}\ngive!(crate::site::AppId);\n"],
        codes: &["E0117"],
    },
    Case {
        name: "inherent-impl",
        later: &["impl crate::site::AppId {\n    pub fn later(&self) -> usize {\n        0\n    }\n}\n"],
        codes: &["E0116"],
    },
    Case {
        name: "inherent-impl-by-a-macro",
        later: &["macro_rules! give {\n    ($t:path) => {\n        impl $t {\n            pub fn later(&self) -> usize {\n                0\n            }\n        }\n    };\n}\ngive!(crate::site::AppId);\n"],
        codes: &["E0116"],
    },
    Case {
        name: "blanket-impl",
        later: &["impl<T: core::fmt::Debug> core::fmt::Display for T {\n    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {\n        write!(f, \"{self:?}\")\n    }\n}\n"],
        codes: &["E0210"],
    },
    Case {
        name: "blanket-impl-by-a-macro",
        later: &["macro_rules! every {\n    ($tr:path) => {\n        impl<T: Clone> $tr for T {}\n    };\n}\nevery!(crate::kinds::Coarse);\n"],
        codes: &["E0210"],
    },
    Case {
        name: "macro-export-reaches-a-private-maker",
        later: &["#[macro_export]\nmacro_rules! forged {\n    () => {\n        $crate::ids::ParseError::new($crate::ids::Key(()), \"AppId\", \"forged\")\n    };\n}\npub fn carry() -> crate::ids::ParseError {\n    forged!()\n}\n"],
        codes: &["E0603", "E0624"],
    },
    Case {
        name: "a-later-trait-by-value-read-as-the-locked-method",
        later: &[SHADOW, "pub mod wrong {\n    use crate::learning::*;\n    pub fn by_value(app: crate::site::AppId) -> &'static str {\n        app.as_str()\n    }\n}\n"],
        codes: &["E0308"],
    },
    Case {
        name: "the-old-path-is-the-core-list-beside-a-module-of-the-crate-name",
        later: &[
            CRATE_NAME_MODULE,
            "const _: &[crate::KindEntry; 0] = KINDS;\n",
        ],
        codes: &["E0308"],
    },
];

const SHADOW: &str = "pub struct Shadowed;\npub trait Shadow {\n    fn as_str(self) -> Shadowed;\n}\nimpl Shadow for crate::site::AppId {\n    fn as_str(self) -> Shadowed {\n        Shadowed\n    }\n}\n";

const CRATE_NAME_MODULE: &str = "pub mod fake {\n    pub const KINDS: &[crate::KindEntry; 0] = &[];\n    pub const ERROR_NAMES: &[&str; 0] = &[];\n}\nmod halvo_schema_core {\n    pub use super::fake as learning;\n}\n";

const COMPILES: &[(&str, &[&str])] = &[
    (
        "from-and-partial-eq-with-a-later-type",
        &["pub struct Later(pub u8);\nimpl From<Later> for crate::site::AppId {\n    fn from(_: Later) -> Self {\n        crate::site::AppId::parse(\"later.exe\").unwrap_or_else(|_| unreachable!())\n    }\n}\nimpl PartialEq<Later> for crate::site::AppId {\n    fn eq(&self, _: &Later) -> bool {\n        false\n    }\n}\npub fn made(later: Later) -> bool {\n    let app = crate::site::AppId::from(later);\n    app == Later(0)\n}\n"],
    ),
    (
        "a-later-trait-by-value-with-a-glob-import",
        &[SHADOW, "pub mod user {\n    use crate::learning::*;\n    pub fn by_value(app: crate::site::AppId) -> Shadowed {\n        app.as_str()\n    }\n    pub fn through_a_reference(app: &crate::site::AppId) -> &str {\n        app.as_str()\n    }\n}\npub mod plain {\n    pub fn by_value(app: crate::site::AppId) -> String {\n        app.as_str().to_owned()\n    }\n}\n"],
    ),
    ("the-line-beside-a-module-of-the-crate-name", &[CRATE_NAME_MODULE]),
];

const CORE_CASES: &[Case] = &[
    Case {
        name: "core-names-the-later-crate",
        later: &["pub fn later_kinds() -> usize {\n    halvo_schema::learning::KINDS.len()\n}\n"],
        codes: &["E0433"],
    },
    Case {
        name: "core-takes-the-later-crate-by-extern-crate",
        later: &["extern crate halvo_schema;\n"],
        codes: &["E0463"],
    },
];

const CORE_TWIN: &str = "pub fn later_kinds() -> usize {\n    crate::learning::KINDS.len()\n}\n";

const TWIN: &[&str] = &[
    RECURRENCE,
    "pub struct Key(());\npub struct Note(u32);\nimpl Note {\n    pub fn new(_: Key) -> Self {\n        Note(0)\n    }\n}\n",
    "pub fn parsed(s: &str) -> Result<crate::site::AppId, crate::ids::ParseError> {\n    crate::site::AppId::parse(s)\n}\n",
    "pub fn rule_of(e: crate::ids::ParseError) -> &'static str {\n    e.rule()\n}\n",
    "pub fn written() -> Option<String> {\n    serde_json::to_string(&broken()).ok()\n}\n",
    "pub fn checked(r: &crate::record::Recurrence) -> bool {\n    serde_json::to_vec(r).is_ok()\n}\n",
    "pub fn declared(name: &str) -> Option<crate::request::ErrorName> {\n    crate::request::ErrorName::read(name).ok()\n}\n",
];

const READ_AS_CODE: &[(&str, &[&str])] = &[
    (
        "issue-1-forms",
        &[
            "pub const C: &core::ffi::CStr = cr#\"\\\"#;\npub const fn after_a_raw_c_string() -> u8 {\n    1\n}\n",
            "pub const TWO: [char; 2] = ['\u{e9}','\"'];\npub const fn after_two_chars() -> u8 {\n    2\n}\n",
            "pub const fn after_a_lifetime<'a>(x: &'a str) -> char {\n    let _ = x;\n    'a'\n}\n",
            "/// a \"quote\npub const fn after_a_doc_comment() -> u8 {\n    4\n}\n",
            "pub fn r#include() -> u8 {\n    3\n}\n",
            "pub const SUM: u8 = after_a_raw_c_string() + after_two_chars() + after_a_doc_comment();\n",
        ],
    ),
    (
        "shebang",
        &[
            "#!/bin/sh \"\n",
            "#[macro_export]\nmacro_rules! hidden {\n    () => {\n        3\n    };\n}\npub const H: u8 = crate::hidden!();\n// \"\n",
        ],
    ),
];

fn copy_rust_files(from: &Path, to: &Path, top: bool) {
    std::fs::create_dir_all(to).unwrap_or_else(|e| panic!("{}: {e}", to.display()));
    let entries = std::fs::read_dir(from).unwrap_or_else(|e| panic!("{}: {e}", from.display()));
    for entry in entries {
        let path = entry.expect("a directory entry").path();
        let name = path.file_name().expect("a file name").to_owned();
        if path.is_dir() {
            if !(top && name == "bin") {
                copy_rust_files(&path, &to.join(&name), false);
            }
        } else if path.extension().is_some_and(|x| x == "rs") {
            std::fs::copy(&path, to.join(&name))
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        }
    }
}

fn fixture(root: &Path, name: &str, later: &[&str]) -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join(name);
    copy_rust_files(&manifest_dir.join("src"), &dir.join("src"), true);
    let (first, rest) = match later.split_first() {
        Some((first, rest)) if first.starts_with("#!") => (*first, rest),
        _ => ("", later),
    };
    let text: String = [first, LISTS]
        .into_iter()
        .chain(rest.iter().copied())
        .collect();
    std::fs::write(dir.join("src").join("learning.rs"), text).expect("learning.rs of the fixture");
    let manifest = std::fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("the manifest");
    assert_eq!(manifest.matches("name = \"halvo-schema\"").count(), 1);
    let core = manifest_dir.join("../halvo-schema-core");
    let core = core.to_str().expect("a path of UTF-8").replace('\\', "/");
    assert_eq!(
        manifest.matches("path = \"../halvo-schema-core\"").count(),
        1
    );
    let manifest = manifest
        .replacen(
            "name = \"halvo-schema\"",
            &format!("name = \"later-file-{name}\""),
            1,
        )
        .replacen(
            "path = \"../halvo-schema-core\"",
            &format!("path = \"{core}\""),
            1,
        )
        + "\n[workspace]\n";
    std::fs::write(dir.join("Cargo.toml"), manifest).expect("the manifest of the fixture");
    std::fs::copy(
        manifest_dir.join("../../Cargo.lock"),
        dir.join("Cargo.lock"),
    )
    .expect("the root Cargo.lock");
    dir
}

fn core_fixture(root: &Path, name: &str, appended: &str) -> PathBuf {
    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("../halvo-schema-core");
    let dir = root.join(name);
    copy_rust_files(&core.join("src"), &dir.join("src"), true);
    let kinds = std::fs::read_to_string(core.join("src").join("kinds.rs")).expect("kinds.rs");
    std::fs::write(dir.join("src").join("kinds.rs"), kinds + appended)
        .expect("kinds.rs of the fixture");
    let manifest = std::fs::read_to_string(core.join("Cargo.toml")).expect("the manifest");
    assert_eq!(manifest.matches("name = \"halvo-schema-core\"").count(), 1);
    let manifest = manifest.replacen(
        "name = \"halvo-schema-core\"",
        &format!("name = \"core-file-{name}\""),
        1,
    ) + "\n[workspace]\n";
    std::fs::write(dir.join("Cargo.toml"), manifest).expect("the manifest of the fixture");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.lock"),
        dir.join("Cargo.lock"),
    )
    .expect("the root Cargo.lock");
    dir
}

struct Build {
    ok: bool,
    codes: BTreeSet<String>,
    files: BTreeSet<String>,
    stderr: String,
}

fn check(root: &Path, dir: &Path) -> Build {
    let out = Command::new(env!("CARGO"))
        .args(["check", "--lib", "--offline", "--message-format=json"])
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_BUILD_RUSTFLAGS")
        .output()
        .expect("cargo starts");
    let mut codes = BTreeSet::new();
    let mut files = BTreeSet::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let message = &value["message"];
        if value["reason"] != "compiler-message" || message["level"] != "error" {
            continue;
        }
        let text = message["message"].as_str().unwrap_or_default();
        let code = message["code"]["code"].as_str();
        if code.is_none() && text.starts_with("aborting due to") {
            continue;
        }
        codes.insert(code.unwrap_or("none").to_owned());
        for span in message["spans"].as_array().into_iter().flatten() {
            if span["is_primary"] == true {
                let mut site = span;
                while site["expansion"].is_object() {
                    site = &site["expansion"]["span"];
                }
                files.insert(
                    site["file_name"]
                        .as_str()
                        .unwrap_or("none")
                        .replace('\\', "/"),
                );
            }
        }
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let tail: Vec<&str> = stderr.lines().rev().take(12).collect();
    Build {
        ok: out.status.success(),
        codes,
        files,
        stderr: tail.into_iter().rev().collect::<Vec<_>>().join("\n"),
    }
}

#[test]
fn a_later_file_that_tries_a_maker_of_ids_rs_or_unsafe_code_does_not_compile() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("later-file-builds");
    let twin = check(&root, &fixture(&root, "twin", TWIN));
    assert!(
        twin.ok && twin.codes.is_empty(),
        "the twin, a later file that uses only what is public, must compile: {:?}\n{}",
        twin.codes,
        twin.stderr
    );
    let wrong = refused(&root, CASES, "src/learning.rs", false);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

fn refused(root: &Path, cases: &[Case], file: &str, core: bool) -> Vec<String> {
    let mut wrong = Vec::new();
    for case in cases {
        let dir = if core {
            core_fixture(root, case.name, case.later.concat().as_str())
        } else {
            fixture(root, case.name, case.later)
        };
        let build = check(root, &dir);
        let want: BTreeSet<String> = case.codes.iter().map(|c| (*c).to_owned()).collect();
        let in_file = build.files.iter().all(|f| f == file);
        if build.ok || build.codes != want || !in_file {
            wrong.push(format!(
                "{}: compiled={} codes={:?} want={:?} files={:?}\n{}",
                case.name, build.ok, build.codes, want, build.files, build.stderr
            ));
        }
    }
    wrong
}

#[test]
fn a_later_file_that_gives_a_core_type_code_of_its_own_does_not_compile() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("later-file-builds");
    let wrong = refused(&root, BOUNDARY, "src/learning.rs", false);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_later_file_compiles_what_the_boundary_allows_and_a_by_value_method_of_its_trait_runs_where_the_trait_is_in_scope(
) {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("later-file-builds");
    let mut wrong = Vec::new();
    for (name, later) in COMPILES {
        let build = check(&root, &fixture(&root, name, later));
        if !build.ok || !build.codes.is_empty() {
            wrong.push(format!(
                "{name}: compiled={} codes={:?}\n{}",
                build.ok, build.codes, build.stderr
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn no_line_of_the_core_crate_can_name_the_later_crate() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("core-file-builds");
    let twin = check(&root, &core_fixture(&root, "twin", CORE_TWIN));
    assert!(
        twin.ok && twin.codes.is_empty(),
        "the twin, a core file that names its own list, must compile: {:?}\n{}",
        twin.codes,
        twin.stderr
    );
    let wrong = refused(&root, CORE_CASES, "src/kinds.rs", true);
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_compiler_reads_as_code_what_follows_each_form_of_the_lexer_cases() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("later-file-builds");
    let mut wrong = Vec::new();
    for (name, later) in READ_AS_CODE {
        let build = check(&root, &fixture(&root, name, later));
        if !build.ok || !build.codes.is_empty() {
            wrong.push(format!(
                "{name}: compiled={} codes={:?}\n{}",
                build.ok, build.codes, build.stderr
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
