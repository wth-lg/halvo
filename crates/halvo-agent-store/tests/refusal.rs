mod common;

use common::*;
use halvo_schema::executable::names::EffectState;
use halvo_schema::executable::record::RUN_LIST_KEYS;
use halvo_schema::executable::{AgentRun, Outcome};
use halvo_schema::AgentId;

fn stored_run(store: &halvo_agent_store::MemoryStore, r: u32) -> Vec<AgentRun> {
    rows(store, "agent_run")
        .iter()
        .map(|row| AgentRun::parse(row.as_bytes()).expect("a run row"))
        .filter(|row| row.run.as_str() == run(r))
        .collect()
}

#[test]
fn refusal_confirm_as_run() {
    let cases = [
        ("email", "send", "cleared", false),
        ("email", "send", "refused", false),
        ("email", "send", "held", true),
        ("utility_bill_payment", "payment", "cleared", false),
        ("utility_bill_payment", "payment", "held", true),
        ("shop_order", "purchase", "cleared", false),
        ("shop_order", "purchase", "held", true),
        ("calendar_cleanup", "delete", "cleared", false),
        ("calendar_cleanup", "delete", "held", true),
        ("tenant_rent", "payment", "held", false),
        ("tenant_rent", "payment", "cleared", false),
        ("tenant_rent", "payment", "refused", true),
        ("lease_renewal_or_change", "sign", "held", false),
        ("lease_renewal_or_change", "sign", "refused", true),
        ("spreadsheet_fill", "local_edit", "cleared", true),
        ("spreadsheet_fill", "local_edit", "refused", true),
        ("spreadsheet_fill", "local_edit", "held", false),
        ("daily_chart_check", "local_edit", "cleared", false),
        ("daily_chart_check", "submit", "refused", true),
    ];
    for (category, class, state, taken) in cases {
        let mut store = store_with(category);
        let answer = put(&mut store, row(1, 1, 1, state, class, 1, None, None));
        if taken {
            assert_eq!(answer, Ok(stored(1)), "{category} {class} {state}");
            assert_eq!(effects(&store).len(), 1);
        } else {
            assert_eq!(
                answer,
                Err(declared("effect")),
                "{category} {class} {state}"
            );
            assert!(effects(&store).is_empty());
        }
    }

    let mut store = unattended_sender();
    assert_eq!(
        put(&mut store, row(1, 1, 1, "held", "send", 1, None, None)),
        Err(declared("effect"))
    );
    assert_eq!(
        put(&mut store, row(1, 1, 1, "cleared", "send", 1, None, None)),
        Ok(stored(1))
    );
    assert_eq!(
        put(&mut store, row(1, 2, 1, "cleared", "delete", 1, None, None)),
        Err(declared("effect"))
    );
    assert_eq!(
        put(&mut store, row(1, 2, 1, "held", "delete", 1, None, None)),
        Ok(stored(1))
    );
}

#[test]
fn refusal_propose_after_end() {
    let mut store = store_with("utility_bill_payment");
    assert_eq!(
        put(&mut store, row(1, 1, 1, "held", "payment", 1, None, None)),
        Ok(stored(1))
    );
    assert_eq!(
        finish(&mut store, run_record(1, Some("replay"), "failed")),
        Ok(stored(1))
    );
    assert_eq!(
        put(&mut store, row(1, 2, 1, "held", "payment", 1, None, None)),
        Err(declared("effect"))
    );
    assert_eq!(end_run(&mut store, 2), Ok(accepted()));
    assert_eq!(
        put(&mut store, row(2, 3, 1, "held", "payment", 1, None, None)),
        Err(declared("effect"))
    );
    assert_eq!(effects(&store).len(), 2);
    assert_eq!(
        put(&mut store, row(3, 4, 1, "held", "payment", 1, None, None)),
        Ok(stored(1))
    );
}

#[test]
fn refusal_end_run_loses() {
    let mut store = unattended_sender();
    let moves = [
        row(1, 1, 1, "cleared", "send", 1, None, None),
        row(1, 1, 2, "releasing", "send", 1, None, None),
        row(1, 2, 1, "held", "delete", 1, None, None),
        row(1, 3, 1, "cleared", "send", 1, None, None),
        row(1, 4, 1, "held", "delete", 1, None, None),
        row(1, 4, 2, "approved", "delete", 1, Some(1), None),
        row(1, 5, 1, "cleared", "send", 1, None, None),
        row(1, 5, 2, "releasing", "send", 1, None, None),
        row(1, 5, 3, "committed", "send", 1, None, None),
        row(1, 6, 1, "held", "delete", 1, None, None),
        row(1, 6, 2, "rejected", "delete", 1, None, None),
        row(2, 7, 1, "cleared", "send", 1, None, None),
        row(2, 7, 2, "releasing", "send", 1, None, None),
    ];
    for m in moves {
        assert_eq!(put(&mut store, m.clone()), Ok(stored(1)), "{m}");
    }
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    let expected = [
        (1, EffectState::Unknown, None, 3),
        (2, EffectState::Rejected, Some("ended"), 2),
        (3, EffectState::Rejected, Some("ended"), 2),
        (4, EffectState::Rejected, Some("ended"), 3),
        (5, EffectState::Committed, None, 3),
        (6, EffectState::Rejected, None, 2),
        (7, EffectState::Releasing, None, 2),
    ];
    for (e, state, cause, seq) in expected {
        let last = last_of(&store, &effect(e));
        assert_eq!(last.state, state, "effect {e}");
        assert_eq!(last.cause.map(|c| c.as_str()), cause, "effect {e}");
        assert_eq!(last.seq.get(), seq, "effect {e}");
    }
    for e in 1..=4 {
        let last = last_of(&store, &effect(e));
        assert_eq!((last.day, last.at), (NOW.day, NOW.at));
        assert_eq!(last.approval, None);
    }
    let record = stored_run(&store, 1);
    assert_eq!(record.len(), 1);
    let record = &record[0];
    assert_eq!(record.mode, None);
    assert_eq!(record.outcome, Outcome::Failed);
    assert_eq!(record.day, NOW.day);
    assert_eq!(record.agent, AgentId::parse(&agent(1)).expect("an id"));
    assert_eq!(record.playbook, None);
    assert_eq!(record.model, None);
    let counts = [
        record.steps,
        record.prompts,
        record.handbacks,
        record.confirmations,
        record.effects,
        record.unknown,
        record.model_calls,
        record.tokens_in,
        record.tokens_out,
        record.wall_s,
    ];
    assert_eq!(counts, [0; 10]);
    let stored: serde_json::Value =
        serde_json::from_str(&rows(&store, "agent_run")[0]).expect("a run row");
    for key in RUN_LIST_KEYS {
        assert_eq!(stored.get(*key), Some(&serde_json::Value::Null), "{key}");
    }
    let before = effects(&store).len();
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    assert_eq!(effects(&store).len(), before);
    assert_eq!(rows(&store, "agent_run").len(), 1);
}

#[test]
fn refusal_repeat_begin() {
    let mut store = store_with("utility_bill_payment");
    let begin = row(1, 1, 3, "releasing", "payment", 1, Some(1), None);
    for m in [
        row(1, 1, 1, "held", "payment", 1, None, None),
        row(1, 1, 2, "approved", "payment", 1, Some(1), None),
        begin.clone(),
    ] {
        assert_eq!(put(&mut store, m), Ok(stored(1)));
    }
    assert_eq!(put(&mut store, begin.clone()), Ok(stored(1)));
    assert_eq!(effects(&store).len(), 3);
    assert_eq!(end_run(&mut store, 1), Ok(accepted()));
    let lost = last_of(&store, &effect(1));
    assert_eq!(lost.state, EffectState::Unknown);
    assert_eq!(lost.seq.get(), 4);
    assert_eq!(put(&mut store, begin), Err(declared("effect")));
    assert_eq!(effects(&store).len(), 4);
    assert_eq!(last_of(&store, &effect(1)).state, EffectState::Unknown);
}
