#[derive(Clone, Copy)]
enum Alphabet {
    UpperBase32,
    AlphaNum,
    AlphaNumUnderscore,
    Url,
    Hex,
    HexMixedCase,
    SendGrid,
    Bech32Upper,
    Letters,
}

impl Alphabet {
    fn holds(self, b: u8) -> bool {
        match self {
            Alphabet::UpperBase32 => b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b),
            Alphabet::AlphaNum => b.is_ascii_alphanumeric(),
            Alphabet::AlphaNumUnderscore => b.is_ascii_alphanumeric() || b == b'_',
            Alphabet::Url => b.is_ascii_alphanumeric() || b == b'_' || b == b'-',
            Alphabet::Hex => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
            Alphabet::HexMixedCase => b.is_ascii_hexdigit(),
            Alphabet::SendGrid => b.is_ascii_alphanumeric() || b"=_-.".contains(&b),
            Alphabet::Bech32Upper => b"QPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L".contains(&b),
            Alphabet::Letters => b.is_ascii_alphabetic(),
        }
    }

    #[cfg(test)]
    fn name(self) -> &'static str {
        match self {
            Alphabet::UpperBase32 => "A-Z 2-7",
            Alphabet::AlphaNum => "an",
            Alphabet::AlphaNumUnderscore => "an and _",
            Alphabet::Url => "url",
            Alphabet::Hex => "hex",
            Alphabet::HexMixedCase => "HEX",
            Alphabet::SendGrid => "an and = _ - .",
            Alphabet::Bech32Upper => "QPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L",
            Alphabet::Letters => "A-Z a-z",
        }
    }
}

struct Row {
    #[cfg_attr(not(test), expect(dead_code))]
    number: u8,
    #[cfg_attr(not(test), expect(dead_code))]
    rule_id: &'static str,
    prefixes: &'static [&'static str],
    alphabet: Alphabet,
    min: usize,
    max: usize,
}

const fn row(
    number: u8,
    rule_id: &'static str,
    prefixes: &'static [&'static str],
    alphabet: Alphabet,
    min: usize,
    max: usize,
) -> Row {
    Row {
        number,
        rule_id,
        prefixes,
        alphabet,
        min,
        max,
    }
}

const ROWS: [Row; 28] = [
    row(
        1,
        "aws-access-token",
        &["AKIA", "ASIA", "ABIA", "ACCA"],
        Alphabet::UpperBase32,
        16,
        16,
    ),
    row(
        2,
        "aws-access-token",
        &[
            "A3TA", "A3TB", "A3TC", "A3TD", "A3TE", "A3TF", "A3TG", "A3TH", "A3TI", "A3TJ", "A3TK",
            "A3TL", "A3TM", "A3TN", "A3TO", "A3TP", "A3TQ", "A3TR", "A3TS", "A3TT", "A3TU", "A3TV",
            "A3TW", "A3TX", "A3TY", "A3TZ", "A3T0", "A3T1", "A3T2", "A3T3", "A3T4", "A3T5", "A3T6",
            "A3T7", "A3T8", "A3T9",
        ],
        Alphabet::UpperBase32,
        16,
        16,
    ),
    row(3, "github-pat", &["ghp_"], Alphabet::AlphaNum, 36, 36),
    row(4, "github-oauth", &["gho_"], Alphabet::AlphaNum, 36, 36),
    row(
        5,
        "github-app-token",
        &["ghu_", "ghs_"],
        Alphabet::AlphaNum,
        36,
        36,
    ),
    row(
        6,
        "github-refresh-token",
        &["ghr_"],
        Alphabet::AlphaNum,
        36,
        36,
    ),
    row(
        7,
        "github-fine-grained-pat",
        &["github_pat_"],
        Alphabet::AlphaNumUnderscore,
        82,
        82,
    ),
    row(8, "gitlab-pat", &["glpat-"], Alphabet::Url, 20, 20),
    row(9, "gitlab-ptt", &["glptt-"], Alphabet::Hex, 40, 40),
    row(10, "gitlab-rrt", &["GR1348941"], Alphabet::Url, 20, 20),
    row(
        11,
        "stripe-access-token",
        &[
            "sk_test_", "sk_live_", "sk_prod_", "rk_test_", "rk_live_", "rk_prod_",
        ],
        Alphabet::AlphaNum,
        10,
        99,
    ),
    row(
        12,
        "sendgrid-api-token",
        &["SG."],
        Alphabet::SendGrid,
        66,
        66,
    ),
    row(
        14,
        "pypi-upload-token",
        &["pypi-AgEIcHlwaS5vcmc"],
        Alphabet::Url,
        50,
        1000,
    ),
    row(15, "gcp-api-key", &["AIza"], Alphabet::Url, 35, 35),
    row(16, "digitalocean-pat", &["dop_v1_"], Alphabet::Hex, 64, 64),
    row(
        17,
        "digitalocean-access-token",
        &["doo_v1_"],
        Alphabet::Hex,
        64,
        64,
    ),
    row(
        19,
        "doppler-api-token",
        &["dp.pt."],
        Alphabet::AlphaNum,
        43,
        43,
    ),
    row(
        20,
        "shopify-access-token",
        &["shpat_"],
        Alphabet::HexMixedCase,
        32,
        32,
    ),
    row(
        21,
        "shopify-custom-access-token",
        &["shpca_"],
        Alphabet::HexMixedCase,
        32,
        32,
    ),
    row(
        22,
        "shopify-private-app-access-token",
        &["shppa_"],
        Alphabet::HexMixedCase,
        32,
        32,
    ),
    row(
        23,
        "shopify-shared-secret",
        &["shpss_"],
        Alphabet::HexMixedCase,
        32,
        32,
    ),
    row(24, "pulumi-api-token", &["pul-"], Alphabet::Hex, 40, 40),
    row(
        25,
        "linear-api-key",
        &["lin_api_"],
        Alphabet::AlphaNum,
        40,
        40,
    ),
    row(
        26,
        "age-secret-key",
        &["AGE-SECRET-KEY-1"],
        Alphabet::Bech32Upper,
        58,
        58,
    ),
    row(
        27,
        "huggingface-access-token",
        &["hf_"],
        Alphabet::Letters,
        34,
        34,
    ),
    row(
        28,
        "rubygems-api-token",
        &["rubygems_"],
        Alphabet::Hex,
        48,
        48,
    ),
    row(29, "databricks-api-token", &["dapi"], Alphabet::Hex, 32, 32),
    row(
        30,
        "twilio-api-key",
        &["SK"],
        Alphabet::HexMixedCase,
        32,
        32,
    ),
];

pub(crate) fn readings(text: &[u8], start: usize, found: &mut impl FnMut(usize)) {
    let rest = &text[start..];
    for row in &ROWS {
        for prefix in row.prefixes {
            let Some(body) = rest.strip_prefix(prefix.as_bytes()) else {
                continue;
            };
            let length = body
                .iter()
                .take(row.max)
                .take_while(|&&b| row.alphabet.holds(b))
                .count();
            if length >= row.min {
                found(start + prefix.len() + length);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ROWS;

    const TABLE: &str = "\
1 aws-access-token AKIA ASIA ABIA ACCA A-Z 2-7 16 16
2 aws-access-token A3TA A3TB A3TC A3TD A3TE A3TF A3TG A3TH A3TI A3TJ A3TK A3TL A3TM A3TN A3TO A3TP A3TQ A3TR A3TS A3TT A3TU A3TV A3TW A3TX A3TY A3TZ A3T0 A3T1 A3T2 A3T3 A3T4 A3T5 A3T6 A3T7 A3T8 A3T9 A-Z 2-7 16 16
3 github-pat ghp_ an 36 36
4 github-oauth gho_ an 36 36
5 github-app-token ghu_ ghs_ an 36 36
6 github-refresh-token ghr_ an 36 36
7 github-fine-grained-pat github_pat_ an and _ 82 82
8 gitlab-pat glpat- url 20 20
9 gitlab-ptt glptt- hex 40 40
10 gitlab-rrt GR1348941 url 20 20
11 stripe-access-token sk_test_ sk_live_ sk_prod_ rk_test_ rk_live_ rk_prod_ an 10 99
12 sendgrid-api-token SG. an and = _ - . 66 66
14 pypi-upload-token pypi-AgEIcHlwaS5vcmc url 50 1000
15 gcp-api-key AIza url 35 35
16 digitalocean-pat dop_v1_ hex 64 64
17 digitalocean-access-token doo_v1_ hex 64 64
19 doppler-api-token dp.pt. an 43 43
20 shopify-access-token shpat_ HEX 32 32
21 shopify-custom-access-token shpca_ HEX 32 32
22 shopify-private-app-access-token shppa_ HEX 32 32
23 shopify-shared-secret shpss_ HEX 32 32
24 pulumi-api-token pul- hex 40 40
25 linear-api-key lin_api_ an 40 40
26 age-secret-key AGE-SECRET-KEY-1 QPZRY9X8GF2TVDW0S3JN54KHCE6MUA7L 58 58
27 huggingface-access-token hf_ A-Z a-z 34 34
28 rubygems-api-token rubygems_ hex 48 48
29 databricks-api-token dapi hex 32 32
30 twilio-api-key SK HEX 32 32
";

    #[test]
    fn key_rows() {
        let code: Vec<String> = ROWS
            .iter()
            .map(|row| {
                format!(
                    "{} {} {} {} {} {}",
                    row.number,
                    row.rule_id,
                    row.prefixes.join(" "),
                    row.alphabet.name(),
                    row.min,
                    row.max
                )
            })
            .collect();
        let table: Vec<&str> = TABLE.lines().collect();
        assert_eq!(code, table);
    }

    fn bytes_named(name: &str) -> Vec<u8> {
        let mut bytes = Vec::new();
        for word in name.split(' ') {
            match word {
                "and" => {}
                "an" => bytes.extend(bytes_named("A-Z a-z 0-9")),
                "hex" => bytes.extend(bytes_named("0-9 a-f")),
                "HEX" => bytes.extend(bytes_named("0-9 a-f A-F")),
                "url" => bytes.extend(bytes_named("A-Z a-z 0-9 _ -")),
                range if range.len() == 3 && range.as_bytes()[1] == b'-' => {
                    let ends = range.as_bytes();
                    bytes.extend(ends[0]..=ends[2]);
                }
                list => bytes.extend(list.bytes()),
            }
        }
        bytes
    }

    #[test]
    fn each_alphabet_takes_the_bytes_that_the_table_names() {
        for row in &ROWS {
            let named = bytes_named(row.alphabet.name());
            for byte in 0..=u8::MAX {
                assert_eq!(
                    row.alphabet.holds(byte),
                    named.contains(&byte),
                    "row {} ({}), byte {byte:#04x}",
                    row.number,
                    row.alphabet.name()
                );
            }
        }
    }
}
