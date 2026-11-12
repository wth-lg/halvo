use std::{env, fs, path::Path};

const BOUNDS: [(&str, &str, &str, &str); 3] = [
    (
        "CAP_AMOUNT_MIN",
        "u64",
        "/$defs/Cap/properties/amount/minimum",
        "The smallest cap amount, in minor units, that the vendored schema admits.",
    ),
    (
        "CAP_AMOUNT_MAX",
        "u64",
        "/$defs/Cap/properties/amount/maximum",
        "The largest cap amount, in minor units, that the vendored schema admits.",
    ),
    (
        "URL_MAX_LENGTH",
        "usize",
        "/$defs/Url/maxLength",
        "The most characters that a `Url` of the vendored schema holds.",
    ),
];

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let schema_path = Path::new(&manifest).join("../../schema/schema.v1.json");
    println!("cargo:rerun-if-changed={}", schema_path.display());
    let text = fs::read_to_string(&schema_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", schema_path.display()));
    let schema: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("the vendored schema is not JSON: {error}"));
    let mut out = String::new();
    for (name, ty, pointer, doc) in BOUNDS {
        let value = schema
            .pointer(pointer)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| {
                panic!("the vendored schema holds no unsigned integer at {pointer}")
            });
        out.push_str(&format!(
            "#[doc = {doc:?}]\npub const {name}: {ty} = {value};\n"
        ));
    }
    let out_dir = env::var("OUT_DIR").expect("cargo sets OUT_DIR");
    fs::write(Path::new(&out_dir).join("bounds.rs"), out)
        .unwrap_or_else(|error| panic!("cannot write bounds.rs: {error}"));
}
