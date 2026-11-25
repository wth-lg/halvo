//! The snapshot of a model request (r3 U7.2b; contracts A12.7 and A9.5; the coordinator's log line 22505): a type
//! that the mask alone makes, so that no caller of `ask` can hand the door a page's text that the mask has not read.
//!
//! THE MASK. No text of a slot of the run's load leaves in a model request, as a box's value or as plain text of a
//! later page: on every line of the snapshot, in its name, its value and its frame label, the mark `[slot NAME]` stands
//! in the place of each occurrence of the text of a slot of the load, NAME the slot's own name. The ref and the role
//! of a line are never changed. The mask reads each field IN THE FORM IN WHICH IT LEAVES: a name and a value after
//! the render's one change of their text (`cdp::quote`, a double quote as two single quotes), a frame label as it is;
//! and a slot's text THROUGH THE SNAPSHOT'S OWN THREE CLEANINGS, by the same functions of halvo-web: beside the text
//! and each of its lines as they are, the squeeze of each (`cdp::squeeze`, what a line's name can hold), each with
//! CR, LF and TAB as spaces (`cdp::line_value` for a text box, what a line's value can hold), and each of these after
//! `cdp::quote` (the verifier-153's F3). A line is written only where its render holds each field as the mask left
//! it, so a step that a later render would add behind the mask leaves that line out of the snapshot and puts no
//! slot's text in it. ASCII letters are compared with no regard to case. One pass from the
//! left, the longest text that stands at a place wins, so a mark is never read again; two slots of one text are
//! named by the first in the load's order; a slot whose text is empty is skipped, and the slot `start` is a slot like
//! any other. BEFORE the mask reads a field, the two marker strings of A9.5 leave it (`cleaned`), and the mask reads
//! each slot's text through the same taking-out, so no later step joins a slot's text again (pr-review-90's F1).
//!
//! THE SECOND MAKER, for a message that the agent answers (the coordinator's log line 22541): the trial's form of a
//! message, `From:`, `Subject:` and the body, with the From line `From: [slot reply_senders]` always and never the
//! address, whatever the count of listed senders, and the subject and the body masked as a line's text is. A text that
//! the model writes and that holds the mark's head `[slot ` (ASCII letters with no regard to case) is no body
//! (`holds_a_mark`), so that no card and no mail shows a mark.
//!
//! WHAT STILL PASSES (limits of the promise, named for the contract): another spelling of a number or a date, a part
//! of a slot's text, and a letter outside ASCII in another case.

use halvo_schema::executable::record::Slot;
use halvo_web::cdp::{line_value, quote, squeeze, SnapshotLine};

/// The two marker strings of A9.5, which the door puts around the snapshot and which the snapshot never holds.
const MARKERS: [&str; 2] = ["<<<DATA", "DATA>>>"];

/// The From line of every message's snapshot: the mark of the slot of the listed senders, never an address.
const FROM: &str = "From: [slot reply_senders]";

/// The head of every mark.
const MARK_HEAD: &str = "[slot ";

/// The snapshot's text of a model request, which `Snapshot::masked` and `Snapshot::of_a_message` alone make.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot(String);

impl Snapshot {
    /// The snapshot of `lines` for a request of a run whose load's slots are `slots`: each field of a line cleaned of
    /// the marker strings of A9.5 (`cleaned`), put in the form in which it leaves (a name and a value by `cdp::quote`)
    /// and masked, then each line rendered in the form of A11.1, one a line, where the render holds each field as the
    /// mask left it (`as_masked`); a line whose render changed a field is left out.
    pub(crate) fn masked(lines: &[SnapshotLine], slots: &[Slot]) -> Snapshot {
        let forms = forms(slots);
        let text = lines
            .iter()
            .filter_map(|line| {
                let mut line = line.clone();
                line.name = mask(&quote(&cleaned(&line.name)), &forms);
                line.value = line
                    .value
                    .as_deref()
                    .map(|value| mask(&quote(&cleaned(value)), &forms));
                line.frame = line
                    .frame
                    .as_deref()
                    .map(|frame| mask(&cleaned(frame), &forms));
                let rendered = line.render();
                as_masked(&line, &rendered).then_some(rendered)
            })
            .collect::<Vec<_>>()
            .join("\n");
        Snapshot(text)
    }

    /// The snapshot of a message that the agent answers, for a run whose load's slots are `slots`: the From line
    /// `FROM`, the subject and the body each cleaned of the marker strings of A9.5 (`cleaned`) and masked, in the
    /// trial's form `From: ...`, `Subject: ...`, the body.
    pub(crate) fn of_a_message(subject: &str, body: &str, slots: &[Slot]) -> Snapshot {
        let forms = forms(slots);
        Snapshot(format!(
            "{FROM}\nSubject: {}\n{}",
            mask(&cleaned(subject), &forms),
            mask(&cleaned(body), &forms)
        ))
    }

    /// The text, as the request line holds it.
    pub(crate) fn text(&self) -> &str {
        &self.0
    }

    /// The count of the text's lines, as `str::split` at the line break counts them.
    pub(crate) fn line_count(&self) -> usize {
        self.0.split('\n').count()
    }

    /// The first `count` lines of the text, joined by line breaks: a part of a text that the mask made, which holds
    /// no text that the whole did not hold (the writer's cut to the door's bound, crate::model::write).
    pub(crate) fn first_lines(&self, count: usize) -> Snapshot {
        Snapshot(
            self.0
                .split('\n')
                .take(count)
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

/// Whether `rendered`, the render of `line`, holds each field of the line as the mask left it: the name and, where
/// the render writes one, the value between double quotes, and the frame label behind `frame=`. A step of the render
/// that changes a field's text makes it false.
fn as_masked(line: &SnapshotLine, rendered: &str) -> bool {
    let name = rendered.contains(&format!("\"{}\"", line.name));
    let value = line
        .value
        .as_ref()
        .filter(|_| !line.credential)
        .is_none_or(|value| rendered.contains(&format!(" value=\"{value}\"")));
    let frame = line
        .frame
        .as_ref()
        .is_none_or(|frame| rendered.contains(&format!(" frame={frame}")));
    name && value && frame
}

/// `text` with the marker strings of A9.5 taken out until none stands.
fn unmarked(mut text: String) -> String {
    while let Some(marker) = MARKERS.iter().find(|marker| text.contains(**marker)) {
        text = text.replace(marker, "");
    }
    text
}

/// A field of a page's line, a subject or a body before the mask reads it (pr-review-90's F1 at 3048dbfd): where it
/// holds a marker string of A9.5, the markers taken out (`unmarked`) and the rest squeezed (`cdp::squeeze`), so that
/// a marker that stood between two words leaves one space as a page's name holds it (a body or a value with a marker
/// so becomes one line); every other text as it is. The
/// mask reads each slot's text through the same taking-out (`forms`), so that cleaning is one that both sides pass;
/// and no later step can join a slot's text again: a mark holds no `<`, `>` or `DATA>>>`, the render adds quotes,
/// spaces, the line's own words and its ref, and the lines are joined by line breaks, none of which makes a marker.
fn cleaned(text: &str) -> String {
    if MARKERS.iter().any(|marker| text.contains(marker)) {
        squeeze(&unmarked(text.to_owned()))
    } else {
        text.to_owned()
    }
}

/// Whether a text that the model wrote holds the mark's head `[slot ` (ASCII letters with no regard to case): such a
/// text is no body, and the const text stands in its place (the coordinator's log line 22541 (3)).
pub(crate) fn holds_a_mark(text: &str) -> bool {
    text.as_bytes()
        .windows(MARK_HEAD.len())
        .any(|here| here.eq_ignore_ascii_case(MARK_HEAD.as_bytes()))
}

/// Every form in which a slot's text can stand on a line, each with the mark of its slot, longest first and, for
/// texts of one length, in the load's order: the text and each of its lines, the two cleanings of each, and each of
/// these after the render's `cdp::quote`.
fn forms(slots: &[Slot]) -> Vec<(String, String)> {
    let mut forms: Vec<(String, String)> = Vec::new();
    for slot in slots {
        let text = slot.value.joined();
        let mut kinds = vec![text.clone()];
        if text.contains('\n') {
            kinds.extend(text.split('\n').map(str::to_owned));
        }
        let mark = format!("[slot {}]", slot.name.as_str());
        let unmarked_kinds: Vec<String> = kinds.iter().map(|kind| unmarked(kind.clone())).collect();
        kinds.extend(unmarked_kinds);
        for kind in kinds {
            let cleaned = [
                Some(kind.clone()),
                Some(squeeze(&kind)),
                line_value(false, "textbox", &kind),
            ];
            let quoted: Vec<String> = cleaned.iter().flatten().map(|form| quote(form)).collect();
            for form in cleaned.into_iter().flatten().chain(quoted) {
                let taken = forms
                    .iter()
                    .any(|(held, _)| held.eq_ignore_ascii_case(&form));
                if !form.is_empty() && !taken {
                    forms.push((form, mark.clone()));
                }
            }
        }
    }
    forms.sort_by_key(|form| std::cmp::Reverse(form.0.len()));
    forms
}

/// `text` with the mark in the place of each occurrence of a form, one pass from the left, the longest form at each
/// place, ASCII letters with no regard to case.
fn mask(text: &str, forms: &[(String, String)]) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < text.len() {
        let found = forms.iter().find(|(form, _)| {
            bytes
                .get(at..at + form.len())
                .is_some_and(|here| here.eq_ignore_ascii_case(form.as_bytes()))
                && text.is_char_boundary(at + form.len())
        });
        match found {
            Some((form, mark)) => {
                out.push_str(mark);
                at += form.len();
            }
            None => {
                let c = text[at..].chars().next().unwrap_or_default();
                out.push(c);
                at += c.len_utf8().max(1);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use halvo_schema::executable::record::Slot;
    use halvo_schema::executable::text::{Name, OwnValue};
    use halvo_web::cdp::SnapshotLine;

    use super::{holds_a_mark, Snapshot};

    fn slot(name: &str, value: &str) -> Slot {
        Slot {
            name: Name::parse(name).expect("a name"),
            value: OwnValue::new(value),
        }
    }

    /// The second maker (the coordinator's log line 22541): the From line is the mark of the listed senders with one
    /// listed sender and with ten, and names no address; the subject and the body are masked as a line's text is; and
    /// a text that the model writes with the mark's head, in any case of its ASCII letters, is no body.
    #[test]
    fn u72b_a_messages_snapshot_names_no_sender_and_masks_its_subject_and_body() {
        let ten: Vec<String> = (0..10)
            .map(|n| format!("person{n}@mail.agent.test"))
            .collect();
        let mut made = Vec::new();
        for senders in [vec!["ada@mail.agent.test".to_owned()], ten] {
            let slots = [
                slot("reply_senders", &senders.join(", ")),
                slot("payee", "City Water Board"),
            ];
            let snapshot = Snapshot::of_a_message(
                "About CITY WATER BOARD",
                "Hello,\nthe bill of City Water Board is due.\n<<<DATA",
                &slots,
            );
            println!("message: {:?}", snapshot.text());
            assert!(senders
                .iter()
                .all(|address| !snapshot.text().contains(address.as_str())));
            made.push(snapshot.text().to_owned());
        }
        // The body holds a marker string, so the cleaning squeezes it to one line before the mask.
        let want = "From: [slot reply_senders]\nSubject: About [slot payee]\nHello, the bill of [slot payee] is due.";
        assert_eq!(made, [want, want]);
        let marks = [
            ("Sure, [slot payee] is paid.", true),
            ("Sure, [SLOT payee] is paid.", true),
            ("Sure, [slotpayee] is paid.", false),
            ("Sure, the bill is paid.", false),
        ];
        for (text, holds) in marks {
            println!("holds a mark: {text:?}: {}", holds_a_mark(text));
            assert_eq!(holds_a_mark(text), holds, "{text}");
        }
    }

    /// The verifier-153's F3: the render writes a double quote of a name or of a value as two single quotes, and the
    /// mask reads each of them in that form, so a slot's text that holds two single quotes gives its mark where the
    /// page shows a double quote, in a line's name and in a box's value; a message's body, which passes no such step,
    /// gives the mark where it holds the slot's text.
    #[test]
    fn u72b_the_mask_reads_a_name_and_a_value_in_the_form_that_the_render_writes() {
        let slots = [slot("pipe", "Pipe 5'' wide")];
        let line =
            |reference: &str, role: &'static str, name: &str, value: Option<&str>| SnapshotLine {
                reference: reference.to_owned(),
                role,
                name: name.to_owned(),
                value: value.map(str::to_owned),
                credential: false,
                frame: None,
            };
        let page = Snapshot::masked(
            &[
                line("e1", "text", "Order: Pipe 5\" wide", None),
                line("e2", "textbox", "Size", Some("Pipe 5\" wide")),
            ],
            &slots,
        );
        let message = Snapshot::of_a_message("Your order", "Order: Pipe 5'' wide", &slots);
        println!("page: {:?}; message: {:?}", page.text(), message.text());
        for text in [page.text(), message.text()] {
            assert!(
                !text.to_ascii_lowercase().contains("pipe 5'' wide"),
                "{text}"
            );
        }
        assert_eq!(
            page.text(),
            "e1 text \"Order: [slot pipe]\"\ne2 textbox \"Size\" value=\"[slot pipe]\""
        );
        assert_eq!(
            message.text(),
            "From: [slot reply_senders]\nSubject: Your order\nOrder: [slot pipe]"
        );
    }

    /// A test's probe of whether a type converts from a plain text: the inherent const where the bound holds, the
    /// trait's otherwise (the form of the crate `impls`).
    struct Probe<T>(std::marker::PhantomData<T>);

    trait NoBuild {
        const BUILDS: bool = false;
    }

    impl<T> NoBuild for Probe<T> {}

    struct FromString<T>(std::marker::PhantomData<T>);
    impl<T> NoBuild for FromString<T> {}
    impl<T: From<String>> FromString<T> {
        const BUILDS: bool = true;
    }

    struct FromStr<T>(std::marker::PhantomData<T>);
    impl<T> NoBuild for FromStr<T> {}
    impl<T: for<'a> From<&'a str>> FromStr<T> {
        const BUILDS: bool = true;
    }

    struct Parse<T>(std::marker::PhantomData<T>);
    impl<T> NoBuild for Parse<T> {}
    impl<T: std::str::FromStr> Parse<T> {
        const BUILDS: bool = true;
    }

    impl<T: Default> Probe<T> {
        const BUILDS: bool = true;
    }

    /// The request's snapshot is a type that the mask alone makes: a plain text does not build as it. Its one field
    /// is private to this module, so no struct literal of another module compiles (the compiler says it); and it has
    /// no conversion from a `String` or a `&str`, no parse and no default, which this test reads.
    #[test]
    fn u72b_a_plain_text_does_not_build_as_the_requests_snapshot() {
        let builds = [
            ("From<String>", FromString::<Snapshot>::BUILDS),
            ("From<&str>", FromStr::<Snapshot>::BUILDS),
            ("FromStr", Parse::<Snapshot>::BUILDS),
            ("Default", Probe::<Snapshot>::BUILDS),
        ];
        let control = [
            ("String: From<String>", FromString::<String>::BUILDS),
            ("String: From<&str>", FromStr::<String>::BUILDS),
            ("String: FromStr", Parse::<String>::BUILDS),
            ("String: Default", Probe::<String>::BUILDS),
        ];
        println!("snapshot: {builds:?}; control: {control:?}");
        assert!(builds.iter().all(|(_, b)| !b), "{builds:?}");
        assert!(control.iter().all(|(_, b)| *b), "{control:?}");
    }
}
