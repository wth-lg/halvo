#[path = "coordinator_support.rs"]
mod coord;
#[path = "items_support.rs"]
mod support;

use coord::{agent, calls_since, coordinator, mark, Both, MANUAL};
use halvo_coord::guard::Table;
use halvo_coord::items::chars::{
    self, blank, columns, combining, composed, mark as is_mark, marks_in_a_row, no_form, MOST_MARKS,
};
use halvo_coord::items::{
    admits, bound_field, class_fields, compose_confirm, confirm_step, native_lines, value_bound,
    ConfirmAnswer, ConfirmMove, Marking, CARD_COLUMNS, CARD_LINES,
};
use halvo_coord::mem::Call;
use halvo_coord::types::{
    Action, AskedEffect, BoundField, BoundFrom, Cause, Choice, ChoiceText, Class, ConfirmAsk,
    Field, FieldFrom, HandbackAsk, HandbackReason, Item, ItemId, ItemKind, Limits, Origin,
    Presenter, RunEnd, RunEvent, RunId, What, YesNo,
};
use halvo_coord::{words, Coordinator};
use support::{ask_of, digest, effect_id, field, fields_of, item_id};

const DEAD_ITEM: &str = "000000000000000000000000000000ff";

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
    "/// The code points of the general category Mc of Unicode 13.0.0.",
    "SPACING_MARKS",
    lambda c: u.category(c) == "Mc",
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
        "",
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

const ROWS: [(Class, &str, &[BoundFrom], Marking); 16] = {
    use BoundFrom::*;
    use Marking::*;
    [
        (Class::Payment, "payee", &[Slot], Deciding),
        (Class::Payment, "amount", &[Extract], Deciding),
        (Class::Payment, "instrument", &[Slot], Deciding),
        (Class::Payment, "origin", &[Const], Deciding),
        (Class::Purchase, "seller", &[Slot], Deciding),
        (Class::Purchase, "items", &[Extract], Deciding),
        (Class::Purchase, "amount", &[Extract], Deciding),
        (Class::Purchase, "instrument", &[Slot], Deciding),
        (Class::Purchase, "origin", &[Const], Deciding),
        (Class::Send, "to", &[Extract], Deciding),
        (Class::Send, "subject", &[Extract], FreeText),
        (Class::Send, "body", &[Model, Const], FreeText),
        (Class::Delete, "items", &[Extract], Deciding),
        (Class::Submit, "origin", &[Const], Deciding),
        (Class::Submit, "fields", &[Const, Slot], FreeText),
        (Class::Sign, "document", &[Slot], Deciding),
    ]
};

const SOURCES: [BoundFrom; 4] = [
    BoundFrom::Const,
    BoundFrom::Slot,
    BoundFrom::Extract,
    BoundFrom::Model,
];

fn rows() -> Vec<(Class, &'static str, &'static [BoundFrom], Marking)> {
    let mut rows = ROWS.to_vec();
    rows.push((Class::Sign, "party", &[BoundFrom::Slot], Marking::Deciding));
    rows
}

#[test]
fn the_table_of_each_field_of_i4_3() {
    for (class, name, sources, marking) in rows() {
        assert_eq!(
            bound_field(class, name),
            Some((sources, marking)),
            "{class:?} {name}"
        );
    }
    for (class, name) in [
        (Class::Payment, "note"),
        (Class::Payment, "seller"),
        (Class::Send, "cc"),
        (Class::Delete, "origin"),
        (Class::LocalEdit, "file"),
        (Class::Read, "origin"),
    ] {
        assert_eq!(bound_field(class, name), None, "{class:?} {name}");
    }
}

fn one(class: Class, name: &str, from: BoundFrom, value: &str) -> ConfirmAsk {
    let mut fields = fields_of(class);
    for f in &mut fields {
        if f.name.as_str() == name {
            *f = field(name, from, value);
        }
    }
    ask_of(1, 1, vec![effect(1, class, fields)])
}

fn effect(n: u32, class: Class, fields: Vec<BoundField>) -> AskedEffect {
    AskedEffect {
        effect: effect_id(n),
        class,
        digest: digest(n),
        digest_ok: true,
        fields,
        new_origin: None,
    }
}

fn line_of(class: Class, name: &str, from: BoundFrom, value: &str) -> String {
    words::text(
        "confirm.field",
        &[
            (
                "label",
                &words::text(&format!("field.{}.{name}", class.name()), &[]).unwrap(),
            ),
            (
                "source",
                &words::text(
                    &format!("from.{}", halvo_coord::items::field_from(from).name()),
                    &[],
                )
                .unwrap(),
            ),
            ("value", value),
        ],
    )
    .unwrap()
}

#[test]
fn each_deciding_field_is_refused_one_column_over_its_bound_and_shown_at_it() {
    let table = Table::new();
    for (class, name, sources, marking) in rows() {
        if marking != Marking::Deciding {
            continue;
        }
        let from = sources[0];
        let most = value_bound(class, name, from).unwrap();
        assert_eq!(
            columns(&line_of(class, name, from, &"W".repeat(most))),
            CARD_COLUMNS,
            "{class:?} {name}: the line at the bound fills the card's columns"
        );
        let at = one(class, name, from, &"W".repeat(most));
        assert!(admits(&at, &table), "{class:?} {name} at {most}");
        let over = one(class, name, from, &"W".repeat(most + 1));
        assert!(!admits(&over, &table), "{class:?} {name} at {}", most + 1);
        let wide = one(class, name, from, &"\u{4e00}".repeat(most / 2 + 1));
        assert!(!admits(&wide, &table), "{class:?} {name} wide");
        let marked = one(class, name, from, &"e\u{301}".repeat(most));
        assert!(admits(&marked, &table), "{class:?} {name} marked");
        for value in ["Ann  Lee", "Ann\u{200b}Lee", " Ann", "Ann\nLee", ""] {
            assert!(
                !admits(&one(class, name, from, value), &table),
                "{class:?} {name} {value:?}"
            );
        }
        assert!(
            admits(&one(class, name, from, "Ann Lee"), &table),
            "{class:?} {name}"
        );
    }
}

#[test]
fn a_field_whose_source_a_9_3_does_not_admit_is_refused_at_the_raise() {
    let table = Table::new();
    for (class, name, sources, _) in rows() {
        for from in SOURCES {
            let ask = one(class, name, from, "Ann Lee");
            assert_eq!(
                admits(&ask, &table),
                sources.contains(&from),
                "{class:?} {name} from {from:?}"
            );
        }
    }
    let mut fields = fields_of(Class::Payment);
    fields.push(field("note", BoundFrom::Model, "Pay me"));
    assert!(!admits(
        &ask_of(1, 1, vec![effect(1, Class::Payment, fields)]),
        &table
    ));
    let mut fields = fields_of(Class::Send);
    fields.push(field("cc", BoundFrom::Extract, "Ann Lee"));
    assert!(
        !admits(&ask_of(1, 1, vec![effect(1, Class::Send, fields)]), &table),
        "a field that A-A9.3 does not bind for a page class"
    );
}

#[test]
fn free_text_is_whole_on_the_page_and_refused_in_the_native_dialog_where_it_breaks_a_line() {
    let table = Table::new();
    for (class, name, from) in [
        (Class::Send, "subject", BoundFrom::Extract),
        (Class::Send, "body", BoundFrom::Model),
        (Class::Submit, "fields", BoundFrom::Slot),
    ] {
        for value in [
            "Line one\n\nLine  two ",
            " \u{200b}x",
            &"W".repeat(CARD_COLUMNS * 4),
        ] {
            assert!(
                admits(&one(class, name, from, value), &table),
                "{class:?} {name} {value:?} on the page"
            );
        }
        let native = |value: &str| {
            let mut fields = fields_of(class);
            for f in &mut fields {
                if f.name.as_str() == name {
                    *f = field(name, from, value);
                }
            }
            ask_of(
                1,
                1,
                vec![
                    effect(1, class, fields),
                    effect(2, Class::Sign, fields_of(Class::Sign)),
                ],
            )
        };
        let most = value_bound(class, name, from).unwrap();
        assert!(admits(&native("Two  blanks"), &table), "{class:?} {name}");
        assert!(
            admits(&native(&"W".repeat(most)), &table),
            "{class:?} {name}"
        );
        for value in [
            "Line one\nLine two".to_owned(),
            "Tab\there".to_owned(),
            "Ann\u{200d}Lee".to_owned(),
            "W".repeat(most + 1),
        ] {
            assert!(
                !admits(&native(&value), &table),
                "{class:?} {name} {value:?} in the native dialog"
            );
        }
    }
}

#[test]
fn a_native_card_over_its_lines_is_refused_and_one_at_them_is_shown() {
    let table = Table::new();
    let payments = |n: u32| {
        ask_of(
            1,
            1,
            (1..=n)
                .map(|k| effect(k, Class::Payment, fields_of(Class::Payment)))
                .collect(),
        )
    };
    let lines = |n: usize| 2 * 4 * n - 1;
    let fit = (1..)
        .take_while(|n| lines(*n) <= CARD_LINES)
        .last()
        .unwrap();
    assert!(admits(&payments(fit as u32), &table), "{fit} payments");
    assert!(
        !admits(&payments(fit as u32 + 1), &table),
        "{} payments",
        fit + 1
    );
    let sends = ask_of(
        1,
        1,
        (1..=8)
            .map(|k| effect(k, Class::Send, fields_of(Class::Send)))
            .collect(),
    );
    assert!(admits(&sends, &table), "the page has no bound in lines");
}

fn halvo(label: &str) -> Field {
    Field {
        label: label.into(),
        value: vec![],
        from: FieldFrom::Halvo,
        from_text: words::text("from.halvo", &[]).unwrap(),
    }
}

#[test]
fn the_lines_of_a_native_card_are_paragraphs_with_label_and_source_first() {
    let field = |label: &str, value: &[&str], key: &str| Field {
        label: label.into(),
        value: value.iter().map(|p| p.to_string()).collect(),
        from: FieldFrom::Site,
        from_text: words::text(key, &[]).unwrap(),
    };
    assert_eq!(
        native_lines(&[
            halvo("It will make this payment"),
            field("Amount", &["42.", "10 USD"], "from.site"),
            field("Pay to", &["City Power"], "from.you"),
        ]),
        Ok(vec![
            "It will make this payment".to_owned(),
            String::new(),
            "Amount (From the site): 42.10 USD".to_owned(),
            String::new(),
            "Pay to (You gave this): City Power".to_owned(),
        ])
    );
    let unmarked = Field {
        from_text: String::new(),
        ..field("Amount", &["42.10 USD"], "from.site")
    };
    assert_eq!(native_lines(&[unmarked]), Err(Cause::Malformed));
    let unmarked_line = Field {
        from_text: String::new(),
        ..halvo("It will make this payment")
    };
    assert_eq!(native_lines(&[unmarked_line]), Err(Cause::Malformed));
    assert_eq!(native_lines(&[]), Ok(vec![]));
    let over = vec![halvo("Line"); CARD_LINES / 2 + 2];
    assert_eq!(native_lines(&over), Err(Cause::Malformed));
    let at = vec![halvo("Line"); CARD_LINES / 2 + 1];
    assert_eq!(native_lines(&at).map(|l| l.len()), Ok(CARD_LINES));
    let wide = field("A", &[&"\u{4e00}".repeat(CARD_COLUMNS / 2)], "from.site");
    assert_eq!(native_lines(&[wide]), Err(Cause::Malformed));
    let broken = field("A", &["x\ny"], "from.site");
    assert_eq!(native_lines(&[broken]), Err(Cause::Malformed));
}

fn start(c: &Coordinator<Both>, n: usize) -> RunId {
    assert_eq!(c.control(&agent(n), Action::RunNow), Ok(()));
    let run = coord::started(c).last().cloned().unwrap();
    assert_eq!(c.on_run(&run, RunEvent::Hello), Ok(()));
    run
}

fn ask_on(run: &RunId, effects: Vec<AskedEffect>) -> RunEvent {
    RunEvent::Confirm(ConfirmAsk::new(run.clone(), agent(0), effects).unwrap())
}

fn open_item(c: &Coordinator<Both>) -> Item {
    let s = c.snapshot();
    assert_eq!(s.items.len(), 1, "{s:?}");
    s.items[0].clone()
}

fn choice(choice: Choice, key: &str, count: usize, safe: YesNo) -> ChoiceText {
    ChoiceText {
        choice,
        text: words::text(key, &[("count", &count.to_string())]).unwrap(),
        safe,
    }
}

fn lines_of(item: &Item) -> Vec<String> {
    let mut lines = Vec::new();
    for f in item.fields() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(if f.value.is_empty() {
            f.label.clone()
        } else {
            words::text(
                "confirm.field",
                &[
                    ("label", &f.label),
                    ("source", &f.from_text),
                    ("value", &f.value.concat()),
                ],
            )
            .unwrap()
        });
    }
    lines
}

/// The card of the native item of `effects`, raised on a fresh coordinator, against that item and
/// the choices of the page for the same ask.
fn card_of(effects: Vec<AskedEffect>, reject: &str, allow: &str) {
    let count = effects.len();
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    assert_eq!(c.on_run(&run, ask_on(&run, effects.clone())), Ok(()));
    let item = open_item(&c);
    assert_eq!(item.presenter(), Some(Presenter::Native));
    let m = mark(&c);
    let card = c.native_card(item.item()).unwrap();
    assert!(calls_since(&c, m).is_empty(), "no port is called");
    assert_eq!(card.title(), item.title());
    assert_eq!(card.lines(), lines_of(&item).as_slice());
    let reject = choice(Choice::Reject, reject, count, YesNo::Yes);
    let allow = choice(Choice::Allow, allow, count, YesNo::No);
    assert_eq!((card.reject(), card.allow()), (&reject, &allow));
    assert_eq!(item.choices().len(), 1);
    assert_eq!(item.choices()[0].choice, Choice::Present);
    let ask = ConfirmAsk::new(run.clone(), agent(0), effects).unwrap();
    let shown = confirm_step(None, ConfirmMove::Issue(&ask)).unwrap();
    let page = compose_confirm(item.item(), &ask, &shown, &Table::new()).unwrap();
    assert_eq!(page.title(), card.title(), "the state's composer");
    assert_eq!(page.fields(), item.fields());
    assert_eq!(c.native_card(item.item()), Ok(card), "a second read");
}

#[test]
fn the_card_of_one_effect_of_one_class() {
    card_of(
        vec![effect(1, Class::Payment, fields_of(Class::Payment))],
        "confirm.payment.reject",
        "confirm.payment.allow",
    );
    card_of(
        vec![effect(1, Class::Sign, fields_of(Class::Sign))],
        "confirm.sign.reject",
        "confirm.sign.allow",
    );
}

#[test]
fn the_card_of_several_effects_of_one_class() {
    card_of(
        vec![
            effect(1, Class::Sign, fields_of(Class::Sign)),
            effect(2, Class::Sign, fields_of(Class::Sign)),
        ],
        "confirm.sign.reject_many",
        "confirm.sign.allow_many",
    );
    card_of(
        vec![
            effect(1, Class::Sign, fields_of(Class::Sign)),
            effect(2, Class::Sign, fields_of(Class::Sign)),
            effect(3, Class::Sign, fields_of(Class::Sign)),
        ],
        "confirm.sign.reject_many",
        "confirm.sign.allow_many",
    );
}

#[test]
fn the_card_of_mixed_classes_names_no_class() {
    card_of(
        vec![
            effect(1, Class::Payment, fields_of(Class::Payment)),
            effect(2, Class::Delete, fields_of(Class::Delete)),
        ],
        "confirm.mixed.reject",
        "confirm.mixed.allow",
    );
    card_of(
        vec![
            effect(1, Class::Delete, fields_of(Class::Delete)),
            effect(2, Class::Sign, fields_of(Class::Sign)),
        ],
        "confirm.mixed.reject",
        "confirm.mixed.allow",
    );
}

#[test]
fn the_card_shows_the_new_site_with_its_source() {
    let mut paid = effect(1, Class::Payment, fields_of(Class::Payment));
    paid.new_origin = Some(Origin::parse("https://pay.example:8443").unwrap());
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    assert_eq!(c.on_run(&run, ask_on(&run, vec![paid])), Ok(()));
    let card = c.native_card(open_item(&c).item()).unwrap();
    assert_eq!(
        card.lines().last().map(String::as_str),
        Some("A site it has not used before (Set by Halvo): pay.example:8443")
    );
}

#[test]
fn an_id_that_is_no_open_native_confirmation_answers_a_cause() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let dead = ItemId::parse(DEAD_ITEM).unwrap();
    assert_eq!(c.native_card(&dead), Err(Cause::UnknownItem), "no item");
    assert_eq!(
        c.on_run(
            &run,
            ask_on(&run, vec![effect(1, Class::Send, fields_of(Class::Send))])
        ),
        Ok(())
    );
    let page = open_item(&c);
    assert_eq!(page.presenter(), Some(Presenter::Page));
    assert_eq!(c.native_card(page.item()), Err(Cause::Presenter), "page");
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let stuck = RunEvent::Handback(HandbackAsk {
        run: run.clone(),
        agent: agent(0),
        reason: HandbackReason::StepFailed,
        origin: None,
    });
    assert_eq!(c.on_run(&run, stuck), Ok(()));
    let plain = open_item(&c);
    assert_ne!(plain.kind(), ItemKind::Confirm);
    assert_eq!(
        c.native_card(plain.item()),
        Err(Cause::UnknownItem),
        "a plain item"
    );
    assert_eq!(c.native_card(&item_id(7)), Err(Cause::UnknownItem));
}

fn items_raised(c: &Coordinator<Both>) -> usize {
    c.ports()
        .ports
        .events
        .borrow()
        .iter()
        .filter(|e| matches!(e.what, What::ItemRaised { .. }))
        .count()
}

/// A request that `admits` refuses shows no card, stores no `item_raised` and stops its run with
/// `kind` (ruling RD-90 item 6, in the form of RD-73 7.4).
fn refused_at_the_raise(effects: Vec<AskedEffect>) {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let before = items_raised(&c);
    let m = mark(&c);
    assert_eq!(c.on_run(&run, ask_on(&run, effects)), Err(Cause::State));
    assert_eq!(items_raised(&c), before, "no item_raised");
    assert!(c.snapshot().items.is_empty(), "no card");
    assert!(calls_since(&c, m)
        .iter()
        .any(|call| matches!(call, Call::Cancel(r) if r == &run)));
    c.ports().ports.set_clock(
        coord::T0 + Limits::PRODUCT.cancel_grace_s * 1000,
        coord::DAY,
    );
    c.tick();
    let ended = c
        .ports()
        .ports
        .events
        .borrow()
        .iter()
        .find_map(|e| match &e.what {
            What::RunEnded { run: r, cause, .. } if r == &run => Some(*cause),
            _ => None,
        });
    assert_eq!(ended, Some(RunEnd::Kind));
}

#[test]
fn a_request_with_a_deciding_value_that_is_not_its_composed_form_ends_its_run_with_kind() {
    let mut paid = fields_of(Class::Payment);
    paid[1] = field("amount", BoundFrom::Extract, "42.10\u{200b} USD");
    refused_at_the_raise(vec![effect(1, Class::Payment, paid)]);
    let mut sent = fields_of(Class::Send);
    sent[0] = field("to", BoundFrom::Extract, "ann@example.com  ");
    refused_at_the_raise(vec![effect(1, Class::Send, sent)]);
    let mut wrong = fields_of(Class::Payment);
    wrong[0] = field("payee", BoundFrom::Model, "City Power");
    refused_at_the_raise(vec![effect(1, Class::Payment, wrong)]);
    refused_at_the_raise(
        (1..=8)
            .map(|k| effect(k, Class::Payment, fields_of(Class::Payment)))
            .collect(),
    );
}

#[test]
fn each_line_that_halvo_sets_with_no_value_fits_a_native_card() {
    let mut keys = vec!["confirm.changed".to_owned()];
    for class in [
        Class::Payment,
        Class::Purchase,
        Class::Send,
        Class::Delete,
        Class::Submit,
        Class::Sign,
    ] {
        keys.push(format!("confirm.{}.action", class.name()));
    }
    for key in keys {
        let line = words::text(&key, &[]).unwrap();
        assert!(columns(&line) <= CARD_COLUMNS, "{key}");
        assert_eq!(native_lines(&[halvo(&line)]), Ok(vec![line]), "{key}");
    }
}

fn ask_for(run: &RunId, n: usize, effects: Vec<AskedEffect>) -> RunEvent {
    RunEvent::Confirm(ConfirmAsk::new(run.clone(), agent(n), effects).unwrap())
}

fn fields_named(class: Class, names: &[&str]) -> Vec<BoundField> {
    let all = fields_of(class);
    names
        .iter()
        .map(|n| all.iter().find(|f| f.name.as_str() == *n).unwrap().clone())
        .collect()
}

/// The end with which the run `run` ended once the grace after `now` has passed.
fn ended_by(c: &Coordinator<Both>, run: &RunId, now: u64) -> Option<RunEnd> {
    c.ports()
        .ports
        .set_clock(now + Limits::PRODUCT.cancel_grace_s * 1000, coord::DAY);
    c.tick();
    c.ports()
        .ports
        .events
        .borrow()
        .iter()
        .find_map(|e| match &e.what {
            What::RunEnded { run: r, cause, .. } if r == run => Some(*cause),
            _ => None,
        })
}

/// Arms the open item of `c` at `T0 + 1_000` and answers it with allow.
fn allow_open_item(c: &Coordinator<Both>) {
    let item = open_item(c);
    c.on_screen(item.item());
    c.ports().ports.set_clock(coord::T0 + 1_000, coord::DAY);
    c.tick();
    let answered = match item.presenter() {
        Some(Presenter::Native) => c.answer_native(item.item(), ConfirmAnswer::Allow),
        _ => c.answer(item.item(), Choice::Allow, None, item.step()),
    };
    assert_eq!(answered, Ok(()));
}

#[test]
fn rd131_own_01a_a_site_subject_of_64_stacked_marks_is_refused_at_the_raise() {
    let mut sent = fields_of(Class::Send);
    sent[1] = field(
        "subject",
        BoundFrom::Extract,
        &format!("Re{}", "\u{301}".repeat(64)),
    );
    refused_at_the_raise(vec![
        effect(1, Class::Sign, fields_of(Class::Sign)),
        effect(2, Class::Send, sent),
    ]);
}

#[test]
fn rd131_every_mark_of_category_m_counts_and_more_than_three_in_a_row_are_refused() {
    for code in [0x301, 0x316, 0x489, 0x20DD, 0x93E, 0x93F, 0xBBE] {
        assert!(combining(c(code)), "U+{code:04X}");
    }
    for ch in ['e', ' ', '\u{4e00}'] {
        assert!(!combining(ch), "{ch:?}");
    }
    assert_eq!(MOST_MARKS, 3);
    assert_eq!(marks_in_a_row("Re\u{301}\u{301}\u{301}"), 3);
    assert_eq!(
        marks_in_a_row("\u{301}\u{301}Re"),
        2,
        "a leading run counts"
    );
    assert_eq!(
        marks_in_a_row("a \u{301}\u{301}\u{301}\u{301}b"),
        4,
        "after a blank"
    );
    assert_eq!(
        marks_in_a_row("\u{915}\u{93e}\u{93e}\u{93e}\u{93e}"),
        4,
        "Mc counts"
    );
    let table = Table::new();
    let native = |subject: &str| {
        let mut sent = fields_of(Class::Send);
        sent[1] = field("subject", BoundFrom::Extract, subject);
        ask_of(
            1,
            1,
            vec![
                effect(1, Class::Sign, fields_of(Class::Sign)),
                effect(2, Class::Send, sent),
            ],
        )
    };
    for code in [0x301, 0x316, 0x489, 0x93E] {
        let m = c(code).to_string();
        assert!(
            admits(&native(&format!("Re{}", m.repeat(3))), &table),
            "U+{code:04X} x3"
        );
        assert!(
            !admits(&native(&format!("Re{}", m.repeat(4))), &table),
            "U+{code:04X} x4"
        );
        assert!(
            !admits(&native(&format!("{}Re", m.repeat(4))), &table),
            "U+{code:04X} x4 leading"
        );
    }
    let page = |subject: &str| {
        let mut sent = fields_of(Class::Send);
        sent[1] = field("subject", BoundFrom::Extract, subject);
        ask_of(1, 1, vec![effect(1, Class::Send, sent)])
    };
    assert!(
        admits(&page(&format!("Re{}", "\u{301}".repeat(64))), &table),
        "the page shows free text whole"
    );
    for (party, ok) in [
        ("\u{921}\u{93c}\u{947}\u{902}", true),
        ("\u{642}\u{652}\u{651}\u{650}", true),
        ("\u{c95}\u{cc6}\u{cc6}\u{c82}", true),
        ("Ann\u{93e}\u{93e}\u{93e}\u{93e}", false),
        ("Ann\u{301}\u{316}\u{489}\u{93e}", false),
    ] {
        let mut signed = fields_of(Class::Sign);
        signed[1] = field("party", BoundFrom::Slot, party);
        assert_eq!(
            admits(&ask_of(1, 1, vec![effect(1, Class::Sign, signed)]), &table),
            ok,
            "{party:?}"
        );
    }
}

#[test]
fn rd132_f1_05_each_class_holds_its_deciding_fields_once_in_the_order_of_a_a9_3() {
    let table = Table::new();
    let deciding: [(Class, &[&str]); 6] = [
        (Class::Payment, &["payee", "amount", "instrument", "origin"]),
        (
            Class::Purchase,
            &["seller", "items", "amount", "instrument", "origin"],
        ),
        (Class::Send, &["to"]),
        (Class::Delete, &["items"]),
        (Class::Submit, &["origin"]),
        (Class::Sign, &["document", "party"]),
    ];
    for (class, names) in deciding {
        let support: Vec<String> = fields_of(class)
            .iter()
            .map(|f| f.name.as_str().to_owned())
            .collect();
        assert_eq!(class_fields(class), support.as_slice(), "{class:?}");
        let got: Vec<&str> = class_fields(class)
            .iter()
            .copied()
            .filter(|n| bound_field(class, n).map(|(_, m)| m) == Some(Marking::Deciding))
            .collect();
        assert_eq!(got, names, "{class:?}");
        assert!(admits(
            &ask_of(1, 1, vec![effect(1, class, fields_of(class))]),
            &table
        ));
        for missing in names {
            let left: Vec<&str> = class_fields(class)
                .iter()
                .copied()
                .filter(|n| n != missing)
                .collect();
            let ask = ask_of(1, 1, vec![effect(1, class, fields_named(class, &left))]);
            assert!(!admits(&ask, &table), "{class:?} with no {missing}");
        }
    }
    let paid = |names: &[&str]| {
        ask_of(
            1,
            1,
            vec![effect(
                1,
                Class::Payment,
                fields_named(Class::Payment, names),
            )],
        )
    };
    assert!(
        !admits(&paid(&["payee", "instrument", "origin"]), &table),
        "no amount"
    );
    let mut twice = fields_of(Class::Payment);
    twice.insert(1, field("payee", BoundFrom::Slot, "Other Shop"));
    assert!(
        !admits(
            &ask_of(1, 1, vec![effect(1, Class::Payment, twice)]),
            &table
        ),
        "two payees"
    );
    assert!(
        !admits(&paid(&["amount", "payee", "instrument", "origin"]), &table),
        "out of order"
    );
    assert!(
        admits(&paid(&["payee", "amount", "instrument", "origin"]), &table),
        "whole"
    );
    let sent = ask_of(
        1,
        1,
        vec![effect(
            1,
            Class::Send,
            fields_named(Class::Send, &["to", "body"]),
        )],
    );
    assert!(admits(&sent, &table), "free text may be absent");
}

fn payment_and_delete() -> Vec<AskedEffect> {
    vec![
        effect(1, Class::Payment, fields_of(Class::Payment)),
        effect(2, Class::Delete, fields_of(Class::Delete)),
    ]
}

#[test]
fn b2_a_re_ask_whose_changed_line_takes_its_native_card_over_its_lines_ends_kind() {
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    assert_eq!(c.on_run(&run, ask_on(&run, payment_and_delete())), Ok(()));
    let card = c.native_card(open_item(&c).item()).unwrap();
    assert_eq!(
        card.lines().len(),
        13,
        "the card with no line confirm.changed"
    );
    let c = coordinator(&[MANUAL], Limits::PRODUCT);
    let run = start(&c, 0);
    let paid = vec![effect(1, Class::Payment, fields_of(Class::Payment))];
    assert_eq!(c.on_run(&run, ask_on(&run, paid)), Ok(()));
    allow_open_item(&c);
    let before = items_raised(&c);
    assert_eq!(
        c.on_run(&run, ask_on(&run, payment_and_delete())),
        Err(Cause::State),
        "15 lines with the line confirm.changed"
    );
    assert_eq!(items_raised(&c), before, "no item_raised");
    assert!(c.snapshot().items.is_empty(), "no card");
    assert_eq!(ended_by(&c, &run, coord::T0 + 1_000), Some(RunEnd::Kind));
}

#[test]
fn b3_a_request_that_takes_the_last_item_of_its_run_and_fails_admits_ends_kind() {
    let limits = Limits {
        items_per_run: 1,
        ..Limits::PRODUCT
    };
    for (bad, end) in [(true, RunEnd::Kind), (false, RunEnd::Budget)] {
        let c = coordinator(&[MANUAL], limits);
        let run = start(&c, 0);
        let first = vec![effect(1, Class::Send, fields_of(Class::Send))];
        assert_eq!(c.on_run(&run, ask_on(&run, first)), Ok(()));
        allow_open_item(&c);
        let mut sent = fields_of(Class::Send);
        if bad {
            sent[0] = field("to", BoundFrom::Extract, "Ann  Lee");
        }
        let second = vec![effect(2, Class::Send, sent)];
        assert_eq!(
            c.on_run(&run, ask_on(&run, second)),
            Err(Cause::State),
            "{bad}"
        );
        assert_eq!(ended_by(&c, &run, coord::T0 + 1_000), Some(end), "{bad}");
    }
}

#[test]
fn b4_a_second_effect_alone_that_breaks_a_rule_of_admits_is_refused() {
    let table = Table::new();
    let good = fields_of(Class::Sign);
    let with = |fields: Vec<BoundField>| {
        ask_of(
            1,
            1,
            vec![
                effect(1, Class::Delete, fields_of(Class::Delete)),
                effect(2, Class::Sign, fields),
            ],
        )
    };
    assert!(admits(&with(good.clone()), &table));
    let cases: Vec<(&str, Vec<BoundField>)> = vec![
        (
            "a source that A-A9.3 does not admit",
            vec![
                field("document", BoundFrom::Model, "Lease"),
                good[1].clone(),
            ],
        ),
        (
            "a field that A-A9.3 does not bind",
            vec![
                good[0].clone(),
                good[1].clone(),
                field("note", BoundFrom::Slot, "x"),
            ],
        ),
        (
            "two blanks in a deciding value",
            vec![good[0].clone(), field("party", BoundFrom::Slot, "Ann  Lee")],
        ),
        (
            "an empty deciding value",
            vec![good[0].clone(), field("party", BoundFrom::Slot, "")],
        ),
        (
            "four marks in a row",
            vec![
                good[0].clone(),
                field("party", BoundFrom::Slot, "Ann\u{301}\u{301}\u{301}\u{301}"),
            ],
        ),
        ("a deciding field missing", vec![good[0].clone()]),
        (
            "a deciding field twice",
            vec![good[0].clone(), good[0].clone(), good[1].clone()],
        ),
        ("out of order", vec![good[1].clone(), good[0].clone()]),
    ];
    for (why, fields) in cases {
        assert!(!admits(&with(fields), &table), "{why}");
    }
    let page = ask_of(
        1,
        1,
        vec![
            effect(1, Class::Send, fields_of(Class::Send)),
            effect(
                2,
                Class::Delete,
                vec![field("items", BoundFrom::Model, "msg-1")],
            ),
        ],
    );
    assert!(!admits(&page, &table), "a page request's second effect");
}

#[test]
fn b5_two_held_native_confirmations_each_answer_their_own_card() {
    let limits = Limits {
        max_live_runs: 2,
        ..Limits::PRODUCT
    };
    let c = coordinator(&[MANUAL, MANUAL], limits);
    let runs = [start(&c, 0), start(&c, 1)];
    let classes = [Class::Payment, Class::Sign];
    for (n, (run, class)) in runs.iter().zip(classes).enumerate() {
        let effects = vec![effect(1 + n as u32, class, fields_of(class))];
        assert_eq!(c.on_run(run, ask_for(run, n, effects)), Ok(()), "{class:?}");
    }
    let items = c.snapshot().items;
    assert_eq!(items.len(), 2);
    let mut titles = Vec::new();
    for item in &items {
        let card = c.native_card(item.item()).unwrap();
        assert_eq!(card.title(), item.title());
        assert_eq!(card.lines(), lines_of(item).as_slice());
        titles.push(card.title().to_owned());
    }
    assert_ne!(titles[0], titles[1]);
}

#[test]
fn b6_the_proven_bounds_are_13_lines_and_96_columns() {
    let read = "run 44227930432: on macOS 13 lines of 96 columns of W read a window 615 points high in 684 visible; on Windows 410 pixels in a work area of 720";
    assert_eq!(CARD_LINES, 13, "{read}");
    assert_eq!(CARD_COLUMNS, 96, "{read}");
}
