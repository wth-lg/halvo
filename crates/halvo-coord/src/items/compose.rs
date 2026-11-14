use super::key::{say, Key};
use super::raise::choices;
use super::{ConfirmState, Confirmation};
use crate::guard::Table;
use crate::types::{
    AgentId, BindResult, BoundFrom, Candidate, CandidateFrom, Cause, Choice, ChoiceText, Class,
    ConfirmAsk, Field, FieldFrom, Item, ItemId, ItemKind, Limits, Origin, Param, ParamEntry,
    Presenter, Unknown, YesNo,
};

const PIECE_MAX: usize = 1024;

/// The kinds whose item holds a title, a text and two choices and nothing else (I3.4); a
/// `link_account` item also holds the field of its site (`compose_link_account`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plain {
    /// `sign_in`.
    SignIn,
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
            Plain::Stuck => ItemKind::Stuck,
            Plain::Ready => ItemKind::Ready,
            Plain::Show => ItemKind::Show,
        }
    }
}

/// The site's name of an origin, which is what a person reads of it (I1.4, ruling IG-14): its
/// host, with `:` and its port where the port is not the usual one of its scheme (443 for
/// `https`, 80 for `http`), and never its scheme.
pub fn site_name(origin: &Origin) -> String {
    let usual = match origin.scheme() {
        "https" => 443,
        "http" => 80,
        _ => 0,
    };
    let mut site = origin.host().to_owned();
    if origin.port() != usual {
        site.push(':');
        site.push_str(&origin.port().to_string());
    }
    site
}

/// A field set by Halvo that names a site: the sentence of `key` as its label and the site's name
/// of `origin` as its value (I3.4, I4.3, ruling IG-14).
fn site_field(key: &str, origin: &Origin) -> Result<Field, Cause> {
    Ok(Field {
        label: say(key)?,
        value: pieces(&[site_name(origin)])?,
        from: FieldFrom::Halvo,
        from_text: say(&format!("from.{}", FieldFrom::Halvo.name()))?,
    })
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
/// `native` the one choice is `present`. Above the values of an effect for which the newest
/// decision released in `table`, the guard's table of this life as it stood at the item's
/// issue, is `allow_once` for the effect in the request's run, stands the line
/// `confirm.changed`, with no value and `from` `halvo` (`Table::newest_allows`, rulings IG-6d,
/// IG-25). For each effect every bound field follows in the
/// request's order, labelled `field.<class>.<name>` with `from` by its source, and after them a
/// line `confirm.new_site` for a `new_origin`, whose value is the site's name of that origin
/// (`site_name`, ruling IG-14); `armed` `yes` only while the item is `armed`.
/// `malformed` for a field that the words table has no label for, or a piece of a value of
/// more than 1,024 bytes; `state` for a confirmation of another request. Its `say_new` and
/// `say_gone` are empty, because the agent's name is not among its inputs: whoever composes
/// the snapshot names the agent with `Item::said_by`.
pub fn compose_confirm(
    item: &ItemId,
    ask: &ConfirmAsk,
    confirmation: &Confirmation,
    table: &Table,
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
        if table.newest_allows(ask.run(), &effect.effect) {
            fields.push(halvo_line("confirm.changed")?);
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
            fields.push(site_field("confirm.new_site", origin)?);
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
    match kind {
        Plain::SignIn => Item::sign_in(item, agent, title, text, choices),
        Plain::Stuck => Item::stuck(item, agent, title, text, choices),
        Plain::Ready => Item::ready(item, agent, title, text, choices),
        Plain::Show => Item::show(item, agent, title, text, choices),
    }
    .said_by(name)
}

/// The item of the second step of a `sign_in` item (I3.1, I3.4): the title `item.sign_in.title`
/// with the agent's name, the text `item.sign_in.bind` with the site's name of `origin` (ruling
/// IG-14), and the two choices `decline` and `allow` with the texts
/// `choice.sign_in.bind_decline` and `choice.sign_in.bind_allow`, the safe one first (ruling
/// IG-15).
pub fn compose_bind(
    item: &ItemId,
    agent: &AgentId,
    name: &str,
    origin: &Origin,
) -> Result<Item, Cause> {
    let title = Key::parse("item.sign_in.title")?.text(&[("name", name)])?;
    let text = Key::parse("item.sign_in.bind")?.text(&[("origin", &site_name(origin))])?;
    let mut choices = Vec::new();
    for (i, result) in BindResult::ALL.iter().enumerate() {
        choices.push(ChoiceText {
            choice: result.choice(),
            text: say(&format!("choice.sign_in.bind_{}", result.name()))?,
            safe: (i == 0).into(),
        });
    }
    Item::sign_in(item.clone(), agent.clone(), title, text, choices).said_by(name)
}

/// The item `link_account` (I3.4): the plain parts and one field, labelled
/// `item.link_account.origin`, whose value is the site's name of the request's `origin` and whose
/// `from` is `halvo` (ruling IG-14).
pub fn compose_link_account(
    item: &ItemId,
    agent: &AgentId,
    name: &str,
    origin: &Origin,
) -> Result<Item, Cause> {
    let (title, text) = title_and_text(ItemKind::LinkAccount, name)?;
    Item::link_account(
        item.clone(),
        agent.clone(),
        title,
        text,
        site_field("item.link_account.origin", origin)?,
        two_choices(ItemKind::LinkAccount)?,
    )
    .said_by(name)
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
    Item::check(
        item.clone(),
        agent.clone(),
        title,
        text,
        fields,
        two_choices(ItemKind::Check)?,
    )
    .said_by(name)
}

/// The item `setup` (I3.5): the plain parts and one entry per parameter with the label
/// `param.<name>`, its candidates numbered from 1, none chosen, and `typed` `yes` only for a
/// parameter with no candidate; with `not_taken`, for an item that asks every parameter again
/// after the owner refused its answers by a name after which another value can pass, one field
/// whose label is `item.setup.not_taken`, with no value and `from` `halvo` (I3.4, ruling IG-34).
/// `malformed` for more than `setup_params` parameters, more than `setup_candidates` candidates
/// of one, or a piece of a value of more than 1,024 bytes.
pub fn compose_setup(
    item: &ItemId,
    agent: &AgentId,
    name: &str,
    params: &[SetupParam],
    not_taken: bool,
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
        let typed = candidates.is_empty();
        entries.push(ParamEntry {
            name: p.param,
            label: say(&format!("param.{}", p.param.name()))?,
            typed: typed.into(),
            candidates,
            full: if typed {
                say("param_typed.full")?
            } else {
                String::new()
            },
            too_long: if typed {
                say("param_typed.too_long")?
            } else {
                String::new()
            },
        });
    }
    let line = if not_taken {
        Some(halvo_line("item.setup.not_taken")?)
    } else {
        None
    };
    Item::setup(
        item.clone(),
        agent.clone(),
        title,
        text,
        line,
        entries,
        two_choices(ItemKind::Setup)?,
    )
    .said_by(name)
}
