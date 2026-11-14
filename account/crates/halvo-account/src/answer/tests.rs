use super::{check, Fault, Raw};
use serde_json::{json, Map, Value};

fn defs(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => panic!("defs are an object"),
    }
}

fn raw(text: &str) -> Raw {
    serde_json::from_str(text).expect("a JSON text")
}

fn object_of(property: Value) -> Value {
    json!({"type": "object", "properties": {"a": property}, "required": ["a"], "additionalProperties": false})
}

#[test]
fn a_keyword_that_the_walk_does_not_know_fails_closed() {
    let none = defs(json!({}));
    for node in [
        json!({"type": "string", "pattern": "^x$"}),
        json!({"type": "string", "format": "uuid"}),
        json!({"type": "integer", "minimum": 0, "multipleOf": 2}),
        json!({"type": "object", "properties": {}, "additionalProperties": false, "minProperties": 1}),
        json!({"anyOf": [{"type": "string"}]}),
        json!({"const": "x"}),
    ] {
        assert_eq!(
            check(&none, &node, &raw(r#""x""#), 0),
            Err(Fault::Mapping),
            "{node}"
        );
    }
}

#[test]
fn a_type_that_the_walk_does_not_know_fails_closed() {
    let none = defs(json!({}));
    for node in [
        json!({"type": "number"}),
        json!({"type": "boolean"}),
        json!({"type": "null"}),
        json!({"type": "array"}),
        json!({}),
        json!({"type": ["string", "null"]}),
    ] {
        assert_eq!(
            check(&none, &node, &raw("null"), 0),
            Err(Fault::Mapping),
            "{node}"
        );
    }
}

#[test]
fn an_object_that_admits_other_keys_fails_closed() {
    let none = defs(json!({}));
    for node in [
        json!({"type": "object", "properties": {}}),
        json!({"type": "object", "properties": {}, "additionalProperties": true}),
        json!({"type": "object", "properties": {}, "additionalProperties": {}}),
        json!({"type": "object", "additionalProperties": false}),
    ] {
        assert_eq!(
            check(&none, &node, &raw("{}"), 0),
            Err(Fault::Mapping),
            "{node}"
        );
    }
}

#[test]
fn a_reference_is_followed_only_when_it_stands_alone_and_names_a_definition() {
    let named = defs(json!({"S": {"type": "string", "enum": ["x"]}}));
    assert_eq!(
        check(&named, &json!({"$ref": "#/$defs/S"}), &raw(r#""x""#), 0),
        Ok(())
    );
    assert_eq!(
        check(&named, &json!({"$ref": "#/$defs/S"}), &raw(r#""y""#), 0),
        Err(Fault::Shape)
    );
    for node in [
        json!({"$ref": "#/$defs/T"}),
        json!({"$ref": "#/definitions/S"}),
        json!({"$ref": "https://example.invalid/s.json"}),
        json!({"$ref": "#/$defs/S", "type": "string"}),
        json!({"$ref": 1}),
    ] {
        assert_eq!(
            check(&named, &node, &raw(r#""x""#), 0),
            Err(Fault::Mapping),
            "{node}"
        );
    }
}

#[test]
fn a_reference_that_leads_back_to_itself_ends_at_the_depth_limit() {
    let looped = defs(json!({"L": {"$ref": "#/$defs/L"}}));
    assert_eq!(
        check(&looped, &json!({"$ref": "#/$defs/L"}), &raw("1"), 0),
        Err(Fault::Mapping)
    );
}

#[test]
fn a_negative_minimum_admits_a_negative_integer_written_with_its_sign() {
    let none = defs(json!({}));
    let node = json!({"type": "integer", "minimum": -5, "maximum": 5});
    assert_eq!(check(&none, &node, &raw("-5"), 0), Ok(()));
    assert_eq!(check(&none, &node, &raw("-6"), 0), Err(Fault::Bound));
    assert_eq!(check(&none, &node, &raw("-0"), 0), Err(Fault::IntegerForm));
    assert_eq!(
        check(&none, &node, &raw("-1.0"), 0),
        Err(Fault::IntegerForm)
    );
}

#[test]
fn an_object_checks_each_property_and_requires_the_required_ones() {
    let none = defs(json!({}));
    let node = object_of(json!({"type": "integer", "minimum": 1, "maximum": 3}));
    assert_eq!(check(&none, &node, &raw(r#"{"a":2}"#), 0), Ok(()));
    assert_eq!(
        check(&none, &node, &raw(r#"{"a":4}"#), 0),
        Err(Fault::Bound)
    );
    assert_eq!(check(&none, &node, &raw("{}"), 0), Err(Fault::Shape));
    assert_eq!(
        check(&none, &node, &raw(r#"{"a":2,"b":2}"#), 0),
        Err(Fault::Shape)
    );
    assert_eq!(
        check(&none, &node, &raw(r#"{"a":2,"a":2}"#), 0),
        Err(Fault::Shape)
    );
    assert_eq!(check(&none, &node, &raw(r#"[2]"#), 0), Err(Fault::Shape));
}

#[test]
fn a_string_length_counts_characters_and_not_bytes() {
    let none = defs(json!({}));
    let node = json!({"type": "string", "maxLength": 2});
    assert_eq!(check(&none, &node, &raw(r#""éé""#), 0), Ok(()));
    assert_eq!(check(&none, &node, &raw(r#""ééé""#), 0), Err(Fault::Bound));
    assert_eq!(check(&none, &node, &raw("2"), 0), Err(Fault::Shape));
}
