use std::collections::HashSet;

use serde_json::Value;

use crate::error::{AnswerFault, CallError};

pub(crate) const FORM: CallError = CallError::Answer(AnswerFault::Form);

pub(crate) fn page(answer: &Value) -> Result<&[Value], CallError> {
    let object = answer.as_object().ok_or(FORM)?;
    if object.contains_key("@odata.nextLink") {
        return Err(CallError::Answer(AnswerFault::Paged));
    }
    object
        .get("value")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or(FORM)
}

pub(crate) fn id(item: &Value) -> Result<String, CallError> {
    let id = item.get("id").and_then(Value::as_str).ok_or(FORM)?;
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
