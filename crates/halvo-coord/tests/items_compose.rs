#[path = "items_support.rs"]
mod support;

use halvo_coord::guard::{guard_step, EffectRecord, EffectState, Gives, Table};
use halvo_coord::items::{
    compose_bind, compose_check, compose_confirm, compose_link_account, compose_plain,
    compose_setup, confirm_step, kept_source, ConfirmMove, Key, Plain, SetupParam,
};
use halvo_coord::types::{
    AgentId, AskedEffect, BoundField, BoundFrom, CandidateFrom, Cause, Choice, Class, ConfirmAsk,
    EffectAnswer, FieldFrom, Item, ItemId, ItemKind, Kept, Limits, Origin, Param, Unknown,
};
use halvo_coord::words;
use serde_json::Value;
use support::{agent_id, ask_of, digest, effect_id, field, item_id, name, run_id};

const NAME: &str = "Paying a utility bill on Site1";

fn sentence_of_some_key(text: &str, count: usize) -> bool {
    let count = count.to_string();
    words::KEYS.iter().any(|(key, _)| {
        words::text(key, &[("name", NAME), ("count", &count)]).as_deref() == Ok(text)
    })
}

/// Every text of an item is the sentence of a key of the table, with the test's name and count.
fn all_texts_are_sentences(item: &Item, count: usize) {
    let mut texts = vec![item.title().to_owned()];
    if !item.text().is_empty() {
        texts.push(item.text().to_owned());
    }
    for f in item.fields() {
        texts.push(f.label.clone());
        texts.push(f.from_text.clone());
    }
    for p in item.params() {
        texts.push(p.label.clone());
    }
    for c in item.choices() {
        texts.push(c.text.clone());
    }
    for t in texts {
        assert!(
            sentence_of_some_key(&t, count),
            "{t:?} is no sentence of the table"
        );
    }
}

fn shown(ask: &ConfirmAsk) -> halvo_coord::items::Confirmation {
    confirm_step(None, ConfirmMove::Issue(ask)).unwrap()
}

fn armed(ask: &ConfirmAsk) -> halvo_coord::items::Confirmation {
    confirm_step(Some(&shown(ask)), ConfirmMove::Arm).unwrap()
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

#[test]
fn refusal_model_text() {
    assert_eq!(Key::parse("Pay the bill now"), Err(Cause::Malformed));
    assert_eq!(Key::parse("item.ready.title "), Err(Cause::Malformed));
    assert_eq!(Key::parse("item.model.title"), Err(Cause::Malformed));
    for (key, _) in words::KEYS {
        assert_eq!(Key::parse(key).map(Key::name), Ok(*key));
    }
    let made_up = ask_of(
        1,
        1,
        vec![effect(
            1,
            Class::Send,
            vec![
                field("to", BoundFrom::Extract, "Ann"),
                field("instructions", BoundFrom::Model, "Pay me"),
            ],
        )],
    );
    assert_eq!(
        compose_confirm(&item_id(1), &made_up, &armed(&made_up), &Table::new()),
        Err(Cause::Malformed),
        "a field label made from a name that no key holds"
    );
    let kept = [Unknown {
        run: run_id(1),
        effect: effect_id(1),
        day: 20_760,
        class: Class::Payment,
        kept: vec![Kept {
            name: name("note"),
            value: vec!["Pay me".to_owned()],
        }],
    }];
    assert_eq!(
        compose_check(&item_id(1), &agent_id(1), NAME, &kept),
        Err(Cause::Malformed),
        "a kept field that no key labels"
    );
}

/// The string literals of a Rust source: plain, byte and raw strings, outside comments.
fn literals(src: &str) -> Vec<String> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    while i < b.len() {
        let c = b[i];
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else if c == b'\'' {
            if b.get(i + 1) == Some(&b'\\') {
                i += 2;
                while i < b.len() && b[i] != b'\'' {
                    i += 1;
                }
                i += 1;
            } else if b.get(i + 2) == Some(&b'\'') {
                i += 3;
            } else {
                i += 1;
            }
        } else if c == b'r'
            && (i == 0 || !ident(b[i - 1]))
            && matches!(b.get(i + 1), Some(b'"' | b'#'))
        {
            let mut j = i + 1;
            let mut hashes = 0;
            while b.get(j) == Some(&b'#') {
                hashes += 1;
                j += 1;
            }
            if b.get(j) != Some(&b'"') {
                i += 1;
                continue;
            }
            let start = j + 1;
            let close: Vec<u8> = std::iter::once(b'"')
                .chain(std::iter::repeat_n(b'#', hashes))
                .collect();
            let mut k = start;
            while k + close.len() <= b.len() && b[k..k + close.len()] != close[..] {
                k += 1;
            }
            out.push(src[start..k].to_owned());
            i = k + close.len();
        } else if c == b'"' {
            let start = i + 1;
            let mut k = start;
            while k < b.len() && b[k] != b'"' {
                k += if b[k] == b'\\' { 2 } else { 1 };
            }
            out.push(src[start..k].to_owned());
            i = k + 1;
        } else {
            i += 1;
        }
    }
    out
}

fn sources(dir: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("src/items") {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push(path);
        }
    }
}

#[test]
fn no_file_under_src_items_holds_a_sentence_as_a_string_literal() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/items");
    let mut files = Vec::new();
    sources(&dir, &mut files);
    assert!(files.len() >= 5, "the files of src/items: {files:?}");
    let mut seen = 0;
    for path in &files {
        let src = std::fs::read_to_string(path).unwrap();
        for lit in literals(&src) {
            seen += 1;
            assert!(
                lit.bytes()
                    .all(|c| matches!(c, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'{' | b'}')),
                "{}: the literal {lit:?} is no key, name or key pattern",
                path.display()
            );
            assert!(
                words::KEYS.iter().all(|(_, s)| *s != lit),
                "{}: the literal {lit:?} is a sentence of the table",
                path.display()
            );
        }
    }
    assert!(seen > 10, "{seen} literals read");
}

#[test]
fn the_literal_reader_finds_a_sentence() {
    let src = "let a = \"Pay this?\"; // \"not this\"\nlet b = r#\"Sign \"it\"\"#; let c = 'x';";
    assert_eq!(
        literals(src),
        vec!["Pay this?".to_owned(), "Sign \"it\"".to_owned()]
    );
}

#[test]
fn a_plain_item_has_its_title_text_and_two_choices() {
    for plain in [Plain::SignIn, Plain::Stuck, Plain::Ready, Plain::Show] {
        let kind = plain.kind();
        let item = compose_plain(&item_id(1), &agent_id(1), plain, NAME).unwrap();
        assert_eq!(item.kind(), kind);
        assert_eq!(item.presenter(), None);
        assert_eq!(
            item.title(),
            words::text(&format!("item.{kind}.title"), &[("name", NAME)]).unwrap()
        );
        assert_eq!(
            item.text(),
            words::text(&format!("item.{kind}.text"), &[]).unwrap()
        );
        let choices: Vec<_> = item
            .choices()
            .iter()
            .map(|c| (c.choice, c.safe.name()))
            .collect();
        let [safe, other] = halvo_coord::items::choices(kind);
        assert_eq!(choices, vec![(safe, "yes"), (other, "no")]);
        for c in item.choices() {
            assert_eq!(
                c.text,
                words::text(&format!("choice.{kind}.{}", c.choice), &[]).unwrap()
            );
        }
        assert!(item.fields().is_empty() && item.params().is_empty());
        all_texts_are_sentences(&item, 1);
    }
}

#[test]
fn a_confirmation_of_the_page_shows_its_fields_and_the_many_controls() {
    let ask = ask_of(
        1,
        1,
        vec![
            effect(1, Class::Send, support::fields_of(Class::Send)),
            effect(2, Class::Send, support::fields_of(Class::Send)),
        ],
    );
    let item = compose_confirm(&item_id(1), &ask, &armed(&ask), &Table::new()).unwrap();
    assert_eq!(item.presenter().map(|p| p.name()), Some("page"));
    assert_eq!(item.armed().name(), "yes");
    assert_eq!(item.text(), "");
    assert_eq!(
        item.title(),
        words::text("confirm.send.title", &[]).unwrap()
    );
    let labels: Vec<_> = item.fields().iter().map(|f| f.label.clone()).collect();
    let want: Vec<_> = [
        "field.send.to",
        "field.send.subject",
        "field.send.body",
        "field.send.to",
        "field.send.subject",
        "field.send.body",
    ]
    .iter()
    .map(|k| words::text(k, &[]).unwrap())
    .collect();
    assert_eq!(labels, want);
    let froms: Vec<_> = item.fields().iter().map(|f| f.from.name()).collect();
    assert_eq!(froms, ["site", "site", "agent", "site", "site", "agent"]);
    let choices: Vec<_> = item
        .choices()
        .iter()
        .map(|c| (c.choice.name(), c.text.clone(), c.safe.name()))
        .collect();
    assert_eq!(
        choices,
        vec![
            (
                "reject",
                words::text("confirm.send.reject_many", &[("count", "2")]).unwrap(),
                "yes"
            ),
            (
                "allow",
                words::text("confirm.send.allow_many", &[("count", "2")]).unwrap(),
                "no"
            ),
        ]
    );
    all_texts_are_sentences(&item, 2);
    let shown_item = compose_confirm(&item_id(1), &ask, &shown(&ask), &Table::new()).unwrap();
    assert_eq!(
        shown_item.armed().name(),
        "no",
        "armed only while the item is armed"
    );
}

#[test]
fn a_native_confirmation_has_the_one_choice_present_and_names_the_native_class() {
    let mut payment = effect(2, Class::Payment, support::fields_of(Class::Payment));
    payment.new_origin = Some(Origin::parse("https://pay.example:443").unwrap());
    let ask = ask_of(
        1,
        1,
        vec![
            effect(1, Class::Payment, support::fields_of(Class::Payment)),
            payment,
        ],
    );
    let item = compose_confirm(&item_id(1), &ask, &armed(&ask), &Table::new()).unwrap();
    assert_eq!(item.presenter().map(|p| p.name()), Some("native"));
    assert_eq!(
        item.title(),
        words::text("confirm.payment.title", &[]).unwrap()
    );
    let choices: Vec<_> = item
        .choices()
        .iter()
        .map(|c| (c.choice.name(), c.text.clone(), c.safe.name()))
        .collect();
    assert_eq!(
        choices,
        vec![(
            "present",
            words::text("confirm.present", &[]).unwrap(),
            "yes"
        )]
    );
    let last = item.fields().last().unwrap();
    assert_eq!(last.label, words::text("confirm.new_site", &[]).unwrap());
    assert_eq!(last.value, vec!["pay.example".to_owned()]);
    assert_eq!(last.from.name(), "halvo");
    assert_eq!(item.fields().len(), 4 + 4 + 1);
    all_texts_are_sentences(&item, 2);
}

#[test]
fn a_piece_longer_than_1024_bytes_is_malformed() {
    let long = "a".repeat(1025);
    let ask = ask_of(
        1,
        1,
        vec![effect(
            1,
            Class::Delete,
            vec![field("items", BoundFrom::Extract, &long)],
        )],
    );
    assert_eq!(
        compose_confirm(&item_id(1), &ask, &armed(&ask), &Table::new()),
        Err(Cause::Malformed)
    );
    let ok = "a".repeat(1024);
    let ask = ask_of(
        1,
        1,
        vec![effect(
            1,
            Class::Delete,
            vec![field("items", BoundFrom::Extract, &ok)],
        )],
    );
    assert!(compose_confirm(&item_id(1), &ask, &armed(&ask), &Table::new()).is_ok());
}

#[test]
fn a_confirmation_is_composed_only_with_its_own_request() {
    let ask = ask_of(
        1,
        1,
        vec![effect(1, Class::Send, support::fields_of(Class::Send))],
    );
    let other = ask_of(
        1,
        1,
        vec![effect(2, Class::Send, support::fields_of(Class::Send))],
    );
    assert_eq!(
        compose_confirm(&item_id(1), &other, &armed(&ask), &Table::new()),
        Err(Cause::State)
    );
}

#[test]
fn the_kept_fields_take_the_source_that_a_a9_3_admits() {
    use BoundFrom::*;
    let table = [
        (Class::Payment, "payee", Some(Slot)),
        (Class::Payment, "amount", Some(Extract)),
        (Class::Payment, "instrument", Some(Slot)),
        (Class::Payment, "origin", Some(Const)),
        (Class::Purchase, "seller", Some(Slot)),
        (Class::Purchase, "items", None),
        (Class::Purchase, "amount", Some(Extract)),
        (Class::Purchase, "instrument", Some(Slot)),
        (Class::Purchase, "origin", Some(Const)),
        (Class::Send, "to", Some(Extract)),
        (Class::Send, "subject", None),
        (Class::Send, "body", None),
        (Class::Delete, "items", Some(Extract)),
        (Class::Submit, "origin", Some(Const)),
        (Class::Submit, "fields", None),
        (Class::Sign, "document", Some(Slot)),
        (Class::Sign, "party", Some(Slot)),
        (Class::LocalEdit, "file", Some(Slot)),
        (Class::LocalEdit, "range", Some(Slot)),
        (Class::Send, "payee", None),
    ];
    for (class, field_, want) in table {
        assert_eq!(kept_source(class, field_), want, "{class}.{field_}");
    }
}

#[test]
fn a_setup_numbers_its_candidates_and_types_a_parameter_with_none() {
    let params = [
        SetupParam {
            param: Param::Start,
            candidates: vec![
                (vec!["site1.example".to_owned()], CandidateFrom::Records),
                (vec!["other.example".to_owned()], CandidateFrom::Site),
            ],
        },
        SetupParam {
            param: Param::Instrument,
            candidates: vec![],
        },
    ];
    let item = compose_setup(
        &item_id(1),
        &agent_id(1),
        NAME,
        &params,
        false,
        &Limits::PRODUCT,
    )
    .unwrap();
    let got: Vec<_> = item
        .params()
        .iter()
        .map(|p| {
            (
                p.name,
                p.typed.name(),
                p.candidates.iter().map(|c| c.n).collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![
            (Param::Start, "no", vec![1, 2]),
            (Param::Instrument, "yes", vec![])
        ]
    );
    all_texts_are_sentences(&item, 1);
    let small = Limits {
        setup_params: 1,
        ..Limits::PRODUCT
    };
    assert_eq!(
        compose_setup(&item_id(1), &agent_id(1), NAME, &params, false, &small),
        Err(Cause::Malformed)
    );
    let few = Limits {
        setup_candidates: 1,
        ..Limits::PRODUCT
    };
    assert_eq!(
        compose_setup(&item_id(1), &agent_id(1), NAME, &params, false, &few),
        Err(Cause::Malformed)
    );
}

fn fixture(state: &str) -> Value {
    let path = format!(
        "{}/../../ui/tests/fixtures/coord/state-{state}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn bound(class: Class, values: &[&str]) -> Vec<BoundField> {
    support::fields_of(class)
        .into_iter()
        .zip(values)
        .map(|(f, v)| BoundField {
            value: vec![(*v).to_owned()],
            ..f
        })
        .collect()
}

const PAYMENT: [&str; 4] = [
    "City Power",
    "42.10 USD",
    "Card ending 4242",
    "site1.example",
];

const MIXED_SEND: [&str; 3] = [
    "Ann Lee",
    "Rent receipt for October",
    "Thank you. Your rent for October came in.",
];

const MIXED_DELETE: [&str; 1] = ["Draft to Ann Lee from October"];

const CHANGED_BEFORE: [&str; 3] = [
    "Ben Ola",
    "Viewing on Friday",
    "Friday at 10 works for the viewing.",
];

const CHANGED_SEND: [&str; 3] = [
    "Ben Ola",
    "Viewing on Friday",
    "Friday at 11 works for the viewing.",
];

/// The items of the fixture states, composed from what each state holds, equal the items that
/// the fixture files hold (I8.3).
#[test]
fn the_composed_items_equal_the_items_of_the_fixtures() {
    let states = [
        "needs_sign_in",
        "needs_sign_in_bind",
        "needs_link_account",
        "needs_stuck",
        "needs_ready",
        "needs_show",
        "needs_setup",
        "setup_three",
        "needs_check",
        "needs_confirm_page",
        "needs_confirm_native",
        "needs_confirm_mixed",
        "needs_confirm_changed",
    ];
    for state in states {
        let file = fixture(state);
        let items = file["snapshot"]["items"].as_array().unwrap();
        assert_eq!(items.len(), 1, "{state}");
        let want = &items[0];
        let item = ItemId::parse(want["item"].as_str().unwrap()).unwrap();
        let agent = AgentId::parse(want["agent"].as_str().unwrap()).unwrap();
        let row = file["snapshot"]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["agent"] == want["agent"])
            .unwrap();
        let name = row["name"].as_str().unwrap();
        let composed = match want["kind"].as_str().unwrap() {
            "sign_in" if state == "needs_sign_in_bind" => {
                let origin = Origin::parse("https://site7.example:443").unwrap();
                compose_bind(&item, &agent, name, &origin)
            }
            "sign_in" => compose_plain(&item, &agent, Plain::SignIn, name),
            "link_account" => {
                let site = want["fields"][0]["value"][0].as_str().unwrap();
                let origin = Origin::parse(&format!("https://{site}:443")).unwrap();
                compose_link_account(&item, &agent, name, &origin)
            }
            "stuck" => compose_plain(&item, &agent, Plain::Stuck, name),
            "ready" => compose_plain(&item, &agent, Plain::Ready, name),
            "show" => compose_plain(&item, &agent, Plain::Show, name),
            "setup" => {
                let params: Vec<SetupParam> = want["params"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|p| SetupParam {
                        param: Param::parse(p["name"].as_str().unwrap()).unwrap(),
                        candidates: p["candidates"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|c| {
                                (
                                    c["value"]
                                        .as_array()
                                        .unwrap()
                                        .iter()
                                        .map(|v| v.as_str().unwrap().to_owned())
                                        .collect(),
                                    CandidateFrom::parse(c["from"].as_str().unwrap()).unwrap(),
                                )
                            })
                            .collect(),
                    })
                    .collect();
                compose_setup(&item, &agent, name, &params, false, &Limits::PRODUCT)
            }
            "check" => {
                let kept = ["payee", "amount", "instrument", "origin"]
                    .iter()
                    .zip(PAYMENT)
                    .map(|(n, v)| Kept {
                        name: support::name(n),
                        value: vec![v.to_owned()],
                    })
                    .collect();
                let unknown = [Unknown {
                    run: run_id(1),
                    effect: effect_id(1),
                    day: 20_759,
                    class: Class::Payment,
                    kept,
                }];
                compose_check(&item, &agent, name, &unknown)
            }
            "confirm" if state == "needs_confirm_page" => {
                let sends = [
                    [
                        "Ann Lee",
                        "Rent receipt for October",
                        "Thank you. Your rent for October came in.",
                    ],
                    [
                        "Ben Ola",
                        "Viewing on Friday",
                        "Friday at 10 works for the viewing.",
                    ],
                    [
                        "Cara Diaz",
                        "Repair date",
                        "The repair is booked for Monday.",
                    ],
                ];
                let ask = ConfirmAsk::new(
                    run_id(1),
                    agent.clone(),
                    sends
                        .iter()
                        .zip(1..)
                        .map(|(v, n)| effect(n, Class::Send, bound(Class::Send, v)))
                        .collect(),
                )
                .unwrap();
                compose_confirm(&item, &ask, &armed(&ask), &Table::new())
            }
            "confirm" if state == "needs_confirm_mixed" => {
                let ask = ConfirmAsk::new(
                    run_id(1),
                    agent.clone(),
                    vec![
                        effect(1, Class::Send, bound(Class::Send, &MIXED_SEND)),
                        effect(2, Class::Delete, bound(Class::Delete, &MIXED_DELETE)),
                    ],
                )
                .unwrap();
                compose_confirm(&item, &ask, &armed(&ask), &Table::new())
            }
            "confirm" if state == "needs_confirm_changed" => {
                let earlier = ConfirmAsk::new(
                    run_id(1),
                    agent.clone(),
                    vec![effect(1, Class::Send, bound(Class::Send, &CHANGED_BEFORE))],
                )
                .unwrap();
                let table = released_table(&Table::new(), 9, &earlier, EffectAnswer::AllowOnce);
                let mut again = effect(1, Class::Send, bound(Class::Send, &CHANGED_SEND));
                again.digest = digest(2);
                let ask = ConfirmAsk::new(run_id(1), agent.clone(), vec![again]).unwrap();
                compose_confirm(&item, &ask, &armed(&ask), &table)
            }
            "confirm" => {
                let ask = ConfirmAsk::new(
                    run_id(1),
                    agent.clone(),
                    vec![effect(1, Class::Payment, bound(Class::Payment, &PAYMENT))],
                )
                .unwrap();
                compose_confirm(&item, &ask, &shown(&ask), &Table::new())
            }
            other => panic!("{state}: kind {other}"),
        };
        let composed = composed.unwrap_or_else(|e| panic!("{state}: {e}"));
        assert_eq!(&serde_json::to_value(&composed).unwrap(), want, "{state}");
    }
}

/// The text of a key of the table with the count.
fn counted(key: &str, count: usize) -> String {
    words::text(key, &[("count", &count.to_string())]).unwrap()
}

/// The labels of the fields of an item, each with whether its value is empty and its source.
fn lines(item: &Item) -> Vec<(String, bool, &'static str)> {
    item.fields()
        .iter()
        .map(|f| (f.label.clone(), f.value.is_empty(), f.from.name()))
        .collect()
}

#[test]
fn refusal_mixed_request() {
    let ask = ask_of(
        1,
        1,
        vec![
            effect(1, Class::Send, support::fields_of(Class::Send)),
            effect(2, Class::Delete, support::fields_of(Class::Delete)),
        ],
    );
    let item = compose_confirm(&item_id(1), &ask, &armed(&ask), &Table::new()).unwrap();
    assert_eq!(item.title(), counted("confirm.mixed.title", 2));
    let choices: Vec<_> = item
        .choices()
        .iter()
        .map(|c| (c.choice.name(), c.text.clone(), c.safe.name()))
        .collect();
    assert_eq!(
        choices,
        vec![
            ("reject", counted("confirm.mixed.reject", 2), "yes"),
            ("allow", counted("confirm.mixed.allow", 2), "no"),
        ]
    );
    let sending = ["send"];
    let removing = ["remov", "keep"];
    let mut controls = vec![item.title().to_lowercase()];
    controls.extend(item.choices().iter().map(|c| c.text.to_lowercase()));
    for text in &controls {
        for word in sending.iter().chain(&removing) {
            assert!(!text.contains(word), "{text:?} holds {word:?}");
        }
    }
    let t = |k: &str| words::text(k, &[]).unwrap();
    assert_eq!(
        lines(&item),
        vec![
            (t("confirm.send.action"), true, "halvo"),
            (t("field.send.to"), false, "site"),
            (t("field.send.subject"), false, "site"),
            (t("field.send.body"), false, "agent"),
            (t("confirm.delete.action"), true, "halvo"),
            (t("field.delete.items"), false, "site"),
        ]
    );
    let send_line = t("confirm.send.action").to_lowercase();
    let delete_line = t("confirm.delete.action").to_lowercase();
    assert!(send_line.contains("send") && !removing.iter().any(|w| send_line.contains(w)));
    assert!(delete_line.contains("remov") && !send_line.contains("remov"));
    assert!(!delete_line.contains("send"));
    all_texts_are_sentences(&item, 2);
}

#[test]
fn a_request_of_two_classes_names_none_and_each_effect_names_its_action() {
    let classes = halvo_coord::items::CONFIRM_CLASSES;
    for a in classes {
        for b in classes {
            let ask = ask_of(
                1,
                1,
                vec![
                    effect(1, a, support::fields_of(a)),
                    effect(2, b, support::fields_of(b)),
                    effect(3, a, support::fields_of(a)),
                ],
            );
            let item = compose_confirm(&item_id(1), &ask, &armed(&ask), &Table::new()).unwrap();
            let action =
                |c: Class| words::text(&format!("confirm.{}.action", c.name()), &[]).unwrap();
            let actions: Vec<String> = item
                .fields()
                .iter()
                .filter(|f| f.value.is_empty())
                .map(|f| f.label.clone())
                .collect();
            let mut class_texts = Vec::new();
            for c in classes {
                for key in ["title", "allow", "reject"] {
                    class_texts
                        .push(words::text(&format!("confirm.{}.{key}", c.name()), &[]).unwrap());
                }
                for key in ["allow_many", "reject_many"] {
                    class_texts.push(counted(&format!("confirm.{}.{key}", c.name()), 3));
                }
            }
            if a == b {
                assert_eq!(
                    item.title(),
                    words::text(&format!("confirm.{}.title", a.name()), &[]).unwrap()
                );
                assert!(actions.is_empty(), "{a}: one class has no action line");
            } else {
                assert_eq!(item.title(), counted("confirm.mixed.title", 3), "{a} {b}");
                assert_eq!(actions, vec![action(a), action(b), action(a)], "{a} {b}");
                assert!(!class_texts.contains(&item.title().to_owned()), "{a} {b}");
                for c in item.choices() {
                    assert!(!class_texts.contains(&c.text), "{a} {b}: {}", c.text);
                }
                let n = |c: Class| support::fields_of(c).len();
                assert_eq!(item.fields().len(), 3 + 2 * n(a) + n(b), "{a} {b}");
                all_texts_are_sentences(&item, 3);
            }
        }
    }
}

#[test]
fn a_mixed_request_with_a_native_class_has_the_one_choice_present_and_names_no_class() {
    let mut payment = effect(2, Class::Payment, support::fields_of(Class::Payment));
    payment.new_origin = Some(Origin::parse("https://pay.example:443").unwrap());
    let ask = ask_of(
        1,
        1,
        vec![
            effect(1, Class::Send, support::fields_of(Class::Send)),
            payment,
        ],
    );
    let item = compose_confirm(&item_id(1), &ask, &armed(&ask), &Table::new()).unwrap();
    assert_eq!(item.presenter().map(|p| p.name()), Some("native"));
    assert_eq!(item.title(), counted("confirm.mixed.title", 2));
    let choices: Vec<_> = item.choices().iter().map(|c| c.choice.name()).collect();
    assert_eq!(choices, ["present"]);
    let t = |k: &str| words::text(k, &[]).unwrap();
    let labels: Vec<String> = item.fields().iter().map(|f| f.label.clone()).collect();
    assert_eq!(labels[0], t("confirm.send.action"));
    assert_eq!(labels[4], t("confirm.payment.action"));
    assert_eq!(labels[9], t("confirm.new_site"));
    assert_eq!(item.fields().len(), 1 + 3 + 1 + 4 + 1);
    all_texts_are_sentences(&item, 2);
}

/// Origins with the site's name of each: its host, with its port where that is not the usual
/// one of its scheme (443 for `https`, 80 for `http`), and never its scheme (I1.4, ruling IG-14).
const SITES: [(&str, &str); 7] = [
    ("https://pay.example:443", "pay.example"),
    ("http://pay.example:80", "pay.example"),
    ("https://pay.example:8443", "pay.example:8443"),
    ("http://pay.example:443", "pay.example:443"),
    ("https://pay.example:80", "pay.example:80"),
    ("http://127.0.0.1:18090", "127.0.0.1:18090"),
    ("https://a-b.example:65535", "a-b.example:65535"),
];

/// The line `confirm.new_site` holds the site's name of the request's `new_origin` (I4.3,
/// ruling IG-14).
#[test]
fn a_new_site_is_shown_by_its_host_and_an_unusual_port_without_its_scheme() {
    let line = words::text("confirm.new_site", &[]).unwrap();
    for (origin, site) in SITES {
        let mut payment = effect(1, Class::Payment, support::fields_of(Class::Payment));
        payment.new_origin = Some(Origin::parse(origin).unwrap());
        let ask = ask_of(1, 1, vec![payment]);
        let item = compose_confirm(&item_id(1), &ask, &shown(&ask), &Table::new()).unwrap();
        let shown: Vec<&Vec<String>> = item
            .fields()
            .iter()
            .filter(|f| f.label == line)
            .map(|f| &f.value)
            .collect();
        assert_eq!(shown, vec![&vec![site.to_owned()]], "{origin}");
    }
}

/// A `link_account` item holds its plain parts and one field, whose label is the entry
/// `item.link_account.origin`, whose value is the site's name of the request's origin and whose
/// `from` is `halvo` (I3.4, ruling IG-14).
#[test]
fn a_link_account_item_shows_the_site_of_the_requests_origin_set_by_halvo() {
    for (origin, site) in SITES {
        let origin = Origin::parse(origin).unwrap();
        let item = compose_link_account(&item_id(1), &agent_id(1), NAME, &origin).unwrap();
        assert_eq!(item.kind(), ItemKind::LinkAccount);
        assert_eq!(item.presenter(), None);
        assert_eq!(
            item.title(),
            words::text("item.link_account.title", &[("name", NAME)]).unwrap()
        );
        assert_eq!(
            item.text(),
            words::text("item.link_account.text", &[]).unwrap()
        );
        let fields: Vec<_> = item
            .fields()
            .iter()
            .map(|f| {
                (
                    f.label.clone(),
                    f.value.clone(),
                    f.from,
                    f.from_text.clone(),
                )
            })
            .collect();
        assert_eq!(
            fields,
            vec![(
                words::text("item.link_account.origin", &[]).unwrap(),
                vec![site.to_owned()],
                FieldFrom::Halvo,
                words::text("from.halvo", &[]).unwrap()
            )],
            "{origin:?}"
        );
        let choices: Vec<_> = item
            .choices()
            .iter()
            .map(|c| (c.choice, c.safe.name()))
            .collect();
        assert_eq!(
            choices,
            vec![(Choice::Decline, "yes"), (Choice::Done, "no")]
        );
        assert!(item.params().is_empty());
        all_texts_are_sentences(&item, 1);
    }
}

/// The guard's table after the release of the decision `answer` on the effects of `ask` by
/// item `n`.
fn released_table(table: &Table, n: u32, ask: &ConfirmAsk, answer: EffectAnswer) -> Table {
    let decision = support::decision_for(ask, answer, 100 * n);
    table
        .record(&item_id(n), ask.run(), &decision)
        .release(&item_id(n))
}

#[test]
fn the_changed_line_stands_above_the_values_of_its_effect_and_after_its_action_line() {
    let t = |k: &str| words::text(k, &[]).unwrap();
    let send = |n: u32, d: u32| {
        let mut e = effect(n, Class::Send, support::fields_of(Class::Send));
        e.digest = digest(d);
        e
    };
    let first = ask_of(1, 1, vec![send(2, 2)]);
    let table = released_table(&Table::new(), 1, &first, EffectAnswer::AllowOnce);
    assert!(table.newest_allows(&run_id(1), &effect_id(2)));
    let one_class = ask_of(1, 1, vec![send(1, 11), send(2, 12)]);
    let item = compose_confirm(&item_id(2), &one_class, &armed(&one_class), &table).unwrap();
    let labels: Vec<String> = item.fields().iter().map(|f| f.label.clone()).collect();
    let mut want: Vec<String> = ["field.send.to", "field.send.subject", "field.send.body"]
        .iter()
        .map(|k| t(k))
        .collect();
    want.push(t("confirm.changed"));
    want.extend(
        ["field.send.to", "field.send.subject", "field.send.body"]
            .iter()
            .map(|k| t(k)),
    );
    assert_eq!(labels, want);
    assert_eq!(item.title(), t("confirm.send.title"));
    all_texts_are_sentences(&item, 2);
    let mut delete = effect(1, Class::Delete, support::fields_of(Class::Delete));
    delete.digest = digest(21);
    let mixed = ask_of(1, 1, vec![delete, send(2, 22)]);
    let item = compose_confirm(&item_id(3), &mixed, &armed(&mixed), &table).unwrap();
    let labels: Vec<String> = item.fields().iter().map(|f| f.label.clone()).collect();
    let want: Vec<String> = [
        "confirm.delete.action",
        "field.delete.items",
        "confirm.send.action",
        "confirm.changed",
        "field.send.to",
        "field.send.subject",
        "field.send.body",
    ]
    .iter()
    .map(|k| t(k))
    .collect();
    assert_eq!(labels, want);
    all_texts_are_sentences(&item, 2);
}

/// The reader of the line: the newest decision released for the effect in the run, by its number
/// of release in this life, decides, and whether a record used its row plays no part (ruling
/// IG-25, IG-26).
#[test]
fn the_reader_gives_the_line_by_the_newest_released_decision_alone() {
    let first = ask_of(
        1,
        1,
        vec![effect(1, Class::Send, support::fields_of(Class::Send))],
    );
    let run = run_id(1);
    let e = effect_id(1);
    let allow = support::decision_for(&first, EffectAnswer::AllowOnce, 100);
    let reject = support::decision_for(&first, EffectAnswer::RejectOnce, 200);
    assert!(!Table::new().newest_allows(&run, &e), "a new life");
    let allowed = released_table(&Table::new(), 1, &first, EffectAnswer::AllowOnce);
    assert!(allowed.newest_allows(&run, &e));
    assert!(!allowed.newest_allows(&run_id(2), &e), "another run");
    assert!(
        !allowed.newest_allows(&run, &effect_id(2)),
        "another effect"
    );
    let rejected = released_table(&Table::new(), 1, &first, EffectAnswer::RejectOnce);
    assert!(!rejected.newest_allows(&run, &e), "reject_once");
    let recorded = Table::new().record(&item_id(1), &run, &allow);
    assert!(
        !recorded.newest_allows(&run, &e),
        "recorded and not released"
    );
    let pending_reject = allowed.record(&item_id(2), &run, &reject);
    assert!(
        pending_reject.newest_allows(&run, &e),
        "a decision recorded and not released does not count"
    );
    assert!(
        !allowed.lapse(&item_id(1)).newest_allows(&run, &e),
        "lapsed"
    );
    let entry = &allow.approvals()[0];
    let record = EffectRecord {
        run: run.clone(),
        agent: agent_id(1),
        effect: e.clone(),
        state: EffectState::Approved,
        class: Class::Send,
        digest: entry.digest.clone(),
        approval: Some(entry.approval.clone()),
    };
    let used = guard_step(&allowed, &record, Gives::Confirm).unwrap();
    assert!(
        used.approvals()[0].2,
        "the record approved marks the row used"
    );
    assert!(
        used.newest_allows(&run, &e),
        "a used row keeps the line (ruling IG-25)"
    );
    let allow_then_reject = released_table(&allowed, 2, &first, EffectAnswer::RejectOnce);
    assert!(
        !allow_then_reject.newest_allows(&run, &e),
        "the newest is reject_once"
    );
    let reject_then_allow = released_table(&rejected, 2, &first, EffectAnswer::AllowOnce);
    assert!(
        reject_then_allow.newest_allows(&run, &e),
        "the newest is allow_once"
    );
    let lapse_after_allow = allowed
        .record(&item_id(2), &run, &reject)
        .lapse(&item_id(2));
    assert!(
        lapse_after_allow.newest_allows(&run, &e),
        "the decision before a lapse decides"
    );
    let out_of_order = Table::new()
        .record(&item_id(1), &run, &allow)
        .record(&item_id(2), &run, &reject)
        .release(&item_id(2))
        .release(&item_id(1));
    assert!(
        out_of_order.newest_allows(&run, &e),
        "the order of release, not of acknowledgement, decides"
    );
    let out_of_order = Table::new()
        .record(&item_id(1), &run, &reject)
        .record(&item_id(2), &run, &allow)
        .release(&item_id(2))
        .release(&item_id(1));
    assert!(
        !out_of_order.newest_allows(&run, &e),
        "the order of release, not of acknowledgement, decides"
    );
}

/// A confirmation composes only with its own request: one of fewer effects, one whose effect
/// has another digest, and one of another effect id with the same digests are each refused
/// with `state` (review findings mixed-A, mixed-B and mixed-H).
#[test]
fn a_confirmation_composes_only_with_a_request_of_its_count_its_effects_and_its_digests() {
    let send = |n: u32, d: u32| {
        let mut e = effect(n, Class::Send, support::fields_of(Class::Send));
        e.digest = digest(d);
        e
    };
    let own = ask_of(1, 1, vec![send(1, 1), send(2, 2)]);
    let confirmation = armed(&own);
    let compose = |ask: &ConfirmAsk| {
        compose_confirm(&item_id(1), ask, &confirmation, &Table::new()).map(|_| ())
    };
    assert_eq!(compose(&own), Ok(()));
    let fewer = ask_of(1, 1, vec![send(1, 1)]);
    assert_eq!(compose(&fewer), Err(Cause::State), "fewer effects");
    let other_digest = ask_of(1, 1, vec![send(1, 1), send(2, 9)]);
    assert_eq!(compose(&other_digest), Err(Cause::State), "another digest");
    let other_effect = ask_of(1, 1, vec![send(1, 1), send(3, 2)]);
    assert_eq!(
        compose(&other_effect),
        Err(Cause::State),
        "another effect id"
    );
}

/// The stems of the words of each confirm class, in lower case: its verb first, then the words
/// of its controls (I4.3).
const VERBS: [(Class, &[&str]); 6] = [
    (Class::Send, &["send"]),
    (Class::Delete, &["remov", "keep"]),
    (Class::Submit, &["submit"]),
    (Class::Purchase, &["order"]),
    (Class::Payment, &["pay"]),
    (Class::Sign, &["sign"]),
];

/// Each action line holds the verb of its own class and no word of another class, and the
/// title and the two controls of a request of several classes hold no word of any class
/// (ruling IG-4, IG-24; the review's B1).
#[test]
fn each_action_line_names_its_own_class_alone_and_the_mixed_entries_name_none() {
    for (class, stems) in VERBS {
        let line = words::text(&format!("confirm.{}.action", class.name()), &[])
            .unwrap()
            .to_lowercase();
        assert!(line.contains(stems[0]), "{class}: {line:?}");
        for (other, theirs) in VERBS {
            if other != class {
                for stem in theirs {
                    assert!(!line.contains(stem), "{class}: {line:?} holds {stem:?}");
                }
            }
        }
    }
    for key in ["title", "reject", "allow"] {
        let text = counted(&format!("confirm.mixed.{key}"), 3).to_lowercase();
        for (class, stems) in VERBS {
            for stem in stems {
                assert!(
                    !text.contains(stem),
                    "confirm.mixed.{key}: {text:?} holds {stem:?} of {class}"
                );
            }
        }
    }
}

/// A request is titled and controlled by the mixed entries wherever the effect of another class
/// stands, also when its first two effects share a class, and each effect has its own action
/// line (ruling IG-24; the verifier's M02).
#[test]
fn a_request_is_mixed_wherever_an_effect_of_another_class_stands() {
    let page = |c: &Class| matches!(c, Class::Send | Class::Delete | Class::Submit);
    for x in halvo_coord::items::CONFIRM_CLASSES {
        for y in halvo_coord::items::CONFIRM_CLASSES {
            if x == y {
                continue;
            }
            for form in [
                vec![x, x, y],
                vec![x, y, y],
                vec![x, x, y, x],
                vec![x, x, x, y],
                vec![x, x, x, x, y],
            ] {
                let effects = form
                    .iter()
                    .zip(1..)
                    .map(|(c, n)| effect(n, *c, support::fields_of(*c)))
                    .collect();
                let ask = ask_of(1, 1, effects);
                let item = compose_confirm(&item_id(1), &ask, &armed(&ask), &Table::new()).unwrap();
                let count = form.len();
                assert_eq!(
                    item.title(),
                    counted("confirm.mixed.title", count),
                    "{form:?}"
                );
                let actions: Vec<String> = item
                    .fields()
                    .iter()
                    .filter(|f| f.value.is_empty())
                    .map(|f| f.label.clone())
                    .collect();
                let want: Vec<String> = form
                    .iter()
                    .map(|c| words::text(&format!("confirm.{}.action", c.name()), &[]).unwrap())
                    .collect();
                assert_eq!(actions, want, "{form:?}");
                let choices: Vec<String> = item.choices().iter().map(|c| c.text.clone()).collect();
                if form.iter().all(page) {
                    assert_eq!(
                        choices,
                        vec![
                            counted("confirm.mixed.reject", count),
                            counted("confirm.mixed.allow", count)
                        ],
                        "{form:?}"
                    );
                } else {
                    assert_eq!(
                        choices,
                        vec![words::text("confirm.present", &[]).unwrap()],
                        "{form:?}"
                    );
                }
            }
        }
    }
}

/// The action lines and the line `confirm.changed` come from Halvo: `from` is `halvo` and
/// `from_text` is the entry `from.halvo` (I3.4, ruling IG-24; the review's B2).
#[test]
fn the_action_lines_and_the_changed_line_are_set_by_halvo() {
    let mut send = effect(2, Class::Send, support::fields_of(Class::Send));
    let first = ask_of(1, 1, vec![send.clone()]);
    let table = released_table(&Table::new(), 1, &first, EffectAnswer::AllowOnce);
    send.digest = digest(22);
    let delete = effect(1, Class::Delete, support::fields_of(Class::Delete));
    let mixed = ask_of(1, 1, vec![delete, send]);
    let item = compose_confirm(&item_id(3), &mixed, &armed(&mixed), &table).unwrap();
    let t = |k: &str| words::text(k, &[]).unwrap();
    let ours = [
        t("confirm.delete.action"),
        t("confirm.send.action"),
        t("confirm.changed"),
    ];
    let lines: Vec<_> = item
        .fields()
        .iter()
        .filter(|f| ours.contains(&f.label))
        .collect();
    assert_eq!(lines.len(), 3);
    for f in lines {
        assert_eq!(f.from.name(), "halvo", "{}", f.label);
        assert_eq!(f.from_text, t("from.halvo"), "{}", f.label);
        assert!(f.value.is_empty(), "{}", f.label);
    }
}
