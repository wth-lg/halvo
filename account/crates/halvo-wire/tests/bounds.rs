use std::{fs, path::PathBuf};

use halvo_wire::{CAP_AMOUNT_MAX, CAP_AMOUNT_MIN, URL_MAX_LENGTH};
use serde_json::Value;

#[test]
fn the_bounds_are_the_numbers_of_the_vendored_schema() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema/schema.v1.json");
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let schema: Value = serde_json::from_str(&text).expect("the vendored schema is JSON");
    assert_eq!(
        schema.pointer("/$defs/Cap/properties/amount/minimum"),
        Some(&Value::from(CAP_AMOUNT_MIN))
    );
    assert_eq!(
        schema.pointer("/$defs/Cap/properties/amount/maximum"),
        Some(&Value::from(CAP_AMOUNT_MAX))
    );
    assert_eq!(
        schema.pointer("/$defs/Url/maxLength"),
        Some(&Value::from(URL_MAX_LENGTH))
    );
    assert_eq!(
        (CAP_AMOUNT_MIN, CAP_AMOUNT_MAX, URL_MAX_LENGTH),
        (1, 100_000_000, 2048)
    );
}
