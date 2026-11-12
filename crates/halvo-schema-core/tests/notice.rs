const ROW: &str = "Public Suffix List | https://publicsuffix.org/list/public_suffix_list.dat | MPL-2.0, https://mozilla.org/MPL/2.0/ | rules embedded by the crate psl 2.1.246, without the comment lines of the list";

fn read(path: &str) -> String {
    match std::fs::read_to_string(path) {
        Ok(text) => text.replace("\r\n", "\n"),
        Err(e) => panic!("{path}: {e}"),
    }
}

#[test]
fn the_notice_is_the_one_row_of_the_public_suffix_list() {
    let notice = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/NOTICE")).unwrap();
    assert_eq!(String::from_utf8(notice).unwrap(), format!("{ROW}\n"));
}

#[test]
fn the_notice_the_manifest_and_the_lock_name_one_version_of_psl() {
    let manifest = read(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
    let pins: Vec<&str> = manifest
        .lines()
        .filter(|line| line.starts_with("psl"))
        .collect();
    assert_eq!(pins.len(), 1, "{pins:?}");
    let version = pins[0]
        .strip_prefix("psl = \"=")
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("not an exact pin: {}", pins[0]));

    let lock = read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.lock"));
    let packages: Vec<&str> = lock
        .split("[[package]]\n")
        .filter(|package| package.starts_with("name = \"psl\"\n"))
        .collect();
    assert_eq!(packages.len(), 1, "{packages:?}");
    assert!(
        packages[0].contains(&format!("\nversion = \"{version}\"\n")),
        "{}",
        packages[0]
    );

    assert!(
        ROW.contains(&format!("the crate psl {version},")),
        "{version}"
    );
}
