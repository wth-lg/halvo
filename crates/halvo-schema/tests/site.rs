mod common;

use common::{assert_strict, refused, round_trip};
use halvo_schema::{
    AppId, Domain, ObservedSite, ParseError, SiteDomain, SiteHash, SiteToken, SiteValue,
};

const HASH: &str = "9f2c4b7a1d3e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5061728394a5b6c7d8e";

fn rule<T: std::fmt::Debug>(result: Result<T, ParseError>) -> &'static str {
    result.expect_err("must be refused").rule()
}

#[test]
fn an_app_id_is_a_bundle_id_or_a_file_name_kept_as_given() {
    let e_acute = "\u{e9}";
    for ok in [
        "com.apple.Safari",
        "chrome.exe",
        "a",
        "Microsoft Edge",
        " padded ",
        "app\u{e9}",
        "\u{a0}",
        "...",
        ".hidden",
        &"x".repeat(255),
        &format!("{}a", e_acute.repeat(127)),
    ] {
        let id = AppId::parse(ok).unwrap();
        assert_eq!(id.as_str(), ok);
        assert_eq!(id.to_string(), ok);
    }
    assert_eq!(rule(AppId::parse("")), "length");
    assert_eq!(rule(AppId::parse(&"x".repeat(256))), "length");
    assert_eq!(rule(AppId::parse(&e_acute.repeat(128))), "length");
    assert_eq!(rule(AppId::parse("/usr/bin/vim")), "separator");
    assert_eq!(rule(AppId::parse("a\\b")), "separator");
    assert_eq!(rule(AppId::parse("a\tb")), "control");
    assert_eq!(rule(AppId::parse("a\u{1f}")), "control");
    assert_eq!(rule(AppId::parse("a\u{7f}")), "control");
    assert_eq!(rule(AppId::parse("\0")), "control");
    assert_eq!(rule(AppId::parse(".")), "dot_name");
    assert_eq!(rule(AppId::parse("..")), "dot_name");
    assert_eq!(AppId::parse("..").unwrap_err().type_name(), "AppId");
    refused::<AppId>("\"/usr/bin/vim\"");
    refused::<AppId>("\"C:\\\\Windows\\\\notepad.exe\"");
    assert_eq!(
        serde_json::to_string(&AppId::parse("say \"hi\"").unwrap()).unwrap(),
        r#""say \"hi\"""#
    );
}

#[test]
fn from_capture_cuts_a_path_to_its_file_name() {
    let cases = [
        ("/usr/bin/vim", Some("vim")),
        (
            "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
            Some("chrome.exe"),
        ),
        ("com.apple.Safari", Some("com.apple.Safari")),
        ("a\\b/c", Some("c")),
        ("a/b\\c", Some("c")),
        ("/usr/bin/", None),
        ("a/..", None),
        ("dir/.", None),
        ("/x/a\tb", None),
        ("", None),
    ];
    for (input, out) in cases {
        let got = AppId::from_capture(input);
        assert_eq!(got.as_ref().ok().map(AppId::as_str), out, "{input:?}");
        if out.is_none() {
            assert_eq!(got.unwrap_err().type_name(), "AppId");
        }
    }
}

#[test]
fn a_domain_is_a_registrable_domain_in_one_written_form() {
    for ok in [
        "shop.test",
        "example.co.uk",
        "www.ck",
        "b.test.ck",
        "user.github.io",
        "a.0xg",
        "a.1x",
        "xn--mnchen-3ya.de",
        "northwind-power.test",
        "a-b.c-d",
    ] {
        let d = Domain::parse(ok).unwrap();
        assert_eq!(d.as_str(), ok);
        assert_eq!(d.to_string(), ok);
    }
    let cases = [
        ("", "ascii_length"),
        ("caf\u{e9}.test", "ascii_length"),
        (".shop.test", "label_join"),
        ("shop..test", "label_join"),
        ("shop.test.", "label_join"),
        (".", "label_join"),
        ("Shop.test", "label_form"),
        ("-shop.test", "label_form"),
        ("shop-.test", "label_form"),
        ("sh_op.test", "label_form"),
        ("[2001:db8::1]", "label_form"),
        ("pay your bill here", "label_form"),
        ("192.0.2.1", "numeric_last_label"),
        ("0x7f.0x1", "numeric_last_label"),
        ("a.0x", "numeric_last_label"),
        ("a.0xff", "numeric_last_label"),
        ("shop.123", "numeric_last_label"),
        ("1", "numeric_last_label"),
        ("test", "public_suffix"),
        ("co.uk", "public_suffix"),
        ("www.shop.test", "public_suffix"),
        ("test.ck", "public_suffix"),
        ("ck", "public_suffix"),
        ("github.io", "public_suffix"),
        ("a.b.example.co.uk", "public_suffix"),
    ];
    for (input, want) in cases {
        assert_eq!(rule(Domain::parse(input)), want, "{input:?}");
    }
    let label63 = "a".repeat(63);
    assert_eq!(
        rule(Domain::parse(&format!("{label63}a.test"))),
        "label_form"
    );
    assert_eq!(
        Domain::parse(&format!("{label63}.test"))
            .unwrap()
            .as_str()
            .len(),
        68
    );
    let name253 = format!("{label63}.{label63}.{label63}.{}", "b".repeat(61));
    let name254 = format!("{label63}.{label63}.{label63}.{}", "b".repeat(62));
    assert_eq!(name253.len(), 253);
    assert_eq!(rule(Domain::parse(&name253)), "public_suffix");
    assert_eq!(rule(Domain::parse(&name254)), "ascii_length");
    refused::<Domain>("\"pay your bill here\"");
    refused::<Domain>("\"www.shop.test\"");
    assert_eq!(
        serde_json::from_str::<Domain>("\"shop.test\"")
            .unwrap()
            .as_str(),
        "shop.test"
    );
}

#[test]
fn registrable_cuts_a_host_name_to_its_registrable_domain() {
    let label63 = "a".repeat(63);
    let name253 = format!("{label63}.{label63}.{label63}.{}", "b".repeat(61));
    let name254 = format!("{label63}.{label63}.{label63}.{}", "b".repeat(62));
    let cut253 = format!("{label63}.{}", "b".repeat(61));
    let cases: Vec<(&str, Option<&str>)> = vec![
        ("www.shop.test", Some("shop.test")),
        ("WWW.Shop.Test.", Some("shop.test")),
        ("SHOP.TEST", Some("shop.test")),
        ("a.b.example.co.uk", Some("example.co.uk")),
        ("www.www.ck", Some("www.ck")),
        ("a.b.test.ck", Some("b.test.ck")),
        ("pages.user.github.io", Some("user.github.io")),
        ("shop.test", Some("shop.test")),
        (&name253, Some(&cut253)),
        (&name254, None),
        ("localhost", None),
        ("192.0.2.1", None),
        ("co.uk", None),
        ("test", None),
        ("test.ck", None),
        ("shop.test..", None),
        ("", None),
        (".", None),
        ("m\u{fc}nchen.de", None),
        ("http://shop.test", None),
        ("shop.test:443", None),
        ("0x7f.0x1", None),
    ];
    for (host, want) in cases {
        let got = Domain::registrable(host);
        assert_eq!(got.as_ref().map(Domain::as_str), want, "{host:?}");
    }
}

#[test]
fn a_site_token_is_up_to_64_bytes_and_its_debug_hides_it() {
    for ok in ["northwind-power", "a", "a.b-c", "0", &"z".repeat(64)] {
        let t = SiteToken::parse(ok).unwrap();
        assert_eq!(t.as_str(), ok);
        assert_eq!(t.to_string(), ok);
        assert_eq!(format!("{t:?}"), "SiteToken");
    }
    assert_eq!(rule(SiteToken::parse("")), "length");
    assert_eq!(rule(SiteToken::parse(&"z".repeat(65))), "length");
    assert_eq!(rule(SiteToken::parse("Northwind Power")), "character");
    assert_eq!(rule(SiteToken::parse("a_b")), "character");
    let wrapped = Some(SiteToken::parse("northwind-power").unwrap());
    assert!(!format!("{wrapped:?}").contains("northwind"));
}

#[test]
fn a_site_hash_is_64_lower_case_hex_digits() {
    assert_eq!(SiteHash::parse(HASH).unwrap().as_str(), HASH);
    assert_eq!(rule(SiteHash::parse(&HASH[1..])), "length");
    assert_eq!(rule(SiteHash::parse(&HASH.to_uppercase())), "hex_digit");
    assert_eq!(rule(SiteHash::parse(&HASH.replace('9', "g"))), "hex_digit");
}

#[test]
fn site_forms() {
    let address = r#"{"key":"northwind-power.test","source":"address_field"}"#;
    let sign_in = r#"{"key":"northwind-power.test","source":"sign_in"}"#;
    let title = format!(r#"{{"key":"{HASH}","source":"title"}}"#);
    let observed_address = r#"{"domain":"northwind-power.test","source":"address_field"}"#;
    let observed_title = r#"{"token":"northwind-power","source":"title"}"#;

    assert_strict::<SiteValue>(address);
    assert_strict::<SiteValue>(sign_in);
    assert_strict::<SiteValue>(&title);
    assert_strict::<ObservedSite>(observed_address);
    assert_strict::<ObservedSite>(observed_title);

    let domain = Domain::parse("northwind-power.test").unwrap();
    assert_eq!(
        round_trip::<SiteValue>(address),
        SiteValue::Domain {
            key: SiteDomain::from(domain.clone()),
            source: halvo_schema::DomainSource::AddressField
        }
    );
    assert_eq!(
        round_trip::<SiteValue>(sign_in),
        SiteValue::Domain {
            key: SiteDomain::from(domain.clone()),
            source: halvo_schema::DomainSource::SignIn
        }
    );
    assert_eq!(
        round_trip::<SiteValue>(&title),
        SiteValue::Title {
            key: SiteHash::parse(HASH).unwrap()
        }
    );
    assert_eq!(
        round_trip::<ObservedSite>(observed_address),
        ObservedSite::AddressField { domain }
    );
    assert_eq!(
        round_trip::<ObservedSite>(observed_title),
        ObservedSite::Title {
            token: SiteToken::parse("northwind-power").unwrap()
        }
    );

    for bad_hash in [&HASH[1..], &HASH.to_uppercase(), "northwind-power"] {
        refused::<SiteValue>(&format!(r#"{{"key":"{bad_hash}","source":"title"}}"#));
    }
    refused::<SiteValue>(r#"{"key":"northwind-power.test","source":"title"}"#);
    refused::<SiteValue>(r#"{"key":"northwind-power","source":"title"}"#);
    refused::<SiteValue>(&format!(r#"{{"key":"{HASH}","source":"address_field"}}"#));
    refused::<SiteValue>(&format!(r#"{{"key":"{HASH}","source":"sign_in"}}"#));
    refused::<SiteValue>(r#"{"key":"northwind-power","source":"address_field"}"#);
    refused::<SiteValue>(r#"{"key":"northwind-power.test","source":"sign-in"}"#);
    refused::<SiteValue>(r#"{"key":"northwind-power.test","source":null}"#);
    refused::<SiteValue>(observed_address);
    refused::<SiteValue>(observed_title);

    refused::<ObservedSite>(address);
    refused::<ObservedSite>(&title);
    refused::<ObservedSite>(r#"{"domain":"northwind-power.test","source":"title"}"#);
    refused::<ObservedSite>(r#"{"token":"northwind-power","source":"address_field"}"#);
    refused::<ObservedSite>(r#"{"domain":"northwind-power.test","source":"sign_in"}"#);
    refused::<ObservedSite>(r#"{"token":"Northwind Power","source":"title"}"#);
    refused::<ObservedSite>(
        r#"{"token":"northwind-power","domain":"northwind-power.test","source":"title"}"#,
    );
}

#[test]
fn site_values_are_clone_eq_and_hash() {
    let a: SiteValue = common::read(r#"{"key":"shop.test","source":"address_field"}"#);
    let b: SiteValue = common::read(r#"{"key":"shop.test","source":"sign_in"}"#);
    assert_ne!(a, b);
    let set: std::collections::HashSet<SiteValue> = [a.clone(), a, b].into_iter().collect();
    assert_eq!(set.len(), 2);
    let o: ObservedSite = common::read(r#"{"token":"shop","source":"title"}"#);
    let set: std::collections::HashSet<ObservedSite> = [o.clone(), o].into_iter().collect();
    assert_eq!(set.len(), 1);
    assert!(!format!(
        "{:?}",
        common::read::<ObservedSite>(r#"{"token":"shop","source":"title"}"#)
    )
    .contains("shop"));
}

#[test]
fn a_domain_has_exactly_one_label_more_than_its_public_suffix() {
    assert_eq!(rule(Domain::parse("b.example.com")), "public_suffix");
    assert_eq!(rule(Domain::parse("a.b.example.com")), "public_suffix");
    assert_eq!(
        Domain::parse("example.com").unwrap().as_str(),
        "example.com"
    );
}

#[test]
fn a_site_domain_holds_rules_1_to_4_and_two_labels_and_not_rule_5() {
    for ok in [
        "github.io",
        "www.shop.test",
        "a.b.example.co.uk",
        "shop.test",
        "a.1x",
    ] {
        assert_eq!(SiteDomain::parse(ok).unwrap().as_str(), ok);
        assert_eq!(SiteDomain::parse(ok).unwrap().to_string(), ok);
    }
    for (input, want) in [
        ("northwind-power", "two_labels"),
        ("test", "two_labels"),
        ("", "ascii_length"),
        ("caf\u{e9}.test", "ascii_length"),
        ("shop..test", "label_join"),
        ("Shop.test", "label_form"),
        ("192.0.2.1", "numeric_last_label"),
    ] {
        assert_eq!(rule(SiteDomain::parse(input)), want, "{input:?}");
    }
    let domain = Domain::registrable("Pages.User.GitHub.io.").unwrap();
    assert_eq!(domain.as_str(), "user.github.io");
    assert_eq!(SiteDomain::from(domain).as_str(), "user.github.io");
    assert_eq!(Domain::registrable("github.io"), None);
    assert_eq!(rule(Domain::parse("github.io")), "public_suffix");
    let a = SiteDomain::parse("a.test").unwrap();
    let b = SiteDomain::parse("b.test").unwrap();
    assert!(a < b);
    let set: std::collections::HashSet<SiteDomain> = [a.clone(), a, b].into_iter().collect();
    assert_eq!(set.len(), 2);
}

#[test]
fn a_stored_site_value_keeps_a_key_that_the_suffix_list_makes_a_suffix() {
    for key in ["github.io", "www.northwind-power.test"] {
        for source in ["address_field", "sign_in"] {
            assert_strict::<SiteValue>(&format!(r#"{{"key":"{key}","source":"{source}"}}"#));
        }
        refused::<ObservedSite>(&format!(r#"{{"domain":"{key}","source":"address_field"}}"#));
    }
}

#[test]
fn a_site_value_is_refused_as_an_array() {
    common::refused_as_array::<SiteValue>(r#"["address_field","northwind-power.test"]"#);
    refused::<SiteValue>(r#"["sign_in","northwind-power.test"]"#);
    refused::<SiteValue>(&format!(r#"["title","{HASH}"]"#));
    refused::<SiteValue>(r#"["northwind-power.test","address_field"]"#);
}

#[test]
fn an_observed_site_is_refused_as_an_array() {
    common::refused_as_array::<ObservedSite>(r#"["title","northwind-power"]"#);
    refused::<ObservedSite>(r#"["address_field","northwind-power.test"]"#);
    refused::<ObservedSite>(r#"["northwind-power","title"]"#);
}

#[test]
fn an_app_id_refuses_every_character_of_the_unicode_category_cc_and_keeps_u_00a0() {
    for c in ['\u{85}', '\u{9b}', '\u{80}', '\u{9f}'] {
        let id = format!("com.example{c}app");
        assert_eq!(rule(AppId::parse(&id)), "control", "{c:?}");
        assert_eq!(
            rule(AppId::from_capture(&format!("C:\\Apps\\{id}.exe"))),
            "control",
            "{c:?}"
        );
        refused::<AppId>(&serde_json::to_string(&id).unwrap());
    }
    for c in ['\u{a0}', '\u{7e}', '\u{a1}', '\u{ad}'] {
        let id = format!("com.example{c}app");
        assert_eq!(AppId::parse(&id).unwrap().as_str(), id, "{c:?}");
    }
    let cc: Vec<char> = (0..=0x10ffff)
        .filter_map(char::from_u32)
        .filter(|c| c.is_control())
        .collect();
    assert_eq!(cc.len(), 65);
    for c in cc {
        assert_eq!(rule(AppId::parse(&format!("a{c}b"))), "control", "{c:?}");
    }
}
