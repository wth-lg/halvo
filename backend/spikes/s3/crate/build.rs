use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-env-changed=S3_SCHEMA");
    let name = env::var("S3_SCHEMA").unwrap_or_else(|_| "extend".to_string());
    assert!(
        !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
        "S3_SCHEMA must be a file name under schemas/, without .json"
    );
    let source = format!("schemas/{name}.json");
    println!("cargo:rerun-if-changed={source}");
    println!("cargo:rustc-env=S3_SCHEMA_NAME={name}");

    let text =
        fs::read_to_string(&source).unwrap_or_else(|error| panic!("cannot read {source}: {error}"));
    let schema: schemars::schema::RootSchema = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("schemars 0.8 cannot read {source}: {error}"));
    let mut space = typify::TypeSpace::new(&typify::TypeSpaceSettings::default());
    space
        .add_root_schema(schema)
        .unwrap_or_else(|error| panic!("typify cannot convert {source}: {error}"));
    let file = syn::parse2::<syn::File>(space.to_stream()).unwrap_or_else(|error| {
        panic!("the code typify wrote for {source} does not parse: {error}")
    });

    let out = Path::new(&env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("generated.rs");
    fs::write(&out, prettyplease::unparse(&file))
        .unwrap_or_else(|error| panic!("cannot write {}: {error}", out.display()));
}
