//! The admission test and the cap states of contract T8. The router holds no list, no balance and
//! no cap: a caller passes the room that it read and the estimates of its running tasks, and drops
//! a task's estimate when the task's run ends, because the room that it reads already holds any
//! hold of that run that is still open.

use crate::policy::Policy;
use crate::prices::{Micro, MICRO_PER_MINOR};
use crate::ruling::Ruling;
use crate::spend::Account;
use crate::tier::CategoryId;

/// The four values of a cap answer: the cap in minor units, what is spent and what is held in
/// micro-units, and the end of the cap period in milliseconds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Room {
    pub cap_minor: u64,
    pub spent: Micro,
    pub held: Micro,
    pub period_end_ms: u64,
}

impl Room {
    /// The cap times `MICRO_PER_MINOR`, less `spent`, less `held`: 0 when that is negative, and
    /// `u64::MAX` when it does not fit a `u64`.
    pub fn left(&self) -> Micro {
        Micro::try_from(self.left_wide()).unwrap_or(Micro::MAX)
    }

    /// The room of an account of the spend rule, or none when the account has no cap.
    pub fn of(account: &Account, period_end_ms: u64) -> Option<Room> {
        account.cap_minor.map(|cap_minor| Room {
            cap_minor,
            spent: account.spent,
            held: account.held,
            period_end_ms,
        })
    }

    fn cap_wide(&self) -> u128 {
        u128::from(self.cap_minor) * u128::from(MICRO_PER_MINOR)
    }

    fn left_wide(&self) -> u128 {
        self.cap_wide()
            .saturating_sub(u128::from(self.spent) + u128::from(self.held))
    }
}

/// What `fits` finds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Fit {
    Overflow,
    Fits,
    Over,
}

/// The arithmetic alone: `Overflow` when the estimate and the running estimates sum to more than a
/// `u64` holds, `Fits` when the sum is at or under `room.left()`, else `Over`.
pub fn fits(room: &Room, estimate: Micro, running: &[Micro]) -> Fit {
    let sum = running.iter().map(|each| u128::from(*each)).sum::<u128>() + u128::from(estimate);
    match Micro::try_from(sum) {
        Err(_) => Fit::Overflow,
        Ok(sum) if sum <= room.left() => Fit::Fits,
        Ok(_) => Fit::Over,
    }
}

/// Why `admit` refuses a task.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AdmitCause {
    Ruling,
    NoCap,
    Overflow,
}

impl AdmitCause {
    pub const ALL: [AdmitCause; 3] = [AdmitCause::Ruling, AdmitCause::NoCap, AdmitCause::Overflow];

    pub const fn as_str(self) -> &'static str {
        match self {
            AdmitCause::Ruling => "ruling",
            AdmitCause::NoCap => "no_cap",
            AdmitCause::Overflow => "overflow",
        }
    }
}

/// The answer of `admit`: start the task, wait until the end of the cap period, or refuse it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Admission {
    Admit,
    Wait { until_ms: u64 },
    Refuse(AdmitCause),
}

/// The test that a caller uses before it starts a task, the first rule that fits: an estimate of 0
/// is `Admit`, because a task whose every step runs on the device spends nothing; the ruling `None`
/// is `Refuse(ruling)`; no room is `Refuse(no_cap)`; then by `fits`, `Overflow` is
/// `Refuse(overflow)`, `Fits` is `Admit` and `Over` is `Wait` until the room's `period_end_ms`.
pub fn admit(
    ruling: &Ruling,
    room: Option<&Room>,
    estimate: Micro,
    running: &[Micro],
) -> Admission {
    if estimate == 0 {
        return Admission::Admit;
    }
    if *ruling == Ruling::None {
        return Admission::Refuse(AdmitCause::Ruling);
    }
    let Some(room) = room else {
        return Admission::Refuse(AdmitCause::NoCap);
    };
    match fits(room, estimate, running) {
        Fit::Overflow => Admission::Refuse(AdmitCause::Overflow),
        Fit::Fits => Admission::Admit,
        Fit::Over => Admission::Wait {
            until_ms: room.period_end_ms,
        },
    }
}

/// The estimate of a task of a category: 0 for every category until the ruling allows a hosted
/// tier, because no row of the policy picks one.
pub fn estimate(_policy: &Policy, _category: CategoryId) -> Micro {
    0
}

/// The state of the cap that the person sees.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CapState {
    NoCap,
    Reached,
    Near,
    Ok,
}

impl CapState {
    pub const ALL: [CapState; 4] = [
        CapState::NoCap,
        CapState::Reached,
        CapState::Near,
        CapState::Ok,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            CapState::NoCap => "no_cap",
            CapState::Reached => "reached",
            CapState::Near => "near",
            CapState::Ok => "ok",
        }
    }
}

/// `no_cap` for no room; `reached` when `refused_for_room` is true or the room left is 0; `near`
/// when five times the room left is at or under the cap in micro-units; else `ok`.
/// `refused_for_room` is true after the first refusal for lack of room: an admission that answered
/// `Wait`, or a gateway answer `over-cap`.
pub fn cap_state(room: Option<&Room>, refused_for_room: bool) -> CapState {
    let Some(room) = room else {
        return CapState::NoCap;
    };
    let left = room.left_wide();
    if refused_for_room || left == 0 {
        CapState::Reached
    } else if left * 5 <= room.cap_wide() {
        CapState::Near
    } else {
        CapState::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release;
    use crate::spend::tests::Draw;
    use crate::spend::{step, Hold, Move, State};
    use crate::tier::Tier;

    const END: u64 = 1_767_225_600_000;

    const ROOM: Room = Room {
        cap_minor: 10,
        spent: 20_000,
        held: 30_000,
        period_end_ms: END,
    };

    fn open() -> Ruling {
        Ruling::test_hosted(&["127.0.0.1:1"])
    }

    #[test]
    fn refusal_admit_ruling() {
        for room in [Some(&ROOM), None] {
            for running in [&[][..], &[1], &[60_000]] {
                for estimate in [1, 4_000, 50_000, 50_001, u64::MAX] {
                    assert_eq!(
                        admit(&release::ruling(), room, estimate, running),
                        Admission::Refuse(AdmitCause::Ruling),
                        "{room:?} {running:?} {estimate}"
                    );
                }
            }
        }
        assert_eq!(admit(&open(), Some(&ROOM), 1, &[]), Admission::Admit);
    }

    #[test]
    fn refusal_admit_no_cap() {
        for estimate in [1, 50_001, u64::MAX] {
            for running in [&[][..], &[u64::MAX]] {
                assert_eq!(
                    admit(&open(), None, estimate, running),
                    Admission::Refuse(AdmitCause::NoCap),
                    "{estimate} {running:?}"
                );
            }
        }
    }

    #[test]
    fn refusal_admit_over_room() {
        assert_eq!(ROOM.left(), 50_000);
        assert_eq!(
            admit(&open(), Some(&ROOM), 50_001, &[]),
            Admission::Wait { until_ms: END }
        );
        assert_eq!(admit(&open(), Some(&ROOM), 50_000, &[]), Admission::Admit);
        let full = Room {
            spent: 70_000,
            period_end_ms: 7,
            ..ROOM
        };
        assert_eq!(
            admit(&open(), Some(&full), 1, &[]),
            Admission::Wait { until_ms: 7 }
        );
        let lowered = Room {
            cap_minor: 3,
            period_end_ms: 9,
            ..ROOM
        };
        assert_eq!(
            admit(&open(), Some(&lowered), 1, &[]),
            Admission::Wait { until_ms: 9 }
        );
    }

    #[test]
    fn refusal_admit_sum() {
        let first = 30_000;
        let second = 20_001;
        assert_eq!(admit(&open(), Some(&ROOM), first, &[]), Admission::Admit);
        assert_eq!(admit(&open(), Some(&ROOM), second, &[]), Admission::Admit);
        assert_eq!(
            admit(&open(), Some(&ROOM), second, &[first]),
            Admission::Wait { until_ms: END }
        );
        assert_eq!(
            admit(&open(), Some(&ROOM), 1, &[first, 20_000]),
            Admission::Wait { until_ms: END }
        );
        assert_eq!(
            admit(&open(), Some(&ROOM), 20_000, &[first]),
            Admission::Admit
        );
        assert_eq!(
            admit(&open(), Some(&ROOM), 1, &[first, 19_999]),
            Admission::Admit
        );
    }

    #[test]
    fn refusal_admit_overflow() {
        let huge = Room {
            cap_minor: u64::MAX,
            spent: 0,
            held: 0,
            period_end_ms: END,
        };
        for (estimate, running) in [
            (u64::MAX, &[1][..]),
            (1, &[u64::MAX][..]),
            (u64::MAX, &[u64::MAX, u64::MAX][..]),
            (2, &[u64::MAX - 1][..]),
            (u64::MAX / 2, &[u64::MAX / 2, 2][..]),
        ] {
            for room in [&ROOM, &huge] {
                assert_eq!(
                    admit(&open(), Some(room), estimate, running),
                    Admission::Refuse(AdmitCause::Overflow),
                    "{estimate} {running:?}"
                );
            }
        }
        assert_eq!(admit(&open(), Some(&huge), u64::MAX, &[]), Admission::Admit);
        assert_eq!(
            admit(&open(), Some(&huge), 1, &[u64::MAX - 1]),
            Admission::Admit
        );
    }

    #[test]
    fn admit_estimate_zero_comes_first() {
        for ruling in [release::ruling(), open()] {
            for room in [Some(&ROOM), None] {
                for running in [&[][..], &[u64::MAX, u64::MAX], &[60_000]] {
                    assert_eq!(
                        admit(&ruling, room, 0, running),
                        Admission::Admit,
                        "{ruling:?} {room:?} {running:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn room_left_and_of() {
        assert_eq!(ROOM.left(), 50_000);
        let used_above_u64 = Room {
            cap_minor: 3_689_348_814_741_911,
            spent: u64::MAX,
            held: u64::MAX,
            period_end_ms: END,
        };
        assert_eq!(used_above_u64.left(), 6_770);
        assert_eq!(fits(&used_above_u64, 6_770, &[]), Fit::Fits);
        assert_eq!(fits(&used_above_u64, 6_771, &[]), Fit::Over);
        assert_eq!(
            admit(&open(), Some(&used_above_u64), 6_771, &[]),
            Admission::Wait { until_ms: END }
        );
        let cap_above_u64 = Room {
            cap_minor: u64::MAX,
            spent: u64::MAX,
            held: 0,
            period_end_ms: END,
        };
        assert_eq!(cap_above_u64.left(), u64::MAX);
        assert_eq!(
            admit(&open(), Some(&cap_above_u64), u64::MAX, &[]),
            Admission::Admit
        );
        assert_eq!(
            Room {
                spent: 100_000,
                held: 0,
                ..ROOM
            }
            .left(),
            0
        );
        assert_eq!(
            Room {
                spent: 99_999,
                held: 0,
                ..ROOM
            }
            .left(),
            1
        );
        assert_eq!(
            Room {
                spent: 60_000,
                held: 60_000,
                ..ROOM
            }
            .left(),
            0
        );
        assert_eq!(
            Room {
                spent: u64::MAX,
                held: u64::MAX,
                ..ROOM
            }
            .left(),
            0
        );
        assert_eq!(
            Room {
                cap_minor: 0,
                spent: 0,
                held: 0,
                ..ROOM
            }
            .left(),
            0
        );
        assert_eq!(
            Room {
                cap_minor: u64::MAX,
                spent: 0,
                held: 0,
                ..ROOM
            }
            .left(),
            u64::MAX
        );
        assert_eq!(
            Room {
                cap_minor: 1_844_674_407_370_955,
                spent: 0,
                held: 0,
                ..ROOM
            }
            .left(),
            18_446_744_073_709_550_000
        );
        let account = Account {
            cap_minor: Some(10),
            active: false,
            spent: 20_000,
            held: 30_000,
        };
        assert_eq!(Room::of(&account, END), Some(ROOM));
        assert_eq!(
            Room::of(
                &Account {
                    cap_minor: Some(0),
                    ..account
                },
                3
            ),
            Some(Room {
                cap_minor: 0,
                period_end_ms: 3,
                ..ROOM
            })
        );
        assert_eq!(
            Room::of(
                &Account {
                    cap_minor: None,
                    ..account
                },
                END
            ),
            None
        );
    }

    #[test]
    fn fits_is_the_arithmetic() {
        assert_eq!(fits(&ROOM, 50_000, &[]), Fit::Fits);
        assert_eq!(fits(&ROOM, 50_001, &[]), Fit::Over);
        assert_eq!(fits(&ROOM, 0, &[50_000]), Fit::Fits);
        assert_eq!(fits(&ROOM, 0, &[50_001]), Fit::Over);
        assert_eq!(fits(&ROOM, 10_000, &[20_000, 20_000]), Fit::Fits);
        assert_eq!(fits(&ROOM, 10_000, &[20_000, 20_001]), Fit::Over);
        assert_eq!(fits(&ROOM, 0, &[]), Fit::Fits);
        let empty = Room {
            spent: 70_000,
            ..ROOM
        };
        assert_eq!(empty.left(), 0);
        assert_eq!(fits(&empty, 0, &[]), Fit::Fits);
        assert_eq!(fits(&empty, 1, &[]), Fit::Over);
        assert_eq!(fits(&ROOM, u64::MAX, &[1]), Fit::Overflow);
        assert_eq!(fits(&ROOM, u64::MAX, &[]), Fit::Over);
        assert_eq!(fits(&ROOM, u64::MAX, &[0, 0]), Fit::Over);
    }

    #[test]
    fn cap_states() {
        assert_eq!(cap_state(None, false), CapState::NoCap);
        assert_eq!(cap_state(None, true), CapState::NoCap);
        assert_eq!(cap_state(Some(&ROOM), false), CapState::Ok);
        assert_eq!(cap_state(Some(&ROOM), true), CapState::Reached);
        let fifth = Room {
            spent: 80_000,
            held: 0,
            ..ROOM
        };
        assert_eq!(fifth.left(), 20_000);
        assert_eq!(cap_state(Some(&fifth), false), CapState::Near);
        assert_eq!(cap_state(Some(&fifth), true), CapState::Reached);
        assert_eq!(
            cap_state(
                Some(&Room {
                    spent: 79_999,
                    ..fifth
                }),
                false
            ),
            CapState::Ok
        );
        assert_eq!(
            cap_state(
                Some(&Room {
                    spent: 99_999,
                    ..fifth
                }),
                false
            ),
            CapState::Near
        );
        assert_eq!(
            cap_state(
                Some(&Room {
                    spent: 100_000,
                    ..fifth
                }),
                false
            ),
            CapState::Reached
        );
        assert_eq!(
            cap_state(
                Some(&Room {
                    spent: 150_000,
                    ..fifth
                }),
                false
            ),
            CapState::Reached
        );
        assert_eq!(
            cap_state(
                Some(&Room {
                    cap_minor: 0,
                    spent: 0,
                    held: 0,
                    ..ROOM
                }),
                false
            ),
            CapState::Reached
        );
        let wide = Room {
            cap_minor: u64::MAX,
            spent: 0,
            held: 0,
            ..ROOM
        };
        assert_eq!(cap_state(Some(&wide), false), CapState::Ok);
        let wide_ok = Room {
            spent: u64::MAX,
            held: u64::MAX,
            cap_minor: 4_611_686_018_427_388,
            ..ROOM
        };
        assert_eq!(cap_state(Some(&wide_ok), false), CapState::Ok);
        let wide_near = Room {
            cap_minor: 4_611_686_018_427_387,
            ..wide_ok
        };
        assert_eq!(cap_state(Some(&wide_near), false), CapState::Near);
    }

    #[test]
    fn estimate_is_zero_for_every_category() {
        let base = Policy::hand_back_all().to_text();
        let hosted_text = base.replacen(
            "utility_bill_payment\tcompile\thand_back\t-\tuntested\t0\t0\n",
            "utility_bill_payment\tcompile\tfrontier\tlow\tline_met\t4\t4\n",
            1,
        );
        assert_ne!(hosted_text, base);
        let hosted = Policy::parse_for(&hosted_text, &Tier::ALL).expect("a hosted table");
        for policy in [release::policy(), &Policy::hand_back_all(), &hosted] {
            for category in CategoryId::ALL {
                assert_eq!(estimate(policy, category), 0, "{category:?}");
            }
        }
    }

    #[test]
    fn admit_closed_lists() {
        let words = |list: &[&str]| list.join(",");
        assert_eq!(
            words(&AdmitCause::ALL.map(AdmitCause::as_str)),
            "ruling,no_cap,overflow"
        );
        assert_eq!(
            words(&CapState::ALL.map(CapState::as_str)),
            "no_cap,reached,near,ok"
        );
    }

    #[test]
    fn fits_beside_an_open_hold() {
        let ruling = open();
        let mut account = Account {
            cap_minor: Some(10),
            active: true,
            spent: 20_000,
            held: 0,
        };
        let read = Room::of(&account, END).expect("a cap");
        assert_eq!(admit(&ruling, Some(&read), 30_000, &[]), Admission::Admit);
        let (next, held) =
            step(&account, &Hold::new(), Move::Reserve { amount: 25_000 }).expect("reserve");
        let (next, sent) = step(&next, &held, Move::Dispatch).expect("dispatch");
        account = next;
        let running: [Micro; 0] = [];
        let read = Room::of(&account, END).expect("a cap");
        assert_eq!(read.left(), 55_000);
        assert_eq!(
            admit(&ruling, Some(&read), 55_000, &running),
            Admission::Admit
        );
        assert_eq!(
            admit(&ruling, Some(&read), 55_001, &running),
            Admission::Wait { until_ms: END }
        );
        assert_eq!(
            admit(&ruling, Some(&read), 55_000, &[30_000]),
            Admission::Wait { until_ms: END }
        );
        let (expired, _) = step(&account, &sent, Move::Expire).expect("expire");
        assert_eq!(Room::of(&expired, END).expect("a cap").left(), 55_000);
        let (full, _) =
            step(&expired, &Hold::new(), Move::Reserve { amount: 55_000 }).expect("reserve");
        assert_eq!(full.spent + full.held, 100_000);
        assert_eq!(
            step(&full, &Hold::new(), Move::Reserve { amount: 1 }).map(|_| ()),
            Err(crate::spend::SpendCause::OverCap)
        );
    }

    struct Task {
        estimate: Micro,
        hold: Hold,
        requests: u32,
    }

    #[test]
    fn drawn_admissions_keep_the_model_invariants() {
        let ruling = open();
        let mut draw = Draw::new(2);
        let mut answers = [0u32; 4];
        for sequence in 0..1_000 {
            let mut account = Account {
                cap_minor: Some(draw.pick(&[1, 3, 10, 40])),
                active: true,
                spent: draw.pick(&[0, 5_000, 20_000]),
                held: 0,
            };
            let mut tasks: Vec<Task> = Vec::new();
            let mut orphans: Vec<Hold> = Vec::new();
            for at in 0..(20 + draw.below(60)) {
                let where_ = format!("sequence {sequence} move {at}");
                match draw.below(10) {
                    0..=2 => {
                        let read = Room::of(&account, END);
                        let left = read.map_or(0, |room| room.left());
                        let any = 1 + draw.below(40_000);
                        let estimate = draw.pick(&[0, 1, left, left + 1, left / 2, any, u64::MAX]);
                        let running: Vec<Micro> = tasks.iter().map(|task| task.estimate).collect();
                        let answer = admit(&ruling, read.as_ref(), estimate, &running);
                        let sum: u128 = running.iter().map(|each| *each as u128).sum::<u128>()
                            + estimate as u128;
                        let want = match read {
                            _ if estimate == 0 => Admission::Admit,
                            None => Admission::Refuse(AdmitCause::NoCap),
                            Some(_) if sum > u64::MAX as u128 => {
                                Admission::Refuse(AdmitCause::Overflow)
                            }
                            Some(room) if sum <= room.left() as u128 => Admission::Admit,
                            Some(room) => Admission::Wait {
                                until_ms: room.period_end_ms,
                            },
                        };
                        assert_eq!(answer, want, "{where_}: {read:?} {estimate} {running:?}");
                        answers[match answer {
                            Admission::Admit => 0,
                            Admission::Wait { .. } => 1,
                            Admission::Refuse(AdmitCause::Overflow) => 2,
                            Admission::Refuse(_) => 3,
                        }] += 1;
                        if answer == Admission::Admit {
                            if let Some(room) = read {
                                assert!(
                                    sum <= room.left() as u128 || estimate == 0,
                                    "{where_}: S6"
                                );
                            }
                            tasks.push(Task {
                                estimate,
                                hold: Hold::new(),
                                requests: 0,
                            });
                        }
                    }
                    3..=6 if !tasks.is_empty() => {
                        let i = draw.below(tasks.len() as u64) as usize;
                        let task = &mut tasks[i];
                        let mv = match task.hold.state {
                            State::New => Move::Reserve {
                                amount: 1 + draw.below(task.estimate.clamp(1, 30_000)),
                            },
                            State::Held => draw.pick(&[Move::Dispatch, Move::Void]),
                            State::Sent => {
                                let cost = draw.below(task.hold.amount + 1);
                                draw.pick(&[Move::Settle { cost }, Move::Expire])
                            }
                            _ => {
                                task.hold = Hold::new();
                                task.requests += 1;
                                continue;
                            }
                        };
                        if let Ok((next_account, next_hold)) = step(&account, &task.hold, mv) {
                            if let Move::Reserve { .. } = mv {
                                let cap = account.cap_minor.expect("a cap") as u128 * 10_000;
                                assert!(
                                    next_account.spent as u128 + next_account.held as u128 <= cap,
                                    "{where_}: S4"
                                );
                            }
                            account = next_account;
                            task.hold = next_hold;
                        }
                        if task.requests >= 3 {
                            let ended = tasks.swap_remove(i);
                            orphans.push(ended.hold);
                        }
                    }
                    7 if !tasks.is_empty() => {
                        let ended = tasks.swap_remove(draw.below(tasks.len() as u64) as usize);
                        orphans.push(ended.hold);
                    }
                    8 if !orphans.is_empty() => {
                        let i = draw.below(orphans.len() as u64) as usize;
                        if let Ok((next, hold)) = step(&account, &orphans[i], Move::Expire) {
                            account = next;
                            orphans[i] = hold;
                        }
                    }
                    9 => {
                        account.cap_minor =
                            draw.pick(&[None, Some(0), Some(1), Some(3), Some(10), Some(40)]);
                    }
                    _ => {}
                }
                let open_holds: u128 = tasks
                    .iter()
                    .map(|task| task.hold)
                    .chain(orphans.iter().copied())
                    .filter(|hold| matches!(hold.state, State::Held | State::Sent))
                    .map(|hold| hold.amount as u128)
                    .sum();
                assert_eq!(
                    account.held as u128, open_holds,
                    "{where_}: the room read holds every open hold"
                );
            }
        }
        assert!(answers.iter().all(|count| *count > 100), "{answers:?}");
    }
}
