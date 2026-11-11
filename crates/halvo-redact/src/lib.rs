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
/// The title is read once, as it is given. A reading is a rule with a start and an end between
/// which the text has the form of the rule; every length that the rule allows gives a reading
/// of its own. The start of a reading holds where the byte before it is one that its rule allows
/// there or closes it, and its end holds where the byte after it is one that its rule allows
/// there or closes it. A byte closes a boundary when it lies outside the title or when a reading
/// that holds covers it, of whatever rule and at whatever place in that reading, so a number
/// that touches a masked identifier, or that one space or one hyphen separates from it, can be
/// masked with it. The readings that hold are the smallest set in which the start and the end
/// of each reading hold.
/// Readings that share a byte form a group, and each group is replaced by the token of its
/// reading that starts first, in the order key, IBAN, card, SSN where several start there. So
/// `mask(&mask(t)) == mask(t)`. Only ASCII bytes are examined; every other byte is copied, and
/// a title in which no rule matches is returned byte for byte. The result never holds more
/// bytes than the title, and the time of one call grows with the length of the title.
///
/// A caller that stores a title masks the whole title and cuts only the result: a group of
/// readings of any length becomes one token, so the stored bytes can come from beyond any cut
/// made before the call, and such a cut can split an identifier that the stored title would
/// then keep in part.
pub fn mask(title: &str) -> String {
    let text = title.as_bytes();
    let mut found = Vec::new();
    for start in 0..text.len() {
        for rule in Rule::ORDER {
            rule.readings(text, start, &mut |end| {
                found.push(Reading { rule, start, end })
            });
        }
    }
    let holds = close(text, &found);
    join(title, &found, &holds)
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

    fn may_precede(self, after: u8) -> bool {
        match self {
            Rule::Key => true,
            Rule::Iban => !after.is_ascii_alphanumeric(),
            Rule::Card | Rule::Ssn => !after.is_ascii_digit(),
        }
    }

    fn readings(self, text: &[u8], start: usize, found: &mut impl FnMut(usize)) {
        match self {
            Rule::Key => key::readings(text, start, found),
            Rule::Iban => iban::readings(text, start, found),
            Rule::Card if cfg!(feature = "control-card-off") => {}
            Rule::Card => card::readings(text, start, found),
            Rule::Ssn => ssn::readings(text, start, found),
        }
    }
}

#[derive(Clone, Copy)]
struct Reading {
    rule: Rule,
    start: usize,
    end: usize,
}

const BEFORE: usize = 0;
const AFTER: usize = 1;

impl Reading {
    fn open_bytes(&self, text: &[u8]) -> [Option<usize>; 2] {
        [
            self.start
                .checked_sub(1)
                .filter(|&b| !self.rule.may_follow(text[b])),
            Some(self.end).filter(|&b| b < text.len() && !self.rule.may_precede(text[b])),
        ]
    }
}

fn slot(byte: usize, side: usize) -> usize {
    byte * 2 + side
}

struct Waits {
    first: Vec<usize>,
    readings: Vec<usize>,
    open: Vec<u8>,
    ready: Vec<usize>,
}

impl Waits {
    fn new(text: &[u8], found: &[Reading]) -> Waits {
        let slots = text.len() * 2;
        let mut first = vec![0usize; slots + 1];
        let mut open = vec![0u8; found.len()];
        for (r, reading) in found.iter().enumerate() {
            for (side, byte) in reading.open_bytes(text).into_iter().enumerate() {
                if let Some(byte) = byte {
                    first[slot(byte, side) + 1] += 1;
                    open[r] += 1;
                }
            }
        }
        for s in 0..slots {
            first[s + 1] += first[s];
        }
        let mut filled = first.clone();
        let mut readings = vec![0usize; first[slots]];
        for (r, reading) in found.iter().enumerate() {
            for (side, byte) in reading.open_bytes(text).into_iter().enumerate() {
                if let Some(byte) = byte {
                    let s = slot(byte, side);
                    readings[filled[s]] = r;
                    filled[s] += 1;
                }
            }
        }
        let ready = (0..found.len()).filter(|&r| open[r] == 0).collect();
        Waits {
            first,
            readings,
            open,
            ready,
        }
    }

    fn release(&mut self, slot: usize) {
        for &r in &self.readings[self.first[slot]..self.first[slot + 1]] {
            self.open[r] -= 1;
            if self.open[r] == 0 {
                self.ready.push(r);
            }
        }
    }
}

fn close(text: &[u8], found: &[Reading]) -> Vec<bool> {
    let mut waits = Waits::new(text, found);
    let mut next: Vec<usize> = (0..=text.len()).collect();
    let mut holds = vec![false; found.len()];
    while let Some(r) = waits.ready.pop() {
        holds[r] = true;
        let mut byte = unmasked_from(&mut next, found[r].start);
        while byte < found[r].end {
            next[byte] = byte + 1;
            waits.release(slot(byte, BEFORE));
            waits.release(slot(byte, AFTER));
            byte = unmasked_from(&mut next, byte + 1);
        }
    }
    holds
}

fn unmasked_from(next: &mut [usize], byte: usize) -> usize {
    let mut found = byte;
    while next[found] != found {
        found = next[found];
    }
    let mut at = byte;
    while next[at] != found {
        let after = next[at];
        next[at] = found;
        at = after;
    }
    found
}

fn join(title: &str, found: &[Reading], holds: &[bool]) -> String {
    let mut out = String::with_capacity(title.len());
    let mut copied = 0;
    let mut group: Option<(Rule, usize, usize)> = None;
    for (reading, _) in found.iter().zip(holds).filter(|(_, &h)| h) {
        match group {
            Some((rule, start, end)) if reading.start < end => {
                group = Some((rule, start, end.max(reading.end)));
            }
            _ => {
                if let Some((rule, start, end)) = group {
                    out.push_str(&title[copied..start]);
                    out.push_str(rule.token());
                    copied = end;
                }
                group = Some((reading.rule, reading.start, reading.end));
            }
        }
    }
    if let Some((rule, start, end)) = group {
        out.push_str(&title[copied..start]);
        out.push_str(rule.token());
        copied = end;
    }
    out.push_str(&title[copied..]);
    out
}
