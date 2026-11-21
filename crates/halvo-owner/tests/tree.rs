//! The tree check of the owner's build (S-68 (4)).

use std::path::{Path, PathBuf};

/// The members of the workspace, as the root Cargo.toml names them on its line 2: src-tauri and every directory under
/// crates/ that holds a Cargo.toml.
fn manifests(root: &Path) -> Vec<PathBuf> {
    let members = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert_eq!(
        members.lines().nth(1),
        Some(r#"members = ["src-tauri", "crates/*"]"#),
        "the members of the workspace changed; this test reads src-tauri and crates/* alone"
    );
    let mut found = vec![root.join("src-tauri").join("Cargo.toml")];
    for entry in std::fs::read_dir(root.join("crates")).unwrap() {
        let manifest = entry.unwrap().path().join("Cargo.toml");
        if manifest.is_file() {
            found.push(manifest);
        }
    }
    found.sort();
    found
}

/// Rule 7: no manifest of the workspace but halvo-derived's own names the feature `test-host`, there in its
/// declaration (line 8) and in its own dev-dependency (line 19), so the owner's build cannot open a store with no key.
#[test]
fn no_manifest_but_halvo_derived_s_own_names_test_host() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let manifests = manifests(&root);
    assert!(manifests
        .iter()
        .any(|m| m.ends_with("crates/halvo-owner/Cargo.toml")));
    let mut named = Vec::new();
    for manifest in &manifests {
        let text = std::fs::read_to_string(manifest).unwrap();
        for (at, line) in text.lines().enumerate() {
            if line.contains("test-host") {
                let place = manifest.strip_prefix(&root).unwrap();
                let place: Vec<_> = place
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect();
                named.push((place.join("/"), at + 1));
            }
        }
    }
    assert_eq!(
        named,
        [
            ("crates/halvo-derived/Cargo.toml".to_owned(), 8),
            ("crates/halvo-derived/Cargo.toml".to_owned(), 19),
        ]
    );
}
