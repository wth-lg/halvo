use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-changed=schema.json");
    let text = fs::read_to_string("schema.json").expect("the probe wrote schema.json");
    let schema: schemars::schema::RootSchema =
        serde_json::from_str(&text).expect("schemars 0.8 reads the probe's schema");
    let mut space = typify::TypeSpace::new(&typify::TypeSpaceSettings::default());
    space
        .add_root_schema(schema)
        .expect("typify converts the probe's schema");
    let file = syn::parse2::<syn::File>(space.to_stream()).expect("the generated code parses");
    let out = Path::new(&env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("generated.rs");
    fs::write(out, prettyplease::unparse(&file)).expect("the build writes generated.rs");
}
