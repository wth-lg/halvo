use std::{fs, path::Path};

use serde_json::{json, Value};
use syn::visit::{self, Visit};

mod generated {
    #![allow(dead_code, unused_imports, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

const GENERATED: &str = include_str!(concat!(env!("OUT_DIR"), "/generated.rs"));
const SCHEMA_NAME: &str = env!("S3_SCHEMA_NAME");
const SENTINEL: &str = "S3-SENTINEL-not-real-data";
const UNKNOWN_KEYS: [&str; 3] = ["window_title", "__proto__", "x"];
const TYPES_THAT_HOLD_UNDECLARED_KEYS: [&str; 5] =
    ["HashMap", "BTreeMap", "IndexMap", "Map", "Value"];

fn serde_has(attrs: &[syn::Attribute], word: &str) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("serde")
            && attr.meta.require_list().is_ok_and(|list| {
                list.tokens
                    .clone()
                    .into_iter()
                    .any(|tree| tree.to_string() == word)
            })
    })
}

fn typify_wrote_the_full_path(path: &syn::Path) -> bool {
    path.leading_colon.is_some() || path.segments.len() > 1
}

#[derive(Default)]
struct NamesOfTypesFromOtherCrates(Vec<String>);

impl<'ast> Visit<'ast> for NamesOfTypesFromOtherCrates {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        if typify_wrote_the_full_path(path) {
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

fn unknown_values() -> [(&'static str, Value); 4] {
    [
        ("string", Value::String(SENTINEL.to_string())),
        ("integer", json!(0)),
        ("null", Value::Null),
        ("object", json!({})),
    ]
}

#[derive(Default)]
struct Shape {
    named_structs: Vec<String>,
    not_denying: Vec<String>,
    flatten_fields: Vec<String>,
    map_fields: Vec<String>,
    tuple_structs: Vec<String>,
    enums: Vec<String>,
    aliases: Vec<String>,
}

impl Shape {
    fn field(&mut self, owner: &str, index: usize, field: &syn::Field) {
        let name = field
            .ident
            .as_ref()
            .map_or_else(|| index.to_string(), ToString::to_string);
        if serde_has(&field.attrs, "flatten") {
            self.flatten_fields.push(format!("{owner}.{name}"));
        }
        if let Some(map) = map_type_in(&field.ty) {
            self.map_fields.push(format!("{owner}.{name}: {map}"));
        }
    }
}

impl<'ast> Visit<'ast> for Shape {
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        let name = item.ident.to_string();
        match &item.fields {
            syn::Fields::Named(_) => {
                self.named_structs.push(name.clone());
                if !serde_has(&item.attrs, "deny_unknown_fields") {
                    self.not_denying.push(name.clone());
                }
            }
            syn::Fields::Unnamed(_) => self.tuple_structs.push(name.clone()),
            syn::Fields::Unit => {}
        }
        for (index, field) in item.fields.iter().enumerate() {
            self.field(&name, index, field);
        }
        visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        let name = item.ident.to_string();
        self.enums.push(name.clone());
        for variant in &item.variants {
            let owner = format!("{name}::{}", variant.ident);
            if matches!(variant.fields, syn::Fields::Named(_)) {
                self.named_structs.push(owner.clone());
                if !serde_has(&item.attrs, "deny_unknown_fields") {
                    self.not_denying.push(owner.clone());
                }
            }
            for (index, field) in variant.fields.iter().enumerate() {
                self.field(&owner, index, field);
            }
        }
        visit::visit_item_enum(self, item);
    }

    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        let name = item.ident.to_string();
        if let Some(map) = map_type_in(&item.ty) {
            self.map_fields.push(format!("{name} = {map}"));
        }
        self.aliases.push(name);
        visit::visit_item_type(self, item);
    }
}

fn rfc6901_token(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

fn object_positions(value: &Value, pointer: String, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            out.push(pointer.clone());
            for (key, member) in map {
                let key = rfc6901_token(key);
                object_positions(member, format!("{pointer}/{key}"), out);
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

fn with_key(body: &Value, pointer: &str, key: &str, value: &Value) -> Value {
    let mut copy = body.clone();
    copy.pointer_mut(pointer)
        .and_then(Value::as_object_mut)
        .expect("the pointer names an object of the body")
        .insert(key.to_string(), value.clone());
    copy
}

fn read_json(path: &str) -> Value {
    let text =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{path} is not JSON: {error}"))
}

fn main() {
    let dir = env!("CARGO_MANIFEST_DIR");
    let schema = read_json(&format!("{dir}/schemas/{SCHEMA_NAME}.json"));
    let own_bodies = format!("{dir}/bodies/{SCHEMA_NAME}.json");
    let bodies_path = if Path::new(&own_bodies).exists() {
        own_bodies
    } else {
        format!("{dir}/bodies/valid.json")
    };
    let bodies = read_json(&bodies_path);
    let bodies = bodies.as_array().expect("the bodies file holds an array");

    let mut shape = Shape::default();
    shape.visit_file(&syn::parse_file(GENERATED).expect("the generated source parses"));

    let validator =
        jsonschema::validator_for(&schema).expect("the jsonschema crate compiles the export");
    let (mut serde_read_valid, mut schema_valid) = (0, 0);
    let (mut cases, mut serde_refused, mut schema_refused) = (0, 0, 0);
    let mut positions = Vec::new();
    let mut serde_read_unknown = Vec::new();
    let mut bodies_as_the_app_sends = Vec::new();
    for body in bodies {
        if let Ok(request) = serde_json::from_value::<generated::Request>(body.clone()) {
            serde_read_valid += 1;
            bodies_as_the_app_sends
                .push(serde_json::to_string(&request).expect("a generated type serializes"));
        }
        schema_valid += usize::from(validator.is_valid(body));
        let mut here = Vec::new();
        object_positions(body, String::new(), &mut here);
        for pointer in &here {
            for key in UNKNOWN_KEYS {
                for (kind, value) in unknown_values() {
                    let injected = with_key(body, pointer, key, &value);
                    cases += 1;
                    match serde_json::from_value::<generated::Request>(injected.clone()) {
                        Ok(_) => serde_read_unknown.push(format!(
                            "{} {key} {kind}",
                            if pointer.is_empty() {
                                "/"
                            } else {
                                pointer.as_str()
                            }
                        )),
                        Err(_) => serde_refused += 1,
                    }
                    schema_refused += usize::from(!validator.is_valid(&injected));
                }
            }
        }
        if positions.is_empty() {
            positions = here;
        }
    }

    println!(
        "S3-TYPIFY {}",
        json!({
            "schema": SCHEMA_NAME,
            "generated_lines": GENERATED.lines().count(),
            "named_structs": shape.named_structs,
            "not_denying": shape.not_denying,
            "flatten_fields": shape.flatten_fields,
            "map_fields": shape.map_fields,
            "tuple_structs": shape.tuple_structs,
            "enums": shape.enums,
            "aliases": shape.aliases,
            "bodies": bodies.len(),
            "object_positions": positions,
            "serde_read_valid": serde_read_valid,
            "serde_written": bodies_as_the_app_sends,
            "unknown_key_cases": cases,
            "serde_refused": serde_refused,
            "serde_read_unknown": serde_read_unknown,
            "jsonschema_valid": schema_valid,
            "jsonschema_refused": schema_refused,
        })
    );
}
