use halvo_secrets::Name;

#[test]
fn a_name_takes_lower_case_letters_digits_hyphens_and_inner_dots() {
    let longest = "a".repeat(64);
    for name in [
        "a",
        "0",
        "account",
        "credential-1",
        "dev.halvo.account",
        "a..b",
        "con1",
        "console",
        "nul-x",
        "com10",
        "x.con",
        longest.as_str(),
    ] {
        let Some(parsed) = Name::new(name) else {
            panic!("{name:?} was refused");
        };
        assert_eq!(parsed.as_str(), name);
    }
}

#[test]
fn a_name_refuses_every_other_form() {
    let too_long = "a".repeat(65);
    for name in [
        "",
        too_long.as_str(),
        "-a",
        "a-",
        ".a",
        "a.",
        "Account",
        "a b",
        "a/b",
        "a\\b",
        "a:b",
        "a_b",
        "a\0b",
        "a\nb",
        "a\rb",
        "a\tb",
        "a*",
        "a?",
        "a\"b",
        "a<b",
        "a>b",
        "a|b",
    ] {
        assert!(Name::new(name).is_none(), "{name:?} was taken");
    }
}

#[test]
fn a_name_refuses_a_device_name_of_windows_before_its_first_dot() {
    let mut devices = vec!["con", "prn", "aux", "nul"];
    let numbered: Vec<String> = (0..10)
        .flat_map(|digit| [format!("com{digit}"), format!("lpt{digit}")])
        .collect();
    devices.extend(numbered.iter().map(String::as_str));
    assert_eq!(devices.len(), 24);
    for device in devices {
        assert!(Name::new(device).is_none(), "{device:?} was taken");
        let with_dot = format!("{device}.x");
        assert!(Name::new(&with_dot).is_none(), "{with_dot:?} was taken");
    }
}

#[test]
fn a_name_refuses_a_character_outside_ascii_at_every_position() {
    let base = "account.r7";
    for position in 0..=base.len() {
        for odd in ['\u{80}', '\u{e9}', '\u{ff}', '\u{2010}', '\u{ff0e}'] {
            let mut name = String::new();
            name.push_str(&base[..position]);
            name.push(odd);
            name.push_str(&base[position..]);
            assert!(Name::new(&name).is_none(), "{name:?} was taken");
        }
    }
}
