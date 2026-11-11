use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const KINDS: &str = "pub const KINDS: &[crate::kinds::KindEntry] = &[];\n";

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
        codes: &["E0061", "E0277"],
    },
    Case {
        name: "parse-error-new-with-a-locked-key",
        later: &[LEAK, "pub fn carry(text: &str) -> crate::ids::ParseError {\n    crate::ids::ParseError::new(crate::record::Key(()), \"AppId\", leak(text))\n}\n"],
        codes: &["E0603"],
    },
    Case {
        name: "parse-error-new-with-a-key-of-its-own",
        later: &[LEAK, "pub struct Key(());\npub fn carry(text: &str) -> crate::ids::ParseError {\n    crate::ids::ParseError::new(Key(()), \"AppId\", leak(text))\n}\n"],
        codes: &["E0277"],
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
        codes: &["E0277"],
    },
    Case {
        name: "string-type",
        later: &[LEAK, "pub struct Key(());\nfn any(s: &str) -> Result<(), &'static str> {\n    Err(leak(s))\n}\ncrate::ids::string_type! {\n    Note [Debug] = any\n}\n"],
        codes: &["E0277"],
    },
    Case {
        name: "name-enum",
        later: &["pub struct Key(());\ncrate::ids::name_enum! {\n    pub enum Mood {\n        Calm = \"calm\",\n    }\n}\n"],
        codes: &["E0277"],
    },
    Case {
        name: "seal-by-its-module",
        later: &["pub struct LaterNote;\nimpl crate::kinds::sealed::Sealed for LaterNote {}\nimpl crate::kinds::Coarse for LaterNote {}\n"],
        codes: &["E0603"],
    },
    Case {
        name: "seal-by-option",
        later: &["pub struct LaterNote;\nimpl crate::kinds::Coarse for Option<LaterNote> {}\n"],
        codes: &["E0277"],
    },
    Case {
        name: "strict-struct",
        later: &["pub struct Key(());\ncrate::ids::strict_struct! {\n    pub struct Note {\n        pub n: u32,\n    }\n    rules(_n) {\n        Ok(())\n    }\n}\n"],
        codes: &["E0277"],
    },
];

const TWIN: &[&str] = &[
    RECURRENCE,
    "pub struct Key(());\npub struct Note(u32);\nimpl Note {\n    pub fn new(_: Key) -> Self {\n        Note(0)\n    }\n}\n",
    "pub fn parsed(s: &str) -> Result<crate::site::AppId, crate::ids::ParseError> {\n    crate::site::AppId::parse(s)\n}\n",
    "pub fn rule_of(e: crate::ids::ParseError) -> &'static str {\n    e.rule()\n}\n",
    "pub fn written() -> Option<String> {\n    serde_json::to_string(&broken()).ok()\n}\n",
    "pub fn checked(r: &crate::record::Recurrence) -> bool {\n    serde_json::to_vec(r).is_ok()\n}\n",
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
    let text: String = [first, KINDS]
        .into_iter()
        .chain(rest.iter().copied())
        .collect();
    std::fs::write(dir.join("src").join("learning.rs"), text).expect("learning.rs of the fixture");
    let manifest = std::fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("the manifest");
    assert_eq!(manifest.matches("name = \"halvo-schema\"").count(), 1);
    let manifest = manifest.replacen(
        "name = \"halvo-schema\"",
        &format!("name = \"later-file-{name}\""),
        1,
    ) + "\n[workspace]\n";
    std::fs::write(dir.join("Cargo.toml"), manifest).expect("the manifest of the fixture");
    std::fs::copy(
        manifest_dir.join("../../Cargo.lock"),
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
    let mut wrong = Vec::new();
    for case in CASES {
        let build = check(&root, &fixture(&root, case.name, case.later));
        let want: BTreeSet<String> = case.codes.iter().map(|c| (*c).to_owned()).collect();
        let in_later = build.files.iter().all(|f| f == "src/learning.rs");
        if build.ok || build.codes != want || !in_later {
            wrong.push(format!(
                "{}: compiled={} codes={:?} want={:?} files={:?}\n{}",
                case.name, build.ok, build.codes, want, build.files, build.stderr
            ));
        }
    }
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
