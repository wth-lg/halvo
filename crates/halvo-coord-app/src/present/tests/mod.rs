use super::Card;
use halvo_coord::items::ConfirmAnswer;
use halvo_coord::types::{Class, Field, FieldFrom};

mod card;
mod outcome;

pub const ALLOW: ConfirmAnswer = ConfirmAnswer::Allow;
pub const REJECT: ConfirmAnswer = ConfirmAnswer::Reject;

pub fn field(label: &str, value: &[&str], from: FieldFrom) -> Field {
    Field {
        label: label.into(),
        value: value.iter().map(|v| v.to_string()).collect(),
        from,
        from_text: String::new(),
    }
}

pub fn pay_fields() -> Vec<Field> {
    vec![
        field("Pay to", &["City Power"], FieldFrom::You),
        field("Amount", &["42.10 USD"], FieldFrom::Site),
        field("Pay with", &["Card ending 4242"], FieldFrom::You),
        field("Site", &["site1.example"], FieldFrom::Halvo),
    ]
}

pub fn pay() -> Card {
    Card::new(&[Class::Payment], &pay_fields()).unwrap()
}
