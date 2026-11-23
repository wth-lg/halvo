use super::chars::{
    below_in_a_row, columns, composed, marks_in_a_row, no_form, orphan_mark, MOST_BELOW, MOST_MARKS,
};
use super::drive::Held;
use super::key::{say, Key};
use super::presenter::request_presenter;
use super::raise::choices;
use super::{ConfirmState, Confirmation};
use crate::guard::Table;
use crate::types::{
    AgentId, AskedEffect, BindResult, BoundField, BoundFrom, Candidate, CandidateFrom, CardEffect,
    Cause, Choice, ChoiceText, Class, ConfirmAsk, Field, FieldFrom, HandbackAsk, HandbackReason,
    Item, ItemId, ItemKind, Limits, Origin, Param, ParamEntry, Presenter, System, Unknown, YesNo,
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
    halvo_said(say(key)?)
}

/// A line of Halvo whose label is `label`, a sentence that the core composed, with no value and
/// `from` `halvo` (I3.4).
fn halvo_said(label: String) -> Result<Field, Cause> {
    Ok(Field {
        label,
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
/// more than 1,024 bytes; `state` for a confirmation of another request. Its `text`, `say_new`
/// and `say_gone` are empty, because the agent's name is not among its inputs: whoever composes
/// the snapshot names the agent with `Item::said_by` and its place with `Item::placed` (I3.4).
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
    let fields = confirm_fields(ask, named, table)?;
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

fn confirm_fields(ask: &ConfirmAsk, named: Named, table: &Table) -> Result<Vec<Field>, Cause> {
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
    Ok(fields)
}

/// The most lines of the native dialog's message, the empty line between two fields counted
/// (ruling RD-90 items 1 and 9): the dialog's proven bound, the smaller of the two systems', read
/// on a hosted screen of 1,024 by 768 by the target dialog_hosted of halvo-coord-app. On macOS a
/// card of `W` lines of 96 columns reads a window of 405, 685 and 965 points for 7, 15 and 23
/// lines in 684 visible, 35 points a line; on Windows one of 23 lines reads 635 pixels in a work
/// area of 720. `CARD_COLUMNS` holds the longest line that Halvo sets with no value.
pub const CARD_LINES: usize = 13;

/// The most columns (`chars::columns`, display width) of the native dialog's title, of one line
/// of its message and of one control text (ruling RD-90 items 1 and 9): the dialog's proven
/// bound, the smaller of the two systems', read on a hosted screen as `CARD_LINES` is.
pub const CARD_COLUMNS: usize = 96;

/// What a bound field is to a confirmation (ruling RD-90 item 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marking {
    /// A field on which the person decides: its value equals its own composed form
    /// (`chars::composed`) at its bound (`value_bound`).
    Deciding,
    /// Free text: shown on the page whole and as it is bound; in the native dialog it holds no
    /// character with no visible form, a line end among them, and its line fits.
    FreeText,
}

/// The table of I4.3 with A-A9.3: for a field of a confirm class, the sources that A-A9.3
/// admits for it and its marking (ruling RD-89 item 4, ruling RD-90 item 3); none for a field
/// that the class does not bind.
pub fn bound_field(class: Class, field: &str) -> Option<(&'static [BoundFrom], Marking)> {
    use BoundFrom::*;
    use Marking::*;
    let row: (&'static [BoundFrom], Marking) = match (class, field) {
        (Class::Payment, "payee" | "instrument") => (&[Slot], Deciding),
        (Class::Payment, "amount") => (&[Extract], Deciding),
        (Class::Payment, "origin") => (&[Const], Deciding),
        (Class::Purchase, "seller" | "instrument") => (&[Slot], Deciding),
        (Class::Purchase, "items" | "amount") => (&[Extract], Deciding),
        (Class::Purchase, "origin") => (&[Const], Deciding),
        (Class::Send, "to") => (&[Extract], Deciding),
        (Class::Send, "subject") => (&[Extract], FreeText),
        (Class::Send, "body") => (&[Model, Const], FreeText),
        (Class::Delete, "items") => (&[Extract], Deciding),
        (Class::Submit, "origin") => (&[Const], Deciding),
        (Class::Submit, "fields") => (&[Const, Slot], FreeText),
        (Class::Sign, "document" | "party") => (&[Slot], Deciding),
        _ => return None,
    };
    Some(row)
}

/// The fields of a confirm class in the order of A-A9.3; none for a class that I4.3 does not
/// confirm.
pub fn class_fields(class: Class) -> &'static [&'static str] {
    match class {
        Class::Payment => &["payee", "amount", "instrument", "origin"],
        Class::Purchase => &["seller", "items", "amount", "instrument", "origin"],
        Class::Send => &["to", "subject", "body"],
        Class::Delete => &["items"],
        Class::Submit => &["origin", "fields"],
        Class::Sign => &["document", "party"],
        _ => &[],
    }
}

/// Whether the fields of `effect` stand in the order of A-A9.3, each at most once, with every
/// deciding field of its class (ruling RD-132 item 2).
fn fields_in_order(effect: &AskedEffect) -> bool {
    let order = class_fields(effect.class);
    let mut next = 0;
    for field in &effect.fields {
        match order[next..].iter().position(|n| *n == field.name.as_str()) {
            Some(k) => next += k + 1,
            None => return false,
        }
    }
    order.iter().all(|n| {
        bound_field(effect.class, n).map(|(_, m)| m) != Some(Marking::Deciding)
            || effect.fields.iter().any(|f| f.name.as_str() == *n)
    })
}

fn field_line(label: &str, source: &str, value: &str) -> Result<String, Cause> {
    Key::parse("confirm.field")?.text(&[("label", label), ("source", source), ("value", value)])
}

/// The most columns of the value of a field of `class` named `field` with the source `from`:
/// what the line `confirm.field` with its label and its source leaves of `CARD_COLUMNS`.
/// `malformed` for a field that the words table has no label for.
pub fn value_bound(class: Class, field: &str, from: BoundFrom) -> Result<usize, Cause> {
    let label = say(&format!("field.{}.{field}", class.name()))?;
    let source = say(&format!("from.{}", field_from(from).name()))?;
    let prefix = field_line(&label, &source, "")?;
    Ok(CARD_COLUMNS.saturating_sub(columns(&prefix)))
}

fn field_admitted(class: Class, field: &BoundField) -> bool {
    let name = field.name.as_str();
    match bound_field(class, name) {
        Some((sources, _)) if !sources.contains(&field.from) => false,
        Some((_, Marking::Deciding)) => value_bound(class, name, field.from)
            .is_ok_and(|most| composed(&field.value.concat(), most)),
        Some((_, Marking::FreeText)) => true,
        None => false,
    }
}

/// Whether a confirm request may show a card (ruling RD-89 item 4, ruling RD-90 items 1 to 4,
/// ruling RD-131 item 1, ruling RD-132 item 2): each effect holds every deciding field of its
/// class, no field twice, in the order of A-A9.3 (`class_fields`); each field is one that
/// A-A9.3 binds for its class, with a source that A-A9.3 admits for it;
/// each deciding field's value equals its own composed form at its bound; and, for a request of
/// the presenter `native`, its card composes (`native_lines`, with the lines `confirm.changed`
/// that `table`, the guard's table of this life, gives). A request that it refuses ends its run
/// at the raise with `kind`, as one of more effects than `request_records` does (ruling RD-73
/// 7.4); nothing is cut and nothing cleaned.
pub fn admits(ask: &ConfirmAsk, table: &Table) -> bool {
    let bound = ask
        .effects()
        .iter()
        .all(|e| e.fields.iter().all(|f| field_admitted(e.class, f)));
    bound
        && ask.effects().iter().all(fields_in_order)
        && (request_presenter(ask) == Presenter::Page
            || confirm_fields(ask, named(ask), table)
                .and_then(|f| native_lines(&f))
                .is_ok())
}

/// The message lines of the native dialog for the fields of an item (ruling RD-89 items 1 to
/// 3): a field with a value is the line `confirm.field` with its label, its source text and its
/// value, its pieces joined with nothing; a field with no value is its label alone; an empty
/// line stands between two fields. `malformed` for a field whose source text is empty, a line
/// that holds a character with no visible form (`chars::no_form`), more than
/// `chars::MOST_MARKS` marks or more than `chars::MOST_BELOW` marks of a below class in a row on
/// its canonical decomposition (ruling RD-131 item 1, ruling RD-137 M1 and M3), a mark with no
/// base before it (ruling RD-137 M2) or more than
/// `CARD_COLUMNS` columns, and more than `CARD_LINES` lines.
pub fn native_lines(fields: &[Field]) -> Result<Vec<String>, Cause> {
    let mut lines = Vec::new();
    for field in fields {
        if field.from_text.is_empty() {
            return Err(Cause::Malformed);
        }
        if !lines.is_empty() {
            lines.push(String::new());
        }
        let line = if field.value.is_empty() {
            field.label.clone()
        } else {
            field_line(&field.label, &field.from_text, &field.value.concat())?
        };
        if line.chars().any(no_form)
            || marks_in_a_row(&line) > MOST_MARKS
            || below_in_a_row(&line) > MOST_BELOW
            || orphan_mark(&line)
            || columns(&line) > CARD_COLUMNS
        {
            return Err(Cause::Malformed);
        }
        lines.push(line);
    }
    if lines.len() > CARD_LINES {
        return Err(Cause::Malformed);
    }
    Ok(lines)
}

/// The card of the native dialog (ruling RD-77 part 12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeCard {
    title: String,
    lines: Vec<String>,
    reject: ChoiceText,
    allow: ChoiceText,
}

impl NativeCard {
    /// The title: that of the native item for the same id.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The message lines, with an empty line between two fields (`native_card`).
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The rejecting control, the safe one, which stands first.
    pub fn reject(&self) -> &ChoiceText {
        &self.reject
    }

    /// The allowing control.
    pub fn allow(&self) -> &ChoiceText {
        &self.allow
    }
}

/// The card of the native dialog of `held` (ruling RD-77 12.1 to 12.3, ruling RD-89 items 1 to
/// 4): the title of the native item that the state holds for the same id, one message line for
/// each of its fields in its order with an empty line between two (`native_lines`), and the two
/// choices that `compose_confirm` gives the same ask under the presenter `page`, the rejecting
/// one first. It calls no port and stores nothing. `stale` for a confirmation that is released
/// or lapsed; `presenter` for one of the presenter `page`; `malformed` for a card whose fields
/// do not compose, a field whose source text is empty, and a card over `CARD_LINES` lines or a
/// line over `CARD_COLUMNS` columns (`native_lines`); its title is a words entry and needs no
/// check.
pub fn native_card(held: &Held) -> Result<NativeCard, Cause> {
    let confirmation = held.confirmation();
    if !confirmation.state().is_open() {
        return Err(Cause::Stale);
    }
    if confirmation.presenter() != Presenter::Native {
        return Err(Cause::Presenter);
    }
    let item = held.page_item()?;
    let named = named(held.ask());
    let count = held.ask().effects().len();
    Ok(NativeCard {
        title: item.title().to_owned(),
        lines: native_lines(item.fields())?,
        reject: confirm_choice(named, Choice::Reject, count, true)?,
        allow: confirm_choice(named, Choice::Allow, count, false)?,
    })
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

/// The item of the first step of a `sign_in` item whose hand-back `ask` the coordinator keeps
/// beside it (I3.1, I3.4): the title `item.sign_in.title` with the agent's name, the text, and
/// the kind's two choices, the safe one first. Where the ask names an origin, the text is
/// `item.sign_in.site` with the site's name of that origin; where the origin is `insecure`, an
/// `http` site outside 127.0.0.0/8 that takes a sign-in over a connection that is not protected,
/// its scheme stands before the site's name, as that is the one case where the scheme is the
/// thing to see: the origin's own text, without its port where the site's name has none. Where
/// the ask's reason is `sign_in`, the text is instead `item.sign_in.site_kept` with that name in
/// both places, or `item.sign_in.text_kept` where the ask names no origin: the same sentence and
/// behind it the two that say that Halvo keeps this agent signed in to the site on this computer
/// and when it removes the sign-in, one whole entry each (I1.1). With no ask, or an ask of
/// another reason that names no origin, it is the item of `compose_plain`, which names no site.
pub fn compose_sign_in(
    item: &ItemId,
    agent: &AgentId,
    name: &str,
    ask: Option<&HandbackAsk>,
) -> Result<Item, Cause> {
    let kept = ask.is_some_and(|a| a.reason == HandbackReason::SignIn);
    let text = match ask.and_then(|a| a.origin.as_ref()) {
        None if !kept => return compose_plain(item, agent, Plain::SignIn, name),
        None => say("item.sign_in.text_kept")?,
        Some(origin) => {
            let full = origin.as_str();
            let site = if !origin.insecure() {
                site_name(origin)
            } else if site_name(origin).contains(':') {
                full.to_owned()
            } else {
                full.rsplit_once(':')
                    .map_or(full, |(no_port, _)| no_port)
                    .to_owned()
            };
            let key = if kept {
                "item.sign_in.site_kept"
            } else {
                "item.sign_in.site"
            };
            Key::parse(key)?.text(&[("origin", &site)])?
        }
    };
    let title = Key::parse("item.sign_in.title")?.text(&[("name", name)])?;
    let choices = two_choices(ItemKind::SignIn)?;
    Item::sign_in(item.clone(), agent.clone(), title, text, choices).said_by(name)
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

/// The item `check` (I3.4): the plain parts and, for each effect of an unknown outcome in order,
/// the kept fields that the card's entry of that effect shows (`CardEffect::unknown`, ruling
/// RD-158 (5)): each labelled `field.<class>.<name>` with `from` by the source that A-A9.3 admits
/// for it, and no byte of a kept value that does not stand on that entry.
pub fn compose_check(
    item: &ItemId,
    agent: &AgentId,
    name: &str,
    unknown: &[Unknown],
) -> Result<Item, Cause> {
    let (title, text) = title_and_text(ItemKind::Check, name)?;
    let mut fields = Vec::new();
    for effect in unknown {
        fields.extend_from_slice(CardEffect::unknown(effect.class, &effect.kept)?.fields());
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
    let line = if not_taken {
        Some(say("item.setup.not_taken")?)
    } else {
        None
    };
    setup_with_line(item, agent, name, params, line, limits)
}

/// `compose_setup` whose one field, where `line` holds a sentence that the core composed, is the
/// line of Halvo with that sentence as its label: the line `item.setup.not_taken`, or the
/// sentence of a refusal `overlap` in its place (I3.5 (c)).
pub(crate) fn setup_with_line(
    item: &ItemId,
    agent: &AgentId,
    name: &str,
    params: &[SetupParam],
    line: Option<String>,
    limits: &Limits,
) -> Result<Item, Cause> {
    setup_on(System::HOST, item, agent, name, params, line, limits)
}

/// What to type for `param` on `system` (r4 U3.22a): the sentence of `param_hint.<name>`, or of
/// `param_hint.<name>_mac` or `param_hint.<name>_windows` where the answer is a path, whose
/// example is a path of that system; the empty text for every other parameter.
pub fn param_hint(param: Param, system: System) -> Result<String, Cause> {
    let key = match (param, system) {
        (Param::Workbook, System::Mac) => "param_hint.workbook_mac",
        (Param::Workbook, System::Windows) => "param_hint.workbook_windows",
        (Param::Sheet, _) => "param_hint.sheet",
        (Param::Range, _) => "param_hint.range",
        (Param::Source, System::Mac) => "param_hint.source_mac",
        (Param::Source, System::Windows) => "param_hint.source_windows",
        _ => return Ok(String::new()),
    };
    say(key)
}

/// `setup_with_line` with the hints of `system` (`param_hint`) in place of this build's.
pub(crate) fn setup_on(
    system: System,
    item: &ItemId,
    agent: &AgentId,
    name: &str,
    params: &[SetupParam],
    line: Option<String>,
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
            hint: param_hint(p.param, system)?,
            needs_answer: if typed {
                say("param_typed.needs_answer")?
            } else {
                String::new()
            },
            choose: match p.param {
                Param::Workbook | Param::Source if typed => Some(say("param_typed.choose")?),
                _ => None,
            },
            refused: None,
            not_taken: None,
        });
    }
    let line = line.map(halvo_said).transpose()?;
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
