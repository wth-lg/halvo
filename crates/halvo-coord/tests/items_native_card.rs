use halvo_coord::items::chars::{self, blank, columns, composed, mark as is_mark, no_form};

/// The program that made the tables of src/items/chars.rs from the doc line of `NO_FORM` to the
/// end of that file, run as `/usr/bin/python3 -I -c <GENERATOR>`.
#[cfg(target_os = "macos")]
const GENERATOR: &str = r#"import sys
import unicodedata as u

if u.unidata_version != "13.0.0":
    sys.exit("unicodedata " + u.unidata_version + ", not 13.0.0")


def ranges(pred):
    out = []
    start = None
    for cp in range(0x110000):
        hit = pred(chr(cp))
        if hit and start is None:
            start = cp
        if not hit and start is not None:
            out.append((start, cp - 1))
            start = None
    if start is not None:
        out.append((start, 0x10FFFF))
    return out


def table(doc, name, pred):
    lines = ["", doc, "const " + name + ": &[(u32, u32)] = &["]
    for lo, hi in ranges(pred):
        lines.append("    (0x%04X, 0x%04X)," % (lo, hi))
    lines.append("];")
    return lines


out = []
out += table(
    "/// The code points of the general categories Cc, Cf, Zl, Zp, Co, Cs and Cn of Unicode 13.0.0.",
    "NO_FORM",
    lambda c: u.category(c) in ("Cc", "Cf", "Zl", "Zp", "Co", "Cs", "Cn"),
)
out += table(
    "/// The code points of the general category Zs of Unicode 13.0.0.",
    "BLANKS",
    lambda c: u.category(c) == "Zs",
)
out += table(
    "/// The code points of the general categories Mn and Me of Unicode 13.0.0.",
    "MARKS",
    lambda c: u.category(c) in ("Mn", "Me"),
)
out += table(
    "/// The assigned code points whose East Asian width is W or F in Unicode 13.0.0.",
    "WIDE",
    lambda c: u.east_asian_width(c) in ("W", "F") and u.category(c) != "Cn",
)
sys.stdout.write("\n".join(out[1:]) + "\n")
"#;

#[cfg(target_os = "macos")]
#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test starts the system's Python 3.9, whose unicodedata is 13.0.0, to make the tables again"
)]
fn the_character_tables_are_those_of_unicodedata_13() {
    let out = std::process::Command::new("/usr/bin/python3")
        .args(["-I", "-c", GENERATOR])
        .output()
        .expect("/usr/bin/python3 runs");
    assert!(
        out.status.success(),
        "the generator: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let source =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/items/chars.rs"))
            .unwrap();
    let at = source
        .find("/// The code points of the general categories Cc,")
        .expect("the doc line of NO_FORM");
    assert!(
        &source.as_bytes()[at..] == out.stdout.as_slice(),
        "src/items/chars.rs differs from the generator's output"
    );
}

#[test]
fn the_tables_are_those_of_unicode_13_0_0() {
    assert_eq!(chars::UNICODE_VERSION, (13, 0, 0));
}

fn c(code: u32) -> char {
    char::from_u32(code).unwrap()
}

#[test]
fn each_range_of_the_closed_list_with_no_visible_form() {
    let lists: [(&str, &[u32]); 9] = [
        ("controls", &[0x0, 0x9, 0xA, 0xD, 0x1F, 0x7F, 0x85, 0x9F]),
        (
            "format",
            &[0xAD, 0x600, 0x61C, 0x180E, 0x200B, 0x2060, 0xFEFF, 0xE0001],
        ),
        ("joiners", &[0x200C, 0x200D]),
        (
            "direction marks",
            &[0x200E, 0x200F, 0x202A, 0x202E, 0x2066, 0x2069],
        ),
        ("line and paragraph separators", &[0x2028, 0x2029]),
        (
            "variation selectors",
            &[0x180B, 0x180D, 0xFE00, 0xFE0F, 0xE0100, 0xE01EF],
        ),
        (
            "shown as none or a blank",
            &[
                0x34F, 0x115F, 0x1160, 0x17B4, 0x17B5, 0x2800, 0x3164, 0xFFA0,
            ],
        ),
        ("private use", &[0xE000, 0xF8FF, 0xF0000, 0x10FFFD]),
        (
            "unassigned in 13.0.0",
            &[0x378, 0x870, 0x1FAE0, 0x1FAF0, 0x2B739, 0x10FFFF],
        ),
    ];
    for (list, codes) in lists {
        for &code in codes {
            assert!(no_form(c(code)), "{list}: U+{code:04X}");
        }
    }
    for code in [
        0x20, 0x41, 0x7E, 0xA0, 0xE9, 0x180A, 0x1810, 0x200A, 0x2030, 0x2070, 0x2074, 0x3000,
        0x4E00, 0xFDFC, 0xFE10, 0x1FAD6, 0x1F600, 0x1D11E, 0x1F9FF,
    ] {
        assert!(!no_form(c(code)), "U+{code:04X} has a form");
    }
}

#[test]
fn a_code_point_assigned_after_13_0_0_has_no_visible_form() {
    for code in [0x1FAE0, 0x1FAE8, 0x870, 0x1E290, 0x11F00] {
        assert!(no_form(c(code)), "U+{code:04X}");
        assert!(
            !composed(&format!("Ann{}Lee", c(code)), 40),
            "U+{code:04X} in a value"
        );
    }
}

#[test]
fn each_blank_of_the_closed_list() {
    let blanks = [
        0x20, 0xA0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008,
        0x2009, 0x200A, 0x202F, 0x205F, 0x3000,
    ];
    for code in 0..=0x10FFFF {
        let Some(ch) = char::from_u32(code) else {
            continue;
        };
        assert_eq!(blank(ch), blanks.contains(&code), "U+{code:04X}");
    }
}

#[test]
fn marks_take_no_column_and_wide_characters_take_two() {
    assert_eq!(columns("City Power"), 10);
    assert_eq!(columns(""), 0);
    assert!(is_mark('\u{301}') && is_mark('\u{20DD}') && !is_mark('e'));
    assert_eq!(columns("e\u{301}"), 1);
    assert_eq!(columns(&"\u{4e00}".repeat(120)), 240, "120 wide characters");
    assert_eq!(
        columns(&"e\u{301}".repeat(61)),
        61,
        "61 letters with a mark"
    );
    assert_eq!(columns("\u{3000}\u{ff21}\u{ac00}\u{1f600}"), 8);
    assert_eq!(
        columns("\u{ff61}\u{e9}"),
        2,
        "a half-width form and a letter"
    );
}

#[test]
fn the_predicate_of_a_deciding_field() {
    let yes = [
        "City Power",
        "42.10 USD",
        "Ann\u{a0}Lee",
        "Nguy\u{1ec5}n",
        "Nguye\u{302}\u{303}n",
        "a\u{301}\u{302}\u{303}",
        "\u{4e00}\u{4e8c}",
        "x",
        "",
    ];
    for value in yes {
        assert!(composed(value, 40), "{value:?}");
    }
    let no = [
        "City  Power",
        " City",
        "City ",
        "City\u{a0} Power",
        "City\u{3000}\u{3000}Power",
        "City\u{200b}Power",
        "City\nPower",
        "City\u{2028}",
        "Ann\u{200d}Lee",
        "\u{202e}rewoP",
        "\u{301}a",
        "a\u{301}\u{302}\u{303}\u{304}",
        "Pay\u{fe0f}",
        "\u{3164}",
        " ",
    ];
    for value in no {
        assert!(!composed(value, 40), "{value:?}");
    }
    assert!(composed(&"W".repeat(40), 40));
    assert!(!composed(&"W".repeat(41), 40));
    assert!(composed(&"\u{4e00}".repeat(20), 40));
    assert!(!composed(&"\u{4e00}".repeat(21), 40));
    assert!(composed(&"e\u{301}".repeat(40), 40));
    assert!(!composed(&"e\u{301}".repeat(41), 40));
}
