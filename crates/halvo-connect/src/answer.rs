use std::collections::HashSet;

use serde_json::Value;

use crate::error::{AnswerFault, CallError};

pub(crate) const FORM: CallError = CallError::Answer(AnswerFault::Form);

pub(crate) fn id(item: &Value) -> Result<String, CallError> {
    checked(item.get("id").and_then(Value::as_str).ok_or(FORM)?)
}

pub(crate) fn checked(id: &str) -> Result<String, CallError> {
    if matches!(id, "" | "." | "..") {
        return Err(CallError::Answer(AnswerFault::Id));
    }
    Ok(id.to_owned())
}

pub(crate) fn text(item: &Value, key: &str) -> Result<String, CallError> {
    item.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(FORM)
}

pub(crate) fn subject(item: &Value) -> Option<String> {
    match item.get("subject") {
        None | Some(Value::Null) => Some(String::new()),
        Some(Value::String(subject)) => Some(subject.clone()),
        Some(_) => None,
    }
}

#[derive(Default)]
pub(crate) struct Ids(HashSet<String>);

impl Ids {
    pub(crate) fn of<'a>(ids: impl Iterator<Item = &'a str>) -> Ids {
        Ids(ids.map(str::to_owned).collect())
    }

    pub(crate) fn first(&mut self, id: &str) -> Result<(), CallError> {
        if self.0.insert(id.to_owned()) {
            Ok(())
        } else {
            Err(CallError::Answer(AnswerFault::Repeated))
        }
    }
}
