/// The blob id of `content`, 40 lower-case hexadecimal digits.
pub fn blob_id(content: &[u8]) -> String {
    let mut data = format!("blob {}\0", content.len()).into_bytes();
    data.extend_from_slice(content);
    sha1(&data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut state: [u32; 5] = [
        0x6745_2301,
        0xEFCD_AB89,
        0x98BA_DCFE,
        0x1032_5476,
        0xC3D2_E1F0,
    ];
    let bits = u64::try_from(data.len())
        .unwrap_or(u64::MAX)
        .wrapping_mul(8);
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());
    for chunk in message.as_chunks::<64>().0 {
        let mut words = [0u32; 80];
        for (word, bytes) in words.iter_mut().zip(chunk.as_chunks::<4>().0) {
            *word = u32::from_be_bytes(*bytes);
        }
        for i in 16..80 {
            words[i] = (words[i - 3] ^ words[i - 8] ^ words[i - 14] ^ words[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (i, word) in words.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
                _ => (b ^ c ^ d, 0xCA62_C1D6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (part, add) in state.iter_mut().zip([a, b, c, d, e]) {
            *part = part.wrapping_add(add);
        }
    }
    let mut digest = [0u8; 20];
    for (bytes, part) in digest.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *bytes = part.to_be_bytes();
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::blob_id;

    #[test]
    fn blob_known() {
        assert_eq!(blob_id(b""), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
        assert_eq!(
            blob_id(b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
    }

    #[test]
    fn blob_of_lengths_across_a_block_boundary() {
        let known = [
            (55, "d1985ddc2983785702b9a90effd5aff2f7cfdca4"),
            (56, "1f973e890f52da1f22fa7e5620a628bc4ee74cb3"),
            (63, "487e57f9763ebddbed2027c4452510c0ae0f95ff"),
            (64, "71b7a71962774fa5c721e1163f935cb61a0e09e6"),
            (119, "1325c3d76ea6a66e75dce490a0b8cb8b12233ee0"),
        ];
        for (length, id) in known {
            assert_eq!(blob_id(&vec![b'a'; length]), id, "{length}");
        }
    }
}
