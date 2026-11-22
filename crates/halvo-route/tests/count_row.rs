use halvo_route::admit::{fits, Fit, Room};
use halvo_route::count::{Count, HoldId, Usage};
use halvo_route::prices::Prices;
use halvo_route::spend::{Account, SpendCause};
use halvo_route::tier::Tier;

const PERIOD_END: u64 = 1_000_000;
const LOW: Tier = Tier::LowCostHosted;
const A: HoldId = HoldId([0xa1; 16]);
const B: HoldId = HoldId([0xb2; 16]);
const C: HoldId = HoldId([0xc3; 16]);

// One micro-unit a token for low_cost_hosted, so a hold is the body's bytes plus the output limit
// and a cost is the tokens in plus the tokens out.
fn prices() -> Prices {
    Prices::parse(
        "tier\tinput\toutput\tsource\tdate\n\
         on_device\t0\t0\tdevice\t2026-11-22\n\
         low_cost_hosted\t1000000\t1000000\ttest\t2026-11-22\n\
         frontier\t2000000\t2000000\ttest\t2026-11-22\n",
    )
    .expect("the test's price file")
}

fn reported(tokens_in: u64, tokens_out: u64) -> Usage {
    Usage::Reported {
        tokens_in,
        tokens_out,
    }
}

fn through_row(count: &Count) -> Count {
    Count::read(&count.row()).expect("the row that a count wrote")
}

fn capped(cap_minor: u64) -> Count {
    Count::new(PERIOD_END)
        .set_cap(cap_minor)
        .expect("a cap of the person")
}

#[test]
fn a_reserve_beside_an_open_hold_is_refused_when_the_sum_does_not_fit() {
    let p = prices();
    let empty = through_row(&capped(1));
    assert_eq!(
        empty
            .reserve(B, &p, LOW, 5_000, 1_000, 2_000)
            .map(|alone| alone.account().held),
        Ok(6_000),
        "6,000 alone fits the cap of 10,000"
    );
    let open = through_row(
        &empty
            .reserve(A, &p, LOW, 5_000, 1_000, 2_000)
            .expect("the first hold"),
    );
    assert_eq!(
        open.reserve(B, &p, LOW, 5_000, 1_000, 2_000),
        Err(SpendCause::OverCap)
    );
    assert_eq!(
        open.reserve(B, &p, LOW, 3_000, 1_000, 2_000)
            .map(|both| both.account().held),
        Ok(10_000),
        "4,000 fits beside the open 6,000"
    );
}

#[test]
fn a_hold_that_expires_beside_an_open_one_frees_its_own_amount_alone() {
    let p = prices();
    let both = through_row(
        &capped(1)
            .reserve(A, &p, LOW, 3_000, 1_000, 1_000)
            .and_then(|count| count.reserve(B, &p, LOW, 2_000, 1_000, 2_000))
            .expect("two holds"),
    );
    assert_eq!(both.expire(999), Ok(both.clone()), "no life has ended");
    let after = through_row(&both.expire(1_000).expect("the expiry of A"));
    assert_eq!(
        (after.account().spent, after.account().held),
        (4_000, 3_000)
    );
    assert_eq!(after.settle(A, &p, Usage::Unsent), Err(SpendCause::NoHold));
    assert_eq!(
        after
            .settle(B, &p, reported(2_000, 500))
            .map(|settled| (settled.account().spent, settled.account().held)),
        Ok((6_500, 0)),
        "B is still open"
    );
}

#[test]
fn the_room_read_with_open_holds_counts_them() {
    let p = prices();
    let count = through_row(
        &capped(1)
            .reserve(A, &p, LOW, 1_500, 500, 2_000)
            .and_then(|count| count.settle(A, &p, reported(1_500, 500)))
            .and_then(|count| count.reserve(B, &p, LOW, 4_000, 1_000, 2_000))
            .expect("a settled hold and an open one"),
    );
    let room = count.room().expect("a cap");
    assert_eq!(
        room,
        Room {
            cap_minor: 1,
            spent: 2_000,
            held: 5_000,
            period_end_ms: PERIOD_END,
        }
    );
    assert_eq!(room.left(), 3_000);
    assert_eq!(fits(&room, 3_001, &[]), Fit::Over);
    assert_eq!(fits(&room, 3_000, &[]), Fit::Fits);
}

#[test]
fn a_written_row_reads_back_as_the_same_account() {
    let p = prices();
    let default = Count::new(PERIOD_END);
    assert_eq!(
        default.account(),
        Account {
            cap_minor: Some(2_000),
            active: true,
            spent: 0,
            held: 0,
        }
    );
    let person = default.set_cap(3).expect("a cap of the person");
    assert_eq!(person.account().cap_minor, Some(3));
    for start in [default, person] {
        let count = start
            .reserve(A, &p, LOW, 5_000, 1_000, 1_000)
            .and_then(|count| count.reserve(B, &p, LOW, 3_000, 1_000, 2_000))
            .and_then(|count| count.settle(A, &p, reported(4_000, 1_000)))
            .expect("a settled hold and an open one");
        let row = count.row();
        let back = Count::read(&row).expect("the row that the count wrote");
        assert_eq!(back, count);
        assert_eq!(back.account(), count.account());
        assert_eq!(back.row(), row);
        assert_eq!(back.room(), count.room());
        assert_eq!(
            back.reserve(C, &p, LOW, 20_000, 2_000, 3_000),
            count.reserve(C, &p, LOW, 20_000, 2_000, 3_000)
        );
        assert_eq!(
            back.reserve(B, &p, LOW, 3_000, 1_000, 2_000),
            count.reserve(B, &p, LOW, 3_000, 1_000, 2_000)
        );
        assert_eq!(
            back.settle(B, &p, reported(2_000, 1_000)),
            count.settle(B, &p, reported(2_000, 1_000))
        );
        assert_eq!(back.expire(2_000), count.expire(2_000));
    }
}

#[test]
fn a_turn_of_the_period_clears_what_was_spent_and_keeps_the_cap_and_the_open_holds() {
    let p = prices();
    let count = through_row(
        &capped(3)
            .reserve(A, &p, LOW, 4_000, 1_000, 2_000)
            .and_then(|count| count.settle(A, &p, reported(3_000, 1_000)))
            .and_then(|count| count.reserve(B, &p, LOW, 5_000, 1_000, 2_000_000))
            .expect("a settled hold and an open one"),
    );
    for end in [PERIOD_END - 1, PERIOD_END] {
        assert_eq!(count.turn(end), count, "an end that is not later: {end}");
    }
    let turned = through_row(&count.turn(PERIOD_END + 1));
    assert_eq!(
        turned.account(),
        Account {
            cap_minor: Some(3),
            active: true,
            spent: 0,
            held: 6_000,
        }
    );
    assert_eq!(
        turned.room().map(|room| room.period_end_ms),
        Some(PERIOD_END + 1)
    );
    assert_eq!(
        turned
            .settle(B, &p, reported(4_000, 1_000))
            .map(|settled| settled.account()),
        Ok(Account {
            cap_minor: Some(3),
            active: true,
            spent: 5_000,
            held: 0,
        }),
        "the open hold settles in the new period"
    );
}
