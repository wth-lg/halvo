use halvo_coord::items::chars::columns;
use halvo_coord::items::{NativeCard, CARD_COLUMNS, CARD_LINES};
use halvo_coord::types::{Cause, Class, Field};
use halvo_coord::words;

/// What the native dialog shows (I4.3, I4.9): a title, its lines, and its two controls, the
/// rejecting one first. The product's card is the core's (`Card::native`); `Card::new` composes
/// one from the words file by the classes of the request's effects. Neither holds two controls
/// of the same text. A card keeps the composed form when its
/// title, each of its lines and each control text holds no character of the closed list of
/// `breaks` (the control characters, the line and paragraph separators and the marks that set the
/// direction of text), when it has at most `LINES` lines, when its title, each line and each
/// control text takes at most `LINE_COLUMNS` columns of display width, and when no control text
/// holds the mark of an
/// access key of the system that shows it (`&` on Windows, none on macOS); the presenter refuses
/// to show a card that breaks that form, and nothing mends it (ruling RD-90 item 10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    title: String,
    lines: Vec<String>,
    reject: String,
    allow: String,
}

impl Card {
    /// The card of a request whose effects have `classes`, in order, and whose bound fields
    /// are `fields`, as the item of the request holds them: the title `confirm.<class>.title`
    /// and the controls `confirm.<class>.reject` and `confirm.<class>.allow`, the `_many`
    /// entries with `{count}` for more than one effect; for effects of more than one class
    /// `confirm.mixed.title`, `confirm.mixed.reject` and `confirm.mixed.allow` with `{count}`.
    /// A field is the line `<label>: <value>`, or its label alone when it has no value; the value
    /// is its pieces joined with nothing, because they are the cut of one value at character
    /// bounds, as the page joins them (I3.4); the label and the value are taken as they are, so a
    /// field that holds a character of `breaks` gives a card that breaks the composed form, which
    /// the presenter refuses to show. `malformed` for no class, for a class whose entries the
    /// words file does not hold (its action line in a request of more than one class), and for two
    /// controls of one text.
    pub fn new(classes: &[Class], fields: &[Field]) -> Result<Card, Cause> {
        let first = *classes.first().ok_or(Cause::Malformed)?;
        let count = classes.len().to_string();
        let count = [("count", count.as_str())];
        let (title, reject, allow) = if classes.iter().all(|c| *c == first) {
            let class = first.name();
            let title = words::text(&format!("confirm.{class}.title"), &[])?;
            if classes.len() == 1 {
                (
                    title,
                    words::text(&format!("confirm.{class}.reject"), &[])?,
                    words::text(&format!("confirm.{class}.allow"), &[])?,
                )
            } else {
                (
                    title,
                    words::text(&format!("confirm.{class}.reject_many"), &count)?,
                    words::text(&format!("confirm.{class}.allow_many"), &count)?,
                )
            }
        } else {
            for class in classes {
                words::text(&format!("confirm.{}.action", class.name()), &[])?;
            }
            (
                words::text("confirm.mixed.title", &count)?,
                words::text("confirm.mixed.reject", &count)?,
                words::text("confirm.mixed.allow", &count)?,
            )
        };
        if reject == allow {
            return Err(Cause::Malformed);
        }
        Ok(Card {
            title,
            lines: fields.iter().map(line).collect(),
            reject,
            allow,
        })
    }

    /// The card of the core's native dialog (ruling RD-77 part 12): its title, its lines and the
    /// texts of its two controls, the rejecting one first, as `halvo_coord` composes them and as
    /// they are, so that the dialog shows the core's card and nothing else. `malformed` for two
    /// controls of one text.
    pub fn native(card: &NativeCard) -> Result<Card, Cause> {
        let (reject, allow) = (&card.reject().text, &card.allow().text);
        if reject == allow {
            return Err(Cause::Malformed);
        }
        Ok(Card {
            title: card.title().to_owned(),
            lines: card.lines().to_vec(),
            reject: reject.clone(),
            allow: allow.clone(),
        })
    }

    /// The title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The lines, one per field, in the item's order.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The lines joined by line feeds: the dialog's message.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// The text of the rejecting control.
    pub fn reject(&self) -> &str {
        &self.reject
    }

    /// The text of the allowing control.
    pub fn allow(&self) -> &str {
        &self.allow
    }

    /// Whether the card keeps the composed form on the system that this build is for.
    pub(crate) fn whole(&self) -> bool {
        self.keeps(MARKS)
    }

    /// Whether the card keeps the composed form on a system whose marks of an access key are
    /// `marks`.
    pub(crate) fn keeps(&self, marks: &[char]) -> bool {
        let short = |text: &str| columns(text) <= LINE_COLUMNS;
        let plain = |text: &str| !text.chars().any(breaks);
        self.lines.len() <= LINES
            && [&self.title, &self.reject, &self.allow]
                .into_iter()
                .chain(&self.lines)
                .all(|text| short(text) && plain(text))
            && [&self.reject, &self.allow]
                .into_iter()
                .all(|text| !text.contains(marks))
    }

    #[cfg(test)]
    pub(crate) fn of(title: &str, lines: &[&str], reject: &str, allow: &str) -> Card {
        Card {
            title: title.into(),
            lines: lines.iter().map(|line| line.to_string()).collect(),
            reject: reject.into(),
            allow: allow.into(),
        }
    }
}

/// The most lines that a card holds, the empty line between two fields counted: the dialog's
/// bound proven on a hosted screen of both systems, `halvo_coord::items::CARD_LINES` (ruling
/// RD-90 items 7 and 9).
pub const LINES: usize = CARD_LINES;

/// The most columns that a card's title, one of its lines or one of its control texts takes,
/// counted as display width by `halvo_coord::items::chars::columns` and not in characters, so
/// that a wide character takes two and a combining mark none: the dialog's bound proven on a
/// hosted screen of both systems, `halvo_coord::items::CARD_COLUMNS` (ruling RD-90 items 7 and 9).
pub const LINE_COLUMNS: usize = CARD_COLUMNS;

/// The marks of an access key in a control text of the task dialog of Windows.
#[cfg(any(test, windows))]
pub const WINDOWS_MARKS: &[char] = &['&'];

#[cfg(windows)]
const MARKS: &[char] = WINDOWS_MARKS;
#[cfg(not(windows))]
const MARKS: &[char] = &[];

/// The closed list of characters that break a line of a card: the control characters, the line
/// and paragraph separators and the marks that set the direction of text.
pub fn breaks(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
}

fn line(field: &Field) -> String {
    if field.value.is_empty() {
        return field.label.clone();
    }
    let value: String = field.value.concat();
    format!("{}: {value}", field.label)
}
