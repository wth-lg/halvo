use std::{fs, path::PathBuf};

use halvo_wire::types::{
    CapBody, CapGetAnswer, CapSetAnswer, CheckoutAnswer, DeleteAnswer, ManageAnswer, Rejected,
    StatusAnswer,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

fn vendored(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../schema")
        .join(name);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{name} is not JSON: {error}"))
}

fn round_trip<T: DeserializeOwned + Serialize>(value: &Value) -> Result<Value, String> {
    let typed: T = serde_json::from_value(value.clone()).map_err(|error| error.to_string())?;
    serde_json::to_value(&typed).map_err(|error| error.to_string())
}

fn read_as(def: &str, value: &Value) -> Result<Value, String> {
    match def {
        "CapBody" => round_trip::<CapBody>(value),
        "CheckoutAnswer" => round_trip::<CheckoutAnswer>(value),
        "StatusAnswer" => round_trip::<StatusAnswer>(value),
        "CapGetAnswer" => round_trip::<CapGetAnswer>(value),
        "CapSetAnswer" => round_trip::<CapSetAnswer>(value),
        "ManageAnswer" => round_trip::<ManageAnswer>(value),
        "DeleteAnswer" => round_trip::<DeleteAnswer>(value),
        "Rejected" => round_trip::<Rejected>(value),
        other => Err(format!("no generated type is read for {other}")),
    }
}

fn samples() -> Vec<(String, String, Value)> {
    let file = vendored("samples.v1.json");
    let routes = vendored("routes.v1.json");
    let mut out = Vec::new();
    let mut answers_expected = 0;
    for route in routes["routes"].as_array().expect("routes is a list") {
        if route["kind"] != "app" {
            continue;
        }
        let name = route["name"].as_str().expect("a route has a name");
        if let Some(request) = route["request"].as_str() {
            let body = file["requests"]
                .get(name)
                .unwrap_or_else(|| panic!("no request sample for {name}"));
            out.push((
                format!("requests/{name}"),
                request.to_string(),
                body.clone(),
            ));
        }
        let answer = route["answer"]
            .as_str()
            .expect("an app route has an answer");
        let results = route["results"]
            .as_object()
            .expect("an app route has results");
        let by_result = file["answers"][name]
            .as_object()
            .unwrap_or_else(|| panic!("no answer samples for {name}"));
        let mut sample_keys: Vec<&String> = by_result.keys().collect();
        let mut result_keys: Vec<&String> = results.keys().collect();
        sample_keys.sort();
        result_keys.sort();
        assert_eq!(
            sample_keys, result_keys,
            "the answer samples of {name} are its results"
        );
        answers_expected += results.len();
        for (result, sample) in by_result {
            out.push((
                format!("answers/{name}/{result}"),
                answer.to_string(),
                sample.clone(),
            ));
        }
    }
    out.push((
        "rejected".to_string(),
        "Rejected".to_string(),
        file["rejected"].clone(),
    ));
    assert_eq!(
        file["requests"].as_object().map(|requests| requests.len()),
        Some(2)
    );
    assert_eq!(out.len(), 2 + answers_expected + 1);
    out
}

fn object_pointers(value: &Value, pointer: String, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            out.push(pointer.clone());
            for (key, member) in map {
                let token = key.replace('~', "~0").replace('/', "~1");
                object_pointers(member, format!("{pointer}/{token}"), out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                object_pointers(item, format!("{pointer}/{index}"), out);
            }
        }
        _ => {}
    }
}

#[test]
fn every_sample_parses_into_its_type_and_prints_back_the_same_value() {
    let samples = samples();
    assert_eq!(samples.len(), 32);
    let failed: Vec<String> = samples
        .iter()
        .filter_map(|(label, def, value)| match read_as(def, value) {
            Ok(printed) if printed == *value => None,
            Ok(printed) => Some(format!("{label}: {def} printed {printed}")),
            Err(error) => Some(format!("{label}: {def} refused it: {error}")),
        })
        .collect();
    assert!(failed.is_empty(), "{failed:#?}");
}

#[test]
fn every_sample_with_an_unknown_key_at_any_object_is_refused() {
    let mut read = Vec::new();
    let mut cases = 0;
    for (label, def, value) in samples() {
        let mut pointers = Vec::new();
        object_pointers(&value, String::new(), &mut pointers);
        for pointer in pointers {
            let mut copy = value.clone();
            copy.pointer_mut(&pointer)
                .and_then(Value::as_object_mut)
                .expect("the pointer names an object")
                .insert("x".to_string(), Value::from("x"));
            cases += 1;
            if read_as(&def, &copy).is_ok() {
                read.push(format!("{label} at {pointer:?}"));
            }
        }
    }
    assert!(cases > 32);
    assert!(read.is_empty(), "read with an unknown key: {read:#?}");
}
