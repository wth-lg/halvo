mod support;

use std::time::Instant;

use halvo_redact::mask;
use support::{grouped, iban, luhn_number};

const KEY_PREFIXES: [(&str, &str, usize); 72] = [
    ("AKIA", "Q2", 16),
    ("ASIA", "Q2", 16),
    ("ABIA", "Q2", 16),
    ("ACCA", "Q2", 16),
    ("A3TA", "Q2", 16),
    ("A3TB", "Q2", 16),
    ("A3TC", "Q2", 16),
    ("A3TD", "Q2", 16),
    ("A3TE", "Q2", 16),
    ("A3TF", "Q2", 16),
    ("A3TG", "Q2", 16),
    ("A3TH", "Q2", 16),
    ("A3TI", "Q2", 16),
    ("A3TJ", "Q2", 16),
    ("A3TK", "Q2", 16),
    ("A3TL", "Q2", 16),
    ("A3TM", "Q2", 16),
    ("A3TN", "Q2", 16),
    ("A3TO", "Q2", 16),
    ("A3TP", "Q2", 16),
    ("A3TQ", "Q2", 16),
    ("A3TR", "Q2", 16),
    ("A3TS", "Q2", 16),
    ("A3TT", "Q2", 16),
    ("A3TU", "Q2", 16),
    ("A3TV", "Q2", 16),
    ("A3TW", "Q2", 16),
    ("A3TX", "Q2", 16),
    ("A3TY", "Q2", 16),
    ("A3TZ", "Q2", 16),
    ("A3T0", "Q2", 16),
    ("A3T1", "Q2", 16),
    ("A3T2", "Q2", 16),
    ("A3T3", "Q2", 16),
    ("A3T4", "Q2", 16),
    ("A3T5", "Q2", 16),
    ("A3T6", "Q2", 16),
    ("A3T7", "Q2", 16),
    ("A3T8", "Q2", 16),
    ("A3T9", "Q2", 16),
    ("ghp_", "Ab3", 36),
    ("gho_", "Ab3", 36),
    ("ghu_", "Ab3", 36),
    ("ghs_", "Ab3", 36),
    ("ghr_", "Ab3", 36),
    ("github_pat_", "Ab3_", 82),
    ("glpat-", "Ab3_-", 20),
    ("glptt-", "b3", 40),
    ("GR1348941", "Ab3_-", 20),
    ("sk_test_", "Ab3", 10),
    ("sk_live_", "Ab3", 10),
    ("sk_prod_", "Ab3", 10),
    ("rk_test_", "Ab3", 10),
    ("rk_live_", "Ab3", 10),
    ("rk_prod_", "Ab3", 10),
    ("SG.", "Ab3=_-.", 66),
    ("pypi-AgEIcHlwaS5vcmc", "Ab3_-", 50),
    ("AIza", "Ab3_-", 35),
    ("dop_v1_", "b3", 64),
    ("doo_v1_", "b3", 64),
    ("dp.pt.", "Ab3", 43),
    ("shpat_", "Bb3", 32),
    ("shpca_", "Bb3", 32),
    ("shppa_", "Bb3", 32),
    ("shpss_", "Bb3", 32),
    ("pul-", "b3", 40),
    ("lin_api_", "Ab3", 40),
    ("AGE-SECRET-KEY-1", "Q9", 58),
    ("hf_", "Ab", 34),
    ("rubygems_", "b3", 48),
    ("dapi", "b3", 32),
    ("SK", "Bb3", 32),
];

fn body(pattern: &str, length: usize) -> String {
    pattern.chars().cycle().take(length).collect()
}

struct Planted {
    kind: String,
    identifier: String,
    token: &'static str,
}

fn identifiers() -> Vec<Planted> {
    let mut planted = Vec::new();
    for digits in [13, 16, 19] {
        let body: String = "1234567890".chars().cycle().take(digits - 1).collect();
        let number = luhn_number(&body);
        for (form, written) in [
            ("plain", number.clone()),
            ("spaces", grouped(&number, ' ')),
            ("hyphens", grouped(&number, '-')),
        ] {
            planted.push(Planted {
                kind: format!("card of {digits} digits, {form}"),
                identifier: written,
                token: "[card]",
            });
        }
    }
    let account = iban("DE", "123456781234567890");
    for (form, written) in [
        ("plain", account.clone()),
        ("groups of four", grouped(&account, ' ')),
    ] {
        planted.push(Planted {
            kind: format!("iban, {form}"),
            identifier: written,
            token: "[iban]",
        });
    }
    planted.push(Planted {
        kind: "ssn".to_owned(),
        identifier: "123-45-6789".to_owned(),
        token: "[ssn]",
    });
    for (prefix, pattern, min) in KEY_PREFIXES {
        planted.push(Planted {
            kind: format!("key with the prefix {prefix}"),
            identifier: format!("{prefix}{}", body(pattern, min)),
            token: "[key]",
        });
    }
    planted
}

#[derive(Clone, Copy, Debug)]
enum Place {
    Start,
    Middle,
    End,
}

impl Place {
    fn title(self, text: &str) -> String {
        match self {
            Place::Start => format!("{text} paid today"),
            Place::Middle => format!("bill {text} paid today"),
            Place::End => format!("bill paid {text}"),
        }
    }
}

#[test]
fn planted_identifiers_are_masked() {
    let mut titles = 0u128;
    let mut nanos = 0u128;
    let mut missed = Vec::new();
    for planted in identifiers() {
        for place in [Place::Start, Place::Middle, Place::End] {
            let title = place.title(&planted.identifier);
            let want = place.title(planted.token);
            let started = Instant::now();
            let masked = mask(&title);
            nanos += started.elapsed().as_nanos();
            titles += 1;
            if masked != want || mask(&masked) != masked {
                missed.push(format!("{} at the {place:?}", planted.kind));
            }
        }
    }
    println!("mask titles={titles} ns_per_title={}", nanos / titles);
    assert!(
        missed.is_empty(),
        "{} of {titles} titles not masked as planted: {}",
        missed.len(),
        missed.join("; ")
    );
}
