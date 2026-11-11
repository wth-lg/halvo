//! The machine of one hold, contract T7.2: the ledger's moves on one hold of one account, as one
//! pure function, `step`, with no clock, no I/O and no state of its own.

use crate::prices::{Micro, MICRO_PER_MINOR};

/// The state of a hold. `settled`, `expired` and `voided` are final.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum State {
    New,
    Held,
    Sent,
    Settled,
    Expired,
    Voided,
}

impl State {
    pub const ALL: [State; 6] = [
        State::New,
        State::Held,
        State::Sent,
        State::Settled,
        State::Expired,
        State::Voided,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            State::New => "new",
            State::Held => "held",
            State::Sent => "sent",
            State::Settled => "settled",
            State::Expired => "expired",
            State::Voided => "voided",
        }
    }
}

/// What the ledger's cap read shows of an account: the cap in minor units, none when the account
/// has none; whether it is active; `spent`, what is counted in the cap period, reported and
/// unreported; and `held`, the sum of its open holds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Account {
    pub cap_minor: Option<u64>,
    pub active: bool,
    pub spent: Micro,
    pub held: Micro,
}

/// One hold: its state, the amount that it holds and the cost that a settle or an expiry stored.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Hold {
    pub state: State,
    pub amount: Micro,
    pub cost: Micro,
}

impl Hold {
    /// A hold that is `new`, with the amount 0 and the cost 0.
    pub const fn new() -> Hold {
        Hold {
            state: State::New,
            amount: 0,
            cost: 0,
        }
    }

    /// What `cost` is above `amount`, or 0.
    pub const fn overrun(&self) -> Micro {
        self.cost.saturating_sub(self.amount)
    }
}

impl Default for Hold {
    fn default() -> Hold {
        Hold::new()
    }
}

/// A move of the ledger on one hold.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Move {
    Reserve { amount: Micro },
    Dispatch,
    Void,
    Settle { cost: Micro },
    Expire,
}

impl Move {
    /// The word of the move: `reserve`, `dispatch`, `void`, `settle` or `expire`.
    pub const fn name(&self) -> &'static str {
        match self {
            Move::Reserve { .. } => "reserve",
            Move::Dispatch => "dispatch",
            Move::Void => "void",
            Move::Settle { .. } => "settle",
            Move::Expire => "expire",
        }
    }
}

/// Why `step` refuses a move.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum SpendCause {
    NotActive,
    NoCap,
    OverCap,
    NoHold,
    Used,
    Repeat,
    OtherArguments,
    State,
}

impl SpendCause {
    pub const ALL: [SpendCause; 8] = [
        SpendCause::NotActive,
        SpendCause::NoCap,
        SpendCause::OverCap,
        SpendCause::NoHold,
        SpendCause::Used,
        SpendCause::Repeat,
        SpendCause::OtherArguments,
        SpendCause::State,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            SpendCause::NotActive => "not_active",
            SpendCause::NoCap => "no_cap",
            SpendCause::OverCap => "over_cap",
            SpendCause::NoHold => "no_hold",
            SpendCause::Used => "used",
            SpendCause::Repeat => "repeat",
            SpendCause::OtherArguments => "other_arguments",
            SpendCause::State => "state",
        }
    }
}

/// The next account and hold after one move, or the cause of its refusal, which changes nothing.
///
/// The moves that go through: `Reserve` of a `new` hold makes it `held`, raises `held` by the
/// amount and sets the hold's amount; `Dispatch` of a `held` hold makes it `sent`; `Void` of a
/// `held` hold makes it `voided` and lowers `held` by its amount; `Settle` of a `sent` hold makes
/// it `settled`, lowers `held` by its amount, raises `spent` by the cost and stores the cost, which
/// may be above the amount (`Hold::overrun`); `Expire` of a `held` or `sent` hold makes it
/// `expired`, lowers `held` by its amount, raises `spent` by its amount and stores that amount as
/// its cost.
///
/// `Reserve` of a `new` hold refuses in this order: the amount 0 is `state`; an account that is not
/// active is `not_active`; one with no cap is `no_cap`; `spent + held + amount` above the cap times
/// `MICRO_PER_MINOR` is `over_cap`. `Reserve` of any other hold is `repeat` when its amount equals
/// the hold's and `other_arguments` when it differs. Any other move of a `new` hold is `no_hold`;
/// `Dispatch` of a hold that is `sent` or final is `used`; every other pair is `state`, and so is a
/// move whose result does not fit a `u64` or would lower `held` below 0.
pub fn step(account: &Account, hold: &Hold, mv: Move) -> Result<(Account, Hold), SpendCause> {
    match (hold.state, mv) {
        (State::New, Move::Reserve { amount }) => {
            let held = reserve(account, amount)?;
            Ok((
                Account { held, ..*account },
                Hold {
                    state: State::Held,
                    amount,
                    ..*hold
                },
            ))
        }
        (_, Move::Reserve { amount }) if amount == hold.amount => Err(SpendCause::Repeat),
        (_, Move::Reserve { .. }) => Err(SpendCause::OtherArguments),
        (State::New, _) => Err(SpendCause::NoHold),
        (State::Held, Move::Dispatch) => Ok((
            *account,
            Hold {
                state: State::Sent,
                ..*hold
            },
        )),
        (_, Move::Dispatch) => Err(SpendCause::Used),
        (State::Held, Move::Void) => end(account, hold, State::Voided, 0, hold.cost),
        (State::Sent, Move::Settle { cost }) => end(account, hold, State::Settled, cost, cost),
        (State::Held | State::Sent, Move::Expire) => {
            end(account, hold, State::Expired, hold.amount, hold.amount)
        }
        _ => Err(SpendCause::State),
    }
}

fn reserve(account: &Account, amount: Micro) -> Result<Micro, SpendCause> {
    if amount == 0 {
        return Err(SpendCause::State);
    }
    if !account.active {
        return Err(SpendCause::NotActive);
    }
    let Some(cap_minor) = account.cap_minor else {
        return Err(SpendCause::NoCap);
    };
    let after = u128::from(account.spent) + u128::from(account.held) + u128::from(amount);
    if after > u128::from(cap_minor) * u128::from(MICRO_PER_MINOR) {
        return Err(SpendCause::OverCap);
    }
    fit(u128::from(account.held) + u128::from(amount))
}

fn end(
    account: &Account,
    hold: &Hold,
    state: State,
    counted: Micro,
    cost: Micro,
) -> Result<(Account, Hold), SpendCause> {
    let held = account
        .held
        .checked_sub(hold.amount)
        .ok_or(SpendCause::State)?;
    let spent = fit(u128::from(account.spent) + u128::from(counted))?;
    Ok((
        Account {
            spent,
            held,
            ..*account
        },
        Hold {
            state,
            cost,
            ..*hold
        },
    ))
}

fn fit(sum: u128) -> Result<Micro, SpendCause> {
    Micro::try_from(sum).map_err(|_| SpendCause::State)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) struct Draw(u64);

    impl Draw {
        pub(crate) fn new(seed: u64) -> Draw {
            Draw(seed)
        }

        pub(crate) fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        pub(crate) fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }

        pub(crate) fn pick<T: Copy>(&mut self, items: &[T]) -> T {
            items[self.below(items.len() as u64) as usize]
        }
    }

    const ACCOUNT: Account = Account {
        cap_minor: Some(10),
        active: true,
        spent: 20_000,
        held: 30_000,
    };

    fn hold(state: State, amount: Micro, cost: Micro) -> Hold {
        Hold {
            state,
            amount,
            cost,
        }
    }

    fn moves(amount: Micro, cost: Micro) -> [Move; 5] {
        [
            Move::Reserve { amount },
            Move::Dispatch,
            Move::Void,
            Move::Settle { cost },
            Move::Expire,
        ]
    }

    fn walk(
        account: Account,
        hold: Hold,
        path: &[(Move, Result<State, SpendCause>)],
    ) -> (Account, Hold) {
        let mut now = (account, hold);
        for (at, (mv, want)) in path.iter().enumerate() {
            match (step(&now.0, &now.1, *mv), want) {
                (Ok(next), Ok(state)) => {
                    assert_eq!(next.1.state, *state, "move {at} {mv:?}");
                    now = next;
                }
                (got, want) => assert_eq!(got.map(|next| next.1.state), *want, "move {at} {mv:?}"),
            }
        }
        now
    }

    #[test]
    fn spend_closed_lists() {
        let words = |list: &[&str]| list.join(",");
        assert_eq!(
            words(&State::ALL.map(State::as_str)),
            "new,held,sent,settled,expired,voided"
        );
        assert_eq!(
            words(&SpendCause::ALL.map(SpendCause::as_str)),
            "not_active,no_cap,over_cap,no_hold,used,repeat,other_arguments,state"
        );
        assert_eq!(
            words(&moves(1, 1).map(|mv| mv.name())),
            "reserve,dispatch,void,settle,expire"
        );
        assert_eq!(Hold::new(), hold(State::New, 0, 0));
        assert_eq!(Hold::default(), Hold::new());
    }

    #[test]
    fn overrun_is_cost_above_amount() {
        assert_eq!(hold(State::Settled, 4_000, 4_001).overrun(), 1);
        assert_eq!(hold(State::Settled, 4_000, 9_000).overrun(), 5_000);
        assert_eq!(hold(State::Settled, 4_000, 4_000).overrun(), 0);
        assert_eq!(hold(State::Settled, 4_000, 3_999).overrun(), 0);
        assert_eq!(hold(State::Settled, 0, u64::MAX).overrun(), u64::MAX);
        assert_eq!(hold(State::Settled, u64::MAX, 0).overrun(), 0);
        assert_eq!(Hold::new().overrun(), 0);
    }

    #[test]
    fn step_answers_every_pair() {
        use SpendCause::{NoHold, OtherArguments, Repeat, Used};
        const NO: SpendCause = SpendCause::State;
        let a = ACCOUNT;
        let after = |spent: Micro, held: Micro| Account { spent, held, ..a };
        type Answer = Result<(Account, Hold), SpendCause>;
        let cases: [(State, Micro, [Answer; 5]); 6] = [
            (
                State::New,
                0,
                [
                    Ok((after(20_000, 37_000), hold(State::Held, 7_000, 0))),
                    Err(NoHold),
                    Err(NoHold),
                    Err(NoHold),
                    Err(NoHold),
                ],
            ),
            (
                State::Held,
                0,
                [
                    Err(OtherArguments),
                    Ok((a, hold(State::Sent, 4_000, 0))),
                    Ok((after(20_000, 26_000), hold(State::Voided, 4_000, 0))),
                    Err(NO),
                    Ok((after(24_000, 26_000), hold(State::Expired, 4_000, 4_000))),
                ],
            ),
            (
                State::Sent,
                0,
                [
                    Err(OtherArguments),
                    Err(Used),
                    Err(NO),
                    Ok((after(29_000, 26_000), hold(State::Settled, 4_000, 9_000))),
                    Ok((after(24_000, 26_000), hold(State::Expired, 4_000, 4_000))),
                ],
            ),
            (
                State::Settled,
                3_000,
                [Err(OtherArguments), Err(Used), Err(NO), Err(NO), Err(NO)],
            ),
            (
                State::Expired,
                4_000,
                [Err(OtherArguments), Err(Used), Err(NO), Err(NO), Err(NO)],
            ),
            (
                State::Voided,
                0,
                [Err(OtherArguments), Err(Used), Err(NO), Err(NO), Err(NO)],
            ),
        ];
        for (state, cost, answers) in cases {
            let amount = if state == State::New { 0 } else { 4_000 };
            let before = hold(state, amount, cost);
            for (mv, want) in moves(7_000, 9_000).into_iter().zip(answers) {
                assert_eq!(step(&a, &before, mv), want, "{state:?} {mv:?}");
            }
            if state != State::New {
                assert_eq!(
                    step(&a, &before, Move::Reserve { amount: 4_000 }),
                    Err(Repeat),
                    "{state:?}"
                );
                assert_eq!(
                    step(&a, &before, Move::Reserve { amount: 0 }),
                    Err(OtherArguments),
                    "{state:?}"
                );
            }
        }
    }

    #[test]
    fn second_send_with_another_body() {
        let (account, sent) = walk(
            ACCOUNT,
            Hold::new(),
            &[
                (Move::Reserve { amount: 5_000 }, Ok(State::Held)),
                (Move::Dispatch, Ok(State::Sent)),
                (
                    Move::Reserve { amount: 6_000 },
                    Err(SpendCause::OtherArguments),
                ),
                (
                    Move::Reserve { amount: 4_999 },
                    Err(SpendCause::OtherArguments),
                ),
                (Move::Reserve { amount: 0 }, Err(SpendCause::OtherArguments)),
                (Move::Reserve { amount: 5_000 }, Err(SpendCause::Repeat)),
                (Move::Dispatch, Err(SpendCause::Used)),
            ],
        );
        assert_eq!(
            account,
            Account {
                held: 35_000,
                ..ACCOUNT
            }
        );
        assert_eq!(sent, hold(State::Sent, 5_000, 0));
        assert_eq!(
            step(&account, &sent, Move::Settle { cost: 4_000 }),
            Ok((
                Account {
                    spent: 24_000,
                    ..ACCOUNT
                },
                hold(State::Settled, 5_000, 4_000)
            ))
        );
    }

    #[test]
    fn settle_after_an_expiry() {
        let (account, expired) = walk(
            ACCOUNT,
            Hold::new(),
            &[
                (Move::Reserve { amount: 5_000 }, Ok(State::Held)),
                (Move::Dispatch, Ok(State::Sent)),
                (Move::Expire, Ok(State::Expired)),
                (Move::Settle { cost: 1_000 }, Err(SpendCause::State)),
                (Move::Settle { cost: 5_000 }, Err(SpendCause::State)),
                (Move::Expire, Err(SpendCause::State)),
                (Move::Void, Err(SpendCause::State)),
                (Move::Dispatch, Err(SpendCause::Used)),
                (Move::Reserve { amount: 5_000 }, Err(SpendCause::Repeat)),
            ],
        );
        assert_eq!(
            account,
            Account {
                spent: 25_000,
                ..ACCOUNT
            }
        );
        assert_eq!(expired, hold(State::Expired, 5_000, 5_000));
        assert_eq!(expired.overrun(), 0);

        let (account, expired) = walk(
            ACCOUNT,
            Hold::new(),
            &[
                (Move::Reserve { amount: 5_000 }, Ok(State::Held)),
                (Move::Expire, Ok(State::Expired)),
                (Move::Settle { cost: 1_000 }, Err(SpendCause::State)),
            ],
        );
        assert_eq!(account.spent, 25_000);
        assert_eq!(account.held, 30_000);
        assert_eq!(expired.cost, 5_000);
    }

    #[test]
    fn reserve_at_exactly_the_room_left() {
        let left = 100_000 - 20_000 - 30_000;
        let (full, held) = walk(
            ACCOUNT,
            Hold::new(),
            &[(Move::Reserve { amount: left }, Ok(State::Held))],
        );
        assert_eq!(
            full,
            Account {
                held: 80_000,
                ..ACCOUNT
            }
        );
        assert_eq!(held.amount, left);
        assert_eq!(
            step(&full, &Hold::new(), Move::Reserve { amount: 1 }),
            Err(SpendCause::OverCap)
        );
        assert_eq!(
            step(&ACCOUNT, &Hold::new(), Move::Reserve { amount: left + 1 }),
            Err(SpendCause::OverCap)
        );
    }

    #[test]
    fn cap_set_below_what_is_spent() {
        let account = Account {
            cap_minor: Some(10),
            active: true,
            spent: 50_000,
            held: 20_000,
        };
        let (account, sent) = walk(
            account,
            Hold::new(),
            &[
                (Move::Reserve { amount: 10_000 }, Ok(State::Held)),
                (Move::Dispatch, Ok(State::Sent)),
            ],
        );
        let lowered = Account {
            cap_minor: Some(3),
            ..account
        };
        for amount in [1, 10_000] {
            assert_eq!(
                step(&lowered, &Hold::new(), Move::Reserve { amount }),
                Err(SpendCause::OverCap)
            );
        }
        let (settled, _) = step(&lowered, &sent, Move::Settle { cost: 15_000 }).expect("settle");
        assert_eq!(settled.spent, 65_000);
        assert_eq!(settled.held, 20_000);
        assert_eq!(
            step(&settled, &Hold::new(), Move::Reserve { amount: 1 }),
            Err(SpendCause::OverCap)
        );
        let other = hold(State::Held, 20_000, 0);
        assert_eq!(
            step(&settled, &other, Move::Expire),
            Ok((
                Account {
                    spent: 85_000,
                    held: 0,
                    ..settled
                },
                hold(State::Expired, 20_000, 20_000)
            ))
        );
    }

    #[test]
    fn settle_above_the_amount_is_an_overrun() {
        let sent = hold(State::Sent, 4_000, 0);
        let (account, settled) =
            step(&ACCOUNT, &sent, Move::Settle { cost: 6_500 }).expect("settle");
        assert_eq!(
            account,
            Account {
                spent: 26_500,
                held: 26_000,
                ..ACCOUNT
            }
        );
        assert_eq!(settled, hold(State::Settled, 4_000, 6_500));
        assert_eq!(settled.overrun(), 2_500);
        let (account, settled) = step(&ACCOUNT, &sent, Move::Settle { cost: 0 }).expect("settle");
        assert_eq!(
            account,
            Account {
                spent: 20_000,
                held: 26_000,
                ..ACCOUNT
            }
        );
        assert_eq!(settled.cost, 0);
    }

    #[test]
    fn held_below_zero_is_state() {
        let low = Account {
            held: 3_999,
            ..ACCOUNT
        };
        for (state, mv) in [
            (State::Held, Move::Void),
            (State::Sent, Move::Settle { cost: 1 }),
            (State::Held, Move::Expire),
            (State::Sent, Move::Expire),
        ] {
            let before = hold(state, 4_000, 0);
            assert_eq!(
                step(&low, &before, mv),
                Err(SpendCause::State),
                "{state:?} {mv:?}"
            );
            let exact = Account { held: 4_000, ..low };
            assert_eq!(
                step(&exact, &before, mv).map(|next| next.0.held),
                Ok(0),
                "{state:?} {mv:?}"
            );
        }
    }

    #[test]
    fn results_that_do_not_fit_are_state() {
        let full = Account {
            cap_minor: Some(u64::MAX),
            active: true,
            spent: u64::MAX - 1,
            held: 4_000,
        };
        let sent = hold(State::Sent, 4_000, 0);
        assert_eq!(
            step(&full, &sent, Move::Settle { cost: 4_001 }),
            Err(SpendCause::State)
        );
        assert_eq!(
            step(&full, &sent, Move::Settle { cost: 1 }).map(|next| next.0.spent),
            Ok(u64::MAX)
        );
        let top = Account {
            spent: u64::MAX - 4_000,
            ..full
        };
        assert_eq!(
            step(&top, &sent, Move::Expire).map(|next| next.0.spent),
            Ok(u64::MAX)
        );
        let over = Account {
            spent: u64::MAX - 3_999,
            ..full
        };
        assert_eq!(step(&over, &sent, Move::Expire), Err(SpendCause::State));

        let heavy = Account {
            cap_minor: Some(u64::MAX),
            active: true,
            spent: 0,
            held: u64::MAX - 5,
        };
        assert_eq!(
            step(&heavy, &Hold::new(), Move::Reserve { amount: 6 }),
            Err(SpendCause::State)
        );
        assert_eq!(
            step(&heavy, &Hold::new(), Move::Reserve { amount: 5 }).map(|next| next.0.held),
            Ok(u64::MAX)
        );
        let small = Account {
            cap_minor: Some(1),
            ..heavy
        };
        assert_eq!(
            step(&small, &Hold::new(), Move::Reserve { amount: 6 }),
            Err(SpendCause::OverCap)
        );
        let wide = Account {
            cap_minor: Some(u64::MAX),
            active: true,
            spent: u64::MAX,
            held: u64::MAX - 1,
        };
        assert_eq!(
            step(&wide, &Hold::new(), Move::Reserve { amount: u64::MAX }),
            Err(SpendCause::State)
        );
        assert_eq!(
            step(&wide, &Hold::new(), Move::Reserve { amount: 1 }).map(|next| next.0.held),
            Ok(u64::MAX)
        );
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Cell {
        Go,
        Again,
        No(SpendCause),
    }

    fn cell(state: State, mv: Move) -> Cell {
        use Cell::{Again, Go, No};
        use SpendCause::{NoHold, Used};
        const BAD: Cell = No(SpendCause::State);
        const TABLE: [[Cell; 6]; 5] = [
            [Go, Again, Again, Again, Again, Again],
            [No(NoHold), Go, No(Used), No(Used), No(Used), No(Used)],
            [No(NoHold), Go, BAD, BAD, BAD, BAD],
            [No(NoHold), BAD, Go, BAD, BAD, BAD],
            [No(NoHold), Go, Go, BAD, BAD, BAD],
        ];
        let row = match mv {
            Move::Reserve { .. } => 0,
            Move::Dispatch => 1,
            Move::Void => 2,
            Move::Settle { .. } => 3,
            Move::Expire => 4,
        };
        TABLE[row][State::ALL
            .iter()
            .position(|each| *each == state)
            .expect("state")]
    }

    fn oracle(a: &Account, h: &Hold, mv: Move) -> Result<(Account, Hold), SpendCause> {
        let wide = |value: Micro| value as u128;
        let narrow = |value: u128| Micro::try_from(value).map_err(|_| SpendCause::State);
        match cell(h.state, mv) {
            Cell::No(cause) => Err(cause),
            Cell::Again => match mv {
                Move::Reserve { amount } if amount == h.amount => Err(SpendCause::Repeat),
                _ => Err(SpendCause::OtherArguments),
            },
            Cell::Go => {
                let (state, amount, held_up, spend, cost) = match mv {
                    Move::Reserve { amount } => {
                        if amount == 0 {
                            return Err(SpendCause::State);
                        }
                        if !a.active {
                            return Err(SpendCause::NotActive);
                        }
                        let cap = a.cap_minor.ok_or(SpendCause::NoCap)?;
                        if wide(a.spent) + wide(a.held) + wide(amount) > wide(cap) * 10_000 {
                            return Err(SpendCause::OverCap);
                        }
                        (State::Held, amount, true, 0, h.cost)
                    }
                    Move::Dispatch => (State::Sent, h.amount, false, 0, h.cost),
                    Move::Void => (State::Voided, h.amount, false, 0, h.cost),
                    Move::Settle { cost } => (State::Settled, h.amount, false, cost, cost),
                    Move::Expire => (State::Expired, h.amount, false, h.amount, h.amount),
                };
                let held = if held_up {
                    narrow(wide(a.held) + wide(amount))?
                } else if matches!(mv, Move::Dispatch) {
                    a.held
                } else {
                    a.held.checked_sub(amount).ok_or(SpendCause::State)?
                };
                let spent = narrow(wide(a.spent) + wide(spend))?;
                Ok((
                    Account { spent, held, ..*a },
                    Hold {
                        state,
                        amount,
                        cost,
                    },
                ))
            }
        }
    }

    struct Book {
        account: Account,
        holds: Vec<Hold>,
        others: Micro,
        spent0: Micro,
        high: u128,
        cap_changed: bool,
        dispatched: Vec<u32>,
        reserved: Vec<u32>,
        ended: Vec<u32>,
        was_held: Vec<bool>,
    }

    impl Book {
        fn check(&self, at: &str) {
            let a = &self.account;
            let open: u128 = self
                .holds
                .iter()
                .filter(|h| matches!(h.state, State::Held | State::Sent))
                .map(|h| h.amount as u128)
                .sum();
            assert_eq!(a.held as u128, self.others as u128 + open, "{at}: held");
            let counted: u128 = self
                .holds
                .iter()
                .filter(|h| matches!(h.state, State::Settled | State::Expired))
                .map(|h| h.cost as u128)
                .sum();
            assert_eq!(
                a.spent as u128,
                self.spent0 as u128 + counted,
                "{at}: spent"
            );
            let overruns: u128 = self.holds.iter().map(|h| h.overrun() as u128).sum();
            let counted_now = a.spent as u128 + a.held as u128;
            assert!(
                counted_now <= self.high + overruns,
                "{at}: {counted_now} above {} + {overruns}",
                self.high
            );
            if let (false, Some(cap)) = (self.cap_changed, a.cap_minor) {
                let start = self.spent0 as u128 + self.others as u128;
                if start <= cap as u128 * 10_000 {
                    assert!(
                        counted_now <= cap as u128 * 10_000 + overruns,
                        "{at}: above the cap"
                    );
                }
            }
            for (i, h) in self.holds.iter().enumerate() {
                assert!(self.dispatched[i] <= 1, "{at}: hold {i} dispatched twice");
                assert!(self.reserved[i] <= 1, "{at}: hold {i} reserved twice");
                assert!(self.ended[i] <= 1, "{at}: hold {i} ended twice");
                if h.state == State::Sent {
                    assert!(self.was_held[i], "{at}: hold {i} sent before held");
                }
            }
        }
    }

    #[test]
    fn drawn_sequences_keep_the_model_invariants() {
        let mut draw = Draw::new(1);
        let mut pairs = std::collections::HashSet::new();
        let mut causes = std::collections::HashSet::new();
        let mut accepted = 0u32;
        for sequence in 0..3_000 {
            let cap_minor = draw.pick(&[None, Some(0), Some(1), Some(2), Some(5), Some(30)]);
            let spent0 = draw.pick(&[0, 1, 5_000, 10_000, 25_000]);
            let others = draw.pick(&[0, 1, 4_000, 10_000]);
            let mut book = Book {
                account: Account {
                    cap_minor,
                    active: draw.below(6) != 0,
                    spent: spent0,
                    held: others,
                },
                holds: Vec::new(),
                others,
                spent0,
                high: spent0 as u128 + others as u128,
                cap_changed: false,
                dispatched: Vec::new(),
                reserved: Vec::new(),
                ended: Vec::new(),
                was_held: Vec::new(),
            };
            book.check(&format!("sequence {sequence} start"));
            for at in 0..(10 + draw.below(30)) {
                let where_ = format!("sequence {sequence} move {at}");
                if draw.below(12) == 0 {
                    book.account.cap_minor = draw.pick(&[None, Some(0), Some(1), Some(3), Some(8)]);
                    book.cap_changed = true;
                    continue;
                }
                if book.holds.is_empty() || (book.holds.len() < 12 && draw.below(4) == 0) {
                    book.holds.push(Hold::new());
                    book.dispatched.push(0);
                    book.reserved.push(0);
                    book.ended.push(0);
                    book.was_held.push(false);
                }
                let i = draw.below(book.holds.len() as u64) as usize;
                let h = book.holds[i];
                let a = book.account;
                let left = match a.cap_minor {
                    Some(cap) => (cap as u128 * 10_000)
                        .saturating_sub(a.spent as u128 + a.held as u128)
                        as u64,
                    None => 7_000,
                };
                let any_amount = 1 + draw.below(9_999);
                let amount = draw.pick(&[0, 1, h.amount, h.amount + 1, left, left + 1, any_amount]);
                let any_cost = draw.below(20_000);
                let cost = draw.pick(&[
                    0,
                    h.amount,
                    h.amount + 1,
                    h.amount.saturating_sub(1),
                    any_cost,
                ]);
                let forward = match h.state {
                    State::New => vec![Move::Reserve { amount }],
                    State::Held => vec![Move::Dispatch, Move::Void, Move::Expire],
                    State::Sent => vec![Move::Settle { cost }, Move::Expire],
                    _ => moves(amount, cost).to_vec(),
                };
                let mv = if draw.below(2) == 0 {
                    draw.pick(&forward)
                } else {
                    draw.pick(&moves(amount, cost))
                };
                let got = step(&a, &h, mv);
                assert_eq!(got, oracle(&a, &h, mv), "{where_}: {a:?} {h:?} {mv:?}");
                pairs.insert((h.state, mv.name()));
                match got {
                    Err(cause) => {
                        causes.insert(cause);
                    }
                    Ok((next_a, next_h)) => {
                        accepted += 1;
                        assert_eq!(next_a.cap_minor, a.cap_minor, "{where_}");
                        assert_eq!(next_a.active, a.active, "{where_}");
                        match mv {
                            Move::Reserve { .. } => {
                                book.reserved[i] += 1;
                                let cap =
                                    a.cap_minor.expect("a reserve needs a cap") as u128 * 10_000;
                                let sum = next_a.spent as u128 + next_a.held as u128;
                                assert!(sum <= cap, "{where_}: S4");
                                book.high = book.high.max(sum);
                            }
                            Move::Dispatch => book.dispatched[i] += 1,
                            _ => book.ended[i] += 1,
                        }
                        if next_h.state == State::Held {
                            book.was_held[i] = true;
                        }
                        book.account = next_a;
                        book.holds[i] = next_h;
                    }
                }
                book.check(&where_);
            }
        }
        assert_eq!(pairs.len(), 30, "every pair of a state and a move");
        assert_eq!(causes.len(), 8, "every cause");
        assert!(accepted > 7_000, "accepted={accepted}");
    }
}
