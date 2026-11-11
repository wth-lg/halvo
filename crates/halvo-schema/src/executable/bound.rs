use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use serde_json::value::RawValue;

use super::json::{self, from_json, list, object_type, opt, FromJson};
use super::names::{ActionClass, FieldName, Source};
use super::text::{OwnValue, WebOrigin};
use super::Refusal;

impl ActionClass {
    /// The bound fields that an effect row keeps, in their order, each with the most bytes of its joined value
    /// where the contract bounds it: an amount 32, an address 320.
    pub fn kept_fields(self) -> Vec<(FieldName, Option<usize>)> {
        self.bound_fields()
            .iter()
            .filter(|f| {
                !matches!(
                    (self, f),
                    (ActionClass::Purchase, FieldName::Items)
                        | (ActionClass::Send, FieldName::Subject | FieldName::Body)
                        | (ActionClass::Submit, FieldName::Fields)
                )
            })
            .map(|f| {
                let max = match f {
                    FieldName::Amount => Some(32),
                    FieldName::To => Some(320),
                    _ => None,
                };
                (*f, max)
            })
            .collect()
    }
}

object_type! {
    /// One field of a bound object: its name, the source of its value, an origin or null, and the value. Which
    /// sources and origins a field admits is the rule of `admit` of halvo-policy, not of this parser.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct BoundField {
        pub name: FieldName = "name" => from_json,
        pub from: Source = "from" => from_json,
        pub origin: Option<WebOrigin> = "origin" => opt(from_json),
        pub value: OwnValue = "value" => from_json,
    }
    rules(_f) { Ok(()) }
}

/// The bound object of an effect, `{"class":"<class>","fields":[<bound field>...]}`, with exactly the fields of
/// its class in their order. A class that makes no effect (`read`, `prepare`, `credential`) has no bound object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bound {
    class: ActionClass,
    fields: Vec<BoundField>,
}

impl Bound {
    /// The bound object when the fields are exactly the fields of the class in their order, each sound, else the
    /// refusal with `malformed`.
    pub fn new(class: ActionClass, fields: Vec<BoundField>) -> Result<Bound, Refusal> {
        let names: Vec<FieldName> = fields.iter().map(|f| f.name).collect();
        if class.bound_fields().is_empty() {
            return Err(Refusal::malformed("Bound", "class_without_effect").at("class"));
        }
        if names != class.bound_fields() {
            return Err(Refusal::malformed("Bound", "fields_of_class").at("fields"));
        }
        for field in &fields {
            field.rules()?;
        }
        Ok(Bound { class, fields })
    }

    /// The class.
    pub fn class(&self) -> ActionClass {
        self.class
    }

    /// The fields in their order.
    pub fn fields(&self) -> &[BoundField] {
        &self.fields
    }

    /// The field of a name, where the class has it.
    pub fn field(&self, name: FieldName) -> Option<&BoundField> {
        self.fields.iter().find(|f| f.name == name)
    }

    pub(in crate::executable) fn from_raw(raw: &RawValue) -> Result<Bound, Refusal> {
        let obj = json::object("Bound", raw)?;
        obj.exactly(&["class", "fields"])?;
        let class: ActionClass = obj.read("class", from_json)?;
        let fields = obj.read("fields", list(BoundField::from_raw))?;
        Bound::new(class, fields)
    }

    /// Reads one bound object and refuses every other input.
    pub fn parse(bytes: &[u8]) -> Result<Bound, Refusal> {
        Bound::from_raw(json::whole("Bound", bytes)?)
    }

    /// The compact JSON of the bound object, whose SHA-256 is its digest.
    pub fn to_json(&self) -> Result<String, Refusal> {
        serde_json::to_string(self).map_err(|_| Refusal::malformed("Bound", "write"))
    }
}

impl Serialize for Bound {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("class", &self.class)?;
        map.serialize_entry("fields", &self.fields)?;
        map.end()
    }
}

impl FromJson for Bound {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        Bound::from_raw(raw)
    }
}

object_type! {
    /// A field that an effect row keeps: its name and its value.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct KeptField {
        pub name: FieldName = "name" => from_json,
        pub value: OwnValue = "value" => from_json,
    }
    rules(_k) { Ok(()) }
}

pub(in crate::executable) fn check_kept(
    class: ActionClass,
    kept: &[KeptField],
) -> Result<(), Refusal> {
    let expected = class.kept_fields();
    if kept.len() != expected.len() {
        return Err(Refusal::malformed("KeptField", "kept_of_class"));
    }
    for (field, (name, max)) in kept.iter().zip(expected) {
        if field.name != name {
            return Err(Refusal::malformed("KeptField", "kept_of_class"));
        }
        if max.is_some_and(|max| field.value.len() > max) {
            return Err(Refusal::malformed("KeptField", "kept_length"));
        }
    }
    Ok(())
}
