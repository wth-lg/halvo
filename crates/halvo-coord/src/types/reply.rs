use super::YesNo;
use super::{AgentId, CandidateFrom, Cause, Choice, Class, Control, FieldFrom, ItemId, ItemKind};
use super::{HistoryRead, Kept, LineKey, Origin, PageCause, Param, Presenter, Status, WorkflowId};
use crate::items::{field_from, kept_source};
use crate::ports::{Refused, Shared};
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
    /// `ask`: the question that a press on the row's Delete opens in the page, which sends
    /// nothing, or null on a row with no Delete (ruling RD-19 (c) rules (2) to (4)).
    pub ask: Option<Ask>,
    /// `ask_stop_sign_in`: the question that a press on the row's `stop_sign_in` opens in the
    /// page, which sends nothing, or null on a row with no such control (r4 U2.4y).
    pub ask_stop_sign_in: Option<Ask>,
    /// `ask_use_model`: the question that a press on the row's `use_model` opens in the page,
    /// which sends nothing, or null on a row with no such control or where a sentence stands in
    /// its place (r4 U3.17b).
    pub ask_use_model: Option<Ask>,
    /// `note_use_model`: the one sentence that a press on the row's `use_model` shows in place of
    /// its question, where no key is saved, no limit is set, or a state could not be read, or null
    /// (r4 U3.17b).
    pub note_use_model: Option<String>,
}

/// What the question of a row's Delete or of its `stop_sign_in` is (ruling RD-19 (c) rules (3)
/// and (4); r4 U2.4y).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AskKind {
    /// `delete`: no run of the agent is live, and the question asks for the delete.
    Delete,
    /// `working`: a run of the agent is live; the notice says that it must be stopped first.
    Working,
    /// `stopping`: the agent's run is stopping; the notice says that the delete comes after.
    Stopping,
    /// `stop_sign_in`: the question asks whether Halvo stops using the sign-in that it keeps for
    /// the agent.
    StopSignIn,
    /// `use_model`: the question asks whether the agent may use an AI model at the host that it
    /// names, with the models and the person's spending limit (r4 U3.17b).
    UseModel,
}

/// The `data-act` of a control of the question: `reject`, the safe control that leaves the
/// agent as it is; `allow`, the one control that sends `agents_delete`, or in the question
/// `stop_sign_in` the one that sends `agents_control` with `stop_sign_in`; `stop`, the control
/// of the notice `working` that sends the row's stop; `done`, the one control of the notice
/// `stopping`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AskAct {
    /// `reject`.
    Reject,
    /// `allow`.
    Allow,
    /// `stop`.
    Stop,
    /// `done`.
    Done,
}

/// A control of the question with its text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AskControl {
    /// `act`.
    pub act: AskAct,
    /// `text`.
    pub text: String,
}

/// An effect of the agent's last run whose outcome is not known, as the question shows it with
/// its kept fields above the controls (ruling RD-19 (c) rule (4), (d) (3)).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AskEffect {
    /// `text`.
    pub text: String,
    /// `fields`: its kept fields, as the card shows them.
    pub fields: Vec<Field>,
}

impl AskEffect {
    /// The effect of `class` whose kept fields are `kept`: the sentence `ask.effect.<class>`, or
    /// `ask.effect.other` for a class with no effect row, and each kept field as the card shows
    /// it, but the `items` of a `delete`, which a person cannot read.
    pub fn unknown(class: Class, kept: &[Kept]) -> Result<AskEffect, Cause> {
        let key = match kept_names(class) {
            Some(_) => format!("ask.effect.{}", class.name()),
            None => "ask.effect.other".to_owned(),
        };
        let mut fields = Vec::new();
        for k in kept {
            if class == Class::Delete && k.name.as_str() == "items" {
                continue;
            }
            fields.extend(kept_field(class, k)?);
        }
        Ok(AskEffect {
            text: words::text(&key, &[])?,
            fields,
        })
    }

    /// The effects of `unknown` under the open item `check` of the agent: one entry in that
    /// item's words, its title, with every kept field, and no sentence of the question's own
    /// (ruling RD-19 (d) (3)).
    pub fn checked(title: String, unknown: &[(Class, &[Kept])]) -> Result<AskEffect, Cause> {
        let mut fields = Vec::new();
        for (class, kept) in unknown {
            for k in kept.iter() {
                if *class == Class::Delete && k.name.as_str() == "items" {
                    continue;
                }
                fields.extend(kept_field(*class, k)?);
            }
        }
        Ok(AskEffect {
            text: title,
            fields,
        })
    }
}

/// The question of a row's Delete (I2.5; ruling RD-19 (c)) or of its `stop_sign_in` (r4 U2.4y):
/// its kind, its title, its sentences in order, the effects of unknown outcome with the sentence
/// on their record, and its controls, the safe one first.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Ask {
    /// `kind`.
    pub kind: AskKind,
    /// `title`.
    pub title: String,
    /// `text`: its sentences, each a whole entry of the words table.
    pub text: Vec<String>,
    /// `effects`: empty but for `delete`.
    pub effects: Vec<AskEffect>,
    /// `record`: the sentence that Halvo keeps no record of those effects after the delete, or
    /// null where `effects` is empty.
    pub record: Option<String>,
    /// `controls`: `reject` and `allow` for `delete` and for `stop_sign_in`, `reject` and `stop`
    /// for `working`, and `done` alone for `stopping`.
    pub controls: Vec<AskControl>,
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
    /// `history` and `unread`, where the card's read of `Owner::agent_history` answered busy:
    /// the card then holds the line of the count of records taken off alone, where that count
    /// is at least 1, and no run. Both keys are left out where the read answered (the
    /// coordinator's word on R6 of U2.4t).
    #[serde(flatten)]
    pub history: Option<BusyHistory>,
}

/// The two keys of a card whose history read answered busy (I2.6): `history`, which says
/// `busy` and nothing else, and on which alone the page asks the card again and, once it asks
/// no more, shows `unread` (RD-148 R6, way C).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BusyHistory {
    /// `history`: `busy`.
    pub history: HistoryRead,
    /// `unread`.
    pub unread: Unread,
}

/// What the card of a busy history read shows once the page asks no more.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Unread {
    /// `text`: the sentence of `card.history_busy`.
    pub text: String,
    /// `again`: the text of the card's control that reads it again, `card.read_again`.
    pub again: String,
}

impl BusyHistory {
    /// The keys of a busy history read, with their words.
    pub fn new() -> Result<BusyHistory, Cause> {
        Ok(BusyHistory {
            history: HistoryRead::Busy,
            unread: Unread {
                text: words::text("card.history_busy", &[])?,
                again: words::text("card.read_again", &[])?,
            },
        })
    }
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
///         r#"{"day":"24 October 2026","text":"It finished the task.","effects":[{"key":"run.effect.payment","text":"It sent this payment, and the site's page afterwards showed the amount you allowed.","fields":["#,
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
///     (Class::Payment, &["payee", "amount", "instrument", "origin"][..], "It sent this payment, and the site's page afterwards showed the amount you allowed.", 4),
///     (Class::Purchase, &["seller", "amount", "instrument", "origin"][..], "It sent this order, and the site's page afterwards showed the total you allowed.", 4),
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
    /// `effects`: one entry for each committed effect of the run, for each effect whose last row
    /// is `rejected` with a null cause, and for each effect whose last state is `unknown`, in the
    /// order of the reply of `agent_history`.
    pub effects: Vec<CardEffect>,
}

/// An effect of a past run on a card (I2.6, rulings H-8, RD-52 (f) and (h) 4, RD-85, RD-126 item
/// 10): a sentence with no placeholder and kept fields of its class as fields of I3.4, made only by
/// [`CardEffect::committed`], [`CardEffect::unshown`], [`CardEffect::not_done`] and
/// [`CardEffect::unknown`].
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

/// Whether a value shows nothing that a person can see (ruling RD-161 R6, widened by the
/// coordinator): no piece, or pieces of white space, control characters (general category Cc)
/// and the zero-width signs U+200B, U+200C, U+200D, U+2060 and U+FEFF alone.
fn shows_nothing(value: &[String]) -> bool {
    value.iter().flat_map(|piece| piece.chars()).all(|c| {
        c.is_whitespace()
            || c.is_control()
            || matches!(
                c,
                '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}'
            )
    })
}

/// The kept field `k` of an effect of `class` as a field of I3.4: the label
/// `field.<class>.<name>`, the value as the row keeps it, an `origin` as the site's name and the
/// `file` of a `local_edit` as the name of the file alone, and the `from` that A-A9.3 admits;
/// none where the kept value or its shown part shows nothing (`shows_nothing`), which is no
/// value; `malformed` where a piece of the shown value would hold more than 1,024 bytes (I3.4).
fn kept_field(class: Class, k: &Kept) -> Result<Option<Field>, Cause> {
    kept_field_as(class, k, k.name.as_str())
}

/// The kept field `k` as `kept_field` shows it, under the label `field.<class>.<label>`.
fn kept_field_as(class: Class, k: &Kept, label: &str) -> Result<Option<Field>, Cause> {
    let name = k.name.as_str();
    let from = field_from(kept_source(class, name).ok_or(Cause::Malformed)?);
    if shows_nothing(&k.value) {
        return Ok(None);
    }
    let value = match (class, name) {
        (_, "origin") => vec![site_name(&k.value)?],
        (Class::LocalEdit, "file") => vec![file_name(&k.value)?],
        _ => k.value.clone(),
    };
    if shows_nothing(&value) {
        return Ok(None);
    }
    if value.iter().any(|piece| piece.len() > 1024) {
        return Err(Cause::Malformed);
    }
    Ok(Some(Field {
        label: words::text(&format!("field.{}.{label}", class.name()), &[])?,
        value,
        from,
        from_text: words::text(&format!("from.{}", from.name()), &[])?,
    }))
}

/// The one kept field of the name `name` where the row keeps that name exactly once; none where
/// it keeps it twice or more, as no entry shows such a name (ruling RD-161 R5).
fn once<'a>(kept: &'a [Kept], name: &str) -> Option<&'a Kept> {
    let mut named = kept.iter().filter(|k| k.name.as_str() == name);
    match (named.next(), named.next()) {
        (Some(k), None) => Some(k),
        _ => None,
    }
}

/// The one rule for which kept fields stand on an entry that the reader cannot fully show (I2.6,
/// ruling RD-52 (h) 4), and on the item `check` (ruling RD-158 (5)): a field of the table of
/// A-A9.3 for `class` stands where the row keeps its name exactly once (`once`) and `kept_field`
/// takes its value whole, shown as `committed` shows it and in the table's order; never the
/// `items` of a `delete`, a name that the table does not hold for the class, a name kept twice, a
/// value that shows nothing (`shows_nothing`; rulings RD-158 (3), RD-161 R6), or any field of a
/// class with no effect row.
fn standing(class: Class, kept: &[Kept]) -> Vec<Field> {
    kept_names(class)
        .unwrap_or_default()
        .iter()
        .filter(|name| !(class == Class::Delete && **name == "items"))
        .filter_map(|name| once(kept, name))
        .filter_map(|k| kept_field(class, k).ok().flatten())
        .collect()
}

impl CardEffect {
    /// The entry of a committed effect of `class` whose effect row keeps `kept`: exactly the
    /// kept fields of the class in the order of A-A9.3, each shown with the label
    /// `field.<class>.<name>`, its value as the row keeps it and the `from` that A-A9.3 admits,
    /// except the `items` of a `delete`, which a person cannot read, and a value that shows
    /// nothing, which is no value (ruling RD-161 R6); an `origin` shows the site's name and the
    /// `file` of a `local_edit` the name of the file alone (I2.6).
    /// `malformed` for a class with no effect row, for kept fields other than those, for an
    /// `origin` that is no origin and for a `file` with no name.
    pub fn committed(class: Class, kept: &[Kept]) -> Result<CardEffect, Cause> {
        let names = kept_names(class).ok_or(Cause::Malformed)?;
        let given: Vec<&str> = kept.iter().map(|k| k.name.as_str()).collect();
        if given != names {
            return Err(Cause::Malformed);
        }
        let mut fields = Vec::new();
        for k in kept {
            if class == Class::Delete && k.name.as_str() == "items" {
                continue;
            }
            fields.extend(kept_field(class, k)?);
        }
        let key = format!("run.effect.{}", class.name());
        Ok(CardEffect {
            class,
            text: words::text(&key, &[])?,
            key,
            fields,
        })
    }

    /// The entry of a committed effect of `class` on which the agents run's read names what the
    /// site's page afterwards gave, `page` (r4 U3.29, rows 1, 2, 11 and 12 of the ruled words of
    /// the coordinator's log lines 20376, 20377 and 20492): for a `payment` or a `purchase` whose
    /// page was read and matched, the entry of `committed`, whose sentence says that the page
    /// showed the allowed amount; for one with another cause, or with none, which reads as a page
    /// that could not be read (way (a) of the coordinator's log line 20557),
    /// `run.effect.<class>.<cause>` with the kept fields as `committed` shows them, but the amount
    /// under `field.<class>.amount_allowed` and, beside `amount_differs`, the page's amount right
    /// after it under `field.<class>.page_amount`, written as digits, a point and two decimals
    /// with no currency sign, and left out where the read holds none; for another class, the entry
    /// of `committed`. `malformed` where `committed` refuses the kept fields.
    pub fn sent(class: Class, kept: &[Kept], page: Option<PageCause>) -> Result<CardEffect, Cause> {
        let plain = CardEffect::committed(class, kept)?;
        if !matches!(class, Class::Payment | Class::Purchase) {
            return Ok(plain);
        }
        let page = page.unwrap_or(PageCause::PageUnread);
        let case = match page {
            PageCause::AmountMatched => return Ok(plain),
            PageCause::AmountDiffers { .. } => "amount_differs",
            PageCause::AmountUnread => "amount_unread",
            PageCause::PageUnread => "page_unread",
        };
        let mut fields = Vec::new();
        for k in kept {
            if k.name.as_str() != "amount" {
                fields.extend(kept_field(class, k)?);
                continue;
            }
            fields.extend(kept_field_as(class, k, "amount_allowed")?);
            if let PageCause::AmountDiffers {
                amount: Some(minor),
            } = page
            {
                fields.push(Field {
                    label: words::text(&format!("field.{}.page_amount", class.name()), &[])?,
                    value: vec![format!("{}.{:02}", minor / 100, minor % 100)],
                    from: FieldFrom::Site,
                    from_text: words::text("from.site", &[])?,
                });
            }
        }
        let key = format!("run.effect.{}.{case}", class.name());
        Ok(CardEffect {
            class,
            text: words::text(&key, &[])?,
            key,
            fields,
        })
    }

    /// The entry of a committed effect that the card's reader refuses (I2.6, rulings RD-52 (f)
    /// and (h) 4): the kept fields of `kept` that the one rule lets stand, and
    /// `run.effect.<class>.partial` where one stands; with none, `run.effect.<class>.unshown`
    /// for a class with an effect row and `run.effect.unshown` for any other. No byte of a kept
    /// value that does not stand reaches the reply.
    pub fn unshown(class: Class, kept: &[Kept]) -> Result<CardEffect, Cause> {
        let fields = standing(class, kept);
        let key = match kept_names(class) {
            Some(_) if !fields.is_empty() => format!("run.effect.{}.partial", class.name()),
            Some(_) => format!("run.effect.{}.unshown", class.name()),
            None => "run.effect.unshown".to_owned(),
        };
        Ok(CardEffect {
            class,
            text: words::text(&key, &[])?,
            key,
            fields,
        })
    }

    /// The entry of an effect whose last state is `unknown` (I2.6, ruling RD-126 item 10), which
    /// says that Halvo could not tell whether it went through: `run.effect.<class>.unknown` for
    /// a class with an effect row and `run.effect.unknown` for any other, with the kept fields of
    /// `kept` that the one rule lets stand.
    pub fn unknown(class: Class, kept: &[Kept]) -> Result<CardEffect, Cause> {
        let key = match kept_names(class) {
            Some(_) => format!("run.effect.{}.unknown", class.name()),
            None => "run.effect.unknown".to_owned(),
        };
        Ok(CardEffect {
            class,
            text: words::text(&key, &[])?,
            key,
            fields: standing(class, kept),
        })
    }

    /// The entry of an effect whose last row is `rejected` with a null cause (I2.6, ruling RD-85
    /// items 1, 5 and 6): `run.effect.<class>.refused` where `refused`, the witness of a no that
    /// the person pressed, and `run.effect.<class>.not_done` otherwise, or `run.effect.refused`
    /// and `run.effect.not_done` for a class with no effect row; with the first kept field of
    /// its class as `committed` shows it, and with no field for a `delete` or where that field
    /// is not kept, is kept twice or more (ruling RD-161 R5), shows nothing or does not read.
    pub fn not_done(class: Class, kept: &[Kept], refused: bool) -> Result<CardEffect, Cause> {
        let end = if refused { "refused" } else { "not_done" };
        let names = kept_names(class);
        let key = match names {
            Some(_) => format!("run.effect.{}.{end}", class.name()),
            None => format!("run.effect.{end}"),
        };
        let first = names
            .and_then(|n| n.first())
            .filter(|_| class != Class::Delete)
            .and_then(|first| once(kept, first))
            .and_then(|k| kept_field(class, k).ok().flatten());
        Ok(CardEffect {
            class,
            text: words::text(&key, &[])?,
            key,
            fields: first.into_iter().collect(),
        })
    }

    /// The class of the effect.
    pub fn class(&self) -> Class {
        self.class
    }

    /// `key`: of the family `run.effect.<class>` or its `unshown`, `partial`, `unknown`,
    /// `not_done` or `refused` entry, or the plain `run.effect.unshown`, `run.effect.unknown`,
    /// `run.effect.not_done` or `run.effect.refused` of a class with no effect row.
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
/// `fields` stand only in a confirmation, a check, an `amounts` item, a `link_account` item, which
/// holds the one field of its site, and a setup that asks again, which holds the one line that
/// says so, and
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
    /// A confirmation's `text` names the agent that asks, the sentence of `confirm.asked_by`
    /// with `{name}` (I3.4).
    pub fn said_by(self, name: &str) -> Result<Item, Cause> {
        let text = if self.kind == ItemKind::Confirm {
            words::text("confirm.asked_by", &[("name", name)])?
        } else {
            self.text
        };
        Ok(Item {
            say_new: words::text("say.item.new", &[("name", name)])?,
            say_gone: words::text("say.item.gone", &[("name", name)])?,
            text,
            ..self
        })
    }

    /// A page confirmation, where the reply holds `count` of them, with the `text` of
    /// `confirm.asked_by_place`: the agent's name and its place `n` among them in the reply's
    /// order (I3.4, ruling RD-17); any other item as it is.
    pub fn placed(self, name: &str, n: usize, count: usize) -> Result<Item, Cause> {
        if self.kind != ItemKind::Confirm || self.presenter != Some(Presenter::Page) {
            return Ok(self);
        }
        let (n, count) = (n.to_string(), count.to_string());
        Ok(Item {
            text: words::text(
                "confirm.asked_by_place",
                &[("name", name), ("n", &n), ("count", &count)],
            )?,
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

    /// An `amounts` item with the fields of its payment or order (r4 U3.29).
    pub fn amounts(
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
            ..Item::of_kind(ItemKind::Amounts, item, agent, YesNo::No, title, text)
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

    /// The item with `refused` as the key `refused` of its parameter `param` (I3.5, I8.1;
    /// ruling RD-81 9.3); the item as it was where it asks no such parameter.
    pub fn refused_at(mut self, param: Param, refused: Refused) -> Item {
        for p in self.params.iter_mut().filter(|p| p.name == param) {
            p.refused = Some(refused);
        }
        self
    }

    /// The item with `sentence` as the key `not_taken` of its parameter `param` (r4 U3.22a); the
    /// item as it was where it asks no such parameter.
    pub fn not_taken_at(mut self, param: Param, sentence: String) -> Item {
        for p in self.params.iter_mut().filter(|p| p.name == param) {
            p.not_taken = Some(sentence.clone());
        }
        self
    }

    /// The item with `line` as the key `stored` of its parameter `param` (r4 U3.22g); the item as
    /// it was where it asks no such parameter.
    pub fn stored_at(mut self, param: Param, line: &str) -> Item {
        for p in self.params.iter_mut().filter(|p| p.name == param) {
            p.stored = Some(line.to_owned());
        }
        self
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

/// A parameter of a setup item (I3.5). Its `Debug` form prints every field as a derived form
/// would, but of `stored` the length in bytes alone, never its text (r4 U3.22g, the coordinator's
/// log line 19851), so that no log line of an item can carry a person's stored path.
#[derive(Clone, PartialEq, Eq, Serialize)]
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
    /// `hint`: what to type, the sentence of `param_hint.<name>` of the parameter, with the
    /// `_mac` or `_windows` form of this build's system where the answer is a path, which the card
    /// shows under the question's words and above its line (r4 U3.22a); empty for every parameter
    /// that the words table gives no hint, and for every parameter with candidates, Which sheet
    /// while it offers a workbook's names among them (r4 U3.22c).
    pub hint: String,
    /// `needs_answer`: for a parameter with `typed` `yes` the sentence of
    /// `param_typed.needs_answer`, which the card shows below the typed line where it stands
    /// empty at a press of the choice `choose`, until the line changes (r4 U3.22b); the same
    /// sentence for a parameter whose candidates come from `files`, the names of a workbook's
    /// worksheets, which the card shows below the list where no entry is chosen at such a press
    /// (r4 U3.22c); empty for every other parameter.
    pub needs_answer: String,
    /// `choose`: for Which spreadsheet file and Where the data comes from, the label of the
    /// control beside the typed line that opens the system's own chooser of a file, the sentence
    /// of `param_typed.choose` (r4 U3.22b); the key is left out for every other parameter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub choose: Option<String>,
    /// `refused`: the refusal by entry that the reader of senders gave the newest answer for the
    /// parameter, the entry's number and kind and no sentence, which is the card's (I3.5, I8.1;
    /// ruling RD-81 9.1 to 9.3); the key is left out while none stands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refused: Option<Refused>,
    /// `not_taken`: the sentence that says why the core did not take the newest typed line of the
    /// parameter, which the card shows below the typed line until the next answer for it (r4
    /// U3.22a); the key is left out while none stands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_taken: Option<String>,
    /// `stored`: the line of the answer that the agent uses now, which the item holds for the
    /// parameter since the press of Change its answers put it up (r4 U3.22g), and which the card
    /// shows in the parameter's typed line as a line that the core holds; the key is left out
    /// while none stands.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stored: Option<String>,
}

impl std::fmt::Debug for ParamEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ParamEntry")
            .field("name", &self.name)
            .field("label", &self.label)
            .field("candidates", &self.candidates)
            .field("typed", &self.typed)
            .field("full", &self.full)
            .field("too_long", &self.too_long)
            .field("hint", &self.hint)
            .field("needs_answer", &self.needs_answer)
            .field("choose", &self.choose)
            .field("refused", &self.refused)
            .field("not_taken", &self.not_taken)
            .field("stored", &self.stored.as_ref().map(String::len))
            .finish()
    }
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

/// The reply `{"path":"<full path>"}` of `agents_choose_file` (I8.1; r4 U3.22b): the full path
/// of the file that the person picked in the system's own chooser, which the card puts in the
/// question's typed line. A closed chooser answers `ClosedReply`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChosenReply {
    /// `path`.
    pub path: String,
}

/// The reply of a refused command (I8.1): `error`, its cause, and `text`, the sentence of the
/// words key `refused.<cause>`, which the page shows beside the control that was pressed
/// (ruling RD-161 R1). The sentence is composed when the reply is written; nothing is stored
/// for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrorReply {
    /// `error`.
    pub error: Cause,
}

impl ErrorReply {
    /// `text`: the sentence of `refused.<cause>`.
    pub fn text(&self) -> Result<String, Cause> {
        words::text(&format!("refused.{}", self.error.name()), &[])
    }
}

impl Serialize for ErrorReply {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Refusal::Cause(self.error).serialize(s)
    }
}

/// What the sentence of a refusal `overlap` says (I3.5 (c), (f)), from what the core holds of the
/// read that followed the newest refusal: the other agent by the name of its row, with the
/// senders that both agents would read, where the read named an agent that the list shows; and
/// nobody where the read did not answer or named no agent that the list shows. The sentence is
/// composed when the item or the reply is written; nothing is stored for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Overlapping {
    /// The read named this agent of the list with these senders.
    Named {
        /// The name of the other agent's row (I2.3).
        name: String,
        /// The senders that both agents would read, in the spelling of the asking agent's list.
        senders: Vec<String>,
        /// Whether further agents share senders: the read named more than one agent, or says
        /// that more agents share senders than it names.
        more: bool,
    },
    /// The read did not answer, or named no agent that the list shows: the sentence names
    /// nobody and says only what the refusal proves.
    Unread,
}

impl Overlapping {
    /// What `shared` gives to say, with the name of an agent's row from `name_of`: the first
    /// entry of the read whose agent has a row; `Unread` where none has one or where the read did
    /// not answer.
    pub fn of(shared: &Shared, name_of: impl Fn(&AgentId) -> Option<String>) -> Overlapping {
        let Shared::Named(read) = shared else {
            return Overlapping::Unread;
        };
        read.entries
            .iter()
            .find_map(|e| name_of(&e.agent).map(|name| (name, e)))
            .map_or(Overlapping::Unread, |(name, e)| Overlapping::Named {
                name,
                senders: e.senders.clone(),
                more: read.more || read.entries.len() > 1,
            })
    }

    /// The line of a set-up item, in the place of `item.setup.not_taken`: the sentence of
    /// `item.setup.overlap`, of `item.setup.overlap_more` where further agents share senders, or
    /// of `item.setup.overlap_unread`.
    pub fn setup_line(&self) -> Result<String, Cause> {
        self.say([
            "item.setup.overlap",
            "item.setup.overlap_more",
            "item.setup.overlap_unread",
        ])
    }

    /// The sentence beside a Switch on that the owner refused with `overlap` (I3.5 (f)): the
    /// sentence of `refused.overlap_named`, of `refused.overlap_more` where further agents share
    /// senders, or of `refused.overlap`, the sentence of the cause, which names nobody.
    pub fn refused(&self) -> Result<String, Cause> {
        self.say([
            "refused.overlap_named",
            "refused.overlap_more",
            "refused.overlap",
        ])
    }

    /// The sentence of the first key of `keys` for one named agent, of the second where further
    /// agents share senders, with `{name}` and `{senders}`, the senders joined by a comma; of
    /// the third for nobody.
    fn say(&self, [one, more, nobody]: [&str; 3]) -> Result<String, Cause> {
        match self {
            Overlapping::Named {
                name,
                senders,
                more: further,
            } => words::text(
                if *further { more } else { one },
                &[("name", name), ("senders", &senders.join(", "))],
            ),
            Overlapping::Unread => words::text(nobody, &[]),
        }
    }
}

/// A refused control of a row (I8.1 `agents_control`) as its reply writes it: `error`, the
/// cause, and `text`, the sentence that the page shows beside the control (RD-161 R1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Any refusal: the sentence of `refused.<cause>`.
    Cause(Cause),
    /// A Switch off that stopped the agent's live run while the owner refused to store its hold
    /// under class B, with the cause of that refusal: the sentence of `refused.switch_not_saved`,
    /// which says both halves, that the task is stopped and that the agent is still on
    /// (RD-155 (1); term (5) of RD-161 R1).
    SwitchNotSaved(Cause),
    /// A `stop_sign_in` whose body `drop_sessions` the owner refused with `store`, a fault of the
    /// store behind which the agent's kept sign-ins may be gone from every read or may all stand,
    /// with the cause of that refusal: the sentence of `refused.sign_in_not_finished`, which says
    /// that Halvo could not finish this and names neither (r4 U3.16).
    SignInNotFinished(Cause),
    /// A `use_model` whose body, the yes `agree_hosted`, the owner refused or that got no reply,
    /// with the cause of that answer: the sentence of `refused.use_model_not_finished`, which says
    /// that Halvo could not finish this and to choose Use an AI model again where the row still
    /// shows it (r4 U3.17b).
    UseModelNotFinished(Cause),
    /// A `this_computer_only` whose body, the way back `withdraw_hosted`, the owner refused or
    /// that got no reply, with the cause of that answer: the sentence of
    /// `refused.this_computer_not_finished`, which says that Halvo could not finish this and to
    /// choose Use this computer only again where the row still shows it (r4 U3.17b).
    ThisComputerNotFinished(Cause),
    /// A Switch on whose state the owner refused with `overlap`, the cause `overlap`, with what
    /// the read that followed gave to say: the sentence of `Overlapping::refused`, which names
    /// the other agent and the shared senders where the read named them, and is the sentence of
    /// `refused.overlap` where it did not answer (ruling RD-161 R4, WAY A; I3.5 (f)).
    Overlap(Overlapping),
}

impl Refusal {
    /// The cause.
    pub fn cause(&self) -> Cause {
        match self {
            Refusal::Cause(cause)
            | Refusal::SwitchNotSaved(cause)
            | Refusal::SignInNotFinished(cause)
            | Refusal::UseModelNotFinished(cause)
            | Refusal::ThisComputerNotFinished(cause) => *cause,
            Refusal::Overlap(_) => Cause::Overlap,
        }
    }

    /// `text`: the sentence of its words key.
    pub fn text(&self) -> Result<String, Cause> {
        match self {
            Refusal::Cause(cause) => ErrorReply { error: *cause }.text(),
            Refusal::SwitchNotSaved(_) => words::text("refused.switch_not_saved", &[]),
            Refusal::SignInNotFinished(_) => words::text("refused.sign_in_not_finished", &[]),
            Refusal::UseModelNotFinished(_) => words::text("refused.use_model_not_finished", &[]),
            Refusal::ThisComputerNotFinished(_) => {
                words::text("refused.this_computer_not_finished", &[])
            }
            Refusal::Overlap(say) => say.refused(),
        }
    }
}

impl Serialize for Refusal {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{Error, SerializeStruct};
        let text = self
            .text()
            .map_err(|_| S::Error::custom("a refusal with no sentence"))?;
        let mut reply = s.serialize_struct("ErrorReply", 2)?;
        reply.serialize_field("error", &self.cause())?;
        reply.serialize_field("text", &text)?;
        reply.end()
    }
}
