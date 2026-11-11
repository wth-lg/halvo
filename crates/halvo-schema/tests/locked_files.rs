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

#[test]
fn no_locked_file_names_an_item_of_the_files_of_later_runs_but_their_kind_lists() {
    for file in LOCKED {
        let text = source(file);
        for module in LATER {
            let named = text.matches(&format!("{module}::")).count();
            if file == "kinds.rs" {
                assert_eq!(named, 1, "{file} names {module}:: {named} times");
                assert_eq!(
                    text.matches(&format!("crate::{module}::KINDS")).count(),
                    1,
                    "{file}"
                );
            } else {
                assert_eq!(named, 0, "{file} names {module}::");
            }
        }
    }
}

#[test]
fn lib_rs_declares_the_four_files_of_later_runs_and_reexports_none_of_their_items() {
    let lib = source("lib.rs");
    for module in LATER {
        assert_eq!(
            lib.matches(&format!("pub mod {module};\n")).count(),
            1,
            "{module}"
        );
        assert_eq!(
            lib.matches(module).count(),
            1,
            "lib.rs names {module} beyond its module line"
        );
    }
}
