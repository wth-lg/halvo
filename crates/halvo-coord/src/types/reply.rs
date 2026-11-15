use super::{AgentId, CandidateFrom, Cause, Choice, Class, Control, FieldFrom, ItemId, ItemKind};
use super::{Kept, LineKey, Origin, Param, Presenter, Status, WorkflowId, YesNo};
use crate::items::{field_from, kept_source};
use crate::words;
use serde::Serialize;

/// The reply of `agents_snapshot` and of `agents_next` with a higher `seq` (I8.1).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Snapshot {
    /// `seq`: this run's own count of states, from 1.
    pub seq: u32,
    /// `rows`, in the order of I2.4.
    pub rows: Vec<Row>,
    /// `items`.
    pub items: Vec<Item>,
    /// `note`, or null.
    pub note: Option<String>,
}

/// A row of the list of agents (I2.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Row {
    /// `agent`.
    pub agent: AgentId,
    /// `workflow`.
    pub workflow: WorkflowId,
    /// `name`: the composed name of I2.3.
    pub name: String,
    /// `status`.
    pub status: Status,
    /// `word`: the text of `status.<status>`.
    pub word: String,
    /// `line`: the text of the status line of I1.3.
    pub line: String,
    /// `say`: the text of `say.<status>` with `{name}`.
    pub say: String,
    /// `level`: the text of `line.level.<reasoning>`.
    pub level: String,
    /// `next`: the day of the first slot after today, or null.
    pub next: Option<String>,
    /// `controls`.
    pub controls: Vec<ControlText>,
}

/// A control of a row with its text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ControlText {
    /// `control`.
    pub control: Control,
    /// `text`: the text of `control.<control>`.
    pub text: String,
}

/// The card of an agent (I2.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Card {
    /// `agent`.
    pub agent: AgentId,
    /// `name`.
    pub name: String,
    /// `lines`: the settings lines of I2.2.
    pub lines: Vec<CardLine>,
    /// `runs`: at most `history_runs`, newest first.
    pub runs: Vec<CardRun>,
}

/// A settings line of a card (I2.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CardLine {
    /// `key`.
    pub key: LineKey,
    /// `text`.
    pub text: String,
}

/// A run of a card (I2.6), with a line for each committed effect of the run, whose kept values
/// stand in its fields and never in its sentence (ruling H-8).
///
/// ```
/// use halvo_coord::types::{CardEffect, CardRun, Class, Kept, Name};
/// let kept = |name: &str, value: &str| Kept {
///     name: Name::parse(name).unwrap(),
///     value: vec![value.to_owned()],
/// };
/// let paid = CardEffect::committed(
///     Class::Payment,
///     &[
///         kept("payee", "City Power"),
///         kept("amount", "42.10 USD"),
///         kept("instrument", "Card ending 4242"),
///         kept("origin", "https://site1.example:443"),
///     ],
/// )
/// .unwrap();
/// let run = CardRun {
///     day: "24 October 2026".to_owned(),
///     text: "It finished the task.".to_owned(),
///     effects: vec![paid],
/// };
/// assert_eq!(
///     serde_json::to_string(&run).unwrap(),
///     concat!(
///         r#"{"day":"24 October 2026","text":"It finished the task.","effects":[{"key":"run.effect.payment","text":"It made this payment.","fields":["#,
///         r#"{"label":"Pay to","value":["City Power"],"from":"you","from_text":"You gave this"},"#,
///         r#"{"label":"Amount","value":["42.10 USD"],"from":"site","from_text":"From the site"},"#,
///         r#"{"label":"Pay with","value":["Card ending 4242"],"from":"you","from_text":"You gave this"},"#,
///         r#"{"label":"Site","value":["site1.example"],"from":"halvo","from_text":"Set by Halvo"}]}]}"#,
///     )
/// );
/// ```
///
/// The seven classes of A-A9.3 each take their own sentence; the `items` of a `delete` are kept
/// in the effect row and not shown, and a class with no effect row, a kept field out of the
/// table's order, a missing one or one that the row does not keep is `malformed`:
///
/// ```
/// use halvo_coord::types::{CardEffect, Cause, Class, Kept, Name};
/// let kept = |names: &[&str]| -> Vec<Kept> {
///     names
///         .iter()
///         .map(|n| {
///             let value = match *n {
///                 "origin" => "https://site4.example:443",
///                 "file" => "Budget.xlsx",
///                 _ => "Plan 2",
///             };
///             Kept { name: Name::parse(n).unwrap(), value: vec![value.to_owned()] }
///         })
///         .collect()
/// };
/// let texts = [
///     (Class::Payment, &["payee", "amount", "instrument", "origin"][..], "It made this payment.", 4),
///     (Class::Purchase, &["seller", "amount", "instrument", "origin"][..], "It placed this order.", 4),
///     (Class::Send, &["to"][..], "It sent this message.", 1),
///     (Class::Delete, &["items"][..], "It removed this.", 0),
///     (Class::Submit, &["origin"][..], "It sent this form.", 1),
///     (Class::Sign, &["document", "party"][..], "It signed this document.", 2),
///     (Class::LocalEdit, &["file", "range"][..], "It changed this file.", 2),
/// ];
/// for (class, names, text, shown) in texts {
///     let e = CardEffect::committed(class, &kept(names)).unwrap();
///     assert_eq!(e.key(), format!("run.effect.{}", class.name()));
///     assert_eq!((e.text(), e.fields().len()), (text, shown));
///     assert!(e.fields().iter().all(|f| !e.text().contains(&f.value[0])));
/// }
/// let labels: Vec<String> = CardEffect::committed(Class::LocalEdit, &kept(&["file", "range"]))
///     .unwrap()
///     .fields()
///     .iter()
///     .map(|f| f.label.clone())
///     .collect();
/// assert_eq!(labels, ["File", "Where in the file"]);
/// for class in [Class::Read, Class::Prepare, Class::Credential] {
///     assert_eq!(CardEffect::committed(class, &[]), Err(Cause::Malformed));
/// }
/// for names in [
///     &["amount", "payee", "instrument", "origin"][..],
///     &["payee", "amount", "instrument"][..],
///     &["payee", "amount", "instrument", "origin", "origin"][..],
///     &["payee", "amount", "instrument", "origin", "items"][..],
/// ] {
///     assert_eq!(CardEffect::committed(Class::Payment, &kept(names)), Err(Cause::Malformed));
/// }
/// assert_eq!(CardEffect::committed(Class::Send, &kept(&["to", "subject"])), Err(Cause::Malformed));
/// ```
///
/// An `origin` is shown as the site's name, its host with the port only where that is not the
/// usual one of its scheme and never the scheme (I1.4), and the `file` of a `local_edit` as the
/// name of the file alone, after the last separator of either kind; a kept value that is no
/// origin, or a file whose name is empty, is `malformed`:
///
/// ```
/// use halvo_coord::types::{CardEffect, Cause, Class, Kept, Name};
/// let one = |name: &str, value: &str| Kept { name: Name::parse(name).unwrap(), value: vec![value.to_owned()] };
/// let site = |origin: &str| {
///     CardEffect::committed(Class::Submit, &[one("origin", origin)]).map(|e| e.fields()[0].value.clone())
/// };
/// assert_eq!(site("https://site1.example:443"), Ok(vec!["site1.example".to_owned()]));
/// assert_eq!(site("http://site1.example:80"), Ok(vec!["site1.example".to_owned()]));
/// assert_eq!(site("https://site1.example:8443"), Ok(vec!["site1.example:8443".to_owned()]));
/// assert_eq!(site("http://site1.example:443"), Ok(vec!["site1.example:443".to_owned()]));
/// assert_eq!(site("site1.example"), Err(Cause::Malformed));
/// let file = |path: &str| {
///     CardEffect::committed(Class::LocalEdit, &[one("file", path), one("range", "B2 to B9")])
///         .map(|e| e.fields()[0].value.clone())
/// };
/// assert_eq!(file("/home/pat/Documents/Budget.xlsx"), Ok(vec!["Budget.xlsx".to_owned()]));
/// assert_eq!(file("C:\\Users\\pat\\Budget.xlsx"), Ok(vec!["Budget.xlsx".to_owned()]));
/// assert_eq!(file("Budget.xlsx"), Ok(vec!["Budget.xlsx".to_owned()]));
/// assert_eq!(file("/home/pat/"), Err(Cause::Malformed));
/// assert_eq!(file("C:Budget.xlsx"), Ok(vec!["Budget.xlsx".to_owned()]));
/// assert_eq!(file("/home/pat/.."), Err(Cause::Malformed));
/// assert_eq!(file("."), Err(Cause::Malformed));
/// for (class, names) in [
///     (Class::Payment, &["payee", "amount", "instrument", "origin"][..]),
///     (Class::Purchase, &["seller", "amount", "instrument", "origin"][..]),
///     (Class::Submit, &["origin"][..]),
/// ] {
///     let kept: Vec<Kept> = names
///         .iter()
///         .map(|n| one(n, if *n == "origin" { "https://shop.example:443" } else { "Plan 2" }))
///         .collect();
///     let e = CardEffect::committed(class, &kept).unwrap();
///     assert_eq!(e.fields().last().unwrap().value, ["shop.example"], "{}", class.name());
/// }
/// ```
///
/// An entry is made only from its class and its kept fields, so its sentence holds no value:
///
/// ```compile_fail,E0451
/// use halvo_coord::types::{CardEffect, Class};
/// let e = CardEffect {
///     class: Class::Payment,
///     key: "run.effect.payment".to_owned(),
///     text: "It paid City Power.".to_owned(),
///     fields: vec![],
/// };
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CardRun {
    /// `day`: the day of its `run_started` event as text.
    pub day: String,
    /// `text`: the sentence of `line.run.<end>`.
    pub text: String,
    /// `effects`: one entry for each committed effect of the run, in the order of the reply of
    /// `agent_history`; empty for a run with no committed effect.
    pub effects: Vec<CardEffect>,
}

/// A committed effect of a past run on a card (I2.6, ruling H-8): the sentence of
/// `run.effect.<class>`, which has no placeholder, and the kept fields of its class as fields of
/// I3.4, made only by [`CardEffect::committed`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CardEffect {
    #[serde(skip)]
    class: Class,
    key: String,
    text: String,
    fields: Vec<Field>,
}

/// The kept fields of each class that has an effect row, in the order of the table of A-A9.3.
fn kept_names(class: Class) -> Option<&'static [&'static str]> {
    Some(match class {
        Class::Payment => &["payee", "amount", "instrument", "origin"],
        Class::Purchase => &["seller", "amount", "instrument", "origin"],
        Class::Send => &["to"],
        Class::Delete => &["items"],
        Class::Submit => &["origin"],
        Class::Sign => &["document", "party"],
        Class::LocalEdit => &["file", "range"],
        Class::Read | Class::Prepare | Class::Credential => return None,
    })
}

/// The site's name of a kept origin (I1.4, ruling IG-14): its host, with its port only where
/// that is not the usual one of its scheme, and never its scheme.
fn site_name(kept: &[String]) -> Result<String, Cause> {
    let origin = Origin::parse(&kept.concat())?;
    let (scheme, rest) = origin.as_str().split_once("://").ok_or(Cause::Malformed)?;
    let (host, port) = rest.rsplit_once(':').ok_or(Cause::Malformed)?;
    let usual = matches!((scheme, port), ("https", "443") | ("http", "80"));
    Ok(if usual {
        host.to_owned()
    } else {
        rest.to_owned()
    })
}

/// The name of a kept file alone, after the last separator of either kind and after the drive
/// of a Windows path that has none; `.` and `..` name no file.
fn file_name(kept: &[String]) -> Result<String, Cause> {
    let path = kept.concat();
    let last = path.rsplit(['/', '\\']).next().unwrap_or_default();
    let name = match last.as_bytes() {
        [drive, b':', ..] if drive.is_ascii_alphabetic() => &last[2..],
        _ => last,
    };
    if matches!(name, "" | "." | "..") {
        Err(Cause::Malformed)
    } else {
        Ok(name.to_owned())
    }
}

impl CardEffect {
    /// The entry of a committed effect of `class` whose effect row keeps `kept`: exactly the
    /// kept fields of the class in the order of A-A9.3, each shown with the label
    /// `field.<class>.<name>`, its value as the row keeps it and the `from` that A-A9.3 admits,
    /// except the `items` of a `delete`, which a person cannot read; an `origin` shows the
    /// site's name and the `file` of a `local_edit` the name of the file alone (I2.6).
    /// `malformed` for a class with no effect row, for kept fields other than those, for an
    /// `origin` that is no origin and for a `file` with no name.
    pub fn committed(class: Class, kept: &[Kept]) -> Result<CardEffect, Cause> {
        let names = kept_names(class).ok_or(Cause::Malformed)?;
        let given: Vec<&str> = kept.iter().map(|k| k.name.as_str()).collect();
        if given != names {
            return Err(Cause::Malformed);
        }
        let say = |key: &str| words::text(key, &[]);
        let mut fields = Vec::new();
        for k in kept {
            let name = k.name.as_str();
            if class == Class::Delete && name == "items" {
                continue;
            }
            let from = field_from(kept_source(class, name).ok_or(Cause::Malformed)?);
            fields.push(Field {
                label: say(&format!("field.{}.{name}", class.name()))?,
                value: match (class, name) {
                    (_, "origin") => vec![site_name(&k.value)?],
                    (Class::LocalEdit, "file") => vec![file_name(&k.value)?],
                    _ => k.value.clone(),
                },
                from,
                from_text: say(&format!("from.{}", from.name()))?,
            });
        }
        let key = format!("run.effect.{}", class.name());
        Ok(CardEffect {
            class,
            text: say(&key)?,
            key,
            fields,
        })
    }

    /// The class of the effect.
    pub fn class(&self) -> Class {
        self.class
    }

    /// `key`: `run.effect.<class>`.
    pub fn key(&self) -> String {
        self.key.clone()
    }

    /// `text`: the sentence of `key`.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// `fields`: the kept fields that the card shows.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }
}

/// An item, as the page gets it (I3.4), made by the one constructor of its kind: a
/// confirmation has its presenter and an empty `text`, every other kind a null presenter and
/// `armed` `no`;
/// `fields` stand only in a confirmation, a check, a `link_account` item, which holds the one
/// field of its site, and a setup that asks again, which holds the one line that says so, and
/// `params` only in a setup.
///
/// ```
/// use halvo_coord::types::{AgentId, Item, ItemId, ItemKind, YesNo};
/// let ready = Item::ready(
///     ItemId::parse("00000000000000000000000000000003").unwrap(),
///     AgentId::parse("00000000000000000000000000000002").unwrap(),
///     String::new(),
///     String::new(),
///     vec![],
/// );
/// assert_eq!((ready.kind(), ready.presenter()), (ItemKind::Ready, None));
/// assert_eq!(ready.armed(), YesNo::No);
/// ```
///
/// A `ready` item with a presenter cannot be written:
///
/// ```compile_fail,E0451
/// use halvo_coord::types::{AgentId, Item, ItemId, ItemKind, Presenter, YesNo};
/// let ready = Item {
///     item: ItemId::parse("00000000000000000000000000000003").unwrap(),
///     agent: AgentId::parse("00000000000000000000000000000002").unwrap(),
///     kind: ItemKind::Ready,
///     presenter: Some(Presenter::Page),
///     armed: YesNo::No,
///     title: String::new(),
///     text: String::new(),
///     fields: vec![],
///     params: vec![],
///     choices: vec![],
/// };
/// ```
///
/// Nor can a confirmation without its presenter:
///
/// ```compile_fail,E0308
/// use halvo_coord::types::{AgentId, Item, ItemId, YesNo};
/// let confirm = Item::confirm(
///     ItemId::parse("00000000000000000000000000000003").unwrap(),
///     AgentId::parse("00000000000000000000000000000002").unwrap(),
///     None,
///     YesNo::Yes,
///     String::new(),
///     vec![],
///     vec![],
/// );
/// ```
///
/// Nor an armed item of another kind than `confirm`:
///
/// ```compile_fail,E0061
/// use halvo_coord::types::{AgentId, Item, ItemId, YesNo};
/// let ready = Item::ready(
///     ItemId::parse("00000000000000000000000000000003").unwrap(),
///     AgentId::parse("00000000000000000000000000000002").unwrap(),
///     YesNo::Yes,
///     String::new(),
///     String::new(),
///     vec![],
/// );
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    item: ItemId,
    agent: AgentId,
    kind: ItemKind,
    step: u32,
    presenter: Option<Presenter>,
    armed: YesNo,
    say_new: String,
    say_gone: String,
    title: String,
    text: String,
    fields: Vec<Field>,
    params: Vec<ParamEntry>,
    choices: Vec<ChoiceText>,
}

impl Item {
    /// The item with `say_new` and `say_gone`, the sentences of `say.item.new` and
    /// `say.item.gone` with `{name}`, the agent's name of I2.3, which the needs view adds to its
    /// status region when a reply brings the item and when a reply drops it (I3.4, I9.2).
    pub fn said_by(self, name: &str) -> Result<Item, Cause> {
        Ok(Item {
            say_new: words::text("say.item.new", &[("name", name)])?,
            say_gone: words::text("say.item.gone", &[("name", name)])?,
            ..self
        })
    }

    fn of_kind(
        kind: ItemKind,
        item: ItemId,
        agent: AgentId,
        armed: YesNo,
        title: String,
        text: String,
    ) -> Item {
        Item {
            item,
            agent,
            kind,
            step: 0,
            presenter: None,
            armed,
            say_new: String::new(),
            say_gone: String::new(),
            title,
            text,
            fields: vec![],
            params: vec![],
            choices: vec![],
        }
    }

    /// A `confirm` item: its presenter, its title, the bound fields of its effects and its
    /// choices (I4.3), with an empty `text`.
    pub fn confirm(
        item: ItemId,
        agent: AgentId,
        presenter: Presenter,
        armed: YesNo,
        title: String,
        fields: Vec<Field>,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            presenter: Some(presenter),
            fields,
            choices,
            ..Item::of_kind(ItemKind::Confirm, item, agent, armed, title, String::new())
        }
    }

    /// A `sign_in` item.
    pub fn sign_in(
        item: ItemId,
        agent: AgentId,
        title: String,
        text: String,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            choices,
            ..Item::of_kind(ItemKind::SignIn, item, agent, YesNo::No, title, text)
        }
    }

    /// A `link_account` item with the field of the site of its request's origin.
    pub fn link_account(
        item: ItemId,
        agent: AgentId,
        title: String,
        text: String,
        site: Field,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            fields: vec![site],
            choices,
            ..Item::of_kind(ItemKind::LinkAccount, item, agent, YesNo::No, title, text)
        }
    }

    /// A `stuck` item.
    pub fn stuck(
        item: ItemId,
        agent: AgentId,
        title: String,
        text: String,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            choices,
            ..Item::of_kind(ItemKind::Stuck, item, agent, YesNo::No, title, text)
        }
    }

    /// A `check` item with the kept fields of its effect.
    pub fn check(
        item: ItemId,
        agent: AgentId,
        title: String,
        text: String,
        fields: Vec<Field>,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            fields,
            choices,
            ..Item::of_kind(ItemKind::Check, item, agent, YesNo::No, title, text)
        }
    }

    /// A `setup` item with its parameters (I3.5), and with the line that says that its answers
    /// were not taken where it asks every parameter again.
    pub fn setup(
        item: ItemId,
        agent: AgentId,
        title: String,
        text: String,
        not_taken: Option<Field>,
        params: Vec<ParamEntry>,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            fields: not_taken.into_iter().collect(),
            params,
            choices,
            ..Item::of_kind(ItemKind::Setup, item, agent, YesNo::No, title, text)
        }
    }

    /// A `ready` item.
    pub fn ready(
        item: ItemId,
        agent: AgentId,
        title: String,
        text: String,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            choices,
            ..Item::of_kind(ItemKind::Ready, item, agent, YesNo::No, title, text)
        }
    }

    /// A `show` item.
    pub fn show(
        item: ItemId,
        agent: AgentId,
        title: String,
        text: String,
        choices: Vec<ChoiceText>,
    ) -> Item {
        Item {
            choices,
            ..Item::of_kind(ItemKind::Show, item, agent, YesNo::No, title, text)
        }
    }

    /// `item`.
    pub fn item(&self) -> &ItemId {
        &self.item
    }

    /// `agent`.
    pub fn agent(&self) -> &AgentId {
        &self.agent
    }

    /// `kind`.
    pub fn kind(&self) -> ItemKind {
        self.kind
    }

    /// `presenter`: none for every kind but `confirm`.
    pub fn presenter(&self) -> Option<Presenter> {
        self.presenter
    }

    /// `armed`.
    pub fn armed(&self) -> YesNo {
        self.armed
    }

    /// `say_new`: empty until `said_by` names the agent.
    pub fn say_new(&self) -> &str {
        &self.say_new
    }

    /// `say_gone`: empty until `said_by` names the agent.
    pub fn say_gone(&self) -> &str {
        &self.say_gone
    }

    /// `title`.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// `text`: empty for a confirmation.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// `fields`: the bound fields of a confirmation or the kept fields of a check.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// `params`: empty but for `setup`.
    pub fn params(&self) -> &[ParamEntry] {
        &self.params
    }

    /// `choices`: the safe one first.
    pub fn choices(&self) -> &[ChoiceText] {
        &self.choices
    }

    /// `step`: the item's step mark (I3.4, ruling RD-87 1), 0 at its first ask and one more at
    /// each move after which the open item asks again; an answer carries the mark of the step
    /// that the person saw.
    pub fn step(&self) -> u32 {
        self.step
    }

    /// The item at the step mark `step`.
    pub fn at_step(self, step: u32) -> Item {
        Item { step, ..self }
    }
}

/// A field of an item (I3.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Field {
    /// `label`.
    pub label: String,
    /// `value`: pieces of at most 1,024 bytes, as bound or as the person gave them.
    pub value: Vec<String>,
    /// `from`.
    pub from: FieldFrom,
    /// `from_text`: the text of `from.<source>`.
    pub from_text: String,
}

/// A parameter of a setup item (I3.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ParamEntry {
    /// `name`.
    pub name: Param,
    /// `label`: the text of `param.<name>`.
    pub label: String,
    /// `candidates`: at most `setup_candidates`.
    pub candidates: Vec<Candidate>,
    /// `typed`: `yes` only for a parameter with no candidate.
    pub typed: YesNo,
    /// `full`: for a parameter with `typed` `yes` the sentence of `param_typed.full`, which the
    /// card shows below the typed line while the line is at the bound, until a removal (RD-76
    /// 1.1 and 1.3 (A); I3.5); empty for every other parameter.
    pub full: String,
    /// `too_long`: for a parameter with `typed` `yes` the sentence of `param_typed.too_long`,
    /// which the card shows below the typed line after an input was refused below the bound
    /// (I3.5); empty for every other parameter.
    pub too_long: String,
}

/// A candidate of a parameter (I3.5).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Candidate {
    /// `n`: from 1.
    pub n: u32,
    /// `value`.
    pub value: Vec<String>,
    /// `from`.
    pub from: CandidateFrom,
}

/// A choice of an item with its text (I3.4).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChoiceText {
    /// `choice`.
    pub choice: Choice,
    /// `text`.
    pub text: String,
    /// `safe`.
    pub safe: YesNo,
}

/// The reply `{"ok":"yes"}` (I8.1), which holds nothing but `ok`.
///
/// ```
/// use halvo_coord::types::OkReply;
/// assert_eq!(serde_json::to_string(&OkReply).unwrap(), r#"{"ok":"yes"}"#);
/// ```
///
/// A reply `ok` that says `no` cannot be written:
///
/// ```compile_fail,E0560
/// use halvo_coord::types::{OkReply, YesNo};
/// let reply = OkReply { ok: YesNo::No };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OkReply;

impl Serialize for OkReply {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut reply = s.serialize_struct("OkReply", 1)?;
        reply.serialize_field("ok", &YesNo::Yes)?;
        reply.end()
    }
}

/// The reply `{"closed":"yes"}` of `agents_next` (I8.1, I8.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ClosedReply {
    /// `closed`.
    pub closed: YesNo,
}

/// The reply of a refused command (I8.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ErrorReply {
    /// `error`.
    pub error: Cause,
}
