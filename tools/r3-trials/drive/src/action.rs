use serde_json::Value;

/// A key that a `press` action names (contracts.md A12.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    Tab,
    Escape,
}

impl Key {
    pub fn as_str(self) -> &'static str {
        match self {
            Key::Enter => "Enter",
            Key::Tab => "Tab",
            Key::Escape => "Escape",
        }
    }
}

/// One action of the grammar `action` (A12.4), with the fields that its action takes and no other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Click { reference: String },
    Fill { reference: String, slot: String },
    Select { reference: String, slot: String },
    Press { reference: String, key: Key },
    Extract { reference: String, name: String },
    Handback,
    Done,
}

impl Action {
    /// The verb word of the action.
    pub fn word(&self) -> &'static str {
        match self {
            Action::Click { .. } => "click",
            Action::Fill { .. } => "fill",
            Action::Select { .. } => "select",
            Action::Press { .. } => "press",
            Action::Extract { .. } => "extract",
            Action::Handback => "handback",
            Action::Done => "done",
        }
    }

    /// The snapshot reference that the action names, when it names one.
    pub fn reference(&self) -> Option<&str> {
        match self {
            Action::Click { reference }
            | Action::Fill { reference, .. }
            | Action::Select { reference, .. }
            | Action::Press { reference, .. }
            | Action::Extract { reference, .. } => Some(reference),
            Action::Handback | Action::Done => None,
        }
    }
}

/// Why a reply is no action: `action_form` when it is no JSON object of the grammar's five keys,
/// `action_fields` when its action lacks a field that it needs. A field that the action does not
/// take is never read, so a `fill` or a `select` names a slot and never a value (A12.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    Form,
    Fields,
}

const KEYS: [&str; 5] = ["action", "ref", "slot", "key", "name"];

fn is_ref(s: &str) -> bool {
    let Some(digits) = s.strip_prefix('e') else {
        return false;
    };
    (1..=6).contains(&digits.len())
        && !digits.starts_with('0')
        && digits.bytes().all(|b| b.is_ascii_digit())
}

/// Whether `s` is a name of A0.7: 1 to 32 bytes of `a` to `z`, `0` to `9` and `_`.
pub fn is_name(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn field(object: &serde_json::Map<String, Value>, key: &str) -> Result<Option<String>, Refused> {
    match object.get(key) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        _ => Err(Refused::Form),
    }
}

/// Parses the text of one reply of the grammar `action` into the action it names.
pub fn parse(text: &str) -> Result<Action, Refused> {
    let value: Value = serde_json::from_str(text.trim()).map_err(|_| Refused::Form)?;
    let object = value.as_object().ok_or(Refused::Form)?;
    if object.len() != KEYS.len() || KEYS.iter().any(|k| !object.contains_key(*k)) {
        return Err(Refused::Form);
    }
    let action = field(object, "action")?.ok_or(Refused::Form)?;
    let reference = field(object, "ref")?;
    let slot = field(object, "slot")?;
    let key = field(object, "key")?;
    let name = field(object, "name")?;
    if reference.as_deref().is_some_and(|r| !is_ref(r))
        || slot.as_deref().is_some_and(|s| !is_name(s))
        || name.as_deref().is_some_and(|s| !is_name(s))
    {
        return Err(Refused::Form);
    }
    let key = match key.as_deref() {
        None => None,
        Some("Enter") => Some(Key::Enter),
        Some("Tab") => Some(Key::Tab),
        Some("Escape") => Some(Key::Escape),
        Some(_) => return Err(Refused::Form),
    };
    match (action.as_str(), reference, slot, key, name) {
        ("click", Some(reference), _, _, _) => Ok(Action::Click { reference }),
        ("fill", Some(reference), Some(slot), _, _) => Ok(Action::Fill { reference, slot }),
        ("select", Some(reference), Some(slot), _, _) => Ok(Action::Select { reference, slot }),
        ("press", Some(reference), _, Some(key), _) => Ok(Action::Press { reference, key }),
        ("extract", Some(reference), _, _, Some(name)) => Ok(Action::Extract { reference, name }),
        ("handback", ..) => Ok(Action::Handback),
        ("done", ..) => Ok(Action::Done),
        ("click" | "fill" | "select" | "press" | "extract", ..) => Err(Refused::Fields),
        _ => Err(Refused::Form),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(action: &str, r: &str, slot: &str, key: &str, name: &str) -> String {
        let q = |v: &str| {
            if v == "null" {
                "null".to_string()
            } else {
                format!("\"{v}\"")
            }
        };
        format!(
            "{{\"action\":\"{action}\",\"ref\":{},\"slot\":{},\"key\":{},\"name\":{}}}",
            q(r),
            q(slot),
            q(key),
            q(name)
        )
    }

    #[test]
    fn each_action_takes_its_own_fields() {
        assert_eq!(
            parse(&reply("click", "e3", "null", "null", "null")),
            Ok(Action::Click {
                reference: "e3".into()
            })
        );
        assert_eq!(
            parse(&reply("fill", "e2", "payee", "null", "null")),
            Ok(Action::Fill {
                reference: "e2".into(),
                slot: "payee".into()
            })
        );
        assert_eq!(
            parse(&reply("select", "e7", "instrument", "null", "null")),
            Ok(Action::Select {
                reference: "e7".into(),
                slot: "instrument".into()
            })
        );
        assert_eq!(
            parse(&reply("press", "e4", "null", "Enter", "null")),
            Ok(Action::Press {
                reference: "e4".into(),
                key: Key::Enter
            })
        );
        assert_eq!(
            parse(&reply("extract", "e5", "null", "null", "amount_due")),
            Ok(Action::Extract {
                reference: "e5".into(),
                name: "amount_due".into()
            })
        );
        assert_eq!(
            parse(&reply("handback", "null", "null", "null", "null")),
            Ok(Action::Handback)
        );
        assert_eq!(
            parse(&reply("done", "null", "null", "null", "null")),
            Ok(Action::Done)
        );
    }

    #[test]
    fn a_field_that_the_action_does_not_take_is_never_used() {
        let seen = reply("fill", "e7", "payment_method", "Enter", "card_ending_4417");
        assert_eq!(
            parse(&seen),
            Ok(Action::Fill {
                reference: "e7".into(),
                slot: "payment_method".into()
            })
        );
        assert_eq!(
            parse(&reply("extract", "e3", "amount_due", "Enter", "amount_due")),
            Ok(Action::Extract {
                reference: "e3".into(),
                name: "amount_due".into()
            })
        );
        assert_eq!(
            parse(&reply("click", "e7", "payee", "Tab", "x")),
            Ok(Action::Click {
                reference: "e7".into()
            })
        );
        assert_eq!(
            parse(&reply("done", "e7", "x", "Tab", "y")),
            Ok(Action::Done)
        );
        assert_eq!(
            parse(&reply("handback", "e2", "null", "Tab", "null")),
            Ok(Action::Handback)
        );
    }

    #[test]
    fn an_action_without_a_field_that_it_needs_is_refused() {
        assert_eq!(
            parse(&reply("fill", "e7", "null", "Enter", "payee")),
            Err(Refused::Fields)
        );
        assert_eq!(
            parse(&reply("select", "e7", "null", "null", "null")),
            Err(Refused::Fields)
        );
        assert_eq!(
            parse(&reply("click", "null", "payee", "null", "null")),
            Err(Refused::Fields)
        );
        assert_eq!(
            parse(&reply("press", "e7", "enter", "null", "null")),
            Err(Refused::Fields)
        );
        assert_eq!(
            parse(&reply("extract", "e7", "amount", "null", "null")),
            Err(Refused::Fields)
        );
        assert_eq!(
            parse(&reply("fill", "null", "payee", "null", "null")),
            Err(Refused::Fields)
        );
    }

    #[test]
    fn a_reply_out_of_the_grammar_is_no_action() {
        for bad in [
            "",
            "click e3",
            "{\"action\":\"click\",\"ref\":\"e3\"}",
            &reply("type", "e3", "null", "null", "null"),
            &reply("click", "e0", "null", "null", "null"),
            &reply("click", "e03", "null", "null", "null"),
            &reply("click", "x3", "null", "null", "null"),
            &reply("click", "e1234567", "null", "null", "null"),
            &reply("fill", "e3", "Payee", "null", "null"),
            &reply("press", "e3", "null", "Space", "null"),
            "{\"action\":\"done\",\"ref\":null,\"slot\":null,\"key\":null,\"name\":null,\"x\":null}",
            "{\"action\":\"done\",\"ref\":1,\"slot\":null,\"key\":null,\"name\":null}",
        ] {
            assert_eq!(parse(bad), Err(Refused::Form), "{bad}");
        }
    }

    #[test]
    fn a_name_is_the_form_of_a0_7() {
        assert!(is_name("amount_due"));
        assert!(is_name(&"a".repeat(32)));
        assert!(!is_name(&"a".repeat(33)));
        assert!(!is_name(""));
        assert!(!is_name("Amount"));
        assert!(!is_name("amount-due"));
    }

    #[test]
    fn an_action_gives_its_verb_word_and_its_reference() {
        let a = Action::Press {
            reference: "e2".into(),
            key: Key::Tab,
        };
        assert_eq!(a.word(), "press");
        assert_eq!(a.reference(), Some("e2"));
        assert_eq!(Action::Done.reference(), None);
    }
}
