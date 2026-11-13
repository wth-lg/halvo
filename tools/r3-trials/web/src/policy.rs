//! Whether a browser's policy page, as text, names a policy (CP-126 E3). The hosted jobs print the
//! page to a PDF with no debugging channel and read its text; this decides on that text.
//!
//! The rule. The text is a policy page when a line begins with "policy name" in any letter case,
//! the header of the page's table; only lines after the first such line count. A word is a run of
//! letters and digits, in which a soft hyphen or a zero-width mark joins and does not split. A
//! policy is named when its name stands as a whole word, in the page's letter case:
//! - whole: a line begins with the word that equals the name and goes on with another word (the
//!   row's value, source or level), or
//! - broken: a line holds one word alone that begins the name, and later lines, up to
//!   `BREAK_LINES` after it, begin with the pieces that spell the rest of the name and nothing
//!   more, each a whole word, in the page's order, other lines between them skipped. The last piece
//!   either goes on with another word on its line, or stands alone with no line after it that
//!   begins with a capital letter (which could go on with the name).
//!
//! - joined (CP-145 (6)): the letters of the name stand in order from the start of a line with
//!   nothing but white space or line breaks between them, and what follows the last letter, with
//!   or without white space or a line break before it, is the value that the job wrote and read
//!   back, in the form in which the page prints it (`printed_value`), standing as the end of its
//!   word. A text reader that splits and joins words (pypdf on Edge's table: `Remot eDebuggingAllow
//!   e` and on the next line `dfalse Platform`) leaves no end of a word to go by; the value shows
//!   where the name ends. With no such value the form names nothing.
//!
//! A longer name that begins with, ends with or holds the name does not name it, nor the name inside
//! another policy's value, nor a name alone on its line with nothing after it (it can be the first
//! piece of a longer name), nor a page that prints another value than the job wrote: where the text
//! cannot tell, the policy is not named.

/// How many lines after a broken name's first piece the other pieces may stand.
pub const BREAK_LINES: usize = 12;

/// What the text shows of one policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Named {
    /// The text has no line that begins with "policy name": no policy page.
    NoPage,
    /// A policy page that does not name the policy by the rule.
    No,
    /// Named by a line that begins with the whole name.
    Whole,
    /// Named by pieces of a broken name.
    Broken,
    /// Named by the name's letters from the start of a line, ended by the value that the job wrote.
    Joined,
}

impl Named {
    pub fn named(self) -> bool {
        matches!(self, Named::Whole | Named::Broken | Named::Joined)
    }

    /// The word printed on a cell's line.
    pub fn word(self) -> &'static str {
        match self {
            Named::NoPage => "no_page",
            Named::No => "no",
            Named::Whole => "whole",
            Named::Broken => "broken",
            Named::Joined => "joined",
        }
    }
}

/// Characters that join the letters on both sides: the soft hyphen and the zero-width marks.
fn joins(c: char) -> bool {
    matches!(
        c,
        '\u{ad}' | '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}' | '\u{feff}'
    )
}

/// The words of one line, in order.
pub fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in line.chars() {
        if c.is_alphanumeric() {
            cur.push(c);
        } else if !joins(c) && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// How the policy page prints the value that the job wrote and read back for `name`, as fixed by
/// CP-145 (6) (b) before any run: `false` for the 0 of `RemoteDebuggingAllowed`, `direct` for the
/// `direct` of `ProxyMode`. Any other pair has no printed form, and the joined form names nothing.
pub fn printed_value(name: &str, value_read: &str) -> Option<&'static str> {
    match (name, value_read) {
        ("RemoteDebuggingAllowed", "0") => Some("false"),
        ("ProxyMode", "direct") => Some("direct"),
        _ => None,
    }
}

/// `named`, with the third form (joined) where the page's printed form of the value that the job
/// wrote and read back is given.
pub fn named_with(text: &str, name: &str, printed: Option<&str>) -> Named {
    let n = named(text, name);
    if n != Named::No {
        return n;
    }
    match printed {
        Some(value) if joined(text, name, value) => Named::Joined,
        _ => Named::No,
    }
}

/// The joined form of the module's rule, on the lines after the header.
fn joined(text: &str, name: &str, value: &str) -> bool {
    if name.is_empty() || value.is_empty() {
        return false;
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let Some(header) = lines.iter().position(|l| {
        let w = words(l);
        w.len() >= 2 && w[0].eq_ignore_ascii_case("policy") && w[1].eq_ignore_ascii_case("name")
    }) else {
        return false;
    };
    let after: Vec<char> = lines[header + 1..].join("\n").chars().collect();
    let starts = std::iter::once(0).chain(
        after
            .iter()
            .enumerate()
            .filter(|(_, c)| **c == '\n')
            .map(|(i, _)| i + 1),
    );
    let skip_white = |mut i: usize| {
        while after.get(i).is_some_and(|c| c.is_whitespace()) {
            i += 1;
        }
        i
    };
    let value: Vec<char> = value.chars().collect();
    starts.into_iter().any(|start| {
        // the first letter of the name begins the line (after its own leading white space)
        let mut i = skip_white(start);
        if after[start..i].contains(&'\n') {
            return false;
        }
        for (k, c) in name.chars().enumerate() {
            if k > 0 {
                i = skip_white(i);
            }
            if after.get(i) != Some(&c) {
                return false;
            }
            i += 1;
        }
        let i = skip_white(i);
        after.get(i..i + value.len()) == Some(&value[..])
            && after
                .get(i + value.len())
                .is_none_or(|c| !c.is_alphanumeric() && !joins(*c))
    })
}

/// Whether `text` names the policy `name` (see the module's rule) by the forms whole and broken.
pub fn named(text: &str, name: &str) -> Named {
    let lines: Vec<Vec<String>> = text.lines().map(words).collect();
    let header = lines.iter().position(|w| {
        w.len() >= 2 && w[0].eq_ignore_ascii_case("policy") && w[1].eq_ignore_ascii_case("name")
    });
    let Some(header) = header else {
        return Named::NoPage;
    };
    if name.is_empty() {
        return Named::No;
    }
    let rows = &lines[header + 1..];
    if rows.iter().any(|w| w.len() >= 2 && w[0] == name) {
        return Named::Whole;
    }
    let capital_after = |i: usize| {
        rows.get(i + 1)
            .and_then(|w| w.first())
            .and_then(|w| w.chars().next())
            .is_some_and(char::is_uppercase)
    };
    for (i, w) in rows.iter().enumerate() {
        let [first] = w.as_slice() else { continue };
        if first.len() >= name.len() || !name.starts_with(first.as_str()) {
            continue;
        }
        let mut rest = &name[first.len()..];
        for (j, row) in rows.iter().enumerate().skip(i + 1).take(BREAK_LINES) {
            let Some(head) = row.first() else { continue };
            if head == rest {
                if row.len() >= 2 || !capital_after(j) {
                    return Named::Broken;
                }
                break;
            }
            if row.len() == 1 && rest.starts_with(head.as_str()) {
                rest = &rest[head.len()..];
            }
        }
    }
    Named::No
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: &str =
        "Policies\nChrome Policies\nPolicy name Policy value Source Applies to Level Status\n";

    #[test]
    fn a_whole_name_that_begins_a_row_is_named() {
        let t = format!("{PAGE}RemoteDebuggingAllowed false Platform Machine Mandatory OK\n");
        assert_eq!(named(&t, "RemoteDebuggingAllowed"), Named::Whole);
        assert_eq!(named(&t, "ProxyMode"), Named::No);
    }

    #[test]
    fn a_longer_name_does_not_name_the_shorter_one_nor_the_reverse() {
        for longer in [
            "RemoteDebuggingAllowedOrigins",
            "DevToolsRemoteDebuggingAllowed",
            "XRemoteDebuggingAllowedY",
        ] {
            let t = format!("{PAGE}{longer} false Platform\n");
            assert_eq!(named(&t, "RemoteDebuggingAllowed"), Named::No, "{longer}");
        }
        let t = format!("{PAGE}ProxyMode direct Platform\n");
        assert_eq!(named(&t, "ProxyModeExtra"), Named::No);
    }

    #[test]
    fn the_name_inside_another_policys_value_is_not_named() {
        let t = format!(
            "{PAGE}ExtensionSettings {{\"note\":\"RemoteDebuggingAllowed ProxyMode\"}} Platform OK\n"
        );
        assert_eq!(named(&t, "RemoteDebuggingAllowed"), Named::No);
        assert_eq!(named(&t, "ProxyMode"), Named::No);
    }

    #[test]
    fn a_name_alone_on_its_line_can_be_the_first_piece_of_a_longer_one() {
        let t = format!("{PAGE}RemoteDebuggingAllowed\nOrigins [\"https://a.test\"] Platform OK\n");
        assert_eq!(named(&t, "RemoteDebuggingAllowed"), Named::No);
    }

    #[test]
    fn a_soft_hyphen_joins_and_another_case_is_another_name() {
        let t = format!("{PAGE}RemoteDebuggingAllowed\u{ad}Origins x Platform\n");
        assert_eq!(named(&t, "RemoteDebuggingAllowed"), Named::No);
        let t = format!("{PAGE}remotedebuggingallowed false Platform\n");
        assert_eq!(named(&t, "RemoteDebuggingAllowed"), Named::No);
        let t = format!("{PAGE}RemoteDebug\u{ad}ging\nAllowed false Platform\n");
        assert_eq!(named(&t, "RemoteDebuggingAllowed"), Named::Broken);
    }

    /// CP-161 F3 (X3b): a longer name broken after the shorter name's first piece does not name the
    /// shorter one, with its value on the line of its second piece or on the line after it; the
    /// shorter name broken the same way is still named.
    #[test]
    fn a_broken_longer_name_does_not_name_the_shorter_one() {
        let t = format!("{PAGE}RemoteDebugging\nAllowedOrigins [\"https://a.test\"] Platform OK\n");
        assert_eq!(named(&t, "RemoteDebuggingAllowed"), Named::No);
        let t = format!("{PAGE}RemoteDebugging\nAllowedOrigins\nfalse Platform OK\n");
        assert_eq!(named(&t, "RemoteDebuggingAllowed"), Named::No);
        let t = format!("{PAGE}RemoteDebugging\nAllowed false Platform OK\n");
        assert_eq!(named(&t, "RemoteDebuggingAllowed"), Named::Broken);
    }

    /// The two shapes of the pages of run 43347520562: Chrome's `Proxy` / `Mode Platform`, and
    /// Edge's `RemoteDebuggingAllow` with `ed` six lines later.
    #[test]
    fn a_name_broken_as_the_hosted_pages_print_it_is_named() {
        let chrome = "Policy name Source\nProxy\nMode Platform\nPolicy Precedence\n";
        assert_eq!(named(chrome, "ProxyMode"), Named::Broken);
        let edge = "Policy Name Policy Value Source Applies\nTo\nRemoteDebuggingAllow\nfalse Platform DeviceMandator\nLevel Status Show\nore\nK\nO\ned\ny\nPolicy Precedence\n";
        assert_eq!(named(edge, "RemoteDebuggingAllowed"), Named::Broken);
        let longer = "Policy Name Policy Value Source Applies\nRemoteDebuggingAllow\nfalse Platform\ned\nOrigins\n";
        assert_eq!(named(longer, "RemoteDebuggingAllowed"), Named::No);
        let far = format!(
            "Policy name Source\nRemoteDebuggingAllow\n{}ed\n",
            "x\n".repeat(BREAK_LINES)
        );
        assert_eq!(named(&far, "RemoteDebuggingAllowed"), Named::No);
    }

    #[test]
    fn a_text_with_no_table_header_is_no_policy_page() {
        assert_eq!(
            named(
                "Search results\nName Value\nRemoteDebuggingAllowed false\n",
                "RemoteDebuggingAllowed"
            ),
            Named::NoPage
        );
        assert_eq!(named("", "ProxyMode"), Named::NoPage);
        assert_eq!(
            named(
                "Policies\nChrome Policies\nREMOTEDEBUGGINGALLOWED\nNo policies set\n",
                "RemoteDebuggingAllowed"
            ),
            Named::NoPage
        );
    }

    /// The verifier's fifteen texts (evidence/verifier-32/model/reader_texts.json), each read for the
    /// two policies of the `org_` cells: the rule names a policy exactly where the text holds it.
    #[test]
    fn the_verifiers_thirty_readings_agree() {
        let texts: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../tests/data/reader_texts.json")).unwrap();
        let mut wrong = Vec::new();
        for t in &texts {
            let text = t["text"].as_str().unwrap();
            let holds: Vec<&str> = t["holds"]
                .as_array()
                .unwrap()
                .iter()
                .map(|h| h.as_str().unwrap())
                .collect();
            for name in ["RemoteDebuggingAllowed", "ProxyMode"] {
                if named(text, name).named() != holds.contains(&name) {
                    wrong.push(format!("{} {name}", t["id"].as_str().unwrap()));
                }
            }
        }
        assert_eq!(texts.len() * 2, 30);
        assert!(
            wrong.is_empty(),
            "readings that differ from the text: {wrong:?}"
        );
    }
}
