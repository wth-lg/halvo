mod card;

/// Returns `title` with every identifier that a rule of this crate finds replaced by the token
/// of its rule: `[card]` for a card number that passes the Luhn check.
///
/// The text is scanned from the start; at each byte the rules are tried in a fixed order and
/// the first match is replaced, and scans repeat until one changes nothing, so
/// `mask(&mask(t)) == mask(t)`. Only ASCII bytes are examined; every other byte is copied, and
/// a title in which no rule matches is returned byte for byte. The result never holds more
/// bytes than the title.
pub fn mask(title: &str) -> String {
    let mut text = title.to_owned();
    loop {
        let next = scan(&text);
        if next == text {
            return text;
        }
        text = next;
    }
}

#[derive(Clone, Copy)]
enum Rule {
    Card,
}

impl Rule {
    const ORDER: [Rule; 1] = [Rule::Card];

    fn token(self) -> &'static str {
        match self {
            Rule::Card => "[card]",
        }
    }

    fn end_of_match(self, text: &[u8], start: usize) -> Option<usize> {
        let before = start.checked_sub(1).map(|p| text[p]);
        match self {
            Rule::Card => {
                if before.is_some_and(|b| b.is_ascii_digit()) {
                    return None;
                }
                card::end_of_match(text, start)
            }
        }
    }
}

fn scan(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut at = 0;
    while at < bytes.len() {
        let found = Rule::ORDER
            .into_iter()
            .find_map(|rule| rule.end_of_match(bytes, at).map(|end| (rule, end)));
        match found {
            Some((rule, end)) => {
                out.push_str(&text[copied..at]);
                out.push_str(rule.token());
                copied = end;
                at = end;
            }
            None => at += 1,
        }
    }
    out.push_str(&text[copied..]);
    out
}
