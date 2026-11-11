#[path = "../tests/generate/mod.rs"]
mod generate;

use std::fs;

fn main() {
    let schema_path = generate::schema_path();
    let text = fs::read_to_string(&schema_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", schema_path.display()));
    let types = generate::generate(&text);
    let types_path = generate::types_path();
    let before = fs::read_to_string(&types_path).unwrap_or_default();
    if before == types {
        println!("write-types: src/types.rs is equal to a fresh generation, kept");
    } else {
        fs::write(&types_path, types)
            .unwrap_or_else(|error| panic!("cannot write {}: {error}", types_path.display()));
        println!("write-types: src/types.rs written");
    }
}
