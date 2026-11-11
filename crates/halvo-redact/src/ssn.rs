const LENGTH: usize = 11;

pub(crate) fn readings(text: &[u8], start: usize, found: &mut impl FnMut(usize)) {
    let Some(number) = text.get(start..start + LENGTH) else {
        return;
    };
    let form = number.iter().enumerate().all(|(k, &b)| match k {
        3 | 6 => b == b'-',
        _ => b.is_ascii_digit(),
    });
    if !form {
        return;
    }
    let (area, group, serial) = (&number[0..3], &number[4..6], &number[7..11]);
    let excluded =
        area == b"000" || area == b"666" || area[0] == b'9' || group == b"00" || serial == b"0000";
    if !excluded {
        found(start + LENGTH);
    }
}
