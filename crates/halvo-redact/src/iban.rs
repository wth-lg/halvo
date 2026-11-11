const SHORTEST: usize = 15;
const LONGEST: usize = 34;

fn is_iban_char(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit()
}

pub(crate) fn end_of_match(text: &[u8], start: usize) -> Option<usize> {
    let mut chars = [0u8; LONGEST];
    let mut ends = [0usize; LONGEST];
    let mut count = 0;
    let mut at = start;
    while count < LONGEST {
        let Some(&c) = text.get(at).filter(|&&c| is_iban_char(c)) else {
            break;
        };
        chars[count] = c;
        ends[count] = at + 1;
        count += 1;
        at += 1;
        if text.get(at) == Some(&b' ') && text.get(at + 1).is_some_and(|&c| is_iban_char(c)) {
            at += 1;
        }
    }
    let country_and_check = count >= 4
        && chars[0].is_ascii_uppercase()
        && chars[1].is_ascii_uppercase()
        && chars[2].is_ascii_digit()
        && chars[3].is_ascii_digit();
    if !country_and_check {
        return None;
    }
    (SHORTEST..=count)
        .rev()
        .find(|&n| {
            !text.get(ends[n - 1]).is_some_and(u8::is_ascii_alphanumeric) && mod_97(&chars[..n])
        })
        .map(|n| ends[n - 1])
}

fn mod_97(chars: &[u8]) -> bool {
    let (country_and_check, account) = chars.split_at(4);
    let remainder = account
        .iter()
        .chain(country_and_check)
        .fold(0u32, |rem, &c| {
            if c.is_ascii_digit() {
                (rem * 10 + u32::from(c - b'0')) % 97
            } else {
                (rem * 100 + u32::from(c - b'A') + 10) % 97
            }
        });
    remainder == 1
}
