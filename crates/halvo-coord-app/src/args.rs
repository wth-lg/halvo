use halvo_coord::types::commands::{Arg, Command};
use halvo_coord::types::{ArgType, Cause};
use serde_json::{Map, Value};

pub(crate) struct Args<'a> {
    map: Option<&'a Map<String, Value>>,
}

impl<'a> Args<'a> {
    pub(crate) fn read(command: &Command, raw: Option<&'a Value>) -> Result<Args<'a>, Cause> {
        let map = match raw.ok_or(Cause::Malformed)? {
            Value::Null => None,
            Value::Object(map) => Some(map),
            _ => return Err(Cause::Malformed),
        };
        let given = |name: &str| map.and_then(|m| m.get(name));
        if let Some(map) = map {
            if map
                .keys()
                .any(|k| !command.args.iter().any(|a| a.name == k))
            {
                return Err(Cause::Malformed);
            }
        }
        let choice = given("choice").and_then(Value::as_str);
        for arg in command.args {
            let wanted = arg.when.is_none_or(|c| choice == Some(c.name()));
            match (wanted, given(arg.name)) {
                (true, Some(value)) if fits(arg, value) => {}
                (false, None) => {}
                _ => return Err(Cause::Malformed),
            }
        }
        Ok(Args { map })
    }

    pub(crate) fn text(&self, name: &str) -> &'a str {
        self.map
            .and_then(|m| m.get(name))
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    pub(crate) fn count(&self, name: &str) -> Option<u32> {
        self.map
            .and_then(|m| m.get(name))
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    }
}

fn fits(arg: &Arg, value: &Value) -> bool {
    match arg.kind {
        ArgType::Count => value.as_u64().is_some_and(|n| u32::try_from(n).is_ok()),
        ArgType::Id | ArgType::Text => value.is_string(),
        ArgType::Name => value
            .as_str()
            .is_some_and(|s| arg.values.is_none_or(|names| names.contains(&s))),
    }
}
