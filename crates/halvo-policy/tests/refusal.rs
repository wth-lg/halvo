mod common;

use halvo_policy::{decide_row, Checkpoint, Decision};
use halvo_schema::executable::names::YesNo;
use halvo_schema::executable::{ActionClass, AllowedClass, Autonomy, Caps};

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
