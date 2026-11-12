mod common;

use common::{config, config_json, digest_of, effect_id};
use halvo_policy::{decide, effect_step, Checkpoint, Effect, EffectMove, StepCause, LIMITS};
use halvo_schema::executable::{ActionClass as C, Digest, EffectState as S};

fn effect(state: S, class: C, digest: Digest, rebinds: u32) -> Effect {
    Effect {
        id: effect_id(1),
        state,
        class,
        digest,
        rebinds,
    }
}

fn d1() -> Digest {
    digest_of('1')
}

fn d2() -> Digest {
    digest_of('2')
}

fn approve(entry: u8, digest: Digest, used: bool) -> EffectMove {
    EffectMove::Approve {
        entry: effect_id(entry),
        digest,
        used,
    }
}

fn rebind(digest: Digest, class: C) -> EffectMove {
    EffectMove::Rebind { digest, class }
}

fn begin(digest: Digest) -> EffectMove {
    EffectMove::Begin { digest }
}

const STATES: [S; 8] = [
    S::Refused,
    S::Cleared,
    S::Held,
    S::Approved,
    S::Rejected,
    S::Releasing,
    S::Committed,
    S::Unknown,
];

const EFFECT_CLASSES: [C; 7] = [
    C::LocalEdit,
    C::Send,
    C::Delete,
    C::Submit,
    C::Purchase,
    C::Payment,
    C::Sign,
];

#[test]
fn propose_makes_refused_cleared_or_held_by_the_decision_of_decide() {
    let observe = config(&config_json(
        "email",
        "observe",
        &[],
        ("yes", "yes", "no"),
        "none",
    ));
    let confirm = config(&config_json(
        "email",
        "draft_and_confirm",
        &[],
        ("yes", "yes", "yes"),
        "graph_mail",
    ));
    let unattended = config(&config_json(
        "spreadsheet_fill",
        "unattended",
        &["send"],
        ("no", "yes", "no"),
        "file",
    ));
    let propose = |d| EffectMove::Propose(d);
    assert_eq!(
        effect_step(None, propose(decide(&observe, C::Send, Checkpoint::No))),
        Ok(S::Refused)
    );
    assert_eq!(
        effect_step(None, propose(decide(&confirm, C::Payment, Checkpoint::No))),
        Ok(S::Held)
    );
    assert_eq!(
        effect_step(None, propose(decide(&unattended, C::Send, Checkpoint::No))),
        Ok(S::Cleared)
    );
    assert_eq!(
        effect_step(
            None,
            propose(decide(&confirm, C::LocalEdit, Checkpoint::Yes))
        ),
        Ok(S::Cleared)
    );
    assert_eq!(
        effect_step(
            None,
            propose(decide(&confirm, C::LocalEdit, Checkpoint::No))
        ),
        Ok(S::Refused)
    );
}

#[test]
fn propose_is_refused_for_an_effect_that_exists_and_for_a_class_that_makes_no_effect() {
    let confirm = config(&config_json(
        "email",
        "draft_and_confirm",
        &[],
        ("yes", "yes", "yes"),
        "graph_mail",
    ));
    for state in STATES {
        let e = effect(state, C::Payment, d1(), 0);
        let m = EffectMove::Propose(decide(&confirm, C::Payment, Checkpoint::No));
        assert_eq!(
            effect_step(Some(&e), m),
            Err(StepCause::Effect),
            "{state:?}"
        );
    }
    for class in [C::Read, C::Prepare, C::Credential] {
        let m = EffectMove::Propose(decide(&confirm, class, Checkpoint::No));
        assert_eq!(effect_step(None, m), Err(StepCause::Effect), "{class:?}");
    }
}

#[test]
fn rebind_answers_effect_class_effect_changed_in_that_order() {
    for state in STATES {
        let e = effect(state, C::Payment, d1(), 0);
        let got = effect_step(Some(&e), rebind(d2(), C::Payment));
        if matches!(state, S::Held | S::Approved) {
            assert_eq!(got, Ok(S::Held), "{state:?}");
        } else {
            assert_eq!(got, Err(StepCause::Effect), "{state:?}");
            assert_eq!(
                effect_step(Some(&e), rebind(d1(), C::Send)),
                Err(StepCause::Effect),
                "state before class: {state:?}"
            );
        }
    }
    assert_eq!(
        effect_step(None, rebind(d2(), C::Payment)),
        Err(StepCause::Effect)
    );
    let held_at_limit = effect(S::Held, C::Payment, d1(), LIMITS.rebinds);
    assert_eq!(
        effect_step(Some(&held_at_limit), rebind(d1(), C::Send)),
        Err(StepCause::Class),
        "class before the same digest and the limit"
    );
    assert_eq!(
        effect_step(Some(&held_at_limit), rebind(d1(), C::Payment)),
        Err(StepCause::Effect),
        "the same digest before the limit"
    );
    assert_eq!(
        effect_step(Some(&held_at_limit), rebind(d2(), C::Payment)),
        Err(StepCause::Changed)
    );
    let approved_at_limit = effect(S::Approved, C::Payment, d1(), LIMITS.rebinds);
    assert_eq!(
        effect_step(Some(&approved_at_limit), rebind(d2(), C::Payment)),
        Err(StepCause::Changed)
    );
    let above = effect(S::Held, C::Payment, d1(), LIMITS.rebinds + 1);
    assert_eq!(
        effect_step(Some(&above), rebind(d2(), C::Payment)),
        Err(StepCause::Changed)
    );
    let below = effect(S::Held, C::Payment, d1(), LIMITS.rebinds - 1);
    assert_eq!(
        effect_step(Some(&below), rebind(d2(), C::Payment)),
        Ok(S::Held)
    );
}

#[test]
fn approve_answers_effect_approval_digest_used_in_that_order() {
    for state in STATES {
        let e = effect(state, C::Payment, d1(), 0);
        let got = effect_step(Some(&e), approve(2, d2(), true));
        if state == S::Held {
            assert_eq!(got, Err(StepCause::Approval));
            assert_eq!(
                effect_step(Some(&e), approve(1, d1(), false)),
                Ok(S::Approved)
            );
        } else {
            assert_eq!(got, Err(StepCause::Effect), "{state:?}");
            assert_eq!(
                effect_step(Some(&e), approve(1, d1(), false)),
                Err(StepCause::Effect),
                "{state:?}"
            );
        }
    }
    assert_eq!(
        effect_step(None, approve(1, d1(), false)),
        Err(StepCause::Effect)
    );
    let held = effect(S::Held, C::Payment, d1(), 0);
    assert_eq!(
        effect_step(Some(&held), approve(2, d1(), false)),
        Err(StepCause::Approval)
    );
    assert_eq!(
        effect_step(Some(&held), approve(1, d2(), true)),
        Err(StepCause::Digest),
        "digest before used"
    );
    assert_eq!(
        effect_step(Some(&held), approve(1, d1(), true)),
        Err(StepCause::Used)
    );
}

#[test]
fn reject_leaves_held_alone_and_end_leaves_held_cleared_or_approved() {
    for state in STATES {
        let e = effect(state, C::Submit, d1(), 0);
        let reject = effect_step(Some(&e), EffectMove::Reject);
        let end = effect_step(Some(&e), EffectMove::End);
        if state == S::Held {
            assert_eq!(reject, Ok(S::Rejected));
        } else {
            assert_eq!(reject, Err(StepCause::Effect), "{state:?}");
        }
        if matches!(state, S::Held | S::Cleared | S::Approved) {
            assert_eq!(end, Ok(S::Rejected), "{state:?}");
        } else {
            assert_eq!(end, Err(StepCause::Effect), "{state:?}");
        }
    }
    assert_eq!(
        effect_step(None, EffectMove::Reject),
        Err(StepCause::Effect)
    );
    assert_eq!(effect_step(None, EffectMove::End), Err(StepCause::Effect));
}

#[test]
fn begin_answers_effect_unknown_approval_digest_in_that_order() {
    assert_eq!(effect_step(None, begin(d1())), Err(StepCause::Effect));
    for class in EFFECT_CLASSES {
        let money_or_sign = matches!(class, C::Purchase | C::Payment | C::Sign);
        for state in STATES {
            let e = effect(state, class, d1(), 0);
            let same = effect_step(Some(&e), begin(d1()));
            let other = effect_step(Some(&e), begin(d2()));
            let (want_same, want_other) = match state {
                S::Refused | S::Rejected | S::Committed | S::Releasing => {
                    (Err(StepCause::Effect), Err(StepCause::Effect))
                }
                S::Unknown => (Err(StepCause::Unknown), Err(StepCause::Unknown)),
                S::Held => (Err(StepCause::Approval), Err(StepCause::Approval)),
                S::Cleared if money_or_sign => (Err(StepCause::Approval), Err(StepCause::Approval)),
                S::Cleared | S::Approved => (Ok(S::Releasing), Err(StepCause::Digest)),
            };
            assert_eq!(same, want_same, "{class:?} {state:?} same digest");
            assert_eq!(other, want_other, "{class:?} {state:?} other digest");
        }
    }
}

#[test]
fn commit_and_lose_leave_releasing_alone() {
    for state in STATES {
        let e = effect(state, C::Payment, d1(), 0);
        let commit = effect_step(Some(&e), EffectMove::Commit);
        let lose = effect_step(Some(&e), EffectMove::Lose);
        if state == S::Releasing {
            assert_eq!(commit, Ok(S::Committed));
            assert_eq!(lose, Ok(S::Unknown));
        } else {
            assert_eq!(commit, Err(StepCause::Effect), "{state:?}");
            assert_eq!(lose, Err(StepCause::Effect), "{state:?}");
        }
    }
    assert_eq!(
        effect_step(None, EffectMove::Commit),
        Err(StepCause::Effect)
    );
    assert_eq!(effect_step(None, EffectMove::Lose), Err(StepCause::Effect));
}

#[test]
fn no_move_leaves_a_final_state() {
    let confirm = config(&config_json(
        "email",
        "draft_and_confirm",
        &[],
        ("yes", "yes", "yes"),
        "graph_mail",
    ));
    for state in [S::Refused, S::Rejected, S::Committed, S::Unknown] {
        let e = effect(state, C::Payment, d1(), 0);
        for m in [
            EffectMove::Propose(decide(&confirm, C::Payment, Checkpoint::No)),
            rebind(d2(), C::Payment),
            approve(1, d1(), false),
            EffectMove::Reject,
            EffectMove::End,
            begin(d1()),
            EffectMove::Commit,
            EffectMove::Lose,
        ] {
            let shown = format!("{m:?}");
            assert!(effect_step(Some(&e), m).is_err(), "{state:?} {shown}");
        }
    }
}

#[test]
fn the_causes_name_the_cause_list() {
    let names: Vec<&str> = [
        StepCause::Effect,
        StepCause::Class,
        StepCause::Changed,
        StepCause::Approval,
        StepCause::Digest,
        StepCause::Used,
        StepCause::Unknown,
    ]
    .iter()
    .map(|c| c.cause().as_str())
    .collect();
    assert_eq!(
        names,
        ["effect", "class", "changed", "approval", "digest", "used", "unknown"]
    );
}
