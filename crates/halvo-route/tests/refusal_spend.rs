use halvo_route::leave::{grant, Consent, LeaveCause, PartKind};
use halvo_route::prices::Micro;
use halvo_route::release;
use halvo_route::spend::{step, Account, Hold, Move, SpendCause, State};
use halvo_route::tier::Tier;

const ROOM: Account = Account {
    cap_minor: Some(10),
    active: true,
    spent: 20_000,
    held: 30_000,
};

const LEFT: Micro = 50_000;

fn hold(state: State, amount: Micro, cost: Micro) -> Hold {
    Hold {
        state,
        amount,
        cost,
    }
}

fn refused(account: &Account, hold: &Hold, mv: Move) -> SpendCause {
    match step(account, hold, mv) {
        Err(cause) => cause,
        Ok(next) => panic!("{mv:?} on {hold:?} went through: {next:?}"),
    }
}

fn reserve_of(account: Account, amount: Micro) -> (Account, Hold) {
    (
        Account {
            held: account.held + amount,
            ..account
        },
        hold(State::Held, amount, 0),
    )
}

#[test]
fn refusal_no_hold() {
    let new = Hold::new();
    let accounts = [
        ROOM,
        Account {
            active: false,
            ..ROOM
        },
        Account {
            cap_minor: None,
            ..ROOM
        },
        Account {
            cap_minor: Some(0),
            ..ROOM
        },
    ];
    for account in &accounts {
        for mv in [
            Move::Dispatch,
            Move::Settle { cost: 0 },
            Move::Settle { cost: 4_000 },
            Move::Void,
            Move::Expire,
        ] {
            assert_eq!(
                refused(account, &new, mv),
                SpendCause::NoHold,
                "{account:?} {mv:?}"
            );
        }
    }
    assert_eq!(
        step(&ROOM, &new, Move::Reserve { amount: 4_000 }),
        Ok(reserve_of(ROOM, 4_000))
    );
}

#[test]
fn refusal_over_room() {
    let new = Hold::new();
    assert_eq!(
        refused(&ROOM, &new, Move::Reserve { amount: LEFT + 1 }),
        SpendCause::OverCap
    );
    for empty in [
        Account {
            cap_minor: Some(5),
            active: true,
            spent: 30_000,
            held: 20_000,
        },
        Account {
            cap_minor: Some(0),
            active: true,
            spent: 0,
            held: 0,
        },
        Account {
            cap_minor: Some(2),
            active: true,
            spent: 30_000,
            held: 0,
        },
        Account {
            cap_minor: Some(2),
            active: true,
            spent: 0,
            held: 20_000,
        },
    ] {
        assert_eq!(
            refused(&empty, &new, Move::Reserve { amount: 1 }),
            SpendCause::OverCap,
            "{empty:?}"
        );
    }
    assert_eq!(
        step(&ROOM, &new, Move::Reserve { amount: LEFT }),
        Ok(reserve_of(ROOM, LEFT))
    );
}

#[test]
fn refusal_second_dispatch() {
    let sent = hold(State::Sent, 4_000, 0);
    assert_eq!(refused(&ROOM, &sent, Move::Dispatch), SpendCause::Used);
    assert_eq!(
        refused(&ROOM, &sent, Move::Reserve { amount: 4_000 }),
        SpendCause::Repeat
    );
    assert_eq!(
        refused(&ROOM, &sent, Move::Reserve { amount: 4_001 }),
        SpendCause::OtherArguments
    );
    assert_eq!(refused(&ROOM, &sent, Move::Dispatch), SpendCause::Used);
    assert_eq!(
        step(&ROOM, &sent, Move::Settle { cost: 3_500 }),
        Ok((
            Account {
                spent: 23_500,
                held: 26_000,
                ..ROOM
            },
            hold(State::Settled, 4_000, 3_500)
        ))
    );
}

#[test]
fn refusal_ruling_none() {
    let ruling = release::ruling();
    for tier in [Tier::LowCostHosted, Tier::Frontier] {
        let answer = grant(
            &ruling,
            tier,
            Tier::Frontier,
            &[PartKind::Text],
            "127.0.0.1:1",
            Consent::Recorded,
        );
        assert_eq!(answer.err(), Some(LeaveCause::Ruling), "{tier:?}");
    }
}

#[test]
fn refusal_no_cap() {
    for account in [
        Account {
            cap_minor: None,
            active: true,
            spent: 0,
            held: 0,
        },
        Account {
            cap_minor: None,
            ..ROOM
        },
    ] {
        for amount in [1, 4_000, u64::MAX] {
            assert_eq!(
                refused(&account, &Hold::new(), Move::Reserve { amount }),
                SpendCause::NoCap,
                "{account:?} {amount}"
            );
        }
    }
}

#[test]
fn refusal_not_active() {
    for cap_minor in [Some(10), None, Some(0)] {
        let account = Account {
            cap_minor,
            active: false,
            ..ROOM
        };
        for amount in [1, 4_000, LEFT + 1] {
            assert_eq!(
                refused(&account, &Hold::new(), Move::Reserve { amount }),
                SpendCause::NotActive,
                "{cap_minor:?} {amount}"
            );
        }
    }
}

#[test]
fn refusal_zero_amount() {
    for account in [
        ROOM,
        Account {
            active: false,
            ..ROOM
        },
        Account {
            cap_minor: None,
            ..ROOM
        },
        Account {
            cap_minor: Some(5),
            ..ROOM
        },
        Account {
            cap_minor: None,
            active: false,
            ..ROOM
        },
    ] {
        assert_eq!(
            refused(&account, &Hold::new(), Move::Reserve { amount: 0 }),
            SpendCause::State,
            "{account:?}"
        );
    }
}

#[test]
fn refusal_final_state() {
    for last in [
        hold(State::Settled, 4_000, 3_500),
        hold(State::Expired, 4_000, 4_000),
        hold(State::Voided, 4_000, 0),
    ] {
        for (mv, cause) in [
            (Move::Reserve { amount: 4_000 }, SpendCause::Repeat),
            (Move::Reserve { amount: 4_001 }, SpendCause::OtherArguments),
            (Move::Reserve { amount: 0 }, SpendCause::OtherArguments),
            (Move::Dispatch, SpendCause::Used),
            (Move::Void, SpendCause::State),
            (Move::Settle { cost: 3_500 }, SpendCause::State),
            (Move::Expire, SpendCause::State),
        ] {
            assert_eq!(refused(&ROOM, &last, mv), cause, "{last:?} {mv:?}");
        }
    }
}
