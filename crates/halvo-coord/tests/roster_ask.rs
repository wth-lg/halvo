use halvo_coord::mem::Config;
use halvo_coord::roster::{deleting_row, gone_row, row, row_with_session, status};
use halvo_coord::types::fixtures::{facts, State};
use halvo_coord::types::{
    Class, Control, Doing, EffectId, Facts, ItemId, ItemKind, Kept, Limits, Live, Name, OwnItem,
    OwnItemKind, Row, RunId, StatusLine, System, Trigger, Unknown,
};
use halvo_coord::words;
use serde_json::{json, Value};

const NAME: &str = "Pay bills on Site1";

fn text(key: &str) -> String {
    words::text(key, &[]).unwrap()
}

fn named(key: &str) -> String {
    words::text(key, &[("name", NAME)]).unwrap()
}

fn ask_of(f: &Facts<Config>) -> Value {
    let line = status(f, &Limits::PRODUCT, None);
    let r = row(f, line, NAME, None, &Limits::PRODUCT).unwrap();
    serde_json::to_value(&r).unwrap()["ask"].clone()
}

fn idle() -> Facts<Config> {
    facts(State::Ten).remove(0)
}

fn working() -> Facts<Config> {
    facts(State::Working).remove(0)
}

fn kept(name: &str, value: &str) -> Kept {
    Kept {
        name: Name::parse(name).unwrap(),
        value: vec![value.to_owned()],
    }
}

fn payment(f: &Facts<Config>) -> Unknown {
    Unknown {
        run: f.runs.first().map_or_else(
            || RunId::parse(&format!("{:032x}", 0x7711)).unwrap(),
            |r| r.run.clone(),
        ),
        effect: EffectId::parse(&format!("{:032x}", 0xe7)).unwrap(),
        day: 20_759,
        class: Class::Payment,
        kept: vec![
            kept("payee", "City Power"),
            kept("amount", "42.10 USD"),
            kept("instrument", "Card ending 4242"),
            kept("origin", "https://pay.site1.example:443"),
        ],
    }
}

const DELETE_TEXT: [&str; 7] = [
    "ask.delete.setup",
    "ask.delete.sign_in",
    "ask.delete.records",
    "ask.delete.final",
    "ask.delete.done",
    "ask.delete.copies",
    "ask.delete.starts",
];

#[test]
fn the_row_of_an_agent_with_no_live_run_holds_the_question_of_its_delete_in_its_reply() {
    let f = idle();
    let ask = ask_of(&f);
    assert_eq!(ask["kind"], "delete", "{ask}");
    assert_eq!(ask["title"], json!(named("ask.delete.title")));
    let line = status(&f, &Limits::PRODUCT, None);
    let next = row(&f, line, NAME, None, &Limits::PRODUCT).unwrap().next;
    let mut want: Vec<String> = Vec::new();
    if let Some(day) = &next {
        want.push(words::text("ask.delete.next", &[("day", day)]).unwrap());
    }
    want.extend(DELETE_TEXT.iter().map(|k| text(k)));
    assert_eq!(ask["text"], json!(want));
    assert_eq!(ask["effects"], json!([]));
    assert_eq!(ask["record"], Value::Null);
    assert_eq!(
        ask["controls"],
        json!([
            {"act": "reject", "text": text("ask.delete.keep")},
            {"act": "allow", "text": text("control.delete")}
        ])
    );
}

/// r4 U2.4y, sentence 1: an idle agent whose member holds a stored session offers
/// `stop_sign_in` before `delete`, with its question in `ask_stop_sign_in`, each text as the
/// coordinator ruled it; with no session, a live run or a queued start the row holds neither.
#[test]
fn the_row_of_an_idle_agent_with_a_kept_sign_in_offers_stop_sign_in_before_delete_with_its_question(
) {
    let f = idle();
    assert!(matches!(f.doing, Doing::Idle(_)), "{:?}", f.doing);
    let line = status(&f, &Limits::PRODUCT, None);
    let today = row(&f, line, NAME, None, &Limits::PRODUCT).unwrap();
    let kept = row_with_session(&f, true, line, NAME, None, &Limits::PRODUCT).unwrap();
    let names = |r: &Row| r.controls.iter().map(|c| c.control).collect::<Vec<_>>();
    let mut want = names(&today);
    assert_eq!(want.last(), Some(&Control::Delete), "{want:?}");
    want.insert(want.len() - 1, Control::StopSignIn);
    assert_eq!(
        names(&kept),
        want,
        "the controls of today with stop_sign_in before delete"
    );
    let control = kept
        .controls
        .iter()
        .find(|c| c.control == Control::StopSignIn);
    assert_eq!(
        control.map(|c| c.text.as_str()),
        Some("Stop using its sign-in")
    );
    let value = serde_json::to_value(&kept).unwrap();
    assert_eq!(
        value["ask_stop_sign_in"],
        json!({
            "kind": "stop_sign_in",
            "title": "Stop using the sign-in of “Pay bills on Site1”?",
            "text": [
                "Halvo will stop using the sign-in that it keeps for this agent, and removes it within 35 days.",
                "The next time the agent runs, it asks you to sign in again."
            ],
            "effects": [],
            "record": null,
            "controls": [
                {"act": "reject", "text": "Keep the sign-in"},
                {"act": "allow", "text": "Stop using its sign-in"}
            ]
        })
    );
    assert_eq!(value["ask"], serde_json::to_value(&today).unwrap()["ask"]);
    let mut queued = idle();
    queued.doing = Doing::Queued(None);
    for (f, session, waiting) in [
        (idle(), false, None),
        (working(), true, None),
        (queued, true, Some(Trigger::Person)),
    ] {
        let line = status(&f, &Limits::PRODUCT, None);
        let r = row_with_session(&f, session, line, NAME, waiting, &Limits::PRODUCT).unwrap();
        assert!(!names(&r).contains(&Control::StopSignIn), "{:?}", f.doing);
        assert_eq!(
            serde_json::to_value(&r).unwrap()["ask_stop_sign_in"],
            Value::Null
        );
    }
}

#[test]
fn the_question_names_the_next_day_of_the_agent_where_it_has_one() {
    let f = idle();
    let line = status(&f, &Limits::PRODUCT, None);
    let r = row(&f, line, NAME, None, &Limits::PRODUCT).unwrap();
    let day = r
        .next
        .clone()
        .expect("the agent of the state ten has a next day");
    let ask = serde_json::to_value(&r).unwrap()["ask"].clone();
    assert_eq!(
        ask["text"][0],
        json!(words::text("ask.delete.next", &[("day", &day)]).unwrap())
    );
}

#[test]
fn a_row_whose_run_is_live_holds_the_notice_working_with_the_safe_control_first_and_the_stop() {
    let ask = ask_of(&working());
    assert_eq!(ask["kind"], "working", "{ask}");
    assert_eq!(ask["title"], json!(named("ask.working.title")));
    assert_eq!(ask["text"], json!([text("ask.working.text")]));
    assert_eq!(ask["effects"], json!([]));
    assert_eq!(
        ask["controls"],
        json!([
            {"act": "reject", "text": text("ask.working.keep")},
            {"act": "stop", "text": text("control.stop")}
        ])
    );
}

#[test]
fn a_row_whose_run_stops_holds_the_notice_stopping_with_one_control() {
    let mut f = working();
    f.doing = Doing::Live(Live::Stopping);
    let ask = ask_of(&f);
    assert_eq!(ask["kind"], "stopping", "{ask}");
    assert_eq!(ask["title"], json!(named("ask.stopping.title")));
    assert_eq!(ask["text"], json!([text("ask.stopping.text")]));
    assert_eq!(
        ask["controls"],
        json!([{"act": "done", "text": text("ask.stopping.done")}])
    );
}

#[test]
fn the_question_shows_each_effect_of_unknown_outcome_with_its_kept_fields_and_the_sentence_on_its_record(
) {
    let mut f = idle();
    f.unknown = vec![payment(&f)];
    f.doing = Doing::Idle(None);
    let ask = ask_of(&f);
    assert_eq!(ask["kind"], "delete");
    let effects = ask["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 1, "{ask}");
    assert_eq!(effects[0]["text"], json!(text("ask.effect.payment")));
    let labels: Vec<&str> = effects[0]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["label"].as_str().unwrap())
        .collect();
    let want: Vec<String> = ["payee", "amount", "instrument", "origin"]
        .iter()
        .map(|n| text(&format!("field.payment.{n}")))
        .collect();
    assert_eq!(labels, want);
    assert_eq!(
        effects[0]["fields"][3]["value"],
        json!(["pay.site1.example"])
    );
    assert_eq!(ask["record"], json!(text("ask.delete.record")));
}

#[test]
fn an_effect_of_unknown_outcome_with_no_kept_field_draws_its_sentence_alone() {
    let mut f = idle();
    let mut u = payment(&f);
    u.class = Class::Delete;
    u.kept = vec![kept("items", "a list")];
    f.unknown = vec![u];
    f.doing = Doing::Idle(None);
    let ask = ask_of(&f);
    assert_eq!(ask["effects"][0]["text"], json!(text("ask.effect.delete")));
    assert_eq!(ask["effects"][0]["fields"], json!([]));
}

#[test]
fn under_an_open_check_item_the_question_names_the_effect_in_that_items_words_and_adds_no_line() {
    let mut f = idle();
    f.unknown = vec![payment(&f)];
    f.doing = Doing::Idle(Some(OwnItem {
        item: ItemId::parse(&format!("{:032x}", 0x9911)).unwrap(),
        kind: OwnItemKind::Check,
    }));
    assert_eq!(
        status(&f, &Limits::PRODUCT, None),
        StatusLine::NeedsYou(ItemKind::Check)
    );
    let ask = ask_of(&f);
    let effects = ask["effects"].as_array().unwrap();
    assert_eq!(effects.len(), 1, "{ask}");
    assert_eq!(effects[0]["text"], json!(named("item.check.title")));
    assert_eq!(effects[0]["fields"].as_array().unwrap().len(), 4);
    assert_ne!(effects[0]["text"], json!(text("ask.effect.payment")));
}

#[test]
fn a_row_whose_last_run_left_an_effect_of_unknown_outcome_says_so_and_not_that_it_needs_nothing() {
    let mut f = idle();
    f.unknown = vec![payment(&f)];
    f.doing = Doing::Idle(None);
    let line = status(&f, &Limits::PRODUCT, None);
    assert_eq!(line, StatusLine::NeedsYou(ItemKind::Check), "{line:?}");
    assert_eq!(
        text(&line.key()),
        text("line.needs_you.check"),
        "the line says that it needs the person to say whether the task went through"
    );
}

#[test]
fn a_row_with_no_delete_holds_no_question() {
    let f = working();
    for r in [
        gone_row(&f, false, System::Mac, NAME).unwrap(),
        deleting_row(&f, NAME).unwrap(),
    ] {
        assert!(r.controls.is_empty());
        assert_eq!(serde_json::to_value(&r).unwrap()["ask"], Value::Null);
    }
}

#[test]
fn the_word_that_the_glossary_reserves_for_the_workflow_run_stands_in_no_text_of_the_question() {
    for key in halvo_coord::words::KEYS.iter().map(|(k, _)| *k) {
        if !key.starts_with("ask.") && key != "line.working.deleting" {
            continue;
        }
        let t = halvo_coord::words::KEYS
            .iter()
            .find(|(k, _)| *k == key)
            .unwrap()
            .1
            .to_lowercase();
        for word in ["delete", "workflow"] {
            assert!(
                !t.split(|c: char| !c.is_alphanumeric()).any(|w| w == word),
                "{key}: {t}"
            );
        }
    }
}
