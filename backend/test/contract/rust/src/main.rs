use std::{env, fs};

use serde_json::{json, Value};
use syn::visit::{self, Visit};

mod generated {
    #![allow(dead_code, unused_imports, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

const GENERATED: &str = include_str!(concat!(env!("OUT_DIR"), "/generated.rs"));
const MISSING: &str = include_str!(concat!(env!("OUT_DIR"), "/missing.txt"));
const UNKNOWN_KEY: &str = "x";
const TYPES_THAT_HOLD_UNDECLARED_KEYS: [&str; 5] =
    ["HashMap", "BTreeMap", "IndexMap", "Map", "Value"];

fn serde_words(attrs: &[syn::Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter(|attr| attr.path().is_ident("serde"))
        .filter_map(|attr| attr.meta.require_list().ok())
        .flat_map(|list| list.tokens.clone().into_iter().map(|tree| tree.to_string()))
        .collect()
}

fn serde_has(attrs: &[syn::Attribute], word: &str) -> bool {
    serde_words(attrs).iter().any(|each| each == word)
}

fn written_with_its_full_path(path: &syn::Path) -> bool {
    path.leading_colon.is_some() || path.segments.len() > 1
}

#[derive(Default)]
struct NamesOfTypesFromOtherCrates(Vec<String>);

impl<'ast> Visit<'ast> for NamesOfTypesFromOtherCrates {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if written_with_its_full_path(path) {
            if let Some(last) = path.segments.last() {
                self.0.push(last.ident.to_string());
            }
        }
        visit::visit_path(self, path);
    }
}

fn map_type_in(ty: &syn::Type) -> Option<String> {
    let mut names = NamesOfTypesFromOtherCrates::default();
    names.visit_type(ty);
    names
        .0
        .into_iter()
        .find(|name| TYPES_THAT_HOLD_UNDECLARED_KEYS.contains(&name.as_str()))
}

#[derive(Default)]
struct Shape {
    counted: Vec<String>,
    open: Vec<String>,
}

impl Shape {
    fn fields(&mut self, owner: &str, fields: &syn::Fields) {
        for (index, field) in fields.iter().enumerate() {
            let name = field
                .ident
                .as_ref()
                .map_or_else(|| index.to_string(), ToString::to_string);
            if serde_has(&field.attrs, "flatten") {
                self.open.push(format!("{owner}.{name}: a flattened field"));
            }
            if let Some(map) = map_type_in(&field.ty) {
                self.open
                    .push(format!("{owner}.{name}: a map field ({map})"));
            }
        }
    }
}

impl<'ast> Visit<'ast> for Shape {
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        let name = item.ident.to_string();
        if matches!(item.fields, syn::Fields::Named(_)) {
            self.counted.push(name.clone());
            if !serde_has(&item.attrs, "deny_unknown_fields") {
                self.open.push(format!("{name}: no deny_unknown_fields"));
            }
        }
        self.fields(&name, &item.fields);
        visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        let name = item.ident.to_string();
        let tagged = serde_has(&item.attrs, "tag");
        for variant in &item.variants {
            let owner = format!("{name}::{}", variant.ident);
            match variant.fields {
                syn::Fields::Named(_) => {
                    self.counted.push(owner.clone());
                    if !serde_has(&item.attrs, "deny_unknown_fields") {
                        self.open.push(format!("{owner}: no deny_unknown_fields"));
                    }
                }
                syn::Fields::Unit if tagged => {
                    self.open
                        .push(format!("{owner}: a unit variant of a tagged enum"));
                }
                _ => {}
            }
            self.fields(&owner, &variant.fields);
        }
        visit::visit_item_enum(self, item);
    }

    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        if let Some(map) = map_type_in(&item.ty) {
            self.open
                .push(format!("{}: an alias of a map ({map})", item.ident));
        }
        visit::visit_item_type(self, item);
    }
}

fn object_positions(value: &Value, pointer: String, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            out.push(pointer.clone());
            for (key, member) in map {
                let token = key.replace('~', "~0").replace('/', "~1");
                object_positions(member, format!("{pointer}/{token}"), out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                object_positions(item, format!("{pointer}/{index}"), out);
            }
        }
        _ => {}
    }
}

fn with_unknown_key(body: &Value, pointer: &str) -> Value {
    let mut copy = body.clone();
    copy.pointer_mut(pointer)
        .and_then(Value::as_object_mut)
        .expect("the pointer names an object of the sample")
        .insert(
            UNKNOWN_KEY.to_string(),
            Value::String(UNKNOWN_KEY.to_string()),
        );
    copy
}

#[cfg(every_def_has_a_type)]
fn read_as(def: &str, value: &Value) -> Result<(), String> {
    use generated::*;
    fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<(), String> {
        serde_json::from_value::<T>(value.clone())
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    match def {
        "CapBody" => parse::<CapBody>(value),
        "CheckoutAnswer" => parse::<CheckoutAnswer>(value),
        "StatusAnswer" => parse::<StatusAnswer>(value),
        "CapGetAnswer" => parse::<CapGetAnswer>(value),
        "CapSetAnswer" => parse::<CapSetAnswer>(value),
        "ManageAnswer" => parse::<ManageAnswer>(value),
        "DeleteAnswer" => parse::<DeleteAnswer>(value),
        "Rejected" => parse::<Rejected>(value),
        other => Err(format!("no generated type is read for {other}")),
    }
}

#[cfg(not(every_def_has_a_type))]
fn read_as(_def: &str, _value: &Value) -> Result<(), String> {
    Err(format!("the generated source has no type for {MISSING}"))
}

fn answer_def(route: &str) -> &'static str {
    match route {
        "checkout-start" => "CheckoutAnswer",
        "status" => "StatusAnswer",
        "cap-get" => "CapGetAnswer",
        "cap-set" => "CapSetAnswer",
        "manage" => "ManageAnswer",
        "account-delete" => "DeleteAnswer",
        _ => "an answer type of no app route",
    }
}

fn samples_of(file: &Value) -> Vec<(String, &'static str, Value)> {
    let mut out = Vec::new();
    if let Some(requests) = file.get("requests").and_then(Value::as_object) {
        for (route, body) in requests {
            out.push((format!("requests/{route}"), "CapBody", body.clone()));
        }
    }
    if let Some(answers) = file.get("answers").and_then(Value::as_object) {
        for (route, by_result) in answers {
            if let Some(by_result) = by_result.as_object() {
                for (result, answer) in by_result {
                    out.push((
                        format!("answers/{route}/{result}"),
                        answer_def(route),
                        answer.clone(),
                    ));
                }
            }
        }
    }
    if let Some(rejected) = file.get("rejected") {
        out.push(("rejected".to_string(), "Rejected", rejected.clone()));
    }
    out
}

fn main() {
    let path = env::args()
        .nth(1)
        .expect("the check passes the path of the samples file");
    let text =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
    let file: Value =
        serde_json::from_str(&text).unwrap_or_else(|error| panic!("{path} is not JSON: {error}"));

    let mut shape = Shape::default();
    shape.visit_file(&syn::parse_file(GENERATED).expect("the generated source parses"));

    let mut read = Vec::new();
    let mut refused = Vec::new();
    let mut unknown_read = Vec::new();
    for (label, def, value) in samples_of(&file) {
        match read_as(def, &value) {
            Ok(()) => read.push(label.clone()),
            Err(error) => refused.push(format!("{label}: {error}")),
        }
        let mut positions = Vec::new();
        object_positions(&value, String::new(), &mut positions);
        for pointer in positions {
            if read_as(def, &with_unknown_key(&value, &pointer)).is_ok() {
                let at = if pointer.is_empty() {
                    "/".to_string()
                } else {
                    pointer
                };
                unknown_read.push(format!(
                    "{def} read {label} with the key {UNKNOWN_KEY} at {at}"
                ));
            }
        }
    }
    let missing: Vec<&str> = MISSING.split(',').filter(|name| !name.is_empty()).collect();
    println!(
        "CONTRACT-RUST {}",
        json!({
            "types": shape.counted.len(),
            "counted": shape.counted,
            "open": shape.open,
            "unknown_read": unknown_read,
            "missing": missing,
            "read": read,
            "refused": refused,
        })
    );
}
