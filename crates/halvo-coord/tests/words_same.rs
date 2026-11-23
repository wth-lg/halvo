use halvo_coord::types::{Category, Cause, Class, ItemKind, Status, StatusLine};
use halvo_coord::words::{self, KEYS};
use std::collections::{BTreeMap, BTreeSet};
use std::process::Command;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../ui/tests/fixtures/coord/words.json"
);

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test starts the crate's own binary coord-words to read what it prints"
)]
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
#[expect(
    clippy::disallowed_methods,
    reason = "the test starts the crate's own binary coord-words to read what it prints"
)]
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
        "working.kill_stuck_mac",
        "working.kill_stuck_windows",
        "working.deleting",
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
        "confirm.field",
        "note.no_service",
        "note.out_of_date",
        "note.roster_full",
        "tray.needs",
        "item.link_account.origin",
        "item.sign_in.bind",
        "item.sign_in.site",
        "item.sign_in.site_kept",
        "item.sign_in.text_kept",
        "choice.sign_in.bind_decline",
        "choice.sign_in.bind_allow",
        "item.setup.not_taken",
        "confirm.asked_by",
        "confirm.asked_by_place",
    ] {
        k.insert(key.to_owned());
    }
    for c in Category::ALL {
        k.insert(format!("category.{}", c.name()));
    }
    for c in [
        "run_now",
        "stop",
        "take_back",
        "switch_off",
        "switch_on",
        "use_model",
        "this_computer_only",
        "stop_sign_in",
        "delete",
    ] {
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
        for end in ["unshown", "not_done", "refused"] {
            k.insert(format!("run.effect.{class}.{end}"));
        }
    }
    for end in ["unshown", "not_done", "refused"] {
        k.insert(format!("run.effect.{end}"));
    }
    for class in [
        "payment",
        "purchase",
        "send",
        "submit",
        "sign",
        "local_edit",
    ] {
        k.insert(format!("run.effect.{class}.partial"));
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
        k.insert(format!("run.effect.{class}.unknown"));
    }
    k.insert("run.effect.unknown".to_owned());
    k.insert("line.run.stopped".to_owned());
    for key in [
        "line.quitting",
        "line.problem.record",
        "line.problem.quit",
        "card.records_dropped",
        "line.off.kept",
        "line.all_set.nothing_first",
        "say_again",
        "line.run.answer_lost",
        "line.problem.sign_in_refused",
        "line.problem.site_declined",
        "line.problem.no_browser",
        "line.problem.browser_start",
        "line.problem.no_channel",
    ] {
        k.insert(key.to_owned());
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
    for key in U34_TEXTS.iter().map(|(k, _)| *k) {
        if key != "line.working.deleting" {
            k.insert(key.to_owned());
        }
    }
    for c in Cause::ALL {
        k.insert(format!("refused.{}", c.name()));
    }
    for key in [
        "card.history_busy",
        "card.read_again",
        "line.needs_you.setup_owed",
        "refused.switch_not_saved",
        "item.setup.overlap",
        "item.setup.overlap_more",
        "item.setup.overlap_unread",
        "refused.overlap_named",
        "refused.overlap_more",
        "ask.stop_sign_in.title",
        "ask.stop_sign_in.stops",
        "ask.stop_sign_in.next",
        "ask.stop_sign_in.keep",
        "refused.sign_in_not_finished",
        "ask.use_model.title",
        "ask.use_model.sends",
        "ask.use_model.models",
        "ask.use_model.never",
        "ask.use_model.bills",
        "ask.use_model.keep",
        "ask.use_model.allow",
        "refused.use_model_not_finished",
    ] {
        k.insert(key.to_owned());
    }
    k
}

/// The texts of the question of a row's Delete, of its two notices and of the line of a delete
/// that waits for its run to end (U3.4; ruling RD-19 (c), (d) (3), (6)), each as it stands; the
/// texts of the closed list that the unit's plain readers read.
const U34_TEXTS: [(&str, &str); 27] = [
    (
        "line.working.deleting",
        "Halvo is stopping this agent and will then remove it.",
    ),
    ("ask.delete.title", "Remove “{name}” from Halvo?"),
    ("ask.delete.next", "Its next task is set for {day}."),
    (
        "ask.delete.setup",
        "Halvo removes what you set up and answered for it, and what it learned.",
    ),
    (
        "ask.delete.sign_in",
        "It also removes its schedule and what it kept on this computer to sign in.",
    ),
    (
        "ask.delete.records",
        "It removes the record of each task and of what it did.",
    ),
    ("ask.delete.final", "You cannot bring it back."),
    (
        "ask.delete.done",
        "Nothing it already paid, ordered or sent is taken back, and nothing at a website is cancelled.",
    ),
    (
        "ask.delete.copies",
        "The copies that Halvo made beside your workbooks stay.",
    ),
    (
        "ask.delete.starts",
        "If it starts a task before you answer, Halvo stops it first and removes it once it has stopped.",
    ),
    (
        "ask.delete.record",
        "Halvo keeps no record of this once the agent is removed.",
    ),
    ("ask.delete.keep", "Keep it"),
    (
        "ask.effect.payment",
        "Halvo does not know whether it made this payment.",
    ),
    (
        "ask.effect.purchase",
        "Halvo does not know whether it placed this order.",
    ),
    (
        "ask.effect.send",
        "Halvo does not know whether it sent this message.",
    ),
    (
        "ask.effect.delete",
        "Halvo does not know whether it removed this.",
    ),
    (
        "ask.effect.submit",
        "Halvo does not know whether it sent this form.",
    ),
    (
        "ask.effect.sign",
        "Halvo does not know whether it signed this document.",
    ),
    (
        "ask.effect.local_edit",
        "Halvo does not know whether it changed this file.",
    ),
    (
        "ask.effect.other",
        "Halvo does not know whether one step of its last task happened.",
    ),
    ("ask.working.title", "“{name}” is working now"),
    (
        "ask.working.text",
        "Stop it first, and you can remove it once it has stopped.",
    ),
    ("ask.working.keep", "Let it work"),
    ("ask.stopping.title", "“{name}” is stopping"),
    ("ask.stopping.text", "You can remove it once it has stopped."),
    ("ask.stopping.done", "OK"),
    (
        "line.needs_you.asks_again",
        "You cancelled its start, so it asks you again before it starts.",
    ),
];

/// Each text of U3.4 stands word for word, holds at most 20 words, and names neither the
/// workflow nor the word delete, which the glossary reserves for the workflow run (I1.6); the
/// question says no more than that the agent is removed from Halvo (the workflow run's S-82 and
/// S-83) and promises nothing at Quit (ruling RD-147 (5)(i)).
#[test]
fn u34_the_texts_of_the_delete_stand_word_for_word() {
    for (key, want) in U34_TEXTS {
        let got = KEYS.iter().find(|(k, _)| *k == key).map(|(_, t)| *t);
        assert_eq!(got, Some(want), "{key}");
        assert!(want.split_whitespace().count() <= 20, "{key}");
        let lower = want.to_lowercase();
        for word in ["delete", "deleted", "workflow", "quit"] {
            assert!(
                !lower
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|w| w == word),
                "{key}: {want}"
            );
        }
    }
}

/// The texts of the item of a failed step and of its row, U3.19: whatever the person answers,
/// the run ends and the agent's next run starts at its first step (the agents contract A6.4), so
/// no text says that the agent goes on after the person's help; the two answers stay.
const U319_TEXTS: [(&str, &str); 5] = [
    (
        "line.needs_you.stuck",
        "It could not get past one step and waits for your answer.",
    ),
    ("item.stuck.title", "“{name}” could not get past one step"),
    (
        "item.stuck.text",
        "You can finish the task yourself in its own window, then choose Done, or choose Stop this time. Either way it tries again next time.",
    ),
    ("choice.stuck.decline", "Stop this time"),
    ("choice.stuck.done", "Done"),
];

#[test]
fn u319_the_texts_of_a_failed_step_stand_word_for_word() {
    for (key, want) in U319_TEXTS {
        let got = KEYS.iter().find(|(k, _)| *k == key).map(|(_, t)| *t);
        assert_eq!(got, Some(want), "{key}");
    }
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
    assert_eq!(seen.len(), 342);
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
    want.insert("item.sign_in.site".into(), vec!["origin"]);
    want.insert("item.sign_in.site_kept".into(), vec!["origin"]);
    want.insert("say.item.new".into(), vec!["name"]);
    want.insert("say.item.gone".into(), vec!["name"]);
    want.insert("confirm.asked_by".into(), vec!["name"]);
    want.insert("confirm.field".into(), vec!["label", "source", "value"]);
    want.insert("card.records_dropped".into(), vec!["count"]);
    want.insert("say_again".into(), vec!["name"]);
    want.insert("confirm.asked_by_place".into(), vec!["count", "n", "name"]);
    for key in [
        "ask.delete.title",
        "ask.working.title",
        "ask.stopping.title",
        "ask.stop_sign_in.title",
    ] {
        want.insert(key.into(), vec!["name"]);
    }
    want.insert("ask.delete.next".into(), vec!["day"]);
    want.insert("ask.use_model.sends".into(), vec!["host"]);
    want.insert("ask.use_model.models".into(), vec!["models"]);
    want.insert(
        "ask.use_model.bills".into(),
        vec!["amount", "host", "period"],
    );
    for key in [
        "item.setup.overlap",
        "item.setup.overlap_more",
        "refused.overlap_named",
        "refused.overlap_more",
    ] {
        want.insert(key.into(), vec!["name", "senders"]);
    }
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

/// The three texts of a stuck or unsaved record, written again from the plain readers' findings
/// (U2.4r; rulings RD-116 W1, RD-118), `line.working.kill_stuck` with a text per system.
#[test]
fn u24r_the_three_texts_stand_word_for_word() {
    let t = |k: &str, pairs: &[(&str, &str)]| words::text(k, pairs).unwrap();
    assert_eq!(
        t("line.working.kill_stuck_mac", &[]),
        "Its task may still run. Right-click Halvo in the Dock, choose Quit. Halvo stops all tasks; that can take seconds."
    );
    assert_eq!(
        t("line.working.kill_stuck_windows", &[]),
        "Its task may still run. Click Halvo's icon by the clock (or under ^), then Quit Halvo."
    );
    let windows = t("line.working.kill_stuck_windows", &[]);
    assert_eq!(
        promises_an_end_or_a_time(&windows),
        Vec::<String>::new(),
        "U4.6: text 2 promises no end and no time at Quit (ruling RD-147 (5)(i))"
    );
    assert!(
        !sends_the_person_to_reopen(&windows),
        "RD-148, RD-150: no text tells a person to quit and reopen Halvo while Quit can leave a \
         run live"
    );
    assert_eq!(
        t("line.problem.record", &[]),
        "Halvo can't read or save this agent's record. The agent starts nothing new. Stop works if shown."
    );
    assert_eq!(
        t("card.records_dropped", &[("count", "3")]),
        "After Halvo started, 3 of this agent's records went unsaved. Its steps may have happened; check outside Halvo before repeating."
    );
    for key in [
        "line.working.kill_stuck_mac",
        "line.working.kill_stuck_windows",
        "line.problem.record",
        "card.records_dropped",
    ] {
        let words = t(key, &[("count", "3")]).split_whitespace().count();
        assert!(words <= 20, "{key}: {words} words");
    }
}

/// The control that takes a queued start back says that this one start is gone and promises no
/// later start, and it shares no text with another control or with a choice of an item (U2.4e;
/// rulings RD-54 (h), RD-55 (b), and RD-149's note of 2026-11-20T20:02:32Z).
#[test]
fn u24e_the_take_back_stands_word_for_word_and_apart_from_every_other_control_and_choice() {
    let take_back = words::text("control.take_back", &[]).unwrap();
    assert_eq!(take_back, "Cancel this start");
    let others: Vec<&str> = KEYS
        .iter()
        .filter(|(k, _)| {
            (k.starts_with("control.") || k.starts_with("choice.")) && *k != "control.take_back"
        })
        .map(|(_, t)| *t)
        .collect();
    assert!(others.len() > 10, "{others:?}");
    assert!(
        others.iter().all(|t| !t.eq_ignore_ascii_case(&take_back)),
        "{others:?}"
    );
}

/// Each text of `texts` stands word for word under its key, in at most 20 words.
fn pinned(texts: &[(&str, &str)]) {
    for (key, text) in texts {
        assert_eq!(words::text(key, &[]).unwrap(), *text, "{key}");
        let words = text.split_whitespace().count();
        assert!(words <= 20, "{key}: {words} words");
    }
}

/// The texts of an entry that shows only the fields that the reader took whole (ruling RD-52 (h)
/// 4), U2.4q.
#[test]
fn u24q_the_six_partial_texts_stand_word_for_word() {
    pinned(&[
        (
            "run.effect.payment.partial",
            "It made this payment, but Halvo shows only the details it could read.",
        ),
        (
            "run.effect.purchase.partial",
            "It placed this order, but Halvo shows only the details it could read.",
        ),
        (
            "run.effect.send.partial",
            "It sent this message, but Halvo shows only the details it could read.",
        ),
        (
            "run.effect.submit.partial",
            "It sent this form, but Halvo shows only the details it could read.",
        ),
        (
            "run.effect.sign.partial",
            "It signed this document, but Halvo shows only the details it could read.",
        ),
        (
            "run.effect.local_edit.partial",
            "It changed a file, but Halvo shows only the details it could read.",
        ),
    ]);
}

/// The texts of the entry of an effect whose last state is unknown (ruling RD-126 item 10),
/// U2.4q.
#[test]
fn u24q_the_eight_unknown_texts_stand_word_for_word() {
    pinned(&[
        (
            "run.effect.payment.unknown",
            "A payment may have been made; Halvo could not tell. Check your bank or the site before you pay again.",
        ),
        (
            "run.effect.purchase.unknown",
            "An order may have been placed; Halvo could not tell. Check the shop's order page before you order again.",
        ),
        (
            "run.effect.send.unknown",
            "A message may have been sent; Halvo could not tell. Look in your sent messages before you send it again.",
        ),
        (
            "run.effect.delete.unknown",
            "Something may have been removed; Halvo could not tell. Look where this agent works for anything missing.",
        ),
        (
            "run.effect.submit.unknown",
            "A form may have been sent; Halvo could not tell. Check the site before you send it again.",
        ),
        (
            "run.effect.sign.unknown",
            "A document may have been signed for you; Halvo could not tell. Check your email before you sign it again.",
        ),
        (
            "run.effect.local_edit.unknown",
            "A file may have been changed; Halvo could not tell. Open it and look before you change it again.",
        ),
        (
            "run.effect.unknown",
            "One step may have been done; Halvo could not tell. Check where this agent works before you run it again.",
        ),
    ]);
}

const WORDS_OF_AN_END: [&str; 41] = [
    "stop",
    "stops",
    "stopped",
    "stopping",
    "end",
    "ends",
    "ended",
    "ending",
    "finish",
    "finishes",
    "finished",
    "finishing",
    "done",
    "complete",
    "completes",
    "completed",
    "completing",
    "halt",
    "halts",
    "halted",
    "halting",
    "cease",
    "ceases",
    "ceased",
    "ceasing",
    "cancel",
    "cancels",
    "cancelled",
    "canceled",
    "cancelling",
    "canceling",
    "kill",
    "kills",
    "killed",
    "killing",
    "close",
    "closes",
    "closed",
    "closing",
    "terminate",
    "terminated",
];

const WORDS_OF_A_TIME: [&str; 15] = [
    "second",
    "seconds",
    "minute",
    "minutes",
    "hour",
    "hours",
    "moment",
    "moments",
    "instant",
    "instantly",
    "immediately",
    "quickly",
    "soon",
    "shortly",
    "promptly",
];

fn promises_an_end_or_a_time(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| {
            WORDS_OF_AN_END.contains(word)
                || WORDS_OF_A_TIME.contains(word)
                || word.contains(|c: char| c.is_numeric())
        })
        .map(str::to_string)
        .collect()
}

#[test]
fn u46_the_guard_of_text_2_reads_words_not_letters() {
    let text =
        "Its task may still run. Click Halvo's icon by the clock (or under ^), then Quit Halvo.";
    let promise = format!("{text} All tasks finish quickly.");
    let warning = format!("{text} Check what it did before you repeat a send.");
    assert_ne!(
        promises_an_end_or_a_time(&promise),
        Vec::<String>::new(),
        "{promise}"
    );
    assert_eq!(
        promises_an_end_or_a_time(&warning),
        Vec::<String>::new(),
        "{warning}"
    );
}

const WORDS_OF_A_REOPEN: [&str; 4] = ["reopen", "reopens", "restart", "restarts"];

const PHRASES_OF_A_REOPEN: [&str; 2] = ["open halvo again", "start halvo again"];

fn sends_the_person_to_reopen(text: &str) -> bool {
    let lower = text.to_lowercase();
    PHRASES_OF_A_REOPEN
        .iter()
        .any(|phrase| lower.contains(phrase))
        || lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| WORDS_OF_A_REOPEN.contains(&word))
}

const TEXT_2: &str =
    "Its task may still run. Click Halvo's icon by the clock (or under ^), then Quit Halvo.";

fn named_by_the_guard(added: &str) -> Vec<String> {
    let text = format!("{TEXT_2} {added}");
    assert_eq!(promises_an_end_or_a_time(TEXT_2), Vec::<String>::new());
    promises_an_end_or_a_time(&text)
}

fn refused_as_a_reopen(added: &str) -> bool {
    assert!(!sends_the_person_to_reopen(TEXT_2));
    sends_the_person_to_reopen(&format!("{TEXT_2} {added}"))
}

#[test]
fn u46_the_guard_names_a_word_that_holds_a_digit() {
    assert_eq!(named_by_the_guard("Then r7."), ["r7"]);
}

#[test]
fn u46_the_guard_names_an_end_word_alone() {
    assert_eq!(named_by_the_guard("Then it finished."), ["finished"]);
}

#[test]
fn u46_the_guard_names_a_time_word_alone() {
    assert_eq!(named_by_the_guard("It is read promptly."), ["promptly"]);
}

#[test]
fn u46_the_guard_reads_a_word_beside_punctuation() {
    assert_eq!(named_by_the_guard("Then finish!"), ["finish"]);
}

#[test]
fn u46_the_guard_reads_a_word_in_capitals() {
    assert_eq!(named_by_the_guard("Then FINISH."), ["finish"]);
}

#[test]
fn u46_the_refusal_names_open_halvo_again() {
    assert!(refused_as_a_reopen("Then open Halvo again."));
}

#[test]
fn u46_the_guard_names_a_task_cancelled_right_away() {
    assert_ne!(
        named_by_the_guard("Every task is cancelled right away."),
        Vec::<String>::new()
    );
}

#[test]
fn u46_the_guard_names_halvo_closing_all_tasks_at_once() {
    assert_ne!(
        named_by_the_guard("Halvo closes all tasks at once."),
        Vec::<String>::new()
    );
}

#[test]
fn u46_the_guard_names_a_task_killed_when_you_quit() {
    assert_ne!(
        named_by_the_guard("Its task is killed when you quit."),
        Vec::<String>::new()
    );
}

#[test]
fn u46_the_guard_names_completing() {
    assert_eq!(
        named_by_the_guard("Its task is completing."),
        ["completing"]
    );
}

#[test]
fn u46_the_guard_names_halting() {
    assert_eq!(named_by_the_guard("Its task is halting."), ["halting"]);
}

#[test]
fn u46_the_guard_names_ceasing() {
    assert_eq!(named_by_the_guard("Its task is ceasing."), ["ceasing"]);
}

#[test]
fn u46_the_guard_names_a_full_width_digit() {
    assert_eq!(named_by_the_guard("Then r\u{ff15}."), ["r\u{ff15}"]);
}

#[test]
fn u46_the_refusal_names_open_halvo_again_with_a_capital() {
    assert!(refused_as_a_reopen("Open Halvo again."));
}

#[test]
fn u46_the_refusal_names_reopen_halvo() {
    assert!(refused_as_a_reopen("Then reopen Halvo."));
}

#[test]
fn u46_the_refusal_names_start_halvo_again() {
    assert!(refused_as_a_reopen("Then start Halvo again."));
}

/// The hold `failures` says what happened and its way out (ruling RD-81).
#[test]
fn rd81_the_line_of_the_hold_failures_names_its_way_out() {
    let line = words::text("line.problem.failures", &[]).unwrap();
    let switch_on = words::text("control.switch_on", &[]).unwrap();
    assert_eq!(
        line,
        "Halvo switched it off after several tries in a row did not work. To try again, choose Switch on."
    );
    assert!(line.contains(&switch_on));
}

/// The two choices of a second step (ruling RD-24 (b)): the one that allows says that the site
/// stays allowed when the agent runs again, and the one that refuses says that the task ends.
#[test]
fn rd24b_the_second_steps_two_choices_say_what_follows_each() {
    let t = |k: &str| words::text(k, &[]).unwrap();
    assert_eq!(
        t("choice.sign_in.bind_allow"),
        "Allow this site now and whenever this agent runs again"
    );
    assert_eq!(
        t("choice.sign_in.bind_decline"),
        "Don't allow this site, and end this task"
    );
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
    assert_eq!(keys.len(), 31);
}

#[test]
fn a_day_is_a_day_number_a_month_name_and_a_year() {
    assert_eq!(words::day(20_760), "3 November 2026");
    assert_eq!(words::day(0), "1 January 1970");
    assert_eq!(words::day(19_782), "29 February 2024");
    assert_eq!(words::day(11_017), "1 March 2000");
    assert_eq!(words::day(20_818), "31 December 2026");
}

/// The answer at the press of a Switch off whose `hold_set` the owner refused under class B, in
/// the coordinator's words of 2026-11-21T05:03:14Z (U2.4s; ruling RD-155 (1)), which the row's
/// line said until term (5) of RD-161 R1 moved it to the answer beside the control (U3.12).
#[test]
fn u24s_the_answer_of_a_switch_off_that_could_not_be_saved_stands_in_the_coordinators_words() {
    assert_eq!(
        words::text("refused.switch_not_saved", &[]).unwrap(),
        "Halvo is stopping the task but could not switch this agent off. It may start again: press Switch off again."
    );
}

/// The two payment texts that ride in U2.4w: the choice that refuses several payments says that
/// it refuses all of them, in at most 20 words with its placeholder, and the label of the card
/// that pays stays `Pay with`.
#[test]
fn u24w_the_two_payment_texts_stand_word_for_word() {
    let table = |key: &str| {
        KEYS.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, text)| *text)
            .unwrap()
    };
    let reject = table("confirm.payment.reject_many");
    assert_eq!(reject, "Don't pay any of these {count}");
    assert!(reject.split_whitespace().count() <= 20, "{reject}");
    assert_eq!(
        words::text("confirm.payment.reject_many", &[("count", "3")]).unwrap(),
        "Don't pay any of these 3"
    );
    assert_eq!(table("field.payment.instrument"), "Pay with");
}
