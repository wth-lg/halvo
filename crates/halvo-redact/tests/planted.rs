mod support;

use std::time::Instant;

use halvo_redact::mask;
use support::{grouped, iban, luhn_number};

const KEY_PREFIXES: [(&str, char, usize); 72] = [
    ("AKIA", 'Q', 16),
    ("ASIA", 'Q', 16),
    ("ABIA", 'Q', 16),
    ("ACCA", 'Q', 16),
    ("A3TA", 'Q', 16),
    ("A3TB", 'Q', 16),
    ("A3TC", 'Q', 16),
    ("A3TD", 'Q', 16),
    ("A3TE", 'Q', 16),
    ("A3TF", 'Q', 16),
    ("A3TG", 'Q', 16),
    ("A3TH", 'Q', 16),
    ("A3TI", 'Q', 16),
    ("A3TJ", 'Q', 16),
    ("A3TK", 'Q', 16),
    ("A3TL", 'Q', 16),
    ("A3TM", 'Q', 16),
    ("A3TN", 'Q', 16),
    ("A3TO", 'Q', 16),
    ("A3TP", 'Q', 16),
    ("A3TQ", 'Q', 16),
    ("A3TR", 'Q', 16),
    ("A3TS", 'Q', 16),
    ("A3TT", 'Q', 16),
    ("A3TU", 'Q', 16),
    ("A3TV", 'Q', 16),
    ("A3TW", 'Q', 16),
    ("A3TX", 'Q', 16),
    ("A3TY", 'Q', 16),
    ("A3TZ", 'Q', 16),
    ("A3T0", 'Q', 16),
    ("A3T1", 'Q', 16),
    ("A3T2", 'Q', 16),
    ("A3T3", 'Q', 16),
    ("A3T4", 'Q', 16),
    ("A3T5", 'Q', 16),
    ("A3T6", 'Q', 16),
    ("A3T7", 'Q', 16),
    ("A3T8", 'Q', 16),
    ("A3T9", 'Q', 16),
    ("ghp_", 'a', 36),
    ("gho_", 'a', 36),
    ("ghu_", 'a', 36),
    ("ghs_", 'a', 36),
    ("ghr_", 'a', 36),
    ("github_pat_", 'a', 82),
    ("glpat-", 'a', 20),
    ("glptt-", 'a', 40),
    ("GR1348941", 'a', 20),
    ("sk_test_", 'a', 10),
    ("sk_live_", 'a', 10),
    ("sk_prod_", 'a', 10),
    ("rk_test_", 'a', 10),
    ("rk_live_", 'a', 10),
    ("rk_prod_", 'a', 10),
    ("SG.", 'a', 66),
    ("pypi-AgEIcHlwaS5vcmc", 'a', 50),
    ("AIza", 'a', 35),
    ("dop_v1_", 'a', 64),
    ("doo_v1_", 'a', 64),
    ("dp.pt.", 'a', 43),
    ("shpat_", 'a', 32),
    ("shpca_", 'a', 32),
    ("shppa_", 'a', 32),
    ("shpss_", 'a', 32),
    ("pul-", 'a', 40),
    ("lin_api_", 'a', 40),
    ("AGE-SECRET-KEY-1", 'Q', 58),
    ("hf_", 'a', 34),
    ("rubygems_", 'a', 48),
    ("dapi", 'a', 32),
    ("SK", 'a', 32),
];

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
    for (prefix, body, min) in KEY_PREFIXES {
        planted.push(Planted {
            kind: format!("key with the prefix {prefix}"),
            identifier: format!("{prefix}{}", body.to_string().repeat(min)),
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
