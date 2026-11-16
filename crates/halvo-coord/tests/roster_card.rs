use halvo_coord::guard::EffectState;
use halvo_coord::mem::{Config, Method, Ports};
use halvo_coord::ports::{Owner, PortError};
use halvo_coord::roster::{card, names, plain_site};
use halvo_coord::types::fixtures::{facts, State};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Body, Class, Digest, EffectId, Event, Kept, Limits, Name, Outcome,
    PastEffect, PastRun, Reply, RunHistory, RunId, Workflow, WorkflowId,
};
use halvo_coord::types::{
    Answered, ApprovalId, DecisionEntry, EffectAnswer, ItemId, LapseCause, Lapsed, Raised, RunEnd,
    What,
};
use serde_json::Value;

const TRACKED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../ui/tests/fixtures/coord");

/// A committed payment with the four kept fields of its class (A-A9.3), as the fixture of the
/// state `card` holds it.
fn payment() -> PastEffect {
    let kept = |name: &str, value: &str| Kept {
        name: Name::parse(name).unwrap(),
        value: vec![value.into()],
    };
    PastEffect {
        effect: EffectId::parse(&format!("{:032x}", 0xe1)).unwrap(),
        digest: Digest::parse(&"ab".repeat(32)).unwrap(),
        class: Class::Payment,
        state: EffectState::Committed,
        cause: None,
        kept: vec![
            kept("payee", "City Power"),
            kept("amount", "42.10 USD"),
            kept("instrument", "Card ending 4242"),
            kept("origin", "https://site1.example:443"),
        ],
    }
}

/// A committed payment whose kept origin is a number form, which the reader refuses, as the
/// fixture of the state `card` holds it after the payment.
fn unreadable_payment() -> PastEffect {
    let mut p = payment();
    p.effect = EffectId::parse(&format!("{:032x}", 0xe4)).unwrap();
    p.kept[3].value = vec!["http://2130706433:80".into()];
    p
}

fn history(n: u32, outcome: Outcome) -> RunHistory {
    RunHistory {
        run: PastRun {
            run: RunId::parse(&format!("{:032x}", 0x300 + n)).unwrap(),
            day: 20_700 + n,
            outcome,
            unknown: 0,
        },
        effects: vec![payment()],
    }
}

fn owner() -> (Ports<Config>, WorkflowId, AgentId) {
    let f = facts(State::Card).remove(0);
    let ports: Ports<Config> = Ports::new(0, 20_760);
    ports
        .workflows
        .borrow_mut()
        .push(Workflow::Full(f.workflow.clone()));
    ports.agents.borrow_mut().push(Agent {
        workflow: f.workflow.id.clone(),
        agent: f.agent.clone(),
        config: f.config.clone(),
        state: AgentState::Active,
        slots: f.slots.clone(),
        session: false,
        last: f.runs.first().cloned(),
        unknown: vec![],
    });
    (ports, f.workflow.id.clone(), f.agent.clone())
}

#[test]
fn the_card_of_the_fixture_is_its_lines_and_the_runs_that_agent_history_gives() {
    let file: Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{TRACKED}/state-card.json")).unwrap(),
    )
    .unwrap();
    let f = facts(State::Card).remove(0);
    let (ports, workflow, agent) = owner();
    for past in &f.runs {
        ports.histories.borrow_mut().push((
            agent.clone(),
            RunHistory {
                run: past.clone(),
                effects: if past.outcome == Outcome::Completed {
                    vec![payment(), unreadable_payment()]
                } else {
                    vec![]
                },
            },
        ));
    }
    let read = ports.agent_history(&workflow, &agent, 5).unwrap();
    let all = facts(State::Card);
    let refs: Vec<_> = all.iter().collect();
    let name = &names(&refs, |w| w.site.as_deref().and_then(plain_site)).unwrap()[0];
    let got = card(&f, name, &read, &Limits::PRODUCT).unwrap();
    assert_eq!(serde_json::to_value(got).unwrap(), file["card"]);
}

#[test]
fn the_memory_owner_gives_the_newest_runs_first_and_at_most_the_count_asked() {
    let (ports, workflow, agent) = owner();
    for n in (0..7).rev() {
        ports
            .histories
            .borrow_mut()
            .push((agent.clone(), history(n, Outcome::Completed)));
    }
    let read = ports.agent_history(&workflow, &agent, 5).unwrap();
    assert_eq!(read.len(), 5);
    assert_eq!(read[0], history(6, Outcome::Completed));
    let f = facts(State::Card).remove(0);
    let limits = Limits {
        history_runs: 3,
        ..Limits::PRODUCT
    };
    let all = ports.agent_history(&workflow, &agent, 7).unwrap();
    let c = card(&f, "N", &all, &limits).unwrap();
    assert_eq!(c.runs.len(), 3);
    assert_eq!(c.runs[0].day, halvo_coord::words::day(20_706));
}

#[test]
fn the_memory_owner_refuses_a_workflow_that_is_gone_another_agent_and_a_refusal_set() {
    let (ports, workflow, agent) = owner();
    let other = AgentId::parse(&format!("{:032x}", 0x999)).unwrap();
    let gone = WorkflowId::parse(&format!("{:032x}", 0x998)).unwrap();
    let name = |e: PortError| match e {
        PortError::Refused(n) => n.as_str().to_owned(),
        other => format!("{other:?}"),
    };
    assert_eq!(
        name(ports.agent_history(&gone, &agent, 5).unwrap_err()),
        "no_workflow"
    );
    assert_eq!(
        name(ports.agent_history(&workflow, &other, 5).unwrap_err()),
        "binding"
    );
    ports.refuse(Method::AgentHistory, PortError::Busy);
    assert_eq!(
        ports.agent_history(&workflow, &agent, 5),
        Err(PortError::Busy)
    );
}

struct Silent;

impl Owner for Silent {
    type Config = Config;

    fn log(&self, _: &[Event]) -> Result<(), PortError> {
        Ok(())
    }

    fn history(&self, _: &AgentId, _: u32) -> Result<Vec<Event>, PortError> {
        Ok(vec![])
    }

    fn workflows(&self) -> Result<Vec<Workflow>, PortError> {
        Ok(vec![])
    }

    fn agents(&self) -> Result<Vec<Agent<Config>>, PortError> {
        Ok(vec![])
    }

    fn send(&self, _: &Body) -> Result<Reply, PortError> {
        Ok(Reply::Null)
    }
}

#[test]
fn an_owner_that_does_not_read_agent_history_answers_no_service() {
    let (_, workflow, agent) = owner();
    assert_eq!(
        Silent.agent_history(&workflow, &agent, 5),
        Err(PortError::NoService)
    );
}

#[test]
fn rd48h_a_card_holds_the_entries_of_two_classes_in_the_order_of_the_reply() {
    let f = facts(State::Card).remove(0);
    let send = PastEffect {
        effect: EffectId::parse(&format!("{:032x}", 0xe2)).unwrap(),
        digest: Digest::parse(&"cd".repeat(32)).unwrap(),
        class: Class::Send,
        state: EffectState::Committed,
        cause: None,
        kept: vec![Kept {
            name: Name::parse("to").unwrap(),
            value: vec!["Ann Lee".into()],
        }],
    };
    for (first, second) in [(payment(), send.clone()), (send.clone(), payment())] {
        let keys = |e: &PastEffect| format!("run.effect.{}", e.class.name());
        let wanted = [keys(&first), keys(&second)];
        let mut h = history(1, Outcome::Completed);
        h.effects = vec![first, second];
        let c = card(&f, "N", &[h], &Limits::PRODUCT).unwrap();
        let effects = serde_json::to_value(&c.runs[0].effects).unwrap();
        let got: Vec<&str> = effects
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["key"].as_str().unwrap())
            .collect();
        assert_eq!(got, wanted, "the entries in the order of the reply");
    }
}

#[test]
fn a_card_run_shows_its_committed_effects_alone_in_the_order_of_the_reply() {
    let f = facts(State::Card).remove(0);
    let mut h = history(1, Outcome::Completed);
    let mut refused = payment();
    refused.state = EffectState::Refused;
    h.effects = vec![refused, payment(), payment()];
    let c = card(&f, "N", &[h], &Limits::PRODUCT).unwrap();
    let effects = serde_json::to_value(&c.runs[0].effects).unwrap();
    assert_eq!(effects.as_array().map(Vec::len), Some(2), "{effects}");
    assert_eq!(effects[0]["key"], "run.effect.payment");
    let mut bad = payment();
    bad.kept.pop();
    let mut h = history(2, Outcome::Completed);
    h.effects = vec![bad];
    let c = card(
        &f,
        "N",
        &[h, history(1, Outcome::Completed)],
        &Limits::PRODUCT,
    )
    .unwrap();
    assert_eq!(keys(&c, 0), ["run.effect.payment.unshown"]);
    assert_eq!(
        keys(&c, 1),
        ["run.effect.payment"],
        "the good run beside it"
    );
}

fn keys(c: &halvo_coord::types::Card, run: usize) -> Vec<String> {
    c.runs[run].effects.iter().map(|e| e.key()).collect()
}

fn one_kept(name: &str, value: &str) -> Kept {
    Kept {
        name: Name::parse(name).unwrap(),
        value: vec![value.into()],
    }
}

fn committed(class: Class, kept: Vec<Kept>) -> PastEffect {
    PastEffect {
        class,
        kept,
        ..payment()
    }
}

/// The card of one run that holds `bad` between two good payments, beside a good run.
fn beside_a_good_run(bad: PastEffect) -> halvo_coord::types::Card {
    let f = facts(State::Card).remove(0);
    let mut h = history(2, Outcome::Completed);
    h.effects = vec![payment(), bad, payment()];
    card(
        &f,
        "N",
        &[h, history(1, Outcome::Completed)],
        &Limits::PRODUCT,
    )
    .unwrap()
}

#[test]
fn rd52f_each_cause_of_a_refused_committed_effect_gives_its_unshown_entry_and_no_kept_byte() {
    let secret = "Zq7 secret value";
    let mut swapped = payment();
    swapped.kept.swap(0, 1);
    swapped.kept[0].value = vec![secret.into()];
    let mut extra = payment();
    extra.kept.push(one_kept("note", secret));
    let mut origin = payment();
    origin.kept[0].value = vec![secret.into()];
    origin.kept[3].value = vec!["https://Site1.example:443".into()];
    let mut number = payment();
    number.kept[0].value = vec![secret.into()];
    number.kept[3].value = vec!["http://127.1:80".into()];
    let causes = [
        ("a refused origin", origin, "run.effect.payment.unshown"),
        ("a number form", number, "run.effect.payment.unshown"),
        (
            "an empty file name",
            committed(
                Class::LocalEdit,
                vec![one_kept("file", "C:\\Books\\"), one_kept("range", secret)],
            ),
            "run.effect.local_edit.unshown",
        ),
        (
            "a class with no effect row",
            committed(Class::Read, vec![one_kept("source", secret)]),
            "run.effect.unshown",
        ),
        (
            "kept names out of order",
            swapped,
            "run.effect.payment.unshown",
        ),
        ("a kept name too many", extra, "run.effect.payment.unshown"),
        (
            "a kept name missing",
            committed(Class::Sign, vec![one_kept("document", secret)]),
            "run.effect.sign.unshown",
        ),
    ];
    for (cause, bad, key) in causes {
        let c = beside_a_good_run(bad);
        assert_eq!(
            keys(&c, 0),
            ["run.effect.payment", key, "run.effect.payment"],
            "{cause}"
        );
        assert_eq!(keys(&c, 1), ["run.effect.payment"], "{cause}");
        let entry = serde_json::to_value(&c.runs[0].effects[1]).unwrap();
        assert_eq!(entry["fields"], serde_json::json!([]), "{cause}");
        assert_eq!(
            entry["text"].as_str(),
            Some(halvo_coord::words::text(key, &[]).unwrap().as_str())
        );
        let all = serde_json::to_string(&c).unwrap();
        assert!(!all.contains(secret), "{cause}: {all}");
    }
}

#[test]
fn rd52f_each_class_with_an_effect_row_has_its_unshown_entry_and_the_plain_one_the_rest() {
    for class in Class::ALL {
        let c = beside_a_good_run(committed(*class, vec![one_kept("zz", "x")]));
        let want = match class {
            Class::Read | Class::Prepare | Class::Credential => "run.effect.unshown".to_owned(),
            _ => format!("run.effect.{}.unshown", class.name()),
        };
        assert_eq!(keys(&c, 0)[1], want, "{class:?}");
    }
}

fn effect_id(n: u32) -> EffectId {
    EffectId::parse(&format!("{n:032x}")).unwrap()
}

fn digest(two: &str) -> Digest {
    Digest::parse(&two.repeat(32)).unwrap()
}

/// A `send` to Ann Lee whose last row is `rejected` with a null cause.
fn rejected(n: u32, d: &str) -> PastEffect {
    PastEffect {
        effect: effect_id(n),
        digest: digest(d),
        class: Class::Send,
        state: EffectState::Rejected,
        cause: None,
        kept: vec![one_kept("to", "Ann Lee")],
    }
}

fn row(f: &mut halvo_coord::types::Facts<Config>, what: What) {
    let seq = std::num::NonZeroU32::new(f.events.len() as u32 + 1000).unwrap();
    f.events.push(Event {
        workflow: f.workflow.id.clone(),
        agent: f.agent.clone(),
        seq,
        day: 20_760,
        at: 0,
        what,
    });
}

fn raised(f: &mut halvo_coord::types::Facts<Config>, item: u32, run: &RunId) {
    row(
        f,
        What::ItemRaised {
            item: ItemId::parse(&format!("{item:032x}")).unwrap(),
            raised: Raised::Confirm(run.clone()),
        },
    );
}

fn answered(
    f: &mut halvo_coord::types::Facts<Config>,
    item: u32,
    n: u32,
    d: &str,
    answer: EffectAnswer,
) {
    row(
        f,
        What::ItemAnswered {
            item: ItemId::parse(&format!("{item:032x}")).unwrap(),
            answer: Answered::Confirm(DecisionEntry {
                approval: ApprovalId::parse(&format!("{:032x}", 0xa0 + n)).unwrap(),
                effect: effect_id(n),
                digest: digest(d),
                answer,
            }),
        },
    );
}

fn run_of(n: u32) -> RunId {
    history(n, Outcome::Declined).run.run
}

/// The keys of the card of one declined run of `effects` over the ledger `f`, with a good run
/// beside it, and the line of that run.
fn declined_card(
    f: &halvo_coord::types::Facts<Config>,
    effects: Vec<PastEffect>,
) -> (Vec<String>, String, Vec<Vec<String>>) {
    let mut h = history(2, Outcome::Declined);
    h.effects = effects;
    let c = card(
        f,
        "N",
        &[h, history(1, Outcome::Completed)],
        &Limits::PRODUCT,
    )
    .unwrap();
    assert_eq!(
        keys(&c, 1),
        ["run.effect.payment"],
        "the good run beside it"
    );
    let fields = c.runs[0]
        .effects
        .iter()
        .map(|e| e.fields().iter().flat_map(|x| x.value.clone()).collect())
        .collect();
    (keys(&c, 0), c.runs[0].text.clone(), fields)
}

fn line(key: &str) -> String {
    halvo_coord::words::text(key, &[]).unwrap()
}

#[test]
fn rd85_a_no_recorded_for_the_effect_and_its_digest_in_an_item_of_this_run_is_a_refusal() {
    let mut f = facts(State::Card).remove(0);
    raised(&mut f, 0x51, &run_of(2));
    answered(&mut f, 0x51, 0xe1, "ab", EffectAnswer::RejectOnce);
    let (keys, text, fields) = declined_card(&f, vec![rejected(0xe1, "ab")]);
    assert_eq!(keys, ["run.effect.send.refused"]);
    assert_eq!(fields, [["Ann Lee"]], "the first kept field of its class");
    assert_eq!(text, line("line.run.declined"));
}

#[test]
fn rd85_with_no_row_of_a_no_the_entry_says_only_that_it_was_not_done() {
    let f = facts(State::Card).remove(0);
    let (keys, text, fields) = declined_card(&f, vec![rejected(0xe1, "ab")]);
    assert_eq!(keys, ["run.effect.send.not_done"]);
    assert_eq!(fields, [["Ann Lee"]]);
    assert_eq!(text, line("line.run.stopped"), "a neutral line");
}

#[test]
fn rd85_two_cards_in_one_run_a_no_on_the_first_and_a_lapse_of_the_second() {
    let mut f = facts(State::Card).remove(0);
    raised(&mut f, 0x51, &run_of(2));
    answered(&mut f, 0x51, 0xe1, "ab", EffectAnswer::RejectOnce);
    raised(&mut f, 0x52, &run_of(2));
    let (keys, ..) = declined_card(&f, vec![rejected(0xe1, "ab"), rejected(0xe3, "ab")]);
    assert_eq!(
        keys,
        ["run.effect.send.refused", "run.effect.send.not_done"]
    );
}

#[test]
fn rd85_a_rebind_with_an_allow_at_the_first_digest_and_a_no_at_the_second() {
    let mut f = facts(State::Card).remove(0);
    raised(&mut f, 0x51, &run_of(2));
    answered(&mut f, 0x51, 0xe1, "ab", EffectAnswer::AllowOnce);
    raised(&mut f, 0x52, &run_of(2));
    answered(&mut f, 0x52, 0xe1, "cd", EffectAnswer::RejectOnce);
    let (keys, ..) = declined_card(&f, vec![rejected(0xe1, "cd")]);
    assert_eq!(keys, ["run.effect.send.refused"]);
}

#[test]
fn rd85_a_no_at_another_digest_is_no_witness() {
    let mut f = facts(State::Card).remove(0);
    raised(&mut f, 0x51, &run_of(2));
    answered(&mut f, 0x51, 0xe1, "ab", EffectAnswer::RejectOnce);
    let (keys, text, _) = declined_card(&f, vec![rejected(0xe1, "cd")]);
    assert_eq!(keys, ["run.effect.send.not_done"]);
    assert_eq!(text, line("line.run.stopped"));
}

#[test]
fn rd85_an_allow_is_no_witness_and_a_no_of_another_run_is_none() {
    let mut f = facts(State::Card).remove(0);
    raised(&mut f, 0x51, &run_of(2));
    answered(&mut f, 0x51, 0xe1, "ab", EffectAnswer::AllowOnce);
    raised(&mut f, 0x52, &run_of(7));
    answered(&mut f, 0x52, 0xe3, "ab", EffectAnswer::RejectOnce);
    let (keys, ..) = declined_card(&f, vec![rejected(0xe1, "ab"), rejected(0xe3, "ab")]);
    assert_eq!(
        keys,
        ["run.effect.send.not_done", "run.effect.send.not_done"]
    );
}

#[test]
fn rd85_a_ledger_whose_item_raised_row_is_gone_gives_no_refusal_and_the_card_stands() {
    let mut f = facts(State::Card).remove(0);
    answered(&mut f, 0x51, 0xe1, "ab", EffectAnswer::RejectOnce);
    let (keys, ..) = declined_card(&f, vec![rejected(0xe1, "ab")]);
    assert_eq!(keys, ["run.effect.send.not_done"]);
}

#[test]
fn rd85_a_no_recorded_and_never_released_is_a_refusal() {
    let mut f = facts(State::Card).remove(0);
    raised(&mut f, 0x51, &run_of(2));
    answered(&mut f, 0x51, 0xe1, "ab", EffectAnswer::RejectOnce);
    row(
        &mut f,
        What::ItemLapsed {
            item: ItemId::parse(&format!("{:032x}", 0x51)).unwrap(),
            lapsed: Lapsed::Confirm(LapseCause::NoRecord),
        },
    );
    let (keys, ..) = declined_card(&f, vec![rejected(0xe1, "ab")]);
    assert_eq!(keys, ["run.effect.send.refused"]);
}

#[test]
fn rd85_entries_stand_for_a_null_cause_alone_whatever_the_runs_end() {
    let mut f = facts(State::Card).remove(0);
    raised(&mut f, 0x51, &run_of(2));
    answered(&mut f, 0x51, 0xe1, "ab", EffectAnswer::RejectOnce);
    let mut ended = rejected(0xe3, "ab");
    ended.cause = Some(Name::parse("ended").unwrap());
    let mut h = history(2, Outcome::Failed);
    h.effects = vec![rejected(0xe1, "ab"), ended];
    let c = card(&f, "N", &[h], &Limits::PRODUCT).unwrap();
    assert_eq!(keys(&c, 0), ["run.effect.send.refused"]);
    assert_eq!(c.runs[0].text, line("line.run.failed"));
}

#[test]
fn rd85_each_class_has_its_not_done_and_refused_entry_with_its_first_kept_field_or_none() {
    let first = |class: Class| -> Vec<Kept> {
        match class {
            Class::Payment => vec![one_kept("payee", "City Power")],
            Class::Purchase => vec![one_kept("seller", "Shop One")],
            Class::Send => vec![one_kept("to", "Ann Lee")],
            Class::Delete => vec![one_kept("items", "id-7")],
            Class::Submit => vec![one_kept("origin", "https://forms.site1.example:443")],
            Class::Sign => vec![one_kept("document", "Lease")],
            Class::LocalEdit => vec![one_kept("file", "Shared/Books.xlsx")],
            _ => vec![],
        }
    };
    let shown = |class: Class| -> Vec<&str> {
        match class {
            Class::Payment => vec!["City Power"],
            Class::Purchase => vec!["Shop One"],
            Class::Send => vec!["Ann Lee"],
            Class::Submit => vec!["forms.site1.example"],
            Class::Sign => vec!["Lease"],
            Class::LocalEdit => vec!["Books.xlsx"],
            _ => vec![],
        }
    };
    for class in Class::ALL {
        for refused in [false, true] {
            let mut f = facts(State::Card).remove(0);
            if refused {
                raised(&mut f, 0x51, &run_of(2));
                answered(&mut f, 0x51, 0xe1, "ab", EffectAnswer::RejectOnce);
            }
            let mut e = rejected(0xe1, "ab");
            e.class = *class;
            e.kept = first(*class);
            let (keys, _, fields) = declined_card(&f, vec![e]);
            let end = if refused { "refused" } else { "not_done" };
            let want = match class {
                Class::Read | Class::Prepare | Class::Credential => format!("run.effect.{end}"),
                _ => format!("run.effect.{}.{end}", class.name()),
            };
            assert_eq!(keys, [want], "{class:?}");
            assert_eq!(fields, [shown(*class)], "{class:?}");
        }
    }
    let mut bad = rejected(0xe1, "ab");
    bad.kept = vec![one_kept("to", "")];
    bad.class = Class::LocalEdit;
    let (keys, _, fields) = declined_card(&facts(State::Card).remove(0), vec![bad]);
    assert_eq!(keys, ["run.effect.local_edit.not_done"]);
    assert_eq!(
        fields,
        [Vec::<String>::new()],
        "a field that cannot be read"
    );
}

#[test]
fn rd48c_the_coordinators_own_row_decides_the_line_only_for_an_end_that_the_coordinator_gives() {
    for (own, store, want) in [
        (RunEnd::Completed, Outcome::Failed, "line.run.failed"),
        (RunEnd::Failed, Outcome::Completed, "line.run.completed"),
        (RunEnd::Lapsed, Outcome::Completed, "line.run.lapsed"),
        (RunEnd::Cancelled, Outcome::Failed, "line.run.cancelled"),
        (RunEnd::Interrupted, Outcome::Failed, "line.run.interrupted"),
    ] {
        let mut f = facts(State::Card).remove(0);
        row(
            &mut f,
            What::RunEnded {
                run: run_of(2),
                cause: own,
            },
        );
        let mut h = history(2, store);
        h.effects = vec![];
        let c = card(&f, "N", &[h], &Limits::PRODUCT).unwrap();
        assert_eq!(c.runs[0].text, line(want), "{own:?} {store:?}");
    }
}
