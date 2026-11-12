mod common;

use common::{bf, bound, config, config_json, effect_id};
use halvo_policy::{
    decide, decide_row, digest, effect_step, preset, validate, Checkpoint, ConfigCause, Decision,
    Effect, EffectMove, StepCause,
};
use halvo_schema::executable::names::YesNo;
use halvo_schema::executable::{
    ActionClass, AllowedClass, Autonomy, Caps, Digest, EffectId, EffectState,
};
use halvo_schema::{AgentId, Category, SeedCategory, WorkflowId};

/// An effect as the store's handler reads it from its rows: the last row's state, class and digest and the count
/// of its accepted rebinds.
struct Rows {
    effect: Option<Effect>,
    id: EffectId,
    class: ActionClass,
}

impl Rows {
    fn new(id: EffectId, class: ActionClass) -> Rows {
        Rows {
            effect: None,
            id,
            class,
        }
    }

    fn at(id: EffectId, state: EffectState, class: ActionClass, digest: Digest) -> Rows {
        Rows {
            effect: Some(Effect {
                id: id.clone(),
                state,
                class,
                digest,
                rebinds: 0,
            }),
            id,
            class,
        }
    }

    fn step(&mut self, m: EffectMove, first_digest: &Digest) -> Result<EffectState, StepCause> {
        let rebound = match &m {
            EffectMove::Rebind { digest, .. } => Some(digest.clone()),
            _ => None,
        };
        let answer = effect_step(self.effect.as_ref(), m)?;
        match (&mut self.effect, rebound) {
            (None, _) => {
                self.effect = Some(Effect {
                    id: self.id.clone(),
                    state: answer,
                    class: self.class,
                    digest: first_digest.clone(),
                    rebinds: 0,
                })
            }
            (Some(e), Some(digest)) => {
                e.state = answer;
                e.digest = digest;
                e.rebinds += 1;
            }
            (Some(e), None) => e.state = answer,
        }
        Ok(answer)
    }
}

fn pay(amount: &str) -> Digest {
    let b = bound(
        ActionClass::Payment,
        &[
            bf("payee", "slot", None, "Northwind Water"),
            bf(
                "amount",
                "extract",
                Some("https://bill.example:443"),
                amount,
            ),
            bf("instrument", "slot", None, "card ending 4242"),
            bf("origin", "const", None, "https://pay.example:443"),
        ],
    );
    match digest(&b) {
        Ok(d) => d,
        Err(r) => panic!("{r}"),
    }
}

fn confirm_config() -> halvo_schema::executable::AgentConfig {
    config(&config_json(
        "utility_bill_payment",
        "draft_and_confirm",
        &[],
        ("yes", "yes", "yes"),
        "web",
    ))
}

fn held_payment(id: EffectId, first: &Digest) -> Rows {
    let mut rows = Rows::new(id, ActionClass::Payment);
    let decided = decide(&confirm_config(), ActionClass::Payment, Checkpoint::No);
    assert_eq!(
        rows.step(EffectMove::Propose(decided), first),
        Ok(EffectState::Held)
    );
    rows
}

fn approve(entry: &EffectId, digest: &Digest, used: bool) -> EffectMove {
    EffectMove::Approve {
        entry: entry.clone(),
        digest: digest.clone(),
        used,
    }
}

#[test]
fn refusal_no_approval() {
    let d = pay("42.17");
    for class in [ActionClass::Payment, ActionClass::Purchase] {
        for state in [EffectState::Held, EffectState::Cleared] {
            let mut rows = Rows::at(effect_id(1), state, class, d.clone());
            assert_eq!(
                rows.step(EffectMove::Begin { digest: d.clone() }, &d),
                Err(StepCause::Approval),
                "{class:?} {state:?}"
            );
        }
    }
    let mut rows = held_payment(effect_id(1), &d);
    assert_eq!(
        rows.step(EffectMove::Begin { digest: d.clone() }, &d),
        Err(StepCause::Approval)
    );
}

#[test]
fn refusal_digest() {
    let shown = pay("42.17");
    let other = pay("4217.00");
    assert_ne!(shown, other);
    let id = effect_id(1);
    let mut rows = held_payment(id.clone(), &shown);
    assert_eq!(
        rows.step(approve(&id, &other, false), &shown),
        Err(StepCause::Digest)
    );
    assert_eq!(
        rows.step(approve(&id, &shown, false), &shown),
        Ok(EffectState::Approved)
    );
}

#[test]
fn refusal_reuse() {
    let d = pay("42.17");
    let first = effect_id(1);
    let second = effect_id(2);
    let mut one = held_payment(first.clone(), &d);
    let mut two = held_payment(second.clone(), &d);
    assert_eq!(
        one.step(approve(&first, &d, false), &d),
        Ok(EffectState::Approved)
    );
    assert_eq!(
        two.step(approve(&first, &d, true), &d),
        Err(StepCause::Approval),
        "an approval given for the first effect, offered to the second"
    );

    let away = pay("42.18");
    let rebind = |digest: &Digest| EffectMove::Rebind {
        digest: digest.clone(),
        class: ActionClass::Payment,
    };
    assert_eq!(one.step(rebind(&away), &d), Ok(EffectState::Held));
    assert_eq!(one.step(rebind(&d), &d), Ok(EffectState::Held));
    assert_eq!(
        one.step(approve(&first, &d, true), &d),
        Err(StepCause::Used),
        "the void approval, after a rebind away and back"
    );
}

#[test]
fn refusal_retry_unknown() {
    let d = pay("42.17");
    let id = effect_id(1);
    let mut rows = held_payment(id.clone(), &d);
    assert_eq!(
        rows.step(approve(&id, &d, false), &d),
        Ok(EffectState::Approved)
    );
    assert_eq!(
        rows.step(EffectMove::Begin { digest: d.clone() }, &d),
        Ok(EffectState::Releasing)
    );
    assert_eq!(rows.step(EffectMove::Lose, &d), Ok(EffectState::Unknown));
    assert_eq!(
        rows.step(EffectMove::Begin { digest: d.clone() }, &d),
        Err(StepCause::Unknown)
    );
    assert_eq!(
        rows.step(
            EffectMove::Begin {
                digest: pay("1.00")
            },
            &d
        ),
        Err(StepCause::Unknown)
    );
}

#[test]
fn refusal_changed_after_approval() {
    let first = pay("42.17");
    let changed = pay("42.18");
    let id = effect_id(1);
    let mut rows = held_payment(id.clone(), &first);
    assert_eq!(
        rows.step(approve(&id, &first, false), &first),
        Ok(EffectState::Approved)
    );
    assert_eq!(
        rows.step(
            EffectMove::Begin {
                digest: changed.clone()
            },
            &first
        ),
        Err(StepCause::Digest),
        "begin of an approved effect with another digest"
    );
    let rebind = |digest: &Digest| EffectMove::Rebind {
        digest: digest.clone(),
        class: ActionClass::Payment,
    };
    assert_eq!(rows.step(rebind(&changed), &first), Ok(EffectState::Held));
    assert_eq!(
        rows.step(approve(&id, &first, true), &first),
        Err(StepCause::Digest),
        "the first approval after the rebind"
    );
    assert_eq!(rows.step(rebind(&first), &first), Ok(EffectState::Held));
    assert_eq!(
        rows.step(approve(&id, &first, true), &first),
        Err(StepCause::Used),
        "the first approval after a rebind back to the first digest"
    );
}

#[test]
fn refusal_begin_after_end() {
    let d = pay("42.17");
    for state in [
        EffectState::Held,
        EffectState::Cleared,
        EffectState::Approved,
    ] {
        let mut rows = Rows::at(effect_id(1), state, ActionClass::Submit, d.clone());
        assert_eq!(
            rows.step(EffectMove::End, &d),
            Ok(EffectState::Rejected),
            "{state:?}"
        );
        assert_eq!(
            rows.step(EffectMove::Begin { digest: d.clone() }, &d),
            Err(StepCause::Effect),
            "{state:?}"
        );
    }
}

fn ids() -> (WorkflowId, AgentId) {
    (
        WorkflowId::from_random([0x30; 16]),
        AgentId::from_random([0x40; 16]),
    )
}

#[test]
fn refusal_allow_split() {
    for &category in SeedCategory::ALL {
        let (workflow, agent) = ids();
        let Some(mut c) = preset(Category::from(category), workflow, agent) else {
            panic!("no preset for {category:?}");
        };
        c.autonomy = Autonomy::Unattended;
        c.allow = vec![AllowedClass::Send];
        c.caps.untrusted = YesNo::Yes;
        c.caps.private = YesNo::Yes;
        for external in [YesNo::Yes, YesNo::No] {
            c.caps.external = external;
            assert_eq!(
                validate(&c),
                Err(ConfigCause::Allow),
                "{category:?} {external:?}"
            );
        }
    }
}

#[test]
fn refusal_unattended_default() {
    let mut presets = 0;
    for &category in SeedCategory::ALL {
        let (workflow, agent) = ids();
        let Some(c) = preset(Category::from(category), workflow, agent) else {
            panic!("no preset for {category:?}");
        };
        assert_ne!(c.autonomy, Autonomy::Unattended, "{category:?}");
        presets += 1;
    }
    assert_eq!(presets, 10);
}

fn allow_sets() -> Vec<Vec<AllowedClass>> {
    (0u8..8)
        .map(|bits| {
            AllowedClass::ALL
                .iter()
                .enumerate()
                .filter(|(i, _)| bits & (1 << i) != 0)
                .map(|(_, a)| *a)
                .collect()
        })
        .collect()
}

fn all_caps() -> Vec<Caps> {
    let yn = |bit: u8| if bit == 1 { YesNo::Yes } else { YesNo::No };
    (0u8..8)
        .map(|bits| common::caps(yn(bits & 1), yn((bits >> 1) & 1), yn((bits >> 2) & 1)))
        .collect()
}

const CHECKPOINTS: [Checkpoint; 2] = [Checkpoint::Yes, Checkpoint::No];

#[test]
fn refusal_observe_effect() {
    let mut tried = 0;
    for &class in ActionClass::ALL {
        if matches!(class, ActionClass::Read | ActionClass::Credential) {
            continue;
        }
        for allow in allow_sets() {
            for caps in all_caps() {
                for checkpoint in CHECKPOINTS {
                    assert_eq!(
                        decide_row(Autonomy::Observe, class, &allow, &caps, checkpoint),
                        Decision::Refuse,
                        "{class:?} under observe with {allow:?} {caps:?} {checkpoint:?}"
                    );
                    tried += 1;
                }
            }
        }
    }
    assert_eq!(tried, 8 * 8 * 8 * 2);
}

#[test]
fn refusal_no_checkpoint() {
    let mut tried = 0;
    for &autonomy in Autonomy::ALL {
        for allow in allow_sets() {
            for caps in all_caps() {
                assert_eq!(
                    decide_row(
                        autonomy,
                        ActionClass::LocalEdit,
                        &allow,
                        &caps,
                        Checkpoint::No
                    ),
                    Decision::Refuse,
                    "local_edit with no checkpoint under {autonomy:?} with {allow:?} {caps:?}"
                );
                tried += 1;
            }
        }
    }
    assert_eq!(tried, 3 * 8 * 8);
}
