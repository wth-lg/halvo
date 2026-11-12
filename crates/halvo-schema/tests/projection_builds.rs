use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

const LATER: &str = "pub const KINDS: &[crate::kinds::KindEntry] = &[];\npub const ERROR_NAMES: &[&str] = &[];\npub trait Pick {\n    type Out;\n}\nimpl Pick for () {\n    type Out = crate::kinds::KindEntry;\n}\n";

const SHOW: &str = " {\n    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        f.write_str(&self.json)\n    }\n}\n";

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

fn fixture(root: &Path, name: &str, later: &str) -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join(name);
    copy_rust_files(&manifest_dir.join("src"), &dir.join("src"), true);
    std::fs::write(
        dir.join("src").join("learning.rs"),
        format!("{LATER}{later}"),
    )
    .expect("learning.rs of the fixture");
    let manifest = std::fs::read_to_string(manifest_dir.join("Cargo.toml")).expect("the manifest");
    assert_eq!(manifest.matches("name = \"halvo-schema\"").count(), 1);
    let manifest = manifest.replacen(
        "name = \"halvo-schema\"",
        &format!("name = \"projection-{name}\""),
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

fn errors(root: &Path, dir: &Path) -> (bool, BTreeSet<(String, String)>) {
    let out = Command::new(env!("CARGO"))
        .args(["check", "--lib", "--offline", "--message-format=json"])
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("CARGO_BUILD_RUSTFLAGS")
        .output()
        .expect("cargo starts");
    let mut found = BTreeSet::new();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let message = &value["message"];
        if value["reason"] != "compiler-message" || message["level"] != "error" {
            continue;
        }
        let Some(code) = message["code"]["code"].as_str() else {
            continue;
        };
        for span in message["spans"].as_array().into_iter().flatten() {
            if span["is_primary"] == true {
                let file = span["file_name"].as_str().unwrap_or("none");
                found.insert((code.to_owned(), file.replace('\\', "/")));
            }
        }
    }
    (out.status.success(), found)
}

#[test]
fn a_method_on_a_projection_does_not_compile_and_a_projection_through_a_foreign_trait_of_a_foreign_type_fails_the_orphan_rule(
) {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("projection-builds");
    let twin = errors(
        &root,
        &fixture(
            &root,
            "twin",
            "pub fn later(entry: &<() as Pick>::Out) -> &str {\n    entry.family\n}\n",
        ),
    );
    assert_eq!(twin, (true, BTreeSet::new()), "the twin must compile");
    let cases = [
        (
            "method-on-a-projection",
            "impl <() as Pick>::Out {\n    pub(crate) fn later_says_ok(&self) -> bool {\n        false\n    }\n}\n".to_owned(),
            "E0118",
        ),
        (
            "foreign-trait-of-a-foreign-type",
            format!("impl std::fmt::Display for <Vec<crate::record::TypedRecord> as IntoIterator>::Item{SHOW}"),
            "E0117",
        ),
    ];
    for (name, later, code) in cases {
        let build = errors(&root, &fixture(&root, name, &later));
        let want: BTreeSet<(String, String)> = [(code.to_owned(), "src/learning.rs".to_owned())]
            .into_iter()
            .collect();
        assert_eq!(build, (false, want), "{name}");
    }
}
