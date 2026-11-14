use halvo_coord::types::{Category, Cause, Class, ItemKind, Status, StatusLine};
use halvo_coord::words::{self, KEYS};
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ui/tests/fixtures/coord/words.json"
);

#[test]
fn words_same() {
    let out = Command::new(env!("CARGO_BIN_EXE_coord-words"))
        .arg("export")
        .output()
        .expect("coord-words runs");
    assert!(
        out.status.success(),
        "coord-words export exited {}",
        out.status
    );
    assert!(out.stderr.is_empty());
    let tracked = std::fs::read(FIXTURE).expect("words.json is tracked");
    assert!(
        out.stdout == tracked,
        "ui/tests/fixtures/coord/words.json differs from `coord-words export`; write the fixtures again with `cargo run -p halvo-coord --bin coord-words -- fixtures ui/tests/fixtures/coord`"
    );
}

#[test]
fn export_is_one_line_of_the_keys_in_byte_order() {
    let text = words::export();
    assert!(text.ends_with("}\n") && text.matches('\n').count() == 1);
    let map: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&text).expect("an object");
    let keys: Vec<&String> = map.keys().collect();
    let mut sorted = keys.clone();
    sorted.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(keys, sorted);
    assert_eq!(map.len(), KEYS.len());
}

#[test]
fn usage_is_refused_with_one_line_on_standard_error() {
    for args in [vec![], vec!["nope"], vec!["export", "more"]] {
        let out = Command::new(env!("CARGO_BIN_EXE_coord-words"))
            .args(&args)
            .output()
            .expect("coord-words runs");
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        let err = String::from_utf8(out.stderr).unwrap();
        assert!(err.starts_with("REFUSED usage: "), "{err}");
        assert_eq!(err.matches('\n').count(), 1, "{err}");
    }
}

fn expected_keys() -> BTreeSet<String> {
    let mut k = BTreeSet::new();
    let statuses = [
        "needs_you",
        "problem",
        "working",
        "waiting_turn",
        "all_set",
        "off",
        "not_yet",
    ];
    for s in statuses {
        k.insert(format!("status.{s}"));
        k.insert(format!("say.{s}"));
    }
    for kind in [
        "confirm",
        "sign_in",
        "link_account",
        "stuck",
        "check",
        "setup",
        "ready",
        "show",
    ] {
        k.insert(format!("line.needs_you.{kind}"));
    }
    for line in [
        "not_yet.category",
        "off.person",
        "problem.failures",
        "working.starting",
        "working.working",
        "working.stopping",
        "waiting_turn.queued",
        "waiting_turn.store",
        "problem.failed",
        "problem.limit",
        "problem.class_mismatch",
        "problem.refused",
        "problem.budget",
        "problem.kind",
        "all_set.tried_today",
        "all_set.new",
        "all_set.done",
        "all_set.nothing",
        "all_set.stopped_early",
    ] {
        k.insert(format!("line.{line}"));
    }
    for end in [
        "completed",
        "nothing_to_do",
        "declined",
        "handed_back",
        "class_mismatch",
        "refused",
        "limit",
        "effect_unknown",
        "failed",
        "cancelled",
        "lapsed",
        "interrupted",
        "deleted",
        "budget",
        "kind",
    ] {
        k.insert(format!("line.run.{end}"));
    }
    for key in [
        "line.level.low",
        "line.level.medium",
        "line.level.high",
        "line.runs_on.on_device",
        "line.asks.observe",
        "line.asks.draft_and_confirm",
        "line.asks.unattended",
        "line.time",
        "line.files",
        "name.with_site",
        "confirm.mixed.title",
        "confirm.mixed.reject",
        "confirm.mixed.allow",
        "confirm.new_site",
        "confirm.changed",
        "confirm.present",
        "note.no_service",
        "note.roster_full",
        "tray.needs",
        "item.link_account.origin",
        "item.sign_in.bind",
        "choice.sign_in.bind_decline",
        "choice.sign_in.bind_allow",
        "item.setup.not_taken",
    ] {
        k.insert(key.to_owned());
    }
    for c in Category::ALL {
        k.insert(format!("category.{}", c.name()));
    }
    for c in ["run_now", "stop", "switch_off", "switch_on", "delete"] {
        k.insert(format!("control.{c}"));
    }
    let choices = [
        ("sign_in", ["decline", "done"]),
        ("link_account", ["decline", "done"]),
        ("stuck", ["decline", "done"]),
        ("check", ["try_again", "went_through"]),
        ("setup", ["none", "choose"]),
        ("ready", ["skip", "start"]),
        ("show", ["decline", "done"]),
    ];
    for (kind, two) in choices {
        k.insert(format!("item.{kind}.title"));
        k.insert(format!("item.{kind}.text"));
        for c in two {
            k.insert(format!("choice.{kind}.{c}"));
        }
    }
    for p in [
        "start",
        "payee",
        "instrument",
        "reply_senders",
        "workbook",
        "sheet",
        "range",
        "source",
        "seller",
    ] {
        k.insert(format!("param.{p}"));
    }
    let fields = [
        ("payment", vec!["payee", "amount", "instrument", "origin"]),
        (
            "purchase",
            vec!["seller", "items", "amount", "instrument", "origin"],
        ),
        ("send", vec!["to", "subject", "body"]),
        ("delete", vec!["items"]),
        ("submit", vec!["origin", "fields"]),
        ("sign", vec!["document", "party"]),
    ];
    for (class, names) in &fields {
        for n in names {
            k.insert(format!("field.{class}.{n}"));
        }
        for e in [
            "title",
            "allow",
            "reject",
            "allow_many",
            "reject_many",
            "action",
        ] {
            k.insert(format!("confirm.{class}.{e}"));
        }
    }
    for s in ["you", "site", "agent", "halvo"] {
        k.insert(format!("from.{s}"));
    }
    for m in 1..=12 {
        k.insert(format!("month.{m}"));
    }
    for class in [
        "payment",
        "purchase",
        "send",
        "delete",
        "submit",
        "sign",
        "local_edit",
    ] {
        k.insert(format!("run.effect.{class}"));
    }
    for n in ["file", "range"] {
        k.insert(format!("field.local_edit.{n}"));
    }
    for key in [
        "say.item.new",
        "say.item.gone",
        "param_typed.full",
        "param_typed.too_long",
    ] {
        k.insert(key.to_owned());
    }
    k
}

#[test]
fn the_table_holds_every_key_of_the_families_of_i1_1_once_and_no_other() {
    let mut seen = BTreeSet::new();
    for (key, text) in KEYS {
        assert!(seen.insert(key.to_string()), "{key} stands twice");
        assert!(!text.is_empty(), "{key} has no sentence");
        assert_eq!(text.trim(), *text, "{key} has a space at an end");
    }
    let expected = expected_keys();
    let missing: Vec<&String> = expected.difference(&seen).collect();
    let extra: Vec<&String> = seen.difference(&expected).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "missing {missing:?}, extra {extra:?}"
    );
    assert_eq!(seen.len(), 214);
}

fn placeholders(text: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}').expect("a closed brace") + open;
        set.insert(rest[open + 1..close].to_owned());
        rest = &rest[close + 1..];
    }
    set
}

#[test]
fn placeholders_are_the_ones_that_the_contract_names() {
    let mut want: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for s in Status::ALL {
        want.insert(format!("say.{}", s.name()), vec!["name"]);
    }
    want.insert("line.time".into(), vec!["minutes"]);
    want.insert("name.with_site".into(), vec!["category", "site"]);
    want.insert("tray.needs".into(), vec!["count"]);
    for class in ["payment", "purchase", "send", "delete", "submit", "sign"] {
        want.insert(format!("confirm.{class}.allow_many"), vec!["count"]);
        want.insert(format!("confirm.{class}.reject_many"), vec!["count"]);
    }
    for key in ["title", "reject", "allow"] {
        want.insert(format!("confirm.mixed.{key}"), vec!["count"]);
    }
    for kind in ItemKind::ALL.iter().filter(|k| **k != ItemKind::Confirm) {
        want.insert(format!("item.{}.title", kind.name()), vec!["name"]);
    }
    want.insert("item.sign_in.bind".into(), vec!["origin"]);
    want.insert("say.item.new".into(), vec!["name"]);
    want.insert("say.item.gone".into(), vec!["name"]);
    for (key, text) in KEYS {
        let got: Vec<String> = placeholders(text).into_iter().collect();
        let expect: Vec<String> = want
            .get(*key)
            .map(|v| v.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default();
        assert_eq!(got, expect, "{key}: {text}");
    }
}

#[test]
fn text_fills_placeholders_and_refuses_what_it_cannot() {
    let got = words::text(
        "say.needs_you",
        &[("name", "Paying a utility bill on Site1")],
    );
    assert!(got.unwrap().contains("Paying a utility bill on Site1"));
    assert_eq!(words::text("say.needs_you", &[]), Err(Cause::Malformed));
    assert_eq!(words::text("no.such.key", &[]), Err(Cause::Malformed));
    assert_eq!(
        words::text("status.off", &[("unused", "x")]),
        Ok("Off".to_owned())
    );
    assert_eq!(
        words::text(
            "name.with_site",
            &[("category", "{site}"), ("site", "Site1")]
        ),
        Ok(words::text(
            "name.with_site",
            &[("category", "{site}"), ("site", "Site1")]
        )
        .unwrap())
    );
    let filled = words::text(
        "name.with_site",
        &[("category", "{site}"), ("site", "Site1")],
    )
    .unwrap();
    assert!(filled.contains("{site}") && filled.contains("Site1"));
}

#[test]
fn fixed_texts_stand_word_for_word() {
    let t = |k: &str| words::text(k, &[]).unwrap();
    let statuses = [
        ("needs_you", "Needs you"),
        ("problem", "Problem"),
        ("working", "Working"),
        ("waiting_turn", "Waiting its turn"),
        ("all_set", "All set"),
        ("off", "Off"),
        ("not_yet", "Not yet"),
    ];
    for (key, word) in statuses {
        assert_eq!(t(&format!("status.{key}")), word);
    }
    let categories = [
        ("utility_bill_payment", "Paying a utility bill"),
        ("tenant_rent", "Handling a tenant's rent"),
        ("lease_renewal_or_change", "Renewing or changing a lease"),
        (
            "tenant_application_review",
            "Reviewing a tenant application",
        ),
        ("email", "Writing or answering email"),
        ("spreadsheet_fill", "Filling in a spreadsheet"),
        ("shop_order", "Ordering from a shop"),
        ("calendar_cleanup", "Tidying a calendar"),
        ("ride_order", "Ordering a ride"),
        ("daily_chart_check", "Checking a chart"),
    ];
    for (key, text) in categories {
        assert_eq!(t(&format!("category.{key}")), text);
    }
    let controls = [
        ("payment", "Pay", "Don't pay"),
        ("purchase", "Order", "Don't order"),
        ("sign", "Sign", "Don't sign"),
        ("send", "Send", "Don't send"),
        ("delete", "Remove", "Keep"),
        ("submit", "Submit", "Don't submit"),
    ];
    for (class, allow, reject) in controls {
        assert_eq!(t(&format!("confirm.{class}.allow")), allow);
        assert_eq!(t(&format!("confirm.{class}.reject")), reject);
    }
    for (n, m) in [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(t(&format!("month.{}", n + 1)), *m);
    }
    let classes: Vec<&str> = Class::ALL.iter().map(|c| c.name()).collect();
    assert!(classes.contains(&"payment"));
}

#[test]
fn every_status_line_has_its_key_and_its_status() {
    let mut keys = BTreeSet::new();
    for line in StatusLine::all() {
        let key = line.key();
        assert!(words::text(&key, &[]).is_ok(), "{key}");
        let status = key.split('.').nth(1).unwrap();
        assert_eq!(line.status().name(), status, "{key}");
        assert!(keys.insert(key));
    }
    assert_eq!(keys.len(), 27);
}

#[test]
fn a_day_is_a_day_number_a_month_name_and_a_year() {
    assert_eq!(words::day(20_760), "3 November 2026");
    assert_eq!(words::day(0), "1 January 1970");
    assert_eq!(words::day(19_782), "29 February 2024");
    assert_eq!(words::day(11_017), "1 March 2000");
    assert_eq!(words::day(20_818), "31 December 2026");
}
