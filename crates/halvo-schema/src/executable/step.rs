use std::num::NonZeroU32;

use serde::{Serialize, Serializer};
use serde_json::value::RawValue;

use super::json::{self, from_json, list, object_type, opt, FromJson};
use super::names::{
    ActionClass, Adapter, ApiOp, Check, LocatorBy, ModelOutput, Source, StepEffect, StepKind, Verb,
    YesNo,
};
use super::text::{Name, OwnText, WebOrigin, V1};
use super::Refusal;

/// The most bytes of the compact JSON of one step.
pub const STEP_JSON_MAX: usize = 2048;

/// The most steps of one playbook.
pub const PLAYBOOK_STEPS_MAX: usize = 64;

/// The most milliseconds of a `wait` step.
pub const WAIT_MS_MAX: u32 = 10_000;

object_type! {
    /// One way to find a control. With `by` `role` it holds the role in `value` and the accessible name in `name`;
    /// with every other `by` its text in `value` and `name` null.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Locator {
        pub by: LocatorBy = "by" => from_json,
        pub value: OwnText = "value" => from_json,
        pub name: Option<OwnText> = "name" => opt(from_json),
    }
    rules(l) {
        if (l.by == LocatorBy::Role) != l.name.is_some() {
            return Err(Refusal::malformed("Locator", "name_of_by").at("name"));
        }
        Ok(())
    }
}

object_type! {
    /// The target of a web step: the role and accessible name of the control as its snapshot line gave them, the
    /// origins of the frames that hold it from the top document inward, whether it is a credential field, and one
    /// or more locators in the order in which the executor tries them.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct WebTarget {
        pub desc: OwnText = "desc" => from_json,
        pub frame: Vec<WebOrigin> = "frame" => list(from_json),
        pub credential: YesNo = "credential" => from_json,
        pub locators: Vec<Locator> = "locators" => list(Locator::from_raw),
    }
    rules(t) {
        if t.locators.is_empty() {
            return Err(Refusal::malformed("WebTarget", "no_locator").at("locators"));
        }
        Ok(())
    }
}

object_type! {
    /// The target of an api step: an adapter and one of its operations.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ApiTarget {
        pub adapter: Adapter = "adapter" => from_json,
        pub op: ApiOp = "op" => from_json,
    }
    rules(t) {
        if t.op.adapter() != t.adapter {
            return Err(Refusal::malformed("ApiTarget", "op_of_adapter").at("op"));
        }
        Ok(())
    }
}

object_type! {
    /// The target of a file step: slot names of the file, the sheet and the range.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct FileTarget {
        pub file: Name = "file" => from_json,
        pub sheet: Option<Name> = "sheet" => opt(from_json),
        pub range: Option<Name> = "range" => opt(from_json),
    }
    rules(_t) { Ok(()) }
}

/// The target of a step: of a web, an api or a file step, told apart by its keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Web(WebTarget),
    Api(ApiTarget),
    File(FileTarget),
}

impl Target {
    /// The kind of step whose target this is.
    pub fn kind(&self) -> StepKind {
        match self {
            Target::Web(_) => StepKind::Web,
            Target::Api(_) => StepKind::Api,
            Target::File(_) => StepKind::File,
        }
    }
}

impl Serialize for Target {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Target::Web(t) => t.serialize(serializer),
            Target::Api(t) => t.serialize(serializer),
            Target::File(t) => t.serialize(serializer),
        }
    }
}

impl FromJson for Target {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        let obj = json::object("Target", raw)?;
        if obj.has("desc") {
            WebTarget::from_raw(raw).map(Target::Web)
        } else if obj.has("adapter") {
            ApiTarget::from_raw(raw).map(Target::Api)
        } else if obj.has("file") {
            FileTarget::from_raw(raw).map(Target::File)
        } else {
            Err(Refusal::malformed("Target", "target_form"))
        }
    }
}

object_type! {
    /// An arg of a step: with `from` `const` it holds `value` and `key` is null; with `slot`, `extract` or
    /// `model` it holds `key` and `value` is null. A `model` key names `reply_body`, the one output of the model.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Arg {
        pub name: Name = "name" => from_json,
        pub from: Source = "from" => from_json,
        pub value: Option<OwnText> = "value" => opt(from_json),
        pub key: Option<Name> = "key" => opt(from_json),
    }
    rules(a) {
        let holds = (a.value.is_some(), a.key.is_some());
        if holds != (a.from == Source::Const, a.from != Source::Const) {
            return Err(Refusal::malformed("Arg", "value_or_key"));
        }
        if a.from == Source::Model {
            let key = a.key.as_ref().map_or("", Name::as_str);
            ModelOutput::parse(key).map_err(|r| r.at("key"))?;
        }
        Ok(())
    }
}

object_type! {
    /// A condition: `url_is` takes a const arg and no target; `present` and `enabled` a target and no arg;
    /// `text_is`, `value_is` and `count_is` both. The target is that of a web step, and no arg of a condition comes
    /// from the model.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Cond {
        pub check: Check = "check" => from_json,
        pub target: Option<WebTarget> = "target" => opt(WebTarget::from_raw),
        pub arg: Option<Arg> = "arg" => opt(Arg::from_raw),
    }
    rules(c) {
        let (target, arg) = match c.check {
            Check::UrlIs => (false, true),
            Check::Present | Check::Enabled => (true, false),
            Check::TextIs | Check::ValueIs | Check::CountIs => (true, true),
        };
        if c.target.is_some() != target || c.arg.is_some() != arg {
            return Err(Refusal::malformed("Cond", "keys_of_check"));
        }
        match &c.arg {
            Some(a) if a.from == Source::Model => {
                Err(Refusal::malformed("Cond", "model_in_condition").at("arg"))
            }
            Some(a) if c.check == Check::UrlIs && a.from != Source::Const => {
                Err(Refusal::malformed("Cond", "url_is_const").at("arg"))
            }
            _ => Ok(()),
        }
    }
}

enum Want {
    None,
    Web,
    Api,
    FileRead,
    FileWrite,
}

const SLOT_CONST: &[Source] = &[Source::Slot, Source::Const];
const CONST: &[Source] = &[Source::Const];
const SLOT: &[Source] = &[Source::Slot];
const EXTRACT: &[Source] = &[Source::Extract];
const MODEL_CONST: &[Source] = &[Source::Model, Source::Const];
const BOUND: &[Source] = &[Source::Const, Source::Slot, Source::Extract];

#[derive(Clone, Copy)]
enum Value {
    Any,
    OneOf(&'static [&'static str]),
    NameForm,
    Millis,
}

type ArgSpec = (&'static str, &'static [Source], Value);

fn bound_args(class: ActionClass) -> Vec<ArgSpec> {
    class
        .bound_fields()
        .iter()
        .map(|f| {
            let sources = if f.as_str() == "body" {
                MODEL_CONST
            } else {
                BOUND
            };
            (f.as_str(), sources, Value::Any)
        })
        .collect()
}

fn shape(step: &ExecutableStep) -> (Want, Vec<ArgSpec>) {
    let request = step.effect == StepEffect::Request;
    let bound = || {
        if request {
            bound_args(step.class)
        } else {
            Vec::new()
        }
    };
    match step.verb {
        Verb::Navigate => (Want::None, vec![("url", SLOT_CONST, Value::Any)]),
        Verb::Click => (Want::Web, bound()),
        Verb::Fill => (Want::Web, vec![("text", SLOT_CONST, Value::Any)]),
        Verb::Select => (Want::Web, vec![("option", SLOT_CONST, Value::Any)]),
        Verb::Press => {
            let mut args = vec![("key", CONST, Value::OneOf(&["Enter", "Tab", "Escape"]))];
            args.extend(bound());
            (Want::Web, args)
        }
        Verb::Upload => (Want::Web, vec![("file", SLOT, Value::Any)]),
        Verb::Extract => (
            Want::Web,
            vec![
                ("as", CONST, Value::NameForm),
                ("parse", CONST, Value::OneOf(&["text", "amount"])),
            ],
        ),
        Verb::Wait => (Want::None, vec![("ms", CONST, Value::Millis)]),
        Verb::Handback => (
            Want::None,
            vec![("reason", CONST, Value::OneOf(&["sign_in"]))],
        ),
        Verb::Call => {
            let args = match &step.target {
                Some(Target::Api(t)) => match t.op {
                    ApiOp::MailList | ApiOp::CalendarList => Vec::new(),
                    ApiOp::MailDraftReply => vec![("body", MODEL_CONST, Value::Any)],
                    ApiOp::MailSend | ApiOp::CalendarDelete => bound_args(t.op.class()),
                },
                _ => Vec::new(),
            };
            (Want::Api, args)
        }
        Verb::Read => (Want::FileRead, vec![("as", CONST, Value::NameForm)]),
        Verb::Write => (Want::FileWrite, vec![("values", EXTRACT, Value::Any)]),
    }
}

fn millis(s: &str) -> bool {
    !s.is_empty()
        && (s == "0" || !s.starts_with('0'))
        && s.len() <= 5
        && s.bytes().all(|b| b.is_ascii_digit())
        && s.parse::<u32>().is_ok_and(|n| n <= WAIT_MS_MAX)
}

fn step_rules(step: &ExecutableStep) -> Result<(), Refusal> {
    let fail = |key, rule| Err(Refusal::malformed("ExecutableStep", rule).at(key));
    if usize::try_from(step.n.get()).map_or(true, |n| n > PLAYBOOK_STEPS_MAX) {
        return Err(Refusal::length("ExecutableStep", "step_number").at("n"));
    }
    if step.verb.kind() != step.kind {
        return fail("verb", "verb_of_kind");
    }
    let (want, args) = shape(step);
    let target_fits = match (&want, &step.target) {
        (Want::None, None) => true,
        (Want::Web, Some(Target::Web(_))) | (Want::Api, Some(Target::Api(_))) => true,
        (Want::FileRead, Some(Target::File(t))) => t.sheet.is_none() && t.range.is_none(),
        (Want::FileWrite, Some(Target::File(t))) => t.sheet.is_some() && t.range.is_some(),
        _ => false,
    };
    if !target_fits {
        return fail("target", "target_of_verb");
    }
    if step.effect == StepEffect::Request && !matches!(step.verb, Verb::Click | Verb::Press) {
        return fail("effect", "effect_of_verb");
    }
    let names: Vec<&str> = step.args.iter().map(|a| a.name.as_str()).collect();
    let wanted: Vec<&str> = args.iter().map(|(name, _, _)| *name).collect();
    if names != wanted {
        return fail("args", "args_of_verb");
    }
    for (arg, (_, sources, value)) in step.args.iter().zip(&args) {
        if !sources.contains(&arg.from) {
            return fail("args", "source_of_arg");
        }
        let text = arg.value.as_ref().map_or("", OwnText::as_str);
        let fits = match value {
            Value::Any => true,
            Value::OneOf(words) => words.contains(&text),
            Value::NameForm => Name::parse(text).is_ok(),
            Value::Millis => millis(text),
        };
        if !fits {
            return fail("args", "value_of_arg");
        }
    }
    Ok(())
}

object_type! {
    /// One compiled step of a playbook (`ExecutableStep`): its number, kind, verb, target, args, conditions, effect
    /// and class. The parser checks the shape of the verb's row (its kind, its target, its args by name and source
    /// and the words of its const args), the closed lists, and the 2,048 bytes of its compact JSON (cause
    /// `length`); `check_playbook` of halvo-policy checks the class.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ExecutableStep {
        pub v: V1 = "v" => from_json,
        pub n: NonZeroU32 = "n" => from_json,
        pub kind: StepKind = "kind" => from_json,
        pub verb: Verb = "verb" => from_json,
        pub target: Option<Target> = "target" => opt(from_json),
        pub args: Vec<Arg> = "args" => list(Arg::from_raw),
        pub pre: Vec<Cond> = "pre" => list(Cond::from_raw),
        pub post: Vec<Cond> = "post" => list(Cond::from_raw),
        pub effect: StepEffect = "effect" => from_json,
        pub class: ActionClass = "class" => from_json,
    }
    rules(s) { step_rules(s) }
    max_len(STEP_JSON_MAX)
}

impl FromJson for ExecutableStep {
    fn from_json(raw: &RawValue) -> Result<Self, Refusal> {
        ExecutableStep::from_raw(raw)
    }
}

/// A playbook: the ordered steps of one agent, 1 to 64, numbered by `n` from 1 in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playbook(Vec<ExecutableStep>);

impl Playbook {
    /// The playbook of the steps, or the refusal: `length` above 64 steps, `malformed` for no step or a step
    /// whose `n` is not its place.
    pub fn new(steps: Vec<ExecutableStep>) -> Result<Playbook, Refusal> {
        if steps.len() > PLAYBOOK_STEPS_MAX {
            return Err(Refusal::length("Playbook", "steps"));
        }
        if steps.is_empty() {
            return Err(Refusal::malformed("Playbook", "no_step"));
        }
        for (i, step) in steps.iter().enumerate() {
            if usize::try_from(step.n.get()).ok() != Some(i + 1) {
                return Err(Refusal::malformed("Playbook", "number_order").at("n"));
            }
            step.rules()?;
            step.check_len()?;
        }
        Ok(Playbook(steps))
    }

    /// The one reader of a playbook file: UTF-8 with no byte-order mark, one step per line in the order of `n`,
    /// each line ended by LF, no CR and no empty line.
    pub fn parse_file(bytes: &[u8]) -> Result<Playbook, Refusal> {
        let text =
            std::str::from_utf8(bytes).map_err(|_| Refusal::malformed("Playbook", "utf8"))?;
        if text.starts_with('\u{feff}') || text.contains('\r') {
            return Err(Refusal::malformed("Playbook", "text_form"));
        }
        let Some(body) = text.strip_suffix('\n') else {
            return Err(Refusal::malformed("Playbook", "last_line"));
        };
        let lines: Vec<&str> = body.split('\n').collect();
        if lines.len() > PLAYBOOK_STEPS_MAX {
            return Err(Refusal::length("Playbook", "steps"));
        }
        let steps = lines
            .into_iter()
            .map(|line| {
                if line.is_empty() {
                    return Err(Refusal::malformed("Playbook", "empty_line"));
                }
                ExecutableStep::parse(line.as_bytes())
            })
            .collect::<Result<Vec<_>, _>>()?;
        Playbook::new(steps)
    }

    /// The file: the compact JSON of each step on its own line, each line ended by LF.
    pub fn to_file(&self) -> Result<String, Refusal> {
        let mut out = String::new();
        for step in &self.0 {
            out.push_str(&step.to_json()?);
            out.push('\n');
        }
        Ok(out)
    }

    /// The steps in order.
    pub fn steps(&self) -> &[ExecutableStep] {
        &self.0
    }
}
