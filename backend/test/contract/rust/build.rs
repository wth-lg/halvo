use std::{env, fs, path::Path};

const DEFS: [&str; 13] = [
    "Cap",
    "Period",
    "Spend",
    "Tier",
    "Url",
    "CapBody",
    "CheckoutAnswer",
    "StatusAnswer",
    "CapGetAnswer",
    "CapSetAnswer",
    "ManageAnswer",
    "DeleteAnswer",
    "Rejected",
];

fn names_of_items(file: &syn::File) -> Vec<String> {
    file.items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Struct(item) => Some(item.ident.to_string()),
            syn::Item::Enum(item) => Some(item.ident.to_string()),
            syn::Item::Type(item) => Some(item.ident.to_string()),
            _ => None,
        })
        .collect()
}

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let schema_path = Path::new(&manifest).join("../../../contract/schema/v1.json");
    println!("cargo:rerun-if-changed={}", schema_path.display());
    println!("cargo:rustc-check-cfg=cfg(every_def_has_a_type)");
    let text = fs::read_to_string(&schema_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", schema_path.display()));
    let schema: schemars::schema::RootSchema = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("schemars 0.8 cannot read the schema export: {error}"));
    let mut space = typify::TypeSpace::new(&typify::TypeSpaceSettings::default());
    space
        .add_root_schema(schema)
        .unwrap_or_else(|error| panic!("typify cannot convert the schema export: {error}"));
    let file = syn::parse2::<syn::File>(space.to_stream())
        .unwrap_or_else(|error| panic!("the code typify wrote does not parse: {error}"));
    let names = names_of_items(&file);
    let missing: Vec<&str> = DEFS
        .iter()
        .copied()
        .filter(|def| !names.iter().any(|name| name == def))
        .collect();
    if missing.is_empty() {
        println!("cargo:rustc-cfg=every_def_has_a_type");
    }
    let out = Path::new(&env::var("OUT_DIR").expect("cargo sets OUT_DIR")).to_path_buf();
    fs::write(out.join("generated.rs"), prettyplease::unparse(&file))
        .unwrap_or_else(|error| panic!("cannot write generated.rs: {error}"));
    fs::write(out.join("missing.txt"), missing.join(","))
        .unwrap_or_else(|error| panic!("cannot write missing.txt: {error}"));
}
