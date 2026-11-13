use halvo_coord::types::{Cause, Class, Field};
use halvo_coord::words;

/// What the native dialog shows (I4.3, I4.9): a title, one line per bound field, and its two
/// controls, the rejecting one first. The title and the two controls come from the words file
/// by the classes of the request's effects, so a card cannot hold the allowing text on its
/// rejecting control, nor two controls of the same text.
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
    /// bounds, as the page joins them (I3.4); a control character, a line or paragraph separator
    /// and a mark that sets the direction of text in a field is a space, so that each field stays
    /// one line and reads in its own order. `malformed` for no class, for a class whose entries the
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
}

fn breaks(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
        )
}

fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if breaks(c) { ' ' } else { c })
        .collect()
}

fn line(field: &Field) -> String {
    let label = one_line(&field.label);
    if field.value.is_empty() {
        return label;
    }
    let value: String = field.value.iter().map(|piece| one_line(piece)).collect();
    format!("{label}: {value}")
}
