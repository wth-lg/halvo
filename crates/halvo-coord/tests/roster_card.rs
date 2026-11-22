use halvo_coord::guard::EffectState;
use halvo_coord::items::compose_check;
use halvo_coord::mem::{Config, Method, Ports};
use halvo_coord::ports::{Owner, PortError};
use halvo_coord::roster::{card, names, plain_site};
use halvo_coord::types::fixtures::{facts, State};
use halvo_coord::types::{
    Agent, AgentId, AgentState, Body, CardEffect, Class, Digest, EffectId, Event, Kept, Limits,
    Name, Outcome, PastEffect, PastRun, Reply, RunHistory, RunId, Unknown, Workflow, WorkflowId,
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

/// A committed payment none of whose kept values the reader takes whole, so that its entry shows
/// no field, as the fixture of the state `card` holds it after the payment: its payee, amount and
/// card each a piece over 1,024 bytes and its origin a number form.
fn unreadable_payment() -> PastEffect {
    let mut p = payment();
    p.effect = EffectId::parse(&format!("{:032x}", 0xe4)).unwrap();
    for k in &mut p.kept[..3] {
        k.value = vec!["Q".repeat(1025)];
    }
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
        refused_run: None,
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

    fn history(&self, _: &AgentId, _: u32) -> Result<(Vec<Event>, u32), PortError> {
        Ok((vec![], 0))
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

    fn replies(
        &self,
        workflow: &halvo_coord::types::WorkflowId,
        agent: &halvo_coord::types::AgentId,
        run: &halvo_coord::types::RunId,
    ) -> Result<Vec<halvo_coord::types::ReplyRow>, PortError> {
        let _ = (workflow, agent, run);
        Ok(vec![])
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
    assert_eq!(keys(&c, 0), ["run.effect.payment.partial"]);
    assert_eq!(shown(&c.runs[0].effects[0]), paid_fields()[..3]);
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

/// The label and the value of each field of `e`.
fn shown(e: &CardEffect) -> Vec<(String, Vec<String>)> {
    e.fields()
        .iter()
        .map(|f| (f.label.clone(), f.value.clone()))
        .collect()
}

fn pair(label: &str, value: &str) -> (String, Vec<String>) {
    (label.to_owned(), vec![value.to_owned()])
}

/// The four fields of `payment()` as its committed entry shows them, in the table's order.
fn paid_fields() -> Vec<(String, Vec<String>)> {
    vec![
        pair("Pay to", "City Power"),
        pair("Amount", "42.10 USD"),
        pair("Pay with", "Card ending 4242"),
        pair("Site", "site1.example"),
    ]
}

/// RD-52 (f) rule 1, turned by (h) 4 in U2.4q: each cause of a refused committed effect gives its
/// entry in its place, with the fields that the reader took whole in the table's order, and the
/// reply holds no byte of a value that it refused or could not tell apart.
#[test]
fn rd52f_each_cause_of_a_refused_committed_effect_gives_its_entry_and_no_refused_byte() {
    let secret = "Zq7 secret value";
    let mut swapped = payment();
    swapped.kept.swap(0, 1);
    let mut extra = payment();
    extra.kept.push(one_kept("note", secret));
    let mut origin = payment();
    origin.kept[3].value = vec!["https://Zq7.example:443".into()];
    let mut number = payment();
    number.kept[3].value = vec!["http://127.1:80".into()];
    let mut twice = payment();
    twice.kept.insert(1, one_kept("payee", secret));
    let three = paid_fields()[..3].to_vec();
    let causes = [
        (
            "a refused origin",
            origin,
            "run.effect.payment.partial",
            three.clone(),
            "Zq7",
        ),
        (
            "a number form",
            number,
            "run.effect.payment.partial",
            three,
            "127.1",
        ),
        (
            "an empty file name",
            committed(
                Class::LocalEdit,
                vec![
                    one_kept("file", &format!("C:\\{secret}\\")),
                    one_kept("range", "B2 to B9"),
                ],
            ),
            "run.effect.local_edit.partial",
            vec![pair("Where in the file", "B2 to B9")],
            secret,
        ),
        (
            "a class with no effect row",
            committed(Class::Read, vec![one_kept("source", secret)]),
            "run.effect.unshown",
            vec![],
            secret,
        ),
        (
            "kept names out of order",
            swapped,
            "run.effect.payment.partial",
            paid_fields(),
            secret,
        ),
        (
            "a kept name too many",
            extra,
            "run.effect.payment.partial",
            paid_fields(),
            secret,
        ),
        (
            "a kept name missing",
            committed(Class::Sign, vec![one_kept("document", "Lease")]),
            "run.effect.sign.partial",
            vec![pair("Document", "Lease")],
            secret,
        ),
        (
            "a kept name twice",
            twice,
            "run.effect.payment.partial",
            paid_fields()[1..].to_vec(),
            secret,
        ),
    ];
    for (cause, bad, key, fields, refused) in causes {
        let c = beside_a_good_run(bad);
        assert_eq!(
            keys(&c, 0),
            ["run.effect.payment", key, "run.effect.payment"],
            "{cause}"
        );
        assert_eq!(keys(&c, 1), ["run.effect.payment"], "{cause}");
        let entry = &c.runs[0].effects[1];
        assert_eq!(entry.text(), line(key), "{cause}");
        assert_eq!(shown(entry), fields, "{cause}");
        let all = serde_json::to_string(&c).unwrap();
        assert!(!all.contains(refused), "{cause}: {all}");
    }
    let mut twice = payment();
    twice.kept.insert(1, one_kept("payee", secret));
    let c = beside_a_good_run(twice);
    let entry = serde_json::to_string(&c.runs[0].effects[1]).unwrap();
    assert!(
        !entry.contains("City Power"),
        "neither payee of two stands: {entry}"
    );
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
                defers: None,
            },
        );
        let mut h = history(2, store);
        h.effects = vec![];
        let c = card(&f, "N", &[h], &Limits::PRODUCT).unwrap();
        assert_eq!(c.runs[0].text, line(want), "{own:?} {store:?}");
    }
}

fn no_piece_over_1024_bytes(c: &halvo_coord::types::Card) {
    for run in &c.runs {
        for e in &run.effects {
            for f in e.fields() {
                for piece in &f.value {
                    assert!(
                        piece.len() <= 1024,
                        "{}: a piece of {} bytes",
                        e.key(),
                        piece.len()
                    );
                }
            }
        }
    }
}

#[test]
fn i34_a_committed_kept_piece_over_1024_bytes_never_stands_and_the_rest_do() {
    let long = "Q".repeat(1025);
    let mut p = payment();
    p.kept[0].value = vec![long.clone()];
    let c = beside_a_good_run(p);
    no_piece_over_1024_bytes(&c);
    assert_eq!(keys(&c, 0)[1], "run.effect.payment.partial");
    assert_eq!(shown(&c.runs[0].effects[1]), paid_fields()[1..]);
    assert!(!serde_json::to_string(&c).unwrap().contains(&long));
}

#[test]
fn i34_a_local_edit_file_name_longer_than_1024_bytes_never_stands_and_its_range_does() {
    let bad = committed(
        Class::LocalEdit,
        vec![
            Kept {
                name: Name::parse("file").unwrap(),
                value: vec![format!("Books/{}", "x".repeat(700)), "y".repeat(700)],
            },
            one_kept("range", "lines 1 to 2"),
        ],
    );
    let c = beside_a_good_run(bad);
    no_piece_over_1024_bytes(&c);
    assert_eq!(keys(&c, 0)[1], "run.effect.local_edit.partial");
    assert_eq!(
        shown(&c.runs[0].effects[1]),
        [pair("Where in the file", "lines 1 to 2")]
    );
}

#[test]
fn i34_a_not_done_entry_shows_no_first_field_over_1024_bytes() {
    let f = facts(State::Card).remove(0);
    let long = "Q".repeat(1025);
    let mut p = rejected(0xe1, "ab");
    p.class = Class::Payment;
    p.kept = vec![one_kept("payee", &long)];
    let mut file = rejected(0xe3, "ab");
    file.class = Class::LocalEdit;
    file.kept = vec![Kept {
        name: Name::parse("file").unwrap(),
        value: vec![format!("Books/{}", "x".repeat(700)), "y".repeat(700)],
    }];
    let (keys, _, fields) = declined_card(&f, vec![p, file]);
    assert_eq!(
        keys,
        [
            "run.effect.payment.not_done",
            "run.effect.local_edit.not_done"
        ]
    );
    assert_eq!(fields, [Vec::<String>::new(), Vec::new()], "no field");
}

#[test]
fn rd101i_a_lapse_after_a_switch_off_that_still_stands_has_the_line_of_a_stopped_run() {
    let stop = |order: &[&str]| {
        let mut f = facts(State::Card).remove(0);
        for step in order {
            let what = match *step {
                "off" => What::HoldSet {
                    cause: halvo_coord::types::HoldSetCause::Person,
                },
                "on" => What::HoldCleared {
                    cause: halvo_coord::types::Cleared::Person,
                },
                _ => What::RunEnded {
                    run: run_of(2),
                    cause: RunEnd::Lapsed,
                    defers: None,
                },
            };
            row(&mut f, what);
        }
        let mut h = history(2, Outcome::Declined);
        h.effects = vec![];
        card(&f, "N", &[h], &Limits::PRODUCT).unwrap().runs[0]
            .text
            .clone()
    };
    assert_eq!(stop(&["off", "end"]), line("line.run.cancelled"));
    assert_eq!(
        stop(&["end", "off"]),
        line("line.run.lapsed"),
        "a lapse before the switch-off"
    );
    assert_eq!(
        stop(&["off", "end", "on"]),
        line("line.run.lapsed"),
        "the switch-off no longer stands"
    );
    assert_eq!(stop(&["end"]), line("line.run.lapsed"));
}

/// The line of run 2 (store outcome `store`) over the ledger rows `rows`, in order.
fn line_over(rows: Vec<What>, store: Outcome) -> String {
    let mut f = facts(State::Card).remove(0);
    for what in rows {
        row(&mut f, what);
    }
    let mut h = history(2, store);
    h.effects = vec![];
    card(&f, "N", &[h], &Limits::PRODUCT).unwrap().runs[0]
        .text
        .clone()
}

fn own(cause: RunEnd) -> What {
    What::RunEnded {
        run: run_of(2),
        cause,
        defers: None,
    }
}

fn off() -> What {
    What::HoldSet {
        cause: halvo_coord::types::HoldSetCause::Person,
    }
}

fn on() -> What {
    What::HoldCleared {
        cause: halvo_coord::types::Cleared::Person,
    }
}

#[test]
fn rd116_r4_every_cause_that_only_the_coordinator_gives_decides_the_line() {
    for cause in [
        RunEnd::Budget,
        RunEnd::Kind,
        RunEnd::Deleted,
        RunEnd::Cancelled,
        RunEnd::Lapsed,
        RunEnd::Interrupted,
    ] {
        assert_eq!(
            line_over(vec![own(cause)], Outcome::Failed),
            line(&format!("line.run.{}", cause.name())),
            "{cause:?}"
        );
    }
}

#[test]
fn rd116_r4_the_switch_off_rule_reads_the_newest_person_hold_rows_before_the_runs_end() {
    let lapsed = line("line.run.lapsed");
    let stopped = line("line.run.cancelled");
    let cases: Vec<(Vec<What>, &String, &str)> = vec![
        (
            vec![off(), on(), own(RunEnd::Lapsed)],
            &lapsed,
            "off, on, lapse",
        ),
        (
            vec![off(), on(), off(), own(RunEnd::Lapsed)],
            &stopped,
            "off, on, off, lapse",
        ),
        (
            vec![off(), on(), off(), on(), own(RunEnd::Lapsed)],
            &lapsed,
            "off, on, off, on, lapse",
        ),
        (
            vec![
                What::HoldSet {
                    cause: halvo_coord::types::HoldSetCause::Failures,
                },
                own(RunEnd::Lapsed),
            ],
            &lapsed,
            "a failures hold is no switch-off",
        ),
        (
            vec![
                off(),
                What::HoldCleared {
                    cause: halvo_coord::types::Cleared::Failures,
                },
                own(RunEnd::Lapsed),
            ],
            &stopped,
            "a failures clear is no switch-on",
        ),
    ];
    for (rows, want, why) in cases {
        assert_eq!(&line_over(rows, Outcome::Declined), want, "{why}");
    }
    assert_eq!(
        line_over(vec![off(), own(RunEnd::Interrupted)], Outcome::Failed),
        line("line.run.interrupted"),
        "an interrupted run after a switch-off keeps its own line"
    );
}

#[test]
fn rd116_r4_the_newest_own_row_of_this_run_decides_and_another_runs_row_does_not() {
    assert_eq!(
        line_over(
            vec![own(RunEnd::Failed), own(RunEnd::Lapsed)],
            Outcome::Declined
        ),
        line("line.run.lapsed"),
        "the newest row of the run"
    );
    let mut f = facts(State::Card).remove(0);
    row(&mut f, own(RunEnd::Lapsed));
    row(
        &mut f,
        What::RunEnded {
            run: run_of(3),
            cause: RunEnd::Cancelled,
            defers: None,
        },
    );
    let mut older = history(2, Outcome::Declined);
    older.effects = vec![];
    let mut newer = history(3, Outcome::Declined);
    newer.effects = vec![];
    let c = card(&f, "N", &[newer, older], &Limits::PRODUCT).unwrap();
    assert_eq!(c.runs[0].text, line("line.run.cancelled"));
    assert_eq!(c.runs[1].text, line("line.run.lapsed"), "its own row alone");
}

#[test]
fn rd116_r4_a_declined_run_with_one_refusal_beside_a_not_done_effect_says_you_said_no() {
    let mut f = facts(State::Card).remove(0);
    raised(&mut f, 0x51, &run_of(2));
    answered(&mut f, 0x51, 0xe1, "ab", EffectAnswer::RejectOnce);
    let (keys, text, _) = declined_card(&f, vec![rejected(0xe1, "ab"), rejected(0xe3, "ab")]);
    assert_eq!(
        keys,
        ["run.effect.send.refused", "run.effect.send.not_done"]
    );
    assert_eq!(text, line("line.run.declined"));
}

#[test]
fn rd116_r4_only_rejected_committed_and_unknown_effects_give_entries() {
    for state in [
        EffectState::Refused,
        EffectState::Cleared,
        EffectState::Releasing,
        EffectState::Approved,
        EffectState::Held,
    ] {
        let mut e = rejected(0xe1, "ab");
        e.state = state;
        let (keys, ..) = declined_card(&facts(State::Card).remove(0), vec![e]);
        assert!(keys.is_empty(), "{state:?}: {keys:?}");
    }
    let mut with_cause = payment();
    with_cause.cause = Some(Name::parse("ended").unwrap());
    let c = beside_a_good_run(with_cause);
    assert_eq!(
        keys(&c, 0),
        ["run.effect.payment"; 3],
        "a committed row with a cause keeps its entry"
    );
}

#[test]
fn rd116_r4_a_not_done_entry_shows_the_first_field_of_its_class_by_name() {
    let mut e = rejected(0xe1, "ab");
    e.class = Class::Payment;
    e.kept = vec![
        one_kept("amount", "42.10 USD"),
        one_kept("payee", "City Power"),
    ];
    let (_, _, fields) = declined_card(&facts(State::Card).remove(0), vec![e]);
    assert_eq!(fields, [["City Power"]]);
}

#[test]
fn rd116_r4_the_piece_bound_is_per_piece_and_takes_1024_bytes() {
    let mut exact = payment();
    exact.kept[0].value = vec!["Q".repeat(1024)];
    let c = beside_a_good_run(exact);
    assert_eq!(keys(&c, 0)[1], "run.effect.payment", "1,024 bytes is shown");
    let mut two = payment();
    two.kept[0].value = vec!["Q".repeat(600), "R".repeat(600)];
    let c = beside_a_good_run(two);
    assert_eq!(
        keys(&c, 0)[1],
        "run.effect.payment",
        "two pieces under the bound"
    );
    assert_eq!(c.runs[0].effects[1].fields()[0].value.len(), 2);
}

#[test]
fn u24r_the_dropped_line_says_its_count_and_no_record_taken_off_gives_no_line() {
    use halvo_coord::roster::dropped_line;
    use halvo_coord::types::{CardLine, LineKey};
    assert_eq!(dropped_line(0), Ok(None));
    for count in [1u32, 3, u32::MAX] {
        let text =
            halvo_coord::words::text("card.records_dropped", &[("count", &count.to_string())])
                .unwrap();
        assert_eq!(
            dropped_line(count),
            Ok(Some(CardLine {
                key: LineKey::RecordsDropped,
                text
            })),
            "{count}"
        );
    }
}

/// RD-52 (h) 4: an unshown payment shows the fields that the reader took whole, its payee, its
/// amount and the card it was paid with, as its committed entry shows them, and not the origin
/// that it refused; its run keeps its day and its line.
#[test]
fn u24q_an_unshown_payment_shows_the_fields_that_the_reader_took_whole_and_none_that_it_refused() {
    let mut p = payment();
    p.kept[3].value = vec!["http://2130706433:80".into()];
    let c = beside_a_good_run(p);
    assert_eq!(
        keys(&c, 0),
        [
            "run.effect.payment",
            "run.effect.payment.partial",
            "run.effect.payment"
        ]
    );
    let (good, unshown) = (&c.runs[0].effects[0], &c.runs[0].effects[1]);
    assert_eq!(unshown.text(), line("run.effect.payment.partial"));
    assert_eq!(unshown.fields(), &good.fields()[..3]);
    assert_eq!(shown(unshown), paid_fields()[..3]);
    assert_eq!(c.runs[0].day, halvo_coord::words::day(20_702));
    assert_eq!(c.runs[0].text, line("line.run.completed"));
    assert!(!serde_json::to_string(&c).unwrap().contains("2130706433"));
}

/// The key, the sentence and the fields of the entry of one committed effect of `class` that
/// keeps `kept`, beside a good run, and the whole card as text.
fn one_entry(
    class: Class,
    kept: Vec<Kept>,
) -> (String, String, Vec<(String, Vec<String>)>, String) {
    let c = beside_a_good_run(committed(class, kept));
    let e = &c.runs[0].effects[1];
    (
        e.key(),
        e.text().to_owned(),
        shown(e),
        serde_json::to_string(&c).unwrap(),
    )
}

fn long() -> String {
    "Q".repeat(1025)
}

fn partial(class: &str) -> (String, String) {
    let key = format!("run.effect.{class}.partial");
    (key.clone(), line(&key))
}

#[test]
fn u24q_payment_a_field_that_the_reader_could_not_check_never_stands() {
    let (key, text, fields, all) = one_entry(
        Class::Payment,
        vec![one_kept("payee", "City Power"), one_kept("amount", &long())],
    );
    assert_eq!(fields, [pair("Pay to", "City Power")]);
    assert_eq!((key, text), partial("payment"));
    assert!(!all.contains(&long()));
}

#[test]
fn u24q_purchase_a_field_that_the_reader_could_not_check_never_stands() {
    let (key, text, fields, all) = one_entry(
        Class::Purchase,
        vec![
            one_kept("seller", "Shop One"),
            one_kept("origin", "https://Zq7.example:443"),
        ],
    );
    assert_eq!(fields, [pair("Shop", "Shop One")]);
    assert_eq!((key, text), partial("purchase"));
    assert!(!all.contains("Zq7"));
}

#[test]
fn u24q_send_a_field_that_the_reader_could_not_check_never_stands() {
    let (key, text, fields, all) = one_entry(
        Class::Send,
        vec![
            one_kept("subject", "Zq7 subject"),
            one_kept("to", "Ann Lee"),
        ],
    );
    assert_eq!(fields, [pair("To", "Ann Lee")]);
    assert_eq!((key, text), partial("send"));
    assert!(!all.contains("Zq7"));
}

#[test]
fn u24q_delete_a_field_that_the_reader_could_not_check_never_stands() {
    let (key, text, fields, all) = one_entry(
        Class::Delete,
        vec![one_kept("items", "id-7"), one_kept("note", "Zq7 note")],
    );
    assert_eq!(key, "run.effect.delete.unshown");
    assert_eq!(text, line("run.effect.delete.unshown"));
    assert!(fields.is_empty(), "{fields:?}");
    assert!(!all.contains("Zq7") && !all.contains("id-7"));
}

#[test]
fn u24q_submit_a_field_that_the_reader_could_not_check_never_stands() {
    let (key, text, fields, all) = one_entry(
        Class::Submit,
        vec![
            one_kept("origin", "https://forms.site1.example:443"),
            one_kept("fields", "Zq7 typed"),
        ],
    );
    assert_eq!(fields, [pair("Site", "forms.site1.example")]);
    assert_eq!((key, text), partial("submit"));
    assert!(!all.contains("Zq7"));
}

#[test]
fn u24q_sign_a_field_that_the_reader_could_not_check_never_stands() {
    let (key, text, fields, all) = one_entry(
        Class::Sign,
        vec![one_kept("document", "Lease"), one_kept("party", &long())],
    );
    assert_eq!(fields, [pair("Document", "Lease")]);
    assert_eq!((key, text), partial("sign"));
    assert!(!all.contains(&long()));
}

#[test]
fn u24q_local_edit_a_field_that_the_reader_could_not_check_never_stands() {
    let (key, text, fields, all) = one_entry(
        Class::LocalEdit,
        vec![
            one_kept("file", "Books/Zq7/.."),
            one_kept("range", "B2 to B9"),
        ],
    );
    assert_eq!(fields, [pair("Where in the file", "B2 to B9")]);
    assert_eq!((key, text), partial("local_edit"));
    assert!(!all.contains("Zq7"));
}

/// A class with no effect row has no table of kept fields, so the reader can check none, and a
/// value that would read for a payment stands no more than one that reads for no class.
fn no_row_class_shows_no_field(class: Class) {
    let (key, text, fields, all) = one_entry(
        class,
        vec![one_kept("payee", "Zq7 payee"), one_kept("zz", "Zq7 zz")],
    );
    assert_eq!(key, "run.effect.unshown", "{class:?}");
    assert_eq!(text, line("run.effect.unshown"), "{class:?}");
    assert!(fields.is_empty(), "{class:?}: {fields:?}");
    assert!(!all.contains("Zq7"), "{class:?}");
}

#[test]
fn u24q_read_a_field_that_the_reader_could_not_check_never_stands() {
    no_row_class_shows_no_field(Class::Read);
}

#[test]
fn u24q_prepare_a_field_that_the_reader_could_not_check_never_stands() {
    no_row_class_shows_no_field(Class::Prepare);
}

#[test]
fn u24q_credential_a_field_that_the_reader_could_not_check_never_stands() {
    no_row_class_shows_no_field(Class::Credential);
}

/// A kept field of `class` that the reader takes whole.
fn readable(class: Class) -> Kept {
    match class {
        Class::Payment => one_kept("payee", "City Power"),
        Class::Purchase => one_kept("seller", "Shop One"),
        Class::Send => one_kept("to", "Ann Lee"),
        Class::Delete => one_kept("items", "id-7"),
        Class::Submit => one_kept("origin", "https://forms.site1.example:443"),
        Class::Sign => one_kept("document", "Lease"),
        Class::LocalEdit => one_kept("range", "B2 to B9"),
        Class::Read | Class::Prepare | Class::Credential => one_kept("source", "Inbox"),
    }
}

fn has_row(class: Class) -> bool {
    !matches!(class, Class::Read | Class::Prepare | Class::Credential)
}

/// An entry with no field that the reader took whole keeps today's key and sentence,
/// `run.effect.<class>.unshown`, and the same entry with one such field takes the partial one; a
/// `delete`, which shows no field, keeps today's in both.
#[test]
fn u24q_an_unshown_entry_with_no_readable_field_keeps_todays_sentence() {
    for class in Class::ALL.iter().copied().filter(|c| has_row(*c)) {
        let today = format!("run.effect.{}.unshown", class.name());
        let (key, text, fields, _) = one_entry(class, vec![one_kept("zz", "x")]);
        assert_eq!((key, text), (today.clone(), line(&today)), "{class:?}");
        assert!(fields.is_empty(), "{class:?}");
        let (key, text, fields, _) = one_entry(class, vec![readable(class), one_kept("zz", "x")]);
        let (want, count) = if class == Class::Delete {
            (today, 0)
        } else {
            (partial(class.name()).0, 1)
        };
        assert_eq!((key, text), (want.clone(), line(&want)), "{class:?}");
        assert_eq!(fields.len(), count, "{class:?}");
    }
    let c = beside_a_good_run(unreadable_payment());
    let e = &c.runs[0].effects[1];
    assert_eq!(
        (e.key().as_str(), e.text()),
        (
            "run.effect.payment.unshown",
            line("run.effect.payment.unshown").as_str()
        )
    );
    assert!(e.fields().is_empty());
}

/// Every kept field of `class` in the table's order, each one that the reader takes whole.
fn whole(class: Class) -> Vec<Kept> {
    match class {
        Class::Payment => payment().kept,
        Class::Purchase => vec![
            one_kept("seller", "Shop One"),
            one_kept("amount", "19.99 USD"),
            one_kept("instrument", "Card ending 4242"),
            one_kept("origin", "https://shop.example:443"),
        ],
        Class::Sign => vec![one_kept("document", "Lease"), one_kept("party", "Bo Chan")],
        Class::LocalEdit => vec![
            one_kept("file", "Shared/Books.xlsx"),
            one_kept("range", "B2 to B9"),
        ],
        other => vec![readable(other)],
    }
}

fn unknown(class: Class, kept: Vec<Kept>) -> PastEffect {
    PastEffect {
        state: EffectState::Unknown,
        ..committed(class, kept)
    }
}

/// RD-126 item 10: an effect whose last state is `unknown` gets an entry in its place that says
/// that Halvo could not tell whether it went through, one for each class with an effect row and
/// the plain one for the rest, with the fields that its committed entry shows.
#[test]
fn u24q_an_effect_whose_last_state_is_unknown_gets_an_entry_that_says_halvo_could_not_tell() {
    for class in Class::ALL.iter().copied() {
        let c = beside_a_good_run(unknown(class, whole(class)));
        let want = if has_row(class) {
            format!("run.effect.{}.unknown", class.name())
        } else {
            "run.effect.unknown".to_owned()
        };
        assert_eq!(
            keys(&c, 0),
            ["run.effect.payment", want.as_str(), "run.effect.payment"],
            "{class:?}"
        );
        let e = &c.runs[0].effects[1];
        assert_eq!(e.text(), line(&want), "{class:?}");
        let fields = CardEffect::committed(class, &whole(class))
            .map(|x| x.fields().to_vec())
            .unwrap_or_default();
        assert_eq!(e.fields(), &fields[..], "{class:?}");
        assert_eq!(
            e.fields().is_empty(),
            !has_row(class) || class == Class::Delete,
            "{class:?}"
        );
    }
}

/// The review's N2 (pr-review-35) under RD-126 item 10: the person allowed a send and then
/// pressed Stop, or Halvo closed, and the store ended the run with the send's last state
/// `unknown`: the run's line is the coordinator's own, and the entry under it says that Halvo
/// could not tell whether the message was sent, with its address.
#[test]
fn u24q_an_unknown_send_stands_under_the_line_of_a_run_that_the_coordinator_ended() {
    for (own, want) in [
        (RunEnd::Cancelled, "line.run.cancelled"),
        (RunEnd::Interrupted, "line.run.interrupted"),
    ] {
        let mut f = facts(State::Card).remove(0);
        row(
            &mut f,
            What::RunEnded {
                run: run_of(2),
                cause: own,
                defers: None,
            },
        );
        let mut h = history(2, Outcome::EffectUnknown);
        h.effects = vec![unknown(Class::Send, vec![one_kept("to", "Ann Lee")])];
        let c = card(&f, "N", &[h], &Limits::PRODUCT).unwrap();
        assert_eq!(c.runs[0].text, line(want), "{own:?}");
        assert_eq!(keys(&c, 0), ["run.effect.send.unknown"], "{own:?}");
        assert_eq!(shown(&c.runs[0].effects[0]), [pair("To", "Ann Lee")]);
    }
}

/// The entry of an unknown last state takes the one rule for which fields may stand, whatever
/// the cause of its last row: a refused origin and a piece over 1,024 bytes never stand.
#[test]
fn u24q_an_unknown_entry_shows_only_the_fields_that_the_reader_took_whole_whatever_its_cause() {
    let mut p = unknown(Class::Payment, payment().kept);
    p.kept[3].value = vec!["http://2130706433:80".into()];
    p.cause = Some(Name::parse("lost").unwrap());
    let c = beside_a_good_run(p);
    assert_eq!(keys(&c, 0)[1], "run.effect.payment.unknown");
    assert_eq!(shown(&c.runs[0].effects[1]), paid_fields()[..3]);
    assert!(!serde_json::to_string(&c).unwrap().contains("2130706433"));
    let c = beside_a_good_run(unknown(Class::Send, vec![one_kept("to", &long())]));
    let e = &c.runs[0].effects[1];
    assert_eq!(
        (e.key().as_str(), e.text()),
        (
            "run.effect.send.unknown",
            line("run.effect.send.unknown").as_str()
        )
    );
    assert!(e.fields().is_empty());
    assert!(!serde_json::to_string(&c).unwrap().contains(&long()));
    let mut twice = unknown(Class::Payment, payment().kept);
    twice.kept.insert(1, one_kept("payee", "Zq7 payee"));
    twice.kept.swap(2, 3);
    let c = beside_a_good_run(twice);
    let e = &c.runs[0].effects[1];
    assert_eq!(e.key(), "run.effect.payment.unknown");
    assert_eq!(
        shown(e),
        paid_fields()[1..],
        "neither payee of two, the rest in the table's order"
    );
    assert!(!serde_json::to_string(e).unwrap().contains("City Power"));
    assert!(!serde_json::to_string(&c).unwrap().contains("Zq7"));
}

/// The kept `amount`, `instrument` and `origin` of `payment()`, each of which the reader takes.
fn paid_rest() -> Vec<Kept> {
    payment().kept[1..].to_vec()
}

/// A payment to `payee` with the rest of `payment()`'s kept fields.
fn paid_to(payee: &str) -> Vec<Kept> {
    let mut kept = vec![one_kept("payee", payee)];
    kept.extend(paid_rest());
    kept
}

/// RD-126 item 10, the verify step's M4: every effect of a run whose last state is unknown gets
/// its own entry, in its place among the run's other entries, each with its own fields.
#[test]
fn u24q_every_unknown_effect_of_a_run_gets_its_own_entry_in_its_place() {
    let mut h = history(2, Outcome::EffectUnknown);
    h.run.unknown = 4;
    h.effects = vec![
        unknown(Class::Send, vec![one_kept("to", "Ann Lee")]),
        payment(),
        unknown(Class::Payment, paid_to("Gas Co")),
        unknown(Class::Send, vec![one_kept("to", "Bo Chan")]),
        unknown(Class::Read, vec![one_kept("source", "Inbox")]),
    ];
    let c = card(&facts(State::Card).remove(0), "N", &[h], &Limits::PRODUCT).unwrap();
    assert_eq!(
        keys(&c, 0),
        [
            "run.effect.send.unknown",
            "run.effect.payment",
            "run.effect.payment.unknown",
            "run.effect.send.unknown",
            "run.effect.unknown"
        ]
    );
    let e = &c.runs[0].effects;
    assert_eq!(shown(&e[0]), [pair("To", "Ann Lee")]);
    assert_eq!(shown(&e[2])[0], pair("Pay to", "Gas Co"));
    assert_eq!(shown(&e[3]), [pair("To", "Bo Chan")]);
    assert!(e[4].fields().is_empty());
}

/// The verify step's M1: a name that the row keeps twice never stands, whether its two values
/// are equal or differ, whether one of them is over the bound, and whether its two rows stand
/// side by side or apart, on the entry of a refused committed effect and on the unknown entry.
#[test]
fn u24q_a_name_kept_twice_never_stands_whatever_its_values_or_its_place() {
    let over = format!("zqr-over{}", "Q".repeat(1025));
    let r = paid_rest();
    let payee = |v: &str| one_kept("payee", v);
    let cases: Vec<(&str, Vec<Kept>)> = vec![
        (
            "equal, side by side",
            [vec![payee("zqr1 Power"), payee("zqr1 Power")], r.clone()].concat(),
        ),
        (
            "equal, apart",
            vec![
                payee("zqr2 Power"),
                r[0].clone(),
                payee("zqr2 Power"),
                r[1].clone(),
                r[2].clone(),
            ],
        ),
        (
            "the first reads, the second is over the bound",
            [vec![payee("zqr3 Power"), payee(&over)], r.clone()].concat(),
        ),
        (
            "the first is over the bound, the second reads",
            [vec![payee(&over), payee("zqr4 Power")], r.clone()].concat(),
        ),
        (
            "they differ, apart",
            vec![
                payee("zqr5a Power"),
                r[0].clone(),
                r[1].clone(),
                payee("zqr5b Power"),
                r[2].clone(),
            ],
        ),
    ];
    for (case, kept) in cases {
        for (effect, want) in [
            (
                committed(Class::Payment, kept.clone()),
                "run.effect.payment.partial",
            ),
            (
                unknown(Class::Payment, kept.clone()),
                "run.effect.payment.unknown",
            ),
        ] {
            let c = beside_a_good_run(effect);
            let e = &c.runs[0].effects[1];
            assert_eq!(e.key(), want, "{case}");
            let labels: Vec<&str> = e.fields().iter().map(|f| f.label.as_str()).collect();
            assert_eq!(labels, ["Amount", "Pay with", "Site"], "{case}: {want}");
            let all = serde_json::to_string(&c).unwrap();
            assert!(!all.contains("zqr"), "{case}: {want}: {all}");
        }
    }
}

/// The verify step's M2: a class with no effect row shows no field on its unshown entry or its
/// unknown entry, also where its row keeps the names of a payment's table with values that would
/// read for a payment.
#[test]
fn u24q_a_class_with_no_effect_row_shows_no_field_even_with_another_tables_names() {
    let kept = vec![
        one_kept("payee", "zqr7 Power"),
        one_kept("amount", "zqr7 1.00 USD"),
        one_kept("instrument", "zqr7 card"),
        one_kept("origin", "https://zqr7site.example:443"),
    ];
    for class in [Class::Read, Class::Prepare, Class::Credential] {
        for (effect, want) in [
            (committed(class, kept.clone()), "run.effect.unshown"),
            (unknown(class, kept.clone()), "run.effect.unknown"),
        ] {
            let c = beside_a_good_run(effect);
            let e = &c.runs[0].effects[1];
            assert_eq!(
                (e.key().as_str(), e.text()),
                (want, line(want).as_str()),
                "{class:?}"
            );
            assert!(e.fields().is_empty(), "{class:?}: {want}");
            let all = serde_json::to_string(&c).unwrap();
            assert!(!all.contains("zqr7"), "{class:?}: {want}: {all}");
        }
    }
}

/// The verify step's M3: a kept value in two pieces, each of which the reader takes, stands with
/// both pieces in order on the unknown entry and on the partial entry.
#[test]
fn u24q_a_value_in_pieces_that_each_read_stands_on_partial_and_unknown_entries() {
    let to = Kept {
        name: Name::parse("to").unwrap(),
        value: vec!["Ann Lee, ".into(), "Bob Ray".into()],
    };
    let both = vec!["Ann Lee, ".to_owned(), "Bob Ray".to_owned()];
    for (effect, want) in [
        (
            unknown(Class::Send, vec![to.clone()]),
            "run.effect.send.unknown",
        ),
        (
            committed(Class::Send, vec![to.clone(), one_kept("note", "zqr9")]),
            "run.effect.send.partial",
        ),
    ] {
        let c = beside_a_good_run(effect);
        let e = &c.runs[0].effects[1];
        assert_eq!(e.key(), want);
        assert_eq!(shown(e), [("To".to_owned(), both.clone())], "{want}");
        let all = serde_json::to_string(&c).unwrap();
        assert!(!all.contains("zqr9"), "{want}: {all}");
    }
}

/// A kept field `name` whose value is `pieces`.
fn kept_pieces(name: &str, pieces: &[&str]) -> Kept {
    Kept {
        name: Name::parse(name).unwrap(),
        value: pieces.iter().map(|p| p.to_string()).collect(),
    }
}

/// Ruling RD-158 (3): a kept value that holds no byte, with no piece or with pieces that are all
/// empty, never stands on a partial or an unknown entry. A row with nothing else to show keeps the
/// unshown entry of the base and the unknown entry with no field; where other fields read, they
/// stand in the table's order and the empty one is left out. Each row keeps fewer names than its
/// class's table, so that the reader refuses it as a whole committed entry.
#[test]
fn u24q_a_kept_value_of_no_byte_never_stands_on_a_partial_or_an_unknown_entry() {
    let empties: [&[&str]; 3] = [&[], &[""], &["", "", ""]];
    for pieces in empties {
        for (class, name, word) in [
            (Class::Payment, "payee", "payment"),
            (Class::Sign, "document", "sign"),
        ] {
            let alone = vec![kept_pieces(name, pieces)];
            for (effect, want) in [
                (
                    committed(class, alone.clone()),
                    format!("run.effect.{word}.unshown"),
                ),
                (
                    unknown(class, alone.clone()),
                    format!("run.effect.{word}.unknown"),
                ),
            ] {
                let c = beside_a_good_run(effect);
                let e = &c.runs[0].effects[1];
                assert_eq!(
                    (e.key(), e.text().to_owned()),
                    (want.clone(), line(&want)),
                    "{pieces:?}"
                );
                assert!(e.fields().is_empty(), "{pieces:?}: {want}");
            }
        }
        let mut kept = paid_rest();
        kept.insert(1, kept_pieces("payee", pieces));
        for (effect, want) in [
            (
                committed(Class::Payment, kept.clone()),
                "run.effect.payment.partial",
            ),
            (
                unknown(Class::Payment, kept.clone()),
                "run.effect.payment.unknown",
            ),
        ] {
            let c = beside_a_good_run(effect);
            let e = &c.runs[0].effects[1];
            assert_eq!(e.key(), want, "{pieces:?}");
            let labels: Vec<&str> = e.fields().iter().map(|f| f.label.as_str()).collect();
            assert_eq!(labels, ["Amount", "Pay with", "Site"], "{pieces:?}: {want}");
        }
    }
}

/// The control of the rule above: a kept value that holds a byte stands as before, a value with
/// one empty piece among them; a value of white space alone shows nothing and no longer stands
/// (U2.4w, ruling RD-161 R6).
#[test]
fn u24q_a_kept_value_that_holds_a_byte_stands_white_space_among_them() {
    let values: [&[&str]; 3] = [&["", "City Power"], &["City Power", ""], &["City Power"]];
    for pieces in values {
        let shown_value: Vec<String> = pieces.iter().map(|p| p.to_string()).collect();
        for (effect, want) in [
            (
                committed(Class::Payment, vec![kept_pieces("payee", pieces)]),
                "run.effect.payment.partial",
            ),
            (
                unknown(Class::Payment, vec![kept_pieces("payee", pieces)]),
                "run.effect.payment.unknown",
            ),
        ] {
            let c = beside_a_good_run(effect);
            let e = &c.runs[0].effects[1];
            assert_eq!(e.key(), want, "{pieces:?}");
            assert_eq!(
                shown(e),
                [("Pay to".to_owned(), shown_value.clone())],
                "{pieces:?}: {want}"
            );
        }
    }
}

/// The verifier's finding 1 (h38) under RD-126 item 10: an unknown send keeps its entry, with its
/// text and its address, under the line of each end that the coordinator alone gives. Every end
/// whose card differs is named.
#[test]
fn u24q_an_unknown_send_stands_under_each_end_that_the_coordinator_alone_gives() {
    let mut differ = Vec::new();
    for (end, key) in [
        (RunEnd::Cancelled, "line.run.cancelled"),
        (RunEnd::Lapsed, "line.run.lapsed"),
        (RunEnd::Interrupted, "line.run.interrupted"),
        (RunEnd::Deleted, "line.run.deleted"),
        (RunEnd::Budget, "line.run.budget"),
        (RunEnd::Kind, "line.run.kind"),
    ] {
        let mut f = facts(State::Card).remove(0);
        row(
            &mut f,
            What::RunEnded {
                run: run_of(2),
                cause: end,
                defers: None,
            },
        );
        let mut h = history(2, Outcome::EffectUnknown);
        h.effects = vec![unknown(Class::Send, vec![one_kept("to", "Ann Lee")])];
        let c = card(&f, "N", &[h], &Limits::PRODUCT).unwrap();
        let got = (
            c.runs[0].text.clone(),
            keys(&c, 0),
            c.runs[0]
                .effects
                .iter()
                .map(|e| (e.text().to_owned(), shown(e)))
                .collect::<Vec<_>>(),
        );
        let want = (
            line(key),
            vec!["run.effect.send.unknown".to_owned()],
            vec![(line("run.effect.send.unknown"), vec![pair("To", "Ann Lee")])],
        );
        if got != want {
            differ.push(format!("{end:?}: {got:?}"));
        }
    }
    assert!(
        differ.is_empty(),
        "{} ends differ: {differ:?}",
        differ.len()
    );
}

/// The verifier's finding 1 (h58): in a declined run, an unknown payment keeps its entry, its text
/// and its four fields after a send that was not done.
#[test]
fn u24q_an_unknown_payment_stands_beside_a_rejected_send_in_a_declined_run() {
    let mut h = history(2, Outcome::Declined);
    h.effects = vec![
        rejected(0xe9, "ab"),
        unknown(Class::Payment, payment().kept),
    ];
    let c = card(&facts(State::Card).remove(0), "N", &[h], &Limits::PRODUCT).unwrap();
    assert_eq!(
        keys(&c, 0),
        ["run.effect.send.not_done", "run.effect.payment.unknown"]
    );
    let e = &c.runs[0].effects[1];
    assert_eq!(e.text(), line("run.effect.payment.unknown"));
    assert_eq!(
        shown(e),
        [
            pair("Pay to", "City Power"),
            pair("Amount", "42.10 USD"),
            pair("Pay with", "Card ending 4242"),
            pair("Site", "site1.example")
        ]
    );
}

/// The verifier's finding 2 (d2, d6, d11, d12) under RD-158 (3): a kept value of no byte, with no
/// piece, one empty piece or three, never stands under a name that is not the first of its table
/// and in each class that shows fields, on the partial entry and on the unknown entry; the fields
/// that read beside it stand. A send stands as an unknown effect alone, because a committed send
/// that keeps `to` alone holds its whole table. Every place whose entry differs is named.
#[test]
fn u24q_a_kept_value_of_no_byte_never_stands_under_any_name_of_any_class() {
    let empties: [&[&str]; 3] = [&[], &[""], &["", "", ""]];
    let mut differ = Vec::new();
    for pieces in empties {
        let places = [
            (
                "the amount of a payment",
                Class::Payment,
                vec![
                    one_kept("payee", "City Power"),
                    kept_pieces("amount", pieces),
                ],
                vec![pair("Pay to", "City Power")],
                true,
            ),
            (
                "the party of a sign",
                Class::Sign,
                vec![
                    one_kept("document", "Lease"),
                    kept_pieces("party", pieces),
                    one_kept("note", "zq6"),
                ],
                vec![pair("Document", "Lease")],
                true,
            ),
            (
                "the seller of a purchase",
                Class::Purchase,
                vec![
                    kept_pieces("seller", pieces),
                    one_kept("amount", "19.99 USD"),
                ],
                vec![pair("Total", "19.99 USD")],
                true,
            ),
            (
                "the range of a local_edit",
                Class::LocalEdit,
                vec![
                    one_kept("file", "Shared/Books.xlsx"),
                    kept_pieces("range", pieces),
                    one_kept("note", "zq7"),
                ],
                vec![pair("File", "Books.xlsx")],
                true,
            ),
            (
                "the to of a send",
                Class::Send,
                vec![kept_pieces("to", pieces)],
                vec![],
                false,
            ),
        ];
        for (place, class, kept, fields, as_committed) in places {
            let mut forms = vec![(
                unknown(class, kept.clone()),
                format!("run.effect.{}.unknown", class.name()),
            )];
            if as_committed {
                forms.push((
                    committed(class, kept.clone()),
                    format!("run.effect.{}.partial", class.name()),
                ));
            }
            for (effect, key) in forms {
                let c = beside_a_good_run(effect);
                let e = &c.runs[0].effects[1];
                let got = (e.key(), shown(e));
                if got != (key.clone(), fields.clone()) {
                    differ.push(format!("{place}, pieces {pieces:?}, as {key}: {got:?}"));
                }
            }
        }
    }
    assert!(
        differ.is_empty(),
        "{} places differ: {differ:?}",
        differ.len()
    );
}

/// The verifier's findings 3 and 4 (d10, h47, h01): a name that the row keeps more than once never
/// stands, whatever its values (one of no byte among them), in every class and for every count, on
/// the entry of a refused committed effect and on the unknown entry, and the card holds no byte of
/// its values. Every case whose entry differs is named.
#[test]
fn u24q_a_name_kept_more_than_once_never_stands_in_any_class_or_count() {
    let cases = [
        (
            "payee twice, a value of no byte and then one that reads",
            Class::Payment,
            vec![
                kept_pieces("payee", &[]),
                one_kept("payee", "zq4 Power"),
                one_kept("amount", "42.10 USD"),
            ],
            vec![pair("Amount", "42.10 USD")],
            "zq4",
        ),
        (
            "payee twice, a value that reads and then one of no byte",
            Class::Payment,
            vec![
                one_kept("payee", "zq4 Power"),
                one_kept("amount", "42.10 USD"),
                kept_pieces("payee", &[""]),
            ],
            vec![pair("Amount", "42.10 USD")],
            "zq4",
        ),
        (
            "to twice in a send",
            Class::Send,
            vec![
                one_kept("to", "zq8 Lee"),
                one_kept("to", "zq8 Lee"),
                one_kept("note", "x"),
            ],
            vec![],
            "zq8",
        ),
        (
            "document twice in a sign, beside a party",
            Class::Sign,
            vec![
                one_kept("document", "zq9 one"),
                one_kept("party", "Bo Chan"),
                one_kept("document", "zq9 two"),
            ],
            vec![pair("Who else signs", "Bo Chan")],
            "zq9",
        ),
        (
            "payee three times with equal values, beside an amount",
            Class::Payment,
            vec![
                one_kept("payee", "zq3 Power"),
                one_kept("amount", "42.10 USD"),
                one_kept("payee", "zq3 Power"),
                one_kept("payee", "zq3 Power"),
            ],
            vec![pair("Amount", "42.10 USD")],
            "zq3",
        ),
        (
            "document three times with three values, beside a party",
            Class::Sign,
            vec![
                one_kept("document", "zq3a"),
                one_kept("document", "zq3b"),
                one_kept("document", "zq3c"),
                one_kept("party", "Bo Chan"),
            ],
            vec![pair("Who else signs", "Bo Chan")],
            "zq3",
        ),
    ];
    let mut differ = Vec::new();
    for (case, class, kept, fields, hidden) in cases {
        let refused = if fields.is_empty() {
            "unshown"
        } else {
            "partial"
        };
        for (effect, key) in [
            (
                committed(class, kept.clone()),
                format!("run.effect.{}.{refused}", class.name()),
            ),
            (
                unknown(class, kept.clone()),
                format!("run.effect.{}.unknown", class.name()),
            ),
        ] {
            let c = beside_a_good_run(effect);
            let e = &c.runs[0].effects[1];
            let all = serde_json::to_string(&c).unwrap();
            let got = (e.key(), shown(e), all.contains(hidden));
            if got != (key.clone(), fields.clone(), false) {
                differ.push(format!("{case}, as {key}: {got:?}"));
            }
        }
    }
    assert!(
        differ.is_empty(),
        "{} cases differ: {differ:?}",
        differ.len()
    );
}

/// The verifier's finding 5 (d7, h41, h02): a kept value stands where any of its pieces holds a
/// byte, whatever the count of its pieces, with its pieces as the row keeps them, and a kept name
/// is counted by its whole spelling, on the entry of a refused committed effect and on the unknown
/// entry. Every case whose entry differs is named.
#[test]
fn u24q_a_value_stands_by_any_piece_with_a_byte_and_a_name_counts_by_its_whole_spelling() {
    let pieces = |label: &str, values: &[&str]| {
        (
            label.to_owned(),
            values
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<String>>(),
        )
    };
    let cases = [
        (
            "a byte in the third piece alone",
            Class::Payment,
            vec![kept_pieces("payee", &["", "", "City Power"])],
            vec![pieces("Pay to", &["", "", "City Power"])],
            "",
        ),
        (
            "a value in three pieces that each read",
            Class::Send,
            vec![
                kept_pieces("to", &["Ann Lee, ", "Bob Ray, ", "Cy Dow"]),
                one_kept("note", "x"),
            ],
            vec![pieces("To", &["Ann Lee, ", "Bob Ray, ", "Cy Dow"])],
            "",
        ),
        (
            "a second name that begins as the table's name does",
            Class::Send,
            vec![one_kept("to", "Ann Lee"), one_kept("total", "zq5")],
            vec![pair("To", "Ann Lee")],
            "zq5",
        ),
    ];
    let mut differ = Vec::new();
    for (case, class, kept, fields, hidden) in cases {
        for (effect, key) in [
            (
                committed(class, kept.clone()),
                format!("run.effect.{}.partial", class.name()),
            ),
            (
                unknown(class, kept.clone()),
                format!("run.effect.{}.unknown", class.name()),
            ),
        ] {
            let c = beside_a_good_run(effect);
            let e = &c.runs[0].effects[1];
            let all = serde_json::to_string(&c).unwrap();
            let got = (
                e.key(),
                shown(e),
                !hidden.is_empty() && all.contains(hidden),
            );
            if got != (key.clone(), fields.clone(), false) {
                differ.push(format!("{case}, as {key}: {got:?}"));
            }
        }
    }
    assert!(
        differ.is_empty(),
        "{} cases differ: {differ:?}",
        differ.len()
    );
}

/// A name that a row keeps more than once stands on no entry, the entry of a rejected effect
/// among them, under either of its keys (U2.4w, ruling RD-161 R5).
#[test]
fn u24w_a_rejected_send_that_keeps_to_twice_shows_no_field_of_that_name_under_either_key() {
    let kept = vec![one_kept("to", "Ann Lee"), one_kept("to", "Zq7 Other")];
    for (refused, key) in [
        (false, "run.effect.send.not_done"),
        (true, "run.effect.send.refused"),
    ] {
        let e = CardEffect::not_done(Class::Send, &kept, refused).unwrap();
        assert_eq!(e.key(), key);
        assert_eq!(shown(&e), Vec::new(), "{key}");
    }
}

/// A kept value that shows nothing, of white space, of zero-width signs or of control characters
/// alone, is no value on a committed entry, on an unknown one and on its item `check` (U2.4w,
/// ruling RD-161 R6 as the coordinator widened it).
#[test]
fn u24w_a_payee_that_shows_nothing_stands_on_no_entry_and_not_on_the_check_item() {
    let labels = |fields: &[halvo_coord::types::Field]| -> Vec<String> {
        fields.iter().map(|f| f.label.clone()).collect()
    };
    for blank in [
        " \t\u{a0}\u{3000}",
        "\u{200b}\u{200c}\u{200d}\u{2060}\u{feff}",
        "\u{7}\u{0}\u{1b}\u{85}",
    ] {
        let mut kept = payment().kept;
        kept[0].value = vec![blank.to_owned()];
        let committed = CardEffect::committed(Class::Payment, &kept).unwrap();
        assert_eq!(committed.key(), "run.effect.payment", "{blank:?}");
        let unknown = CardEffect::unknown(Class::Payment, &kept).unwrap();
        let check = compose_check(
            &ItemId::parse(&format!("{:032x}", 0x61)).unwrap(),
            &AgentId::parse(&format!("{:032x}", 2)).unwrap(),
            "N",
            &[Unknown {
                run: run_of(2),
                effect: effect_id(0xe1),
                day: 20_760,
                class: Class::Payment,
                kept,
            }],
        )
        .unwrap();
        for (what, shown) in [
            ("committed", labels(committed.fields())),
            ("unknown", labels(unknown.fields())),
            ("check", labels(check.fields())),
        ] {
            assert_eq!(shown, ["Amount", "Pay with", "Site"], "{what} {blank:?}");
        }
    }
}
