use super::key::{say, Key};
use super::raise::choices;
use super::{ConfirmState, Confirmation};
use crate::types::{
    AgentId, BoundFrom, Candidate, CandidateFrom, Cause, Choice, ChoiceText, Class, ConfirmAsk,
    Field, FieldFrom, Item, ItemId, ItemKind, Limits, Param, ParamEntry, Presenter, Unknown, YesNo,
};

const PIECE_MAX: usize = 1024;

/// The kinds whose item holds a title, a text and two choices and nothing else (I3.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plain {
    /// `sign_in`.
    SignIn,
    /// `link_account`.
    LinkAccount,
    /// `stuck`.
    Stuck,
    /// `ready`.
    Ready,
    /// `show`.
    Show,
}

impl Plain {
    /// The kind's name in `ItemKind`.
    pub const fn kind(self) -> ItemKind {
        match self {
            Plain::SignIn => ItemKind::SignIn,
            Plain::LinkAccount => ItemKind::LinkAccount,
            Plain::Stuck => ItemKind::Stuck,
            Plain::Ready => ItemKind::Ready,
            Plain::Show => ItemKind::Show,
        }
    }
}

/// A parameter of a setup with the values of its candidates and where each came from, as the
/// coordinator found them (I3.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupParam {
    /// The parameter.
    pub param: Param,
    /// The candidates: a value in pieces and its source.
    pub candidates: Vec<(Vec<String>, CandidateFrom)>,
}

/// The page's word for the source of a bound value (I4.3): `slot` is `you`, `extract` is
/// `site`, `model` is `agent` and `const` is `halvo`.
pub const fn field_from(from: BoundFrom) -> FieldFrom {
    match from {
        BoundFrom::Slot => FieldFrom::You,
        BoundFrom::Extract => FieldFrom::Site,
        BoundFrom::Model => FieldFrom::Agent,
        BoundFrom::Const => FieldFrom::Halvo,
    }
}

/// The source of a kept field of an effect, the one source that A-A9.3 admits for that field of
/// that class; none for a field that the effect row does not keep.
pub fn kept_source(class: Class, field: &str) -> Option<BoundFrom> {
    use BoundFrom::*;
    match (class, field) {
        (Class::Payment | Class::Purchase, "amount") => Some(Extract),
        (Class::Payment | Class::Purchase | Class::Submit, "origin") => Some(Const),
        (Class::Payment, "payee") | (Class::Purchase, "seller") => Some(Slot),
        (Class::Payment | Class::Purchase, "instrument") => Some(Slot),
        (Class::Send, "to") | (Class::Delete, "items") => Some(Extract),
        (Class::Sign, "document" | "party") | (Class::LocalEdit, "file" | "range") => Some(Slot),
        _ => None,
    }
}

fn pieces(value: &[String]) -> Result<Vec<String>, Cause> {
    if value.iter().all(|p| p.len() <= PIECE_MAX) {
        Ok(value.to_vec())
    } else {
        Err(Cause::Malformed)
    }
}

fn field(class: Class, name: &str, value: &[String], from: FieldFrom) -> Result<Field, Cause> {
    Ok(Field {
        label: say(&format!("field.{}.{name}", class.name()))?,
        value: pieces(value)?,
        from,
        from_text: say(&format!("from.{}", from.name()))?,
    })
}

fn choice_text(kind: ItemKind, choice: Choice, safe: bool) -> Result<ChoiceText, Cause> {
    Ok(ChoiceText {
        choice,
        text: say(&format!("choice.{}.{}", kind.name(), choice.name()))?,
        safe: safe.into(),
    })
}

fn two_choices(kind: ItemKind) -> Result<Vec<ChoiceText>, Cause> {
    let [safe, other] = choices(kind);
    Ok(vec![
        choice_text(kind, safe, true)?,
        choice_text(kind, other, false)?,
    ])
}

fn title_and_text(kind: ItemKind, name: &str) -> Result<(String, String), Cause> {
    Ok((
        Key::parse(&format!("item.{}.title", kind.name()))?.text(&[("name", name)])?,
        say(&format!("item.{}.text", kind.name()))?,
    ))
}

/// What names a confirmation: the one class of all its effects, or none where its effects have
/// more than one class (I4.3, ruling IG-4).
#[derive(Clone, Copy)]
enum Named {
    One(Class),
    Mixed,
}

fn named(ask: &ConfirmAsk) -> Named {
    let first = ask.effects()[0].class;
    if ask.effects().iter().all(|e| e.class == first) {
        Named::One(first)
    } else {
        Named::Mixed
    }
}

fn counted(key: &str, count: usize) -> Result<String, Cause> {
    Key::parse(key)?.text(&[("count", &count.to_string())])
}

fn confirm_title(named: Named, count: usize) -> Result<String, Cause> {
    match named {
        Named::One(class) => say(&format!("confirm.{}.title", class.name())),
        Named::Mixed => counted("confirm.mixed.title", count),
    }
}

fn confirm_choice(
    named: Named,
    choice: Choice,
    count: usize,
    safe: bool,
) -> Result<ChoiceText, Cause> {
    let text = match named {
        Named::One(class) => {
            let key = format!("confirm.{}.{}", class.name(), choice.name());
            if count > 1 {
                counted(&format!("{key}_many"), count)?
            } else {
                say(&key)?
            }
        }
        Named::Mixed => counted(&format!("confirm.mixed.{}", choice.name()), count)?,
    };
    Ok(ChoiceText {
        choice,
        text,
        safe: safe.into(),
    })
}

/// A line of an item set by Halvo with no value: the sentence of `key` as its label.
fn halvo_line(key: &str) -> Result<Field, Cause> {
    Ok(Field {
        label: say(key)?,
        value: Vec::new(),
        from: FieldFrom::Halvo,
        from_text: say(&format!("from.{}", FieldFrom::Halvo.name()))?,
    })
}

/// The item of a confirmation as the page gets it (I3.4, I4.3). A request whose effects have
/// one class takes the title and the two controls of that class, the `_many` entries with
/// `{count}` for more than one effect; a request whose effects have more than one class takes
/// `confirm.mixed.title`, `confirm.mixed.reject` and `confirm.mixed.allow` with `{count}`,
/// which name no class, and before the fields of each effect a line `confirm.<class>.action`
/// of that effect's class, with no value and `from` `halvo` (ruling IG-4). For the presenter
/// `native` the one choice is `present`. For each effect every bound field follows in the
/// request's order, labelled `field.<class>.<name>` with `from` by its source, and after them a
/// line `confirm.new_site` for a `new_origin`; `armed` `yes` only while the item is `armed`.
/// `malformed` for a field that the words table has no label for, or a piece of a value of
/// more than 1,024 bytes; `state` for a confirmation of another request.
pub fn compose_confirm(
    item: &ItemId,
    ask: &ConfirmAsk,
    confirmation: &Confirmation,
) -> Result<Item, Cause> {
    let same = ask.effects().len() == confirmation.asked().len()
        && ask
            .effects()
            .iter()
            .zip(confirmation.asked())
            .all(|(e, (effect, digest))| e.effect == *effect && e.digest == *digest);
    if !same {
        return Err(Cause::State);
    }
    let presenter = confirmation.presenter();
    let named = named(ask);
    let count = ask.effects().len();
    let mut fields = Vec::new();
    for effect in ask.effects() {
        if let Named::Mixed = named {
            fields.push(halvo_line(&format!(
                "confirm.{}.action",
                effect.class.name()
            ))?);
        }
        for f in &effect.fields {
            fields.push(field(
                effect.class,
                f.name.as_str(),
                &f.value,
                field_from(f.from),
            )?);
        }
        if let Some(origin) = &effect.new_origin {
            fields.push(Field {
                label: say("confirm.new_site")?,
                value: pieces(&[origin.as_str().to_owned()])?,
                from: FieldFrom::Halvo,
                from_text: say(&format!("from.{}", FieldFrom::Halvo.name()))?,
            });
        }
    }
    let choices = match presenter {
        Presenter::Native => vec![ChoiceText {
            choice: Choice::Present,
            text: say("confirm.present")?,
            safe: YesNo::Yes,
        }],
        Presenter::Page => vec![
            confirm_choice(named, Choice::Reject, count, true)?,
            confirm_choice(named, Choice::Allow, count, false)?,
        ],
    };
    Ok(Item::confirm(
        item.clone(),
        ask.agent().clone(),
        presenter,
        (*confirmation.state() == ConfirmState::Armed).into(),
        confirm_title(named, count)?,
        fields,
        choices,
    ))
}

/// The item of a plain kind (I3.4): `item.<kind>.title` with the agent's name, `item.<kind>.text`
/// and the kind's two choices, the safe one first.
pub fn compose_plain(
    item: &ItemId,
    agent: &AgentId,
    kind: Plain,
    name: &str,
) -> Result<Item, Cause> {
    let (title, text) = title_and_text(kind.kind(), name)?;
    let choices = two_choices(kind.kind())?;
    let (item, agent) = (item.clone(), agent.clone());
    Ok(match kind {
        Plain::SignIn => Item::sign_in(item, agent, title, text, choices),
        Plain::LinkAccount => Item::link_account(item, agent, title, text, choices),
        Plain::Stuck => Item::stuck(item, agent, title, text, choices),
        Plain::Ready => Item::ready(item, agent, title, text, choices),
        Plain::Show => Item::show(item, agent, title, text, choices),
    })
}

/// The item `check` (I3.4): the plain parts and the kept fields of the effects of an unknown
/// outcome, each labelled `field.<class>.<name>` with `from` by the source that A-A9.3 admits
/// for it. `malformed` for a kept field that A-A9.3 does not keep or that the words table has
/// no label for.
pub fn compose_check(
    item: &ItemId,
    agent: &AgentId,
    name: &str,
    unknown: &[Unknown],
) -> Result<Item, Cause> {
    let (title, text) = title_and_text(ItemKind::Check, name)?;
    let mut fields = Vec::new();
    for effect in unknown {
        for kept in &effect.kept {
            let from = kept_source(effect.class, kept.name.as_str()).ok_or(Cause::Malformed)?;
            fields.push(field(
                effect.class,
                kept.name.as_str(),
                &kept.value,
                field_from(from),
            )?);
        }
    }
    Ok(Item::check(
        item.clone(),
        agent.clone(),
        title,
        text,
        fields,
        two_choices(ItemKind::Check)?,
    ))
}

/// The item `setup` (I3.5): the plain parts and one entry per parameter with the label
/// `param.<name>`, its candidates numbered from 1, none chosen, and `typed` `yes` only for a
/// parameter with no candidate. `malformed` for more than `setup_params` parameters, more than
/// `setup_candidates` candidates of one, or a piece of a value of more than 1,024 bytes.
pub fn compose_setup(
    item: &ItemId,
    agent: &AgentId,
    name: &str,
    params: &[SetupParam],
    limits: &Limits,
) -> Result<Item, Cause> {
    if params.len() > limits.setup_params as usize {
        return Err(Cause::Malformed);
    }
    let (title, text) = title_and_text(ItemKind::Setup, name)?;
    let mut entries = Vec::new();
    for p in params {
        if p.candidates.len() > limits.setup_candidates as usize {
            return Err(Cause::Malformed);
        }
        let mut candidates = Vec::new();
        for (n, (value, from)) in (1..).zip(&p.candidates) {
            candidates.push(Candidate {
                n,
                value: pieces(value)?,
                from: *from,
            });
        }
        entries.push(ParamEntry {
            name: p.param,
            label: say(&format!("param.{}", p.param.name()))?,
            typed: candidates.is_empty().into(),
            candidates,
        });
    }
    Ok(Item::setup(
        item.clone(),
        agent.clone(),
        title,
        text,
        entries,
        two_choices(ItemKind::Setup)?,
    ))
}
