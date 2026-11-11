const LENGTH: usize = 11;

pub(crate) fn end_of_match(text: &[u8], start: usize) -> Option<usize> {
    let number = text.get(start..start + LENGTH)?;
    let form = number.iter().enumerate().all(|(k, &b)| match k {
        3 | 6 => b == b'-',
        _ => b.is_ascii_digit(),
    });
    if !form || text.get(start + LENGTH).is_some_and(u8::is_ascii_digit) {
        return None;
    }
    let (area, group, serial) = (&number[0..3], &number[4..6], &number[7..11]);
    let excluded =
        area == b"000" || area == b"666" || area[0] == b'9' || group == b"00" || serial == b"0000";
    (!excluded).then_some(start + LENGTH)
}
