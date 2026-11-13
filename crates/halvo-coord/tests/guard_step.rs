#[path = "items_support.rs"]
mod support;

use halvo_coord::guard::{check, guard_step, EffectRecord, EffectState, Gives, Refusal, Table};
use halvo_coord::mem;
use halvo_coord::ports::PortError;
use halvo_coord::types::{
    Autonomy, Class, ConfigView, Decision, DecisionEntry, EffectAnswer, Presenter,
};
use support::{agent_id, approval, digest, effect_id, item_id, run_id};

fn entry(effect: u32, d: u32, p: u32, answer: EffectAnswer) -> DecisionEntry {
    DecisionEntry {
        approval: approval(p),
        effect: effect_id(effect),
        digest: digest(d),
        answer,
    }
}

/// The table after three items of run 1: item 1 allowed and released (`e1` send with `d1` and
/// `p1`, `e2` payment with `d2` and `p2`), item 2 rejected and released (`e3` with `d3` and
/// `p3`), and item 3 allowed and recorded but not released (`e4` with `d4` and `p4`).
fn table() -> Table {
    let allowed = Decision::new(vec![
        entry(1, 1, 1, EffectAnswer::AllowOnce),
        entry(2, 2, 2, EffectAnswer::AllowOnce),
    ])
    .unwrap();
    let rejected = Decision::new(vec![entry(3, 3, 3, EffectAnswer::RejectOnce)]).unwrap();
    let recorded = Decision::new(vec![entry(4, 4, 4, EffectAnswer::AllowOnce)]).unwrap();
    Table::new()
        .record(&item_id(1), &run_id(1), &allowed)
        .release(&item_id(1))
        .record(&item_id(2), &run_id(1), &rejected)
        .release(&item_id(2))
        .record(&item_id(3), &run_id(1), &recorded)
}

fn class_of(effect: u32) -> Class {
    if effect == 2 {
        Class::Payment
    } else {
        Class::Send
    }
}

fn rec(state: EffectState, effect: u32, d: u32, p: Option<u32>) -> EffectRecord {
    EffectRecord {
        run: run_id(1),
        agent: agent_id(1),
        effect: effect_id(effect),
        state,
        class: class_of(effect),
        digest: digest(d),
        approval: p.map(approval),
    }
}

fn approved(effect: u32, d: u32, p: u32) -> EffectRecord {
    rec(EffectState::Approved, effect, d, Some(p))
}

fn releasing(effect: u32, d: u32, p: u32) -> EffectRecord {
    rec(EffectState::Releasing, effect, d, Some(p))
}

/// The guard with a policy that gives `confirm`, so that only an approval can pass.
fn step(t: &Table, r: &EffectRecord) -> Result<Table, Refusal> {
    guard_step(t, r, Gives::Confirm)
}

/// Beside its rows the table keeps, for each run and effect id, the fact that the effect has
/// begun: set when the owner acknowledged the store of a record `releasing`, `committed` or
/// `unknown` of it, and not where the guard passes the record, so that a record whose store
/// failed began nothing; a lapse and a stop keep it, and a new life starts with none (I4.7,
/// ruling IG-32).
#[test]
fn the_fact_begun_is_set_when_the_owner_acknowledged_a_record_releasing_committed_or_unknown() {
    let t = table();
    for state in EffectState::ALL {
        let after = t.stored(&rec(*state, 1, 1, Some(1)));
        let begun = matches!(
            state,
            EffectState::Releasing | EffectState::Committed | EffectState::Unknown
        );
        assert_eq!(after.has_begun(&run_id(1), &effect_id(1)), begun, "{state}");
        assert!(
            !after.has_begun(&run_id(2), &effect_id(1)),
            "{state}: another run"
        );
        assert!(
            !after.has_begun(&run_id(1), &effect_id(2)),
            "{state}: another effect"
        );
    }
    let passed = step(&t, &releasing(1, 1, 1)).unwrap();
    assert!(
        !passed.has_begun(&run_id(1), &effect_id(1)),
        "the guard's pass begins nothing"
    );
    let begun = passed.stored(&releasing(1, 1, 1));
    assert!(begun
        .lapse(&item_id(1))
        .stop(&run_id(1))
        .has_begun(&run_id(1), &effect_id(1)));
    assert!(!Table::new().has_begun(&run_id(1), &effect_id(1)));
    let mut other = rec(EffectState::Committed, 1, 1, None);
    other.run = run_id(2);
    let both = begun.stored(&other);
    assert!(both.has_begun(&run_id(1), &effect_id(1)) && both.has_begun(&run_id(2), &effect_id(1)));
}

#[test]
fn refusal_guard_unmatched() {
    let t = table();
    assert_eq!(
        step(&t, &approved(1, 1, 9)).err(),
        Some(Refusal::Approval),
        "an approval id that no row holds"
    );
    let used = step(&t, &approved(1, 1, 1)).expect("the first use of p1");
    assert_eq!(
        step(&used, &approved(2, 1, 1)).err(),
        Some(Refusal::Approval),
        "the used row of p1 for another effect"
    );
    assert_eq!(
        step(&used, &approved(1, 2, 1)).err(),
        Some(Refusal::Approval),
        "the used row of p1 with another digest"
    );
    assert_eq!(
        step(&t, &approved(2, 1, 1)).err(),
        Some(Refusal::Approval),
        "the unused row of p1 for another effect"
    );
}

#[test]
fn refusal_guard_digest() {
    let t = table();
    for d in [2, 3, 4, 9] {
        assert_eq!(
            step(&t, &approved(1, d, 1)).err(),
            Some(Refusal::Approval),
            "e1 with p1 and the digest d{d}"
        );
    }
    assert!(
        step(&t, &approved(1, 1, 1)).is_ok(),
        "and with its own digest"
    );
}

#[test]
fn refusal_guard_money() {
    let t = table();
    assert_eq!(
        step(&t, &releasing(2, 2, 2)).err(),
        Some(Refusal::Approval),
        "the payment with its released row, which no approved record used"
    );
    assert_eq!(
        step(&t, &releasing(2, 2, 9)).err(),
        Some(Refusal::Approval),
        "the payment with an id that no row holds"
    );
    let no_approval = rec(EffectState::Releasing, 2, 2, None);
    for gives in [Gives::Run, Gives::Confirm, Gives::NoConfig] {
        assert_eq!(
            guard_step(&t, &no_approval, gives).err(),
            Some(Refusal::Approval),
            "the payment with no approval, the policy answering {gives:?}"
        );
    }
    let used = step(&t, &approved(2, 2, 2)).expect("the approved record uses p2");
    assert!(
        step(&used, &releasing(2, 2, 2)).is_ok(),
        "and passes once an approved record used the row"
    );
}

#[test]
fn refusal_guard_releasing_other() {
    let t = table();
    assert_eq!(
        step(&t, &releasing(1, 1, 2)).err(),
        Some(Refusal::Approval),
        "e1, of the class send, with p2, the released allow_once row of e2"
    );
    assert!(
        step(&t, &releasing(1, 1, 1)).is_ok(),
        "and passes with its own approval, used or not"
    );
}

#[test]
fn refusal_guard_repeat() {
    let t = table();
    let used = step(&t, &approved(1, 1, 1)).expect("the first record uses p1");
    assert_ne!(used, t, "the first record marks the row used");
    let again = step(&used, &approved(1, 1, 1)).expect("the repeat passes");
    assert_eq!(again, used, "and marks nothing");
}

#[test]
fn refusal_guard_no_approval_confirm() {
    let config = config(Autonomy::DraftAndConfirm);
    let policy = mem::policy::Policy::new(&[(Autonomy::DraftAndConfirm, Class::Send, false)]);
    let r = rec(EffectState::Releasing, 1, 1, None);
    assert_eq!(
        check(&policy, Some(&config), &table(), &r).err(),
        Some(Refusal::Approval)
    );
    assert_eq!(
        policy.asked(),
        vec![(Autonomy::DraftAndConfirm, Class::Send)]
    );
}

#[test]
fn refusal_guard_no_approval_run() {
    let config = config(Autonomy::Unattended);
    let policy = mem::policy::Policy::new(&[(Autonomy::Unattended, Class::Send, true)]);
    let r = rec(EffectState::Releasing, 1, 1, None);
    let t = table();
    assert_eq!(check(&policy, Some(&config), &t, &r), Ok(t.clone()));
    assert_eq!(policy.asked(), vec![(Autonomy::Unattended, Class::Send)]);
}

#[test]
fn the_memory_policy_gives_both_answers_by_its_rows() {
    let policy = mem::policy::Policy::new(&[
        (Autonomy::Unattended, Class::LocalEdit, true),
        (Autonomy::DraftAndConfirm, Class::LocalEdit, false),
    ]);
    let local = EffectRecord {
        class: Class::LocalEdit,
        ..rec(EffectState::Releasing, 7, 7, None)
    };
    let t = table();
    assert!(check(&policy, Some(&config(Autonomy::Unattended)), &t, &local).is_ok());
    assert_eq!(
        check(
            &policy,
            Some(&config(Autonomy::DraftAndConfirm)),
            &t,
            &local
        )
        .err(),
        Some(Refusal::Approval)
    );
    assert_eq!(
        check(&policy, Some(&config(Autonomy::Observe)), &t, &local).err(),
        Some(Refusal::Approval),
        "a pair with no row gives confirm"
    );
    assert_eq!(
        policy.asked(),
        vec![
            (Autonomy::Unattended, Class::LocalEdit),
            (Autonomy::DraftAndConfirm, Class::LocalEdit),
            (Autonomy::Observe, Class::LocalEdit),
        ]
    );
}

#[test]
fn a_record_with_no_approval_of_an_agent_with_no_configuration_is_refused_unasked() {
    let policy =
        mem::policy::Policy::<mem::Config>::new(&[(Autonomy::Unattended, Class::Send, true)]);
    let r = rec(EffectState::Releasing, 1, 1, None);
    assert_eq!(
        check(&policy, None, &table(), &r).err(),
        Some(Refusal::Approval)
    );
    assert!(policy.asked().is_empty());
    assert_eq!(
        guard_step(&table(), &r, Gives::NoConfig).err(),
        Some(Refusal::Approval)
    );
}

#[test]
fn a_policy_that_fails_refuses_the_record() {
    let policy = mem::policy::Policy::new(&[(Autonomy::Unattended, Class::Send, true)]);
    policy.fail(PortError::NoService);
    let r = rec(EffectState::Releasing, 1, 1, None);
    assert_eq!(
        check(&policy, Some(&config(Autonomy::Unattended)), &table(), &r).err(),
        Some(Refusal::Approval)
    );
}

#[test]
fn a_releasing_record_of_a_native_class_passes_only_on_a_used_row_whatever_the_policy() {
    let t = table();
    let used = step(&t, &approved(2, 2, 2)).unwrap();
    for gives in [Gives::Run, Gives::Confirm, Gives::NoConfig] {
        assert_eq!(
            guard_step(&t, &releasing(2, 2, 2), gives).err(),
            Some(Refusal::Approval),
            "unused, {gives:?}"
        );
        assert!(
            guard_step(&used, &releasing(2, 2, 2), gives).is_ok(),
            "used, {gives:?}"
        );
    }
    for class in [Class::Purchase, Class::Payment, Class::Sign] {
        let r = EffectRecord {
            class,
            ..rec(EffectState::Releasing, 1, 1, None)
        };
        for gives in [Gives::Run, Gives::Confirm, Gives::NoConfig] {
            assert_eq!(
                guard_step(&t, &r, gives).err(),
                Some(Refusal::Approval),
                "{class} with no approval, {gives:?}"
            );
        }
    }
}

/// A file edit with no approval passes when the table gives `run` (ruling XA-1), although the
/// presenter table puts `local_edit`, a class that it does not name, in the native dialog.
#[test]
fn a_releasing_record_with_no_approval_of_any_other_class_follows_the_policy() {
    let t = table();
    for class in Class::ALL {
        if matches!(class, Class::Purchase | Class::Payment | Class::Sign) {
            continue;
        }
        let r = EffectRecord {
            class: *class,
            ..rec(EffectState::Releasing, 1, 1, None)
        };
        assert_eq!(
            guard_step(&t, &r, Gives::Run),
            Ok(t.clone()),
            "{class}, run"
        );
        for gives in [Gives::Confirm, Gives::NoConfig] {
            assert_eq!(
                guard_step(&t, &r, gives).err(),
                Some(Refusal::Approval),
                "{class}, {gives:?}"
            );
        }
    }
}

#[test]
fn every_other_state_passes_whatever_it_holds() {
    let t = table().stop(&run_id(1));
    for state in EffectState::ALL {
        if matches!(state, EffectState::Approved | EffectState::Releasing) {
            continue;
        }
        for p in [None, Some(9), Some(3)] {
            assert_eq!(
                step(&t, &rec(*state, 9, 9, p)),
                Ok(t.clone()),
                "{state} with {p:?} for a stopped run"
            );
        }
    }
}

#[test]
fn an_approved_record_with_no_approval_is_refused() {
    for gives in [Gives::Run, Gives::Confirm] {
        assert_eq!(
            guard_step(&table(), &rec(EffectState::Approved, 1, 1, None), gives).err(),
            Some(Refusal::Approval)
        );
    }
}

#[test]
fn a_row_counts_only_from_the_release_of_its_decision() {
    let t = table();
    assert_eq!(step(&t, &approved(4, 4, 4)).err(), Some(Refusal::Approval));
    assert_eq!(step(&t, &releasing(4, 4, 4)).err(), Some(Refusal::Approval));
    let released = t.release(&item_id(3));
    assert!(step(&released, &approved(4, 4, 4)).is_ok());
    assert!(step(&released, &releasing(4, 4, 4)).is_ok());
}

/// A row counts for a record of its own run alone, so a record that names another run passes on
/// no row, and none passes once the row's run was stopped (I4.7, L9, L10).
#[test]
fn a_record_of_another_run_passes_on_no_row() {
    let t = table();
    for state in [EffectState::Approved, EffectState::Releasing] {
        let other = EffectRecord {
            run: run_id(2),
            ..rec(state, 1, 1, Some(1))
        };
        assert_eq!(step(&t, &other).err(), Some(Refusal::Approval), "{state}");
        assert_eq!(
            step(&t.stop(&run_id(1)), &other).err(),
            Some(Refusal::Approval),
            "{state} after run 1 stopped"
        );
    }
}

/// An approval id that a row of a reject_once decision holds passes nothing, also beside an
/// allow_once row of the same id, which a port that breaks L3 would make (L9).
#[test]
fn an_approval_id_of_a_reject_once_row_passes_nothing() {
    let rejected = Decision::new(vec![entry(1, 1, 1, EffectAnswer::RejectOnce)]).unwrap();
    let allowed = Decision::new(vec![entry(1, 1, 1, EffectAnswer::AllowOnce)]).unwrap();
    let t = Table::new()
        .record(&item_id(1), &run_id(1), &rejected)
        .release(&item_id(1))
        .record(&item_id(2), &run_id(1), &allowed)
        .release(&item_id(2));
    for r in [approved(1, 1, 1), releasing(1, 1, 1)] {
        assert_eq!(step(&t, &r).err(), Some(Refusal::Approval), "{r:?}");
    }
}

/// The release of one item's decision counts that item's rows and no other item's.
#[test]
fn a_release_counts_the_rows_of_its_own_item_alone() {
    let later = Decision::new(vec![entry(5, 5, 5, EffectAnswer::AllowOnce)]).unwrap();
    let t = table()
        .record(&item_id(4), &run_id(1), &later)
        .release(&item_id(4));
    assert!(step(&t, &approved(5, 5, 5)).is_ok(), "item 4's own row");
    assert_eq!(
        step(&t, &approved(4, 4, 4)).err(),
        Some(Refusal::Approval),
        "item 3's row, recorded and not released"
    );
}

/// A second `release` of one item renumbers no row: its decision keeps the number of its first
/// release, so that a later item's decision for the same effect stays the newest (I4.7, ruling
/// IG-25; the reader of U2.2b's copy O4).
#[test]
fn a_second_release_of_one_item_renumbers_no_row() {
    let allowed = Decision::new(vec![entry(1, 1, 1, EffectAnswer::AllowOnce)]).unwrap();
    let rejected = Decision::new(vec![entry(1, 2, 2, EffectAnswer::RejectOnce)]).unwrap();
    let t = Table::new()
        .record(&item_id(1), &run_id(1), &allowed)
        .release(&item_id(1))
        .record(&item_id(2), &run_id(1), &rejected)
        .release(&item_id(2));
    assert_eq!(
        t.newest_released(&run_id(1), &effect_id(1)),
        Some(EffectAnswer::RejectOnce)
    );
    let again = t.release(&item_id(1));
    assert_eq!(
        again.newest_released(&run_id(1), &effect_id(1)),
        Some(EffectAnswer::RejectOnce),
        "item 1's decision does not become the newest"
    );
    assert!(!again.newest_allows(&run_id(1), &effect_id(1)));
    assert_eq!(again, t, "the table does not change");
}

#[test]
fn the_ids_of_a_reject_once_decision_pass_nothing() {
    let t = table();
    assert_eq!(step(&t, &approved(3, 3, 3)).err(), Some(Refusal::Approval));
    assert_eq!(step(&t, &releasing(3, 3, 3)).err(), Some(Refusal::Approval));
}

#[test]
fn the_rows_of_an_item_that_lapsed_pass_nothing() {
    let t = table()
        .release(&item_id(3))
        .lapse(&item_id(3))
        .lapse(&item_id(1));
    for r in [
        approved(1, 1, 1),
        releasing(1, 1, 1),
        approved(4, 4, 4),
        releasing(4, 4, 4),
    ] {
        assert_eq!(step(&t, &r).err(), Some(Refusal::Approval), "{r:?}");
    }
}

#[test]
fn no_approved_or_releasing_record_passes_for_a_stopped_run() {
    let t = table().stop(&run_id(1));
    let used = table().stop(&run_id(2));
    assert!(
        step(&used, &approved(1, 1, 1)).is_ok(),
        "another run was stopped"
    );
    for r in [approved(1, 1, 1), releasing(1, 1, 1)] {
        assert_eq!(step(&t, &r).err(), Some(Refusal::Approval), "{r:?}");
    }
    let no_approval = rec(EffectState::Releasing, 1, 1, None);
    assert_eq!(
        guard_step(&t, &no_approval, Gives::Run).err(),
        Some(Refusal::Approval)
    );
}

/// The guard with effect, digest and approval id varied apart, as the model's guard moves do:
/// a record passes only when all three are those of one released `allow_once` row.
#[test]
fn effect_digest_and_approval_id_varied_apart() {
    let t = table().release(&item_id(3));
    let rows = [(1, 1, 1), (2, 2, 2), (4, 4, 4)];
    let mut tried = 0;
    let mut passed = 0;
    for effect in [1, 2, 3, 4, 9] {
        for d in [1, 2, 3, 4, 9] {
            for p in [1, 2, 3, 4, 9] {
                let row = rows.contains(&(effect, d, p));
                tried += 2;
                let a = step(&t, &approved(effect, d, p));
                assert_eq!(a.is_ok(), row, "approved e{effect} d{d} p{p}");
                let native = class_of(effect) == Class::Payment;
                let r = step(&t, &releasing(effect, d, p));
                assert_eq!(r.is_ok(), row && !native, "releasing e{effect} d{d} p{p}");
                if let Ok(after) = a {
                    passed += 1;
                    let r = step(&after, &releasing(effect, d, p));
                    assert!(r.is_ok(), "releasing e{effect} d{d} p{p} after its use");
                }
            }
        }
    }
    assert_eq!((tried, passed), (250, 3));
}

#[test]
fn the_effect_states_are_the_eight_of_the_effect_machine() {
    let names: Vec<_> = EffectState::ALL.iter().map(|s| s.name()).collect();
    assert_eq!(
        names,
        [
            "refused",
            "cleared",
            "held",
            "approved",
            "rejected",
            "releasing",
            "committed",
            "unknown"
        ]
    );
    for s in EffectState::ALL {
        assert_eq!(EffectState::parse(s.name()), Ok(*s));
    }
    assert!(EffectState::parse("other").is_err());
    assert_eq!(Refusal::Approval.name(), "approval");
}

#[test]
fn the_table_takes_presenter_native_from_the_presenter_table() {
    for class in Class::ALL {
        let native = halvo_coord::items::presenter(*class) == Presenter::Native;
        let t = table();
        let r = EffectRecord {
            class: *class,
            ..releasing(1, 1, 1)
        };
        assert_eq!(step(&t, &r).is_ok(), !native, "{class}");
    }
}

fn config(autonomy: Autonomy) -> mem::Config {
    let line = format!(
        concat!(
            r#"{{"v":1,"workflow":"{w}","agent":"{a}","rev":1,"category":"email","#,
            r#""tier":"on_device","reasoning":"medium","autonomy":"{autonomy}","allow":[],"#,
            r#""caps":{{"untrusted":"yes","private":"yes","external":"yes"}},"#,
            r#""limits":{{"steps":40,"wall_s":600,"model_tokens":0}},"budget":null,"#,
            r#""permissions":[],"runtime":"graph_mail","playbook":null}}"#
        ),
        w = support::workflow_id(1),
        a = agent_id(1),
        autonomy = autonomy.name(),
    );
    let c = mem::Config::parse(&line).expect("a configuration record");
    assert_eq!(c.autonomy(), autonomy);
    c
}
