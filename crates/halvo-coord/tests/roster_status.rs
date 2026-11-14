use halvo_coord::items::{ConfirmAnswer, ConfirmState};
use halvo_coord::mem::Config;
use halvo_coord::roster::status;
use halvo_coord::types::fixtures::{facts, State};
use halvo_coord::types::{
    Doing, Event, Facts, Failure, HoldSetCause, ItemId, ItemKind, Limits, Live, OwnItem,
    OwnItemKind, RunEnd, RunId, RunItem, RunItemKind, StatusLine, Trigger, What,
};
use std::num::NonZeroU32;

const TODAY: u32 = 20_760;

fn utility() -> Facts<Config> {
    facts(State::Ten).remove(0)
}

fn item(n: u8) -> ItemId {
    ItemId::parse(&format!("{:032x}", 0x9900 + u32::from(n))).unwrap()
}

fn run(n: u8) -> RunId {
    RunId::parse(&format!("{:032x}", 0x7700 + u32::from(n))).unwrap()
}

fn push(f: &mut Facts<Config>, day: u32, what: What) {
    let seq = NonZeroU32::MIN.saturating_add(f.events.len() as u32);
    f.events.push(Event {
        workflow: f.workflow.id.clone(),
        agent: f.agent.clone(),
        seq,
        day,
        at: i64::from(day) * 86_400_000_000_000,
        what,
    });
}

fn ended(f: &mut Facts<Config>, n: u8, day: u32, cause: RunEnd) {
    push(
        f,
        day,
        What::RunStarted {
            run: run(n),
            trigger: Trigger::Person,
            slot: None,
        },
    );
    push(f, day, What::RunEnded { run: run(n), cause });
}

fn line(f: &Facts<Config>) -> StatusLine {
    status(f, &Limits::PRODUCT, None)
}

#[test]
fn an_open_item_gives_needs_you_with_its_kind_before_every_other_row() {
    for kind in [OwnItemKind::Ready, OwnItemKind::Check, OwnItemKind::Setup] {
        let mut f = utility();
        push(
            &mut f,
            TODAY,
            What::HoldSet {
                cause: HoldSetCause::Person,
            },
        );
        f.doing = Doing::Idle(Some(OwnItem {
            item: item(1),
            kind,
        }));
        assert_eq!(line(&f), StatusLine::NeedsYou(kind.kind()));
        f.doing = Doing::Queued(Some(OwnItem {
            item: item(1),
            kind,
        }));
        assert_eq!(line(&f), StatusLine::NeedsYou(kind.kind()));
    }
    for kind in RunItemKind::ALL {
        let mut f = utility();
        f.store_ended = false;
        f.doing = Doing::Live(Live::Waiting(RunItem {
            item: item(2),
            kind: *kind,
        }));
        assert_eq!(line(&f), StatusLine::NeedsYou(kind.kind()), "{kind:?}");
    }
}

#[test]
fn a_confirmation_counts_for_needs_you_only_while_it_is_shown_or_armed() {
    let mut f = utility();
    f.store_ended = false;
    f.doing = Doing::Live(Live::Waiting(RunItem {
        item: item(3),
        kind: RunItemKind::Confirm,
    }));
    for state in [ConfirmState::Shown, ConfirmState::Armed] {
        assert_eq!(
            status(&f, &Limits::PRODUCT, Some(&state)),
            StatusLine::NeedsYou(ItemKind::Confirm)
        );
    }
    let answered = ConfirmState::Answered(ConfirmAnswer::Allow);
    assert_eq!(
        status(&f, &Limits::PRODUCT, Some(&answered)),
        StatusLine::Working
    );
}

#[test]
fn the_holds_not_yet_off_and_failures_give_their_lines_in_that_order() {
    let mut f = facts(State::Ten).remove(1);
    push(
        &mut f,
        TODAY,
        What::HoldSet {
            cause: HoldSetCause::Person,
        },
    );
    assert_eq!(line(&f), StatusLine::NotYet);
    let mut f = utility();
    push(
        &mut f,
        TODAY,
        What::HoldSet {
            cause: HoldSetCause::Failures,
        },
    );
    assert_eq!(line(&f), StatusLine::Failures);
    push(
        &mut f,
        TODAY,
        What::HoldSet {
            cause: HoldSetCause::Person,
        },
    );
    f.doing = Doing::Live(Live::Stopping);
    f.store_ended = false;
    assert_eq!(line(&f), StatusLine::Off);
}

#[test]
fn a_live_run_gives_working_with_its_phase() {
    for (live, want) in [
        (Live::Starting, StatusLine::Starting),
        (Live::Working, StatusLine::Working),
        (Live::Stopping, StatusLine::Stopping),
    ] {
        let mut f = utility();
        f.store_ended = false;
        f.doing = Doing::Live(live);
        assert_eq!(line(&f), want);
    }
}

#[test]
fn a_queued_start_comes_before_a_run_that_the_store_has_not_ended() {
    let mut f = utility();
    f.store_ended = false;
    f.doing = Doing::Queued(None);
    assert_eq!(line(&f), StatusLine::Queued);
    f.doing = Doing::Idle(None);
    assert_eq!(line(&f), StatusLine::Store);
    assert_eq!(StatusLine::Store.key(), "line.waiting_turn.store");
}

#[test]
fn a_last_run_of_the_failure_class_gives_problem_with_its_cause() {
    for failure in Failure::ALL {
        let mut f = utility();
        ended(&mut f, 1, TODAY, RunEnd::from(*failure));
        assert_eq!(line(&f), StatusLine::Problem(*failure));
        ended(&mut f, 2, TODAY, RunEnd::Completed);
        assert_eq!(line(&f), StatusLine::Done);
    }
}

#[test]
fn a_day_with_starts_per_day_starts_gives_tried_today() {
    let mut f = utility();
    let limits = Limits::PRODUCT;
    for n in 0..limits.starts_per_day - 1 {
        ended(&mut f, n as u8, TODAY, RunEnd::Completed);
    }
    assert_eq!(line(&f), StatusLine::Done);
    ended(&mut f, 99, TODAY, RunEnd::Completed);
    assert_eq!(line(&f), StatusLine::TriedToday);
    f.today = TODAY + 1;
    assert_eq!(line(&f), StatusLine::Done);
}

#[test]
fn the_end_of_the_last_run_gives_the_line_of_all_set() {
    let mut f = utility();
    f.events.clear();
    assert_eq!(line(&f), StatusLine::New);
    push(
        &mut f,
        TODAY,
        What::RunStarted {
            run: run(1),
            trigger: Trigger::Person,
            slot: None,
        },
    );
    assert_eq!(line(&f), StatusLine::New);
    push(
        &mut f,
        TODAY,
        What::RunEnded {
            run: run(1),
            cause: RunEnd::Cancelled,
        },
    );
    assert_eq!(line(&f), StatusLine::StoppedEarly);
    for (end, want) in [
        (RunEnd::Completed, StatusLine::Done),
        (RunEnd::NothingToDo, StatusLine::Nothing),
        (RunEnd::Interrupted, StatusLine::StoppedEarly),
        (RunEnd::Lapsed, StatusLine::StoppedEarly),
        (RunEnd::Deleted, StatusLine::StoppedEarly),
        (RunEnd::Declined, StatusLine::StoppedEarly),
        (RunEnd::HandedBack, StatusLine::StoppedEarly),
        (RunEnd::EffectUnknown, StatusLine::StoppedEarly),
    ] {
        let mut g = f.clone();
        ended(&mut g, 2, TODAY, end);
        assert_eq!(line(&g), want, "{end:?}");
    }
}

#[test]
fn the_store_line_stands_after_queued_and_before_problem_in_the_order_of_i1_3() {
    let all = StatusLine::all();
    assert!(all.contains(&StatusLine::Store));
    let at = |l: StatusLine| all.iter().position(|x| *x == l).unwrap();
    assert_eq!(at(StatusLine::Store), at(StatusLine::Queued) + 1);
    assert_eq!(
        at(StatusLine::Problem(Failure::ALL[0])),
        at(StatusLine::Store) + 1
    );
}

#[test]
fn a_day_whose_last_start_has_no_end_yet_counts_that_start_for_tried_today() {
    let mut f = utility();
    let limits = Limits::PRODUCT;
    for n in 0..limits.starts_per_day - 1 {
        ended(&mut f, n as u8, TODAY, RunEnd::Completed);
    }
    push(
        &mut f,
        TODAY,
        What::RunStarted {
            run: run(98),
            trigger: Trigger::Person,
            slot: None,
        },
    );
    assert_eq!(line(&f), StatusLine::TriedToday);
}

#[test]
fn rows_that_tie_in_group_and_name_stand_in_ascending_agent_id() {
    use halvo_coord::roster::{order, row, Placed};
    let named = |n: u32, raised: Option<u64>, line: StatusLine| {
        let mut f = utility();
        f.agent = halvo_coord::types::AgentId::parse(&format!("{:032x}", 0x5000 + n)).unwrap();
        Placed {
            row: row(&f, line, "Same name").unwrap(),
            raised,
        }
    };
    for (line, raised) in [
        (StatusLine::Done, None),
        (StatusLine::NeedsYou(ItemKind::Stuck), Some(7)),
    ] {
        let mut rows = vec![
            named(3, raised, line),
            named(1, raised, line),
            named(2, raised, line),
        ];
        order(&mut rows);
        let agents: Vec<u32> = rows
            .iter()
            .map(|p| u32::from_str_radix(p.row.agent.as_str(), 16).unwrap() - 0x5000)
            .collect();
        assert_eq!(agents, [1, 2, 3], "{line:?}");
    }
}
