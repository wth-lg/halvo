mod generate;

use std::fs;

use serde_json::Value;

fn read(path: std::path::PathBuf) -> String {
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn serde_words(attrs: &[syn::Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("serde"))
        .filter_map(|attr| attr.meta.require_list().ok())
        .flat_map(|list| list.tokens.clone().into_iter().map(|tree| tree.to_string()))
        .collect()
}

#[test]
fn committed_types_equal_a_fresh_generation() {
    let fresh = generate::generate(&read(generate::schema_path()));
    let committed = read(generate::types_path());
    assert!(
        committed == fresh,
        "src/types.rs differs from what typify 0.8.0 generates from account/schema/schema.v1.json: \
         run cargo run --manifest-path account/Cargo.toml --locked -p halvo-wire --example write-types"
    );
}

#[test]
fn a_changed_schema_changes_the_types() {
    let mut schema: Value =
        serde_json::from_str(&read(generate::schema_path())).expect("the vendored schema is JSON");
    schema
        .pointer_mut("/$defs/StatusAnswer/properties/result/enum")
        .and_then(Value::as_array_mut)
        .expect("StatusAnswer has a result enum")
        .push(Value::from("trial"));
    let changed =
        generate::generate(&serde_json::to_string_pretty(&schema).expect("a value prints"));
    let committed = read(generate::types_path());
    assert!(
        changed != committed,
        "a schema with one more result value gave the committed types"
    );
    assert!(changed.contains("Trial") && !committed.contains("Trial"));
}

#[test]
fn every_def_has_a_type_and_every_object_type_is_closed() {
    let schema: Value =
        serde_json::from_str(&read(generate::schema_path())).expect("the vendored schema is JSON");
    let defs = schema["$defs"].as_object().expect("the schema has $defs");
    let file = syn::parse_file(&read(generate::types_path())).expect("src/types.rs parses");
    let mut names = Vec::new();
    let mut structs = Vec::new();
    for item in &file.items {
        match item {
            syn::Item::Struct(item) => {
                names.push(item.ident.to_string());
                if matches!(item.fields, syn::Fields::Named(_)) {
                    let closed = serde_words(&item.attrs)
                        .iter()
                        .any(|word| word == "deny_unknown_fields");
                    structs.push((item.ident.to_string(), closed));
                }
            }
            syn::Item::Enum(item) => names.push(item.ident.to_string()),
            syn::Item::Type(item) => names.push(item.ident.to_string()),
            _ => {}
        }
    }
    let missing: Vec<&String> = defs.keys().filter(|def| !names.contains(def)).collect();
    assert!(missing.is_empty(), "no generated type for {missing:?}");
    let objects: Vec<&String> = defs
        .iter()
        .filter(|(_, def)| def["type"] == "object")
        .map(|(name, _)| name)
        .collect();
    let mut struct_names: Vec<&String> = structs.iter().map(|(name, _)| name).collect();
    struct_names.sort();
    let mut object_names = objects.clone();
    object_names.sort();
    assert_eq!(
        struct_names, object_names,
        "one struct with named fields for each object schema"
    );
    let open: Vec<&String> = structs
        .iter()
        .filter(|(_, closed)| !closed)
        .map(|(name, _)| name)
        .collect();
    assert!(
        open.is_empty(),
        "structs without deny_unknown_fields: {open:?}"
    );
}
