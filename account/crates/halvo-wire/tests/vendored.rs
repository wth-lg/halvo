use std::{
    fs,
    path::{Path, PathBuf},
};

const FILES: [(&str, &str); 4] = [
    ("schema.v1.json", "contract/schema/v1.json"),
    ("routes.v1.json", "contract/routes/v1.json"),
    ("samples.v1.json", "contract/samples/v1.json"),
    ("prompt-budget.json", "gate/prompt-budget.json"),
];

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

const H0: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

fn sha256_hex(bytes: &[u8]) -> String {
    let mut message = bytes.to_vec();
    let bit_length = (bytes.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_be_bytes());
    let mut h = H0;
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, value) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(value);
        }
    }
    h.iter().map(|word| format!("{word:08x}")).collect()
}

fn schema_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schema")
}

#[derive(Clone)]
struct Record {
    commit: String,
    lines: Vec<(String, String, String)>,
}

fn lower_hex(text: &str, length: usize) -> bool {
    text.len() == length
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn parse_record(text: &str) -> Result<Record, String> {
    if !text.ends_with('\n') || text.contains('\r') {
        return Err("vendored.txt ends with a newline and holds no carriage return".to_string());
    }
    let mut rows = text.lines();
    let commit = rows
        .next()
        .and_then(|first| first.strip_prefix("commit "))
        .filter(|commit| lower_hex(commit, 40))
        .ok_or("the first line is `commit` and 40 lower-case hexadecimal digits")?
        .to_string();
    let mut lines = Vec::new();
    for row in rows {
        let words: Vec<&str> = row.split(' ').collect();
        if words.len() != 3 || !lower_hex(words[0], 64) {
            return Err(format!(
                "a file line is `<sha256> <name> <backend path>`: {row:?}"
            ));
        }
        lines.push((
            words[0].to_string(),
            words[1].to_string(),
            words[2].to_string(),
        ));
    }
    Ok(Record { commit, lines })
}

fn record_text() -> String {
    let path = schema_dir().join("vendored.txt");
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn record() -> Record {
    parse_record(&record_text()).unwrap_or_else(|error| panic!("{error}"))
}

#[test]
fn the_record_parser_refuses_each_malformed_form() {
    let good = record_text();
    assert!(parse_record(&good).is_ok());
    let commit = record().commit;
    let first_sum = &record().lines[0].0;
    let cases = [
        ("carriage returns", good.replace('\n', "\r\n")),
        ("no final newline", good.trim_end_matches('\n').to_string()),
        (
            "an upper-case commit",
            good.replacen(&commit, &commit.to_uppercase(), 1),
        ),
        ("a short commit", good.replacen(&commit, &commit[1..], 1)),
        (
            "another first word",
            good.replacen("commit ", "commit: ", 1),
        ),
        ("a short sum", good.replacen(first_sum, &first_sum[1..], 1)),
        (
            "an upper-case sum",
            good.replacen(first_sum, &first_sum.to_uppercase(), 1),
        ),
        (
            "a fourth word",
            good.replacen(" contract/schema/v1.json", " contract/schema/v1.json x", 1),
        ),
    ];
    for (label, text) in cases {
        assert!(text != good, "{label}: the case changes the text");
        assert!(parse_record(&text).is_err(), "{label} was read");
    }
}

#[test]
fn sha256_gives_the_published_digests() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
    );
    assert_eq!(
        sha256_hex(&[b'a'; 1_000_000]),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

#[test]
fn the_record_names_one_commit_and_the_four_vendored_files() {
    let record = record();
    let named: Vec<(&str, &str)> = record
        .lines
        .iter()
        .map(|(_, name, path)| (name.as_str(), path.as_str()))
        .collect();
    assert_eq!(named, FILES.to_vec());
    assert!(record.lines.iter().all(|(sum, _, _)| lower_hex(sum, 64)));
    assert_eq!(record.commit.len(), 40);
}

fn differing(record: &Record, dir: &Path) -> Vec<String> {
    record
        .lines
        .iter()
        .filter_map(|(sum, name, _)| {
            let path = dir.join(name);
            let bytes = fs::read(&path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
            let actual = sha256_hex(&bytes);
            (actual != *sum).then(|| format!("{name}: recorded {sum}, file has {actual}"))
        })
        .collect()
}

#[test]
fn each_vendored_file_has_its_recorded_sha256() {
    let record = record();
    let differ = differing(&record, &schema_dir());
    assert!(differ.is_empty(), "{differ:#?}");
    assert_eq!(record.lines.len(), FILES.len());
}

#[test]
fn a_changed_byte_or_a_changed_sum_is_reported() {
    let record = record();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("vendored-copy");
    fs::create_dir_all(&dir).expect("the copy's directory is made");
    for (_, name, _) in &record.lines {
        fs::copy(schema_dir().join(name), dir.join(name)).expect("a vendored file is copied");
    }
    assert!(differing(&record, &dir).is_empty());
    let routes = dir.join("routes.v1.json");
    let mut bytes = fs::read(&routes).expect("the copy is read");
    bytes[0] ^= 1;
    fs::write(&routes, &bytes).expect("the copy is written");
    let reported = differing(&record, &dir);
    assert_eq!(reported.len(), 1, "{reported:#?}");
    assert!(reported[0].starts_with("routes.v1.json: "));
    fs::copy(schema_dir().join("routes.v1.json"), &routes).expect("the copy is restored");
    let mut changed = record.clone();
    let sum = &mut changed.lines[2].0;
    let first = if sum.starts_with('0') { "1" } else { "0" };
    sum.replace_range(0..1, first);
    let reported = differing(&changed, &dir);
    assert_eq!(reported.len(), 1, "{reported:#?}");
    assert!(reported[0].starts_with("samples.v1.json: "));
}
