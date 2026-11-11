use std::path::PathBuf;

pub const HEADER: &str = "//! The types that typify 0.8.0 generates from `account/schema/schema.v1.json`.\n//! `cargo run -p halvo-wire --example write-types` writes this file, and the test\n//! `committed_types_equal_a_fresh_generation` fails when it differs.\n\n";

pub fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn schema_path() -> PathBuf {
    crate_dir().join("../../schema/schema.v1.json")
}

pub fn types_path() -> PathBuf {
    crate_dir().join("src/types.rs")
}

pub fn generate(schema_text: &str) -> String {
    let schema: schemars::schema::RootSchema = serde_json::from_str(schema_text)
        .unwrap_or_else(|error| panic!("schemars 0.8 cannot read the schema: {error}"));
    let mut space = typify::TypeSpace::new(&typify::TypeSpaceSettings::default());
    space
        .add_root_schema(schema)
        .unwrap_or_else(|error| panic!("typify cannot convert the schema: {error}"));
    let file = syn::parse2::<syn::File>(space.to_stream())
        .unwrap_or_else(|error| panic!("the code that typify wrote does not parse: {error}"));
    format!("{HEADER}{}", prettyplease::unparse(&file))
}
