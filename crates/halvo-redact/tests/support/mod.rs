pub fn luhn_number(body: &str) -> String {
    let sum: u32 = body
        .bytes()
        .rev()
        .enumerate()
        .map(|(k, b)| {
            let d = u32::from(b - b'0');
            if k.is_multiple_of(2) {
                let x = d * 2;
                if x > 9 {
                    x - 9
                } else {
                    x
                }
            } else {
                d
            }
        })
        .sum();
    format!("{body}{}", (10 - sum % 10) % 10)
}

pub fn grouped(chars: &str, separator: char) -> String {
    let mut out = String::new();
    for (k, c) in chars.chars().enumerate() {
        if k > 0 && k.is_multiple_of(4) {
            out.push(separator);
        }
        out.push(c);
    }
    out
}
