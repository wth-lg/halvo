mod card;
mod iban;
mod key;
mod ssn;

/// Returns `title` with every identifier that a rule of this crate finds replaced by the token
/// of its rule: `[key]` for a key or token that starts with a prefix of the rules of gitleaks
/// v8.30.1 that this crate ports, `[iban]` for an IBAN that passes the mod 97 check, `[card]`
/// for a card number that passes the Luhn check and `[ssn]` for a US Social Security number of
/// the form `DDD-DD-DDDD` whose groups are not excluded.
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
    Key,
    Iban,
    Card,
    Ssn,
}

impl Rule {
    const ORDER: [Rule; 4] = [Rule::Key, Rule::Iban, Rule::Card, Rule::Ssn];

    fn token(self) -> &'static str {
        match self {
            Rule::Key => "[key]",
            Rule::Iban => "[iban]",
            Rule::Card => "[card]",
            Rule::Ssn => "[ssn]",
        }
    }

    fn may_follow(self, before: u8) -> bool {
        match self {
            Rule::Key | Rule::Iban => !before.is_ascii_alphanumeric(),
            Rule::Card | Rule::Ssn => !before.is_ascii_digit(),
        }
    }

    fn end_of_match(self, text: &[u8], start: usize) -> Option<usize> {
        if start > 0 && !self.may_follow(text[start - 1]) {
            return None;
        }
        match self {
            Rule::Key => key::end_of_match(text, start),
            Rule::Iban => iban::end_of_match(text, start),
            Rule::Card if cfg!(feature = "control-card-off") => None,
            Rule::Card => card::end_of_match(text, start),
            Rule::Ssn => ssn::end_of_match(text, start),
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
