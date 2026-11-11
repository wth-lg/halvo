const SHORTEST: usize = 15;
const LONGEST: usize = 34;

fn is_iban_char(b: u8) -> bool {
    b.is_ascii_uppercase() || b.is_ascii_digit()
}

pub(crate) fn readings(text: &[u8], start: usize, found: &mut impl FnMut(usize)) {
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
        return;
    }
    let mut account = 0;
    for n in 5..=count {
        account = mod_97_step(account, chars[n - 1]);
        if n >= SHORTEST
            && chars[..4]
                .iter()
                .fold(account, |rem, &c| mod_97_step(rem, c))
                == 1
        {
            found(ends[n - 1]);
        }
    }
}

fn mod_97_step(rem: u32, c: u8) -> u32 {
    if c.is_ascii_digit() {
        (rem * 10 + u32::from(c - b'0')) % 97
    } else {
        (rem * 100 + u32::from(c - b'A') + 10) % 97
    }
}
