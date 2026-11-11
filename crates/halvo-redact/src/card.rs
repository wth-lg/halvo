const SHORTEST: usize = 13;
const LONGEST: usize = 19;

pub(crate) fn end_of_match(text: &[u8], start: usize) -> Option<usize> {
    let mut digits = [0u8; LONGEST];
    let mut ends = [0usize; LONGEST];
    let mut count = 0;
    let mut at = start;
    while count < LONGEST {
        let Some(&b) = text.get(at).filter(|b| b.is_ascii_digit()) else {
            break;
        };
        digits[count] = b - b'0';
        ends[count] = at + 1;
        count += 1;
        at += 1;
        let separator = text.get(at).is_some_and(|&s| s == b' ' || s == b'-');
        if separator && text.get(at + 1).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
    }
    (SHORTEST..=count)
        .rev()
        .find(|&n| !text.get(ends[n - 1]).is_some_and(u8::is_ascii_digit) && luhn(&digits[..n]))
        .map(|n| ends[n - 1])
}

fn luhn(digits: &[u8]) -> bool {
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(k, &d)| {
            let d = u32::from(d);
            match (k % 2, d * 2) {
                (0, _) => d,
                (_, doubled) if doubled > 9 => doubled - 9,
                (_, doubled) => doubled,
            }
        })
        .sum();
    sum.is_multiple_of(10)
}
