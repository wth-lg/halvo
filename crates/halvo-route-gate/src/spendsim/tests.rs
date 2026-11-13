use std::collections::BTreeMap;

use super::*;
use halvo_route::spend::State;

const SEEDS: std::ops::RangeInclusive<u64> = 1..=8;

fn steps_of<'a>(outcome: &'a Outcome, case: &str) -> Vec<&'a Step> {
    outcome
        .steps
        .iter()
        .filter(|step| step.case == case)
        .collect()
}

fn counts_of(outcome: &Outcome, case: &str) -> Counts {
    outcome
        .cases
        .iter()
        .find(|(name, _)| *name == case)
        .map(|(_, counts)| *counts)
        .expect("a case of the outcome")
}

fn answers(steps: &[&Step]) -> Vec<String> {
    steps
        .iter()
        .map(|step| {
            let answer = match step.answer {
                Ok(()) => "ok",
                Err(cause) => cause.as_str(),
            };
            format!("{} {answer}", step.mv.name())
        })
        .collect()
}

fn moved(cases: &[(&'static str, Counts)]) -> Vec<(&'static str, [u64; 5])> {
    cases
        .iter()
        .map(|(name, c)| {
            (
                *name,
                [
                    c.reserves,
                    c.refused_over_cap,
                    c.expired,
                    c.overruns,
                    c.above_cap,
                ],
            )
        })
        .collect()
}

fn recount(outcome: &Outcome) -> Vec<(&'static str, [u64; 5])> {
    let mut found = Vec::new();
    for case in CASES {
        let mut counts = Counts::default();
        let mut amounts: BTreeMap<u64, u64> = BTreeMap::new();
        for step in steps_of(outcome, case) {
            match (step.mv, step.answer) {
                (Move::Reserve { amount }, answer) => {
                    counts.reserves += 1;
                    if answer == Err(SpendCause::OverCap) {
                        counts.refused_over_cap += 1;
                    }
                    if answer.is_ok() {
                        amounts.insert(step.hold, amount);
                        let cap = step.cap_minor.map(|cap| u128::from(cap) * 10_000);
                        let counted = u128::from(step.spent) + u128::from(step.held);
                        if cap.is_none_or(|cap| counted > cap) {
                            counts.above_cap += 1;
                        }
                    }
                }
                (Move::Expire, Ok(())) => counts.expired += 1,
                (Move::Settle { cost }, Ok(())) if cost > amounts[&step.hold] => {
                    counts.overruns += 1
                }
                _ => {}
            }
        }
        found.push((case, counts));
    }
    moved(&found)
}

#[test]
fn seed_one_prints_seven_lines_that_pass() {
    let outcome = simulate(1);
    let lines = outcome.lines();
    assert_eq!(lines.len(), 7, "{lines:?}");
    for (line, case) in lines.iter().zip(CASES) {
        assert!(
            line.starts_with(&format!("spend case={case} tasks=")),
            "{line}"
        );
        let keys: Vec<&str> = line
            .split(' ')
            .skip(1)
            .map(|word| word.split('=').next().unwrap_or(""))
            .collect();
        assert_eq!(
            keys,
            [
                "case",
                "tasks",
                "reserves",
                "admitted",
                "refused_over_cap",
                "expired",
                "overruns",
                "above_cap",
                "stopped_mid_run"
            ]
        );
    }
    assert_eq!(
        lines[6],
        "spend cases=6 overruns=0 above_cap=0 admit_sum=ok result=pass"
    );
    assert!(outcome.passed());
    assert_eq!(outcome.faults, 0);
}

#[test]
fn the_same_seed_gives_the_same_run_and_another_seed_another() {
    assert_eq!(simulate(1), simulate(1));
    assert_ne!(simulate(1).export(), simulate(2).export());
}

#[test]
fn every_seed_passes_and_its_counts_are_those_of_its_moves() {
    for seed in SEEDS {
        let outcome = simulate(seed);
        assert!(outcome.passed(), "seed {seed}: {:?}", outcome.lines());
        assert_eq!(recount(&outcome), moved(&outcome.cases), "seed {seed}");
        assert_eq!(outcome.steps.len() as u64, {
            let mut total = 0;
            for case in CASES {
                total += steps_of(&outcome, case).len() as u64;
            }
            total
        });
    }
}

fn replayed() -> Vec<(String, Outcome)> {
    let mut runs: Vec<(String, Outcome)> = SEEDS
        .map(|seed| (format!("seed {seed}"), simulate(seed)))
        .collect();
    runs.push((
        "no cap".to_owned(),
        simulate_with(1, Prices::release(), None),
    ));
    runs
}

#[test]
fn every_move_is_the_answer_of_step_on_the_values_before_it() {
    for (seed, outcome) in replayed() {
        for case in CASES {
            let steps = steps_of(&outcome, case);
            let mut accounts: BTreeMap<u64, Account> = BTreeMap::new();
            let mut holds: BTreeMap<u64, Hold> = BTreeMap::new();
            let mut last_hold = 0;
            for (index, step) in steps.iter().enumerate() {
                assert_eq!(step.step, index as u64 + 1, "{seed} {case}");
                match holds.get(&step.hold) {
                    None => {
                        assert_eq!(step.hold, last_hold + 1, "{seed} {case} {index}");
                        last_hold = step.hold;
                    }
                    Some(known) => assert_ne!(
                        known.state,
                        State::New,
                        "{seed} {case} {index}: a hold number is used again"
                    ),
                }
                let before = accounts.get(&step.account).copied().unwrap_or(Account {
                    cap_minor: step.cap_minor,
                    active: true,
                    spent: 0,
                    held: 0,
                });
                let before = Account {
                    cap_minor: step.cap_minor,
                    ..before
                };
                let hold = holds.get(&step.hold).copied().unwrap_or(Hold::new());
                let (account, hold) = match step::step(&before, &hold, step.mv) {
                    Ok(next) => {
                        assert_eq!(step.answer, Ok(()), "{seed} {case} {index}");
                        next
                    }
                    Err(cause) => {
                        assert_eq!(step.answer, Err(cause), "{seed} {case} {index}");
                        (before, hold)
                    }
                };
                assert_eq!(
                    (step.spent, step.held),
                    (account.spent, account.held),
                    "{seed} {case} {index}"
                );
                let answer = match step.answer {
                    Ok(()) => "ok",
                    Err(cause) => cause.as_str(),
                };
                let cap = before
                    .cap_minor
                    .map_or_else(|| "null".to_owned(), |cap| cap.to_string());
                let line = step.line();
                assert!(
                    line.starts_with(&format!(
                        "{{\"case\":\"{case}\",\"account\":{},\"step\":{},\"hold\":{},\"move\":\"{}\",\"arg\":",
                        step.account,
                        index + 1,
                        step.hold,
                        step.mv.name()
                    )),
                    "{seed} {line}"
                );
                assert!(
                    line.ends_with(&format!(
                        ",\"answer\":\"{answer}\",\"cap\":{cap},\"spent\":{},\"held\":{}}}",
                        account.spent, account.held
                    )),
                    "{seed} {line}"
                );
                accounts.insert(step.account, account);
                holds.insert(step.hold, hold);
                match step.mv {
                    Move::Reserve { amount } => {
                        let request = step.request.expect("a reserve names its request");
                        assert_eq!(request.amount, amount);
                        assert_eq!(
                            amount,
                            Prices::release().hold(
                                request.tier,
                                request.body_bytes,
                                request.max_output_tokens
                            )
                        );
                    }
                    _ => assert_eq!(step.request, None),
                }
            }
        }
    }
}

mod step {
    pub use halvo_route::spend::step;
}

#[test]
fn near_cap_runs_tasks_until_a_reserve_answers_over_cap() {
    for seed in SEEDS {
        let outcome = simulate(seed);
        let steps = steps_of(&outcome, "near_cap");
        let counts = counts_of(&outcome, "near_cap");
        let last = steps.last().expect("a move");
        assert_eq!(
            (last.mv.name(), last.answer),
            ("reserve", Err(SpendCause::OverCap))
        );
        assert!(steps
            .iter()
            .all(|step| step.account == 1 && step.cap_minor == Some(NEAR_CAP_MINOR)));
        assert_eq!(counts.refused_over_cap, 1);
        assert_eq!(counts.admitted, 0);
        assert!(counts.tasks >= 2, "seed {seed}: {counts:?}");
        assert!(steps.len() < 1_000, "seed {seed}: {} moves", steps.len());
        assert_eq!(
            counts.reserves,
            steps
                .iter()
                .filter(|step| step.mv.name() == "reserve")
                .count() as u64
        );
        assert_eq!(counts.tasks, counts.reserves.div_ceil(3));
    }
}

#[test]
fn fan_out_admits_by_the_room_and_no_admitted_task_meets_the_cap() {
    for seed in SEEDS {
        let outcome = simulate(seed);
        let steps = steps_of(&outcome, "fan_out");
        let counts = counts_of(&outcome, "fan_out");
        assert_eq!(counts.tasks, 240);
        assert!(counts.admitted > 0 && counts.admitted < 240, "{counts:?}");
        assert_eq!((counts.refused_over_cap, counts.stopped_mid_run), (0, 0));
        assert_eq!(counts.reserves, counts.admitted * 3);
        assert_eq!(steps.len() as u64, counts.admitted * 9);
        let accounts: Vec<u64> = steps.iter().map(|step| step.account).collect();
        assert!(accounts.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(steps
            .iter()
            .all(|step| step.cap_minor.is_some_and(|cap| (10..=100).contains(&cap))));
        let interleaved = steps.windows(2).any(|pair| {
            pair[0].account == pair[1].account
                && pair[0].hold != pair[1].hold
                && pair[0].mv.name() != "settle"
        });
        assert!(interleaved, "seed {seed}: no two tasks interleave");
    }
}

#[test]
fn cap_lowered_lowers_the_cap_while_a_hold_is_sent() {
    for seed in SEEDS {
        let outcome = simulate(seed);
        let steps = steps_of(&outcome, "cap_lowered");
        let counts = counts_of(&outcome, "cap_lowered");
        let n = steps.len();
        assert!(n >= 7, "seed {seed}");
        assert_eq!(
            answers(&steps[n - 4..]),
            ["reserve ok", "dispatch ok", "settle ok", "reserve over_cap"]
        );
        let default_cap = release::cap().map(|cap| cap.amount);
        assert!(steps[..n - 2]
            .iter()
            .all(|step| step.cap_minor == default_cap));
        let spent_at_lowering = steps[n - 3].spent;
        assert!(spent_at_lowering >= LOWER_AFTER);
        assert_eq!(
            answers(&steps[n - 7..n - 4]),
            ["reserve ok", "dispatch ok", "settle ok"]
        );
        assert!(steps[n - 7].spent < LOWER_AFTER, "seed {seed}");
        assert!(steps[n - 5].spent >= LOWER_AFTER, "seed {seed}");
        let lowered = Some(spent_at_lowering / 10_000);
        assert_eq!(
            (steps[n - 2].cap_minor, steps[n - 1].cap_minor),
            (lowered, lowered)
        );
        assert_eq!(counts.tasks, 2);
        assert_eq!(counts.refused_over_cap, 1);
        assert_eq!(steps[n - 1].hold, steps[n - 4].hold + 1);
    }
}

#[test]
fn stream_cut_is_reserve_dispatch_expire() {
    let outcome = simulate(1);
    let steps = steps_of(&outcome, "stream_cut");
    assert_eq!(answers(&steps), ["reserve ok", "dispatch ok", "expire ok"]);
    let amount = match steps[0].mv {
        Move::Reserve { amount } => amount,
        _ => 0,
    };
    assert_eq!((steps[2].spent, steps[2].held), (amount, 0));
    let counts = counts_of(&outcome, "stream_cut");
    assert_eq!((counts.tasks, counts.reserves, counts.expired), (1, 1, 1));
}

#[test]
fn error_burst_holds_one_expiry_for_a_resend_and_three_for_new_ids() {
    let outcome = simulate(1);
    let steps = steps_of(&outcome, "error_burst");
    let counts = counts_of(&outcome, "error_burst");
    assert_eq!(
        (counts.tasks, counts.reserves, counts.expired),
        (40, 100, 80)
    );
    let first: Vec<&Step> = steps
        .iter()
        .copied()
        .filter(|step| step.account == 1)
        .collect();
    let second: Vec<&Step> = steps
        .iter()
        .copied()
        .filter(|step| step.account == 2)
        .collect();
    assert_eq!((first.len(), second.len()), (100, 180));
    for task in first.chunks(5) {
        assert_eq!(
            answers(task),
            [
                "reserve ok",
                "dispatch ok",
                "expire ok",
                "reserve repeat",
                "dispatch used"
            ]
        );
        assert!(task.iter().all(|step| step.hold == task[0].hold));
        assert_eq!(task[3].request, task[0].request);
    }
    for task in second.chunks(9) {
        let holds: Vec<u64> = task.iter().map(|step| step.hold).collect();
        assert_eq!(holds[0] + 1, holds[3]);
        assert_eq!(holds[3] + 1, holds[6]);
        assert_eq!(task[0].request, task[3].request);
        assert_eq!(task[0].request, task[6].request);
    }
    let first_spent = first.last().expect("a move").spent;
    let amounts: u64 = first
        .iter()
        .filter(|step| step.answer.is_ok())
        .filter_map(|step| match step.mv {
            Move::Reserve { amount } => Some(amount),
            _ => None,
        })
        .sum();
    assert_eq!(first_spent, amounts);
}

#[test]
fn second_device_uses_up_the_room_and_stops_both_tasks_mid_run() {
    for seed in SEEDS {
        let outcome = simulate(seed);
        let steps = steps_of(&outcome, "second_device");
        let counts = counts_of(&outcome, "second_device");
        assert_eq!(
            (
                counts.tasks,
                counts.admitted,
                counts.refused_over_cap,
                counts.stopped_mid_run
            ),
            (2, 2, 2, 2),
            "seed {seed}"
        );
        let n = steps.len();
        assert_eq!(
            answers(&steps[n - 2..]),
            ["reserve over_cap", "reserve over_cap"]
        );
        assert_eq!(
            answers(&steps[..6])
                .iter()
                .filter(|a| a.ends_with(" ok"))
                .count(),
            6
        );
        let last = steps[n - 1];
        let left = SECOND_DEVICE_MINOR * 10_000 - last.spent - last.held;
        let smallest = Sim::new(seed, Prices::release(), None, step::step)
            .built(Tier::LowCostHosted, 1, 1)
            .expect("the smallest request")
            .amount;
        assert!(left < smallest, "seed {seed}: {left} left");
        assert!(n > 10, "seed {seed}: the other device made no hold");
    }
}

#[test]
fn a_cap_that_does_not_parse_ends_every_case_that_takes_it() {
    let outcome = simulate_with(1, Prices::release(), None);
    assert!(outcome.passed());
    for case in ["cap_lowered", "stream_cut"] {
        assert_eq!(answers(&steps_of(&outcome, case)), ["reserve no_cap"]);
    }
    let burst = counts_of(&outcome, "error_burst");
    assert_eq!((burst.tasks, burst.reserves, burst.expired), (40, 40, 0));
}

fn overrunning(account: &Account, hold: &Hold, mv: Move) -> Result<(Account, Hold), SpendCause> {
    match mv {
        Move::Settle { .. } => step::step(
            account,
            hold,
            Move::Settle {
                cost: hold.amount + 1,
            },
        ),
        _ => step::step(account, hold, mv),
    }
}

fn past_the_cap(account: &Account, hold: &Hold, mv: Move) -> Result<(Account, Hold), SpendCause> {
    if above_cap(account) || !matches!(mv, Move::Reserve { .. }) {
        return step::step(account, hold, mv);
    }
    let open = Account {
        cap_minor: Some(u64::MAX / 10_000),
        ..*account
    };
    step::step(&open, hold, mv).map(|(next, hold)| {
        (
            Account {
                cap_minor: account.cap_minor,
                ..next
            },
            hold,
        )
    })
}

#[test]
fn a_machine_that_overruns_or_passes_the_cap_fails_the_run() {
    let over = run(Sim::new(1, Prices::release(), Some(2000), overrunning));
    assert!(!over.passed());
    for case in ["near_cap", "fan_out", "cap_lowered", "second_device"] {
        assert!(counts_of(&over, case).overruns > 0, "{case}");
    }
    let sum: u64 = over.cases.iter().map(|(_, counts)| counts.overruns).sum();
    assert_eq!(
        over.lines()[6],
        format!("spend cases=6 overruns={sum} above_cap=0 admit_sum=ok result=fail")
    );
    let last = over.lines().pop().expect("a line");
    assert!(last.starts_with("spend cases=6 overruns="), "{last}");
    assert!(
        last.ends_with(" above_cap=0 admit_sum=ok result=fail"),
        "{last}"
    );

    let past = run(Sim::new(1, Prices::release(), Some(2000), past_the_cap));
    assert!(!past.passed());
    let near = counts_of(&past, "near_cap");
    assert!(near.above_cap > 0, "{near:?}");
    assert_eq!(recount(&past), moved(&past.cases));
    let sum: u64 = past.cases.iter().map(|(_, counts)| counts.above_cap).sum();
    assert_eq!(
        past.lines()[6],
        format!("spend cases=6 overruns=0 above_cap={sum} admit_sum=ok result=fail")
    );
    for step in &past.steps {
        let line = step.line();
        let tail = format!(",\"spent\":{},\"held\":{}}}", step.spent, step.held);
        assert!(line.ends_with(&tail), "{line}");
    }
    assert!(past
        .lines()
        .last()
        .is_some_and(|line| line.ends_with("admit_sum=ok result=fail")));
}

fn never_over(account: &Account, hold: &Hold, mv: Move) -> Result<(Account, Hold), SpendCause> {
    let open = Account {
        cap_minor: Some(u64::MAX / 10_000),
        ..*account
    };
    step::step(&open, hold, mv).map(|(next, hold)| {
        (
            Account {
                cap_minor: account.cap_minor,
                ..next
            },
            hold,
        )
    })
}

#[test]
fn a_case_that_does_not_end_stops_at_the_bound_with_a_fault() {
    let outcome = run(Sim::new(1, Prices::release(), Some(2000), never_over));
    let near = steps_of(&outcome, "near_cap");
    assert!(near.len() as u64 >= MOST_MOVES, "{} moves", near.len());
    assert!((near.len() as u64) < MOST_MOVES + 9, "{} moves", near.len());
    assert!(outcome.faults >= 1);
    assert!(!outcome.passed());
}

#[test]
fn an_admission_above_the_room_is_a_miss() {
    let mut sim = Sim::new(1, Prices::release(), Some(2000), step::step);
    let room = Room {
        cap_minor: 1,
        spent: 0,
        held: 0,
        period_end_ms: 0,
    };
    let task = Task::new(
        vec![Request {
            tier: Tier::Frontier,
            body_bytes: 1,
            max_output_tokens: 1,
            amount: 10_001,
        }],
        false,
    );
    let mut running = Vec::new();
    assert!(sim.admit_by(|_, _, _| Fit::Fits, &room, &task, &mut running));
    assert!(!sim.outcome.admit_sum_ok);
    assert_eq!((running, sim.counts.admitted), (vec![10_001], 1));
    let mut sim = Sim::new(1, Prices::release(), Some(2000), step::step);
    let mut running = Vec::new();
    assert!(!sim.admit_by(fits, &room, &task, &mut running));
    assert!(sim.outcome.admit_sum_ok && running.is_empty());
    let outcome = Outcome {
        admit_sum_ok: false,
        ..simulate(1)
    };
    assert!(!outcome.passed());
    assert_eq!(
        outcome.lines()[6],
        "spend cases=6 overruns=0 above_cap=0 admit_sum=miss result=fail"
    );
    let outcome = Outcome {
        faults: 1,
        ..simulate(1)
    };
    assert!(!outcome.passed());
}

#[test]
fn above_cap_and_sum_within_are_their_rules() {
    let account = |cap_minor, spent, held| Account {
        cap_minor,
        active: true,
        spent,
        held,
    };
    assert!(!above_cap(&account(Some(1), 10_000, 0)));
    assert!(!above_cap(&account(Some(1), 4_000, 6_000)));
    assert!(above_cap(&account(Some(1), 4_000, 6_001)));
    assert!(above_cap(&account(Some(0), 0, 1)));
    assert!(!above_cap(&account(Some(0), 0, 0)));
    assert!(above_cap(&account(None, 0, 1)));
    assert!(!above_cap(&account(None, 0, 0)));
    assert!(!above_cap(&account(Some(u64::MAX), u64::MAX, u64::MAX)));
    assert!(above_cap(&account(Some(1), u64::MAX, u64::MAX)));
    let room = Room {
        cap_minor: 2,
        spent: 5_000,
        held: 5_000,
        period_end_ms: 7,
    };
    assert!(sum_within(&room, 4_000, &[6_000]));
    assert!(!sum_within(&room, 4_001, &[6_000]));
    assert!(sum_within(&room, 10_000, &[]));
    assert!(!sum_within(&room, 0, &[10_001]));
    let full = Room {
        spent: 30_000,
        ..room
    };
    assert!(sum_within(&full, 0, &[]));
    assert!(!sum_within(&full, 1, &[]));
    assert!(!sum_within(&room, u64::MAX, &[u64::MAX]));
}

#[test]
fn the_match_of_tiers_and_the_bodies_of_requests() {
    assert_eq!(wire_tier(Tier::OnDevice), None);
    assert_eq!(
        wire_tier(Tier::LowCostHosted),
        Some(WireTier::LowCostHosted)
    );
    assert_eq!(wire_tier(Tier::Frontier), Some(WireTier::Frontier));
    let mut sim = Sim::new(1, Prices::release(), Some(2000), step::step);
    let small = sim.built(Tier::LowCostHosted, 1, 1).expect("a body");
    let large = sim
        .built(Tier::Frontier, MOST_CONTENT, MOST_OUTPUT)
        .expect("a body");
    let written = Body {
        tier: WireTier::LowCostHosted,
        effort: Effort::Medium,
        max_output_tokens: 1,
        messages: vec![Message {
            role: Role::User,
            content: "x".to_owned(),
        }],
    }
    .to_bytes()
    .expect("a body");
    assert_eq!(
        written,
        b"{\"tier\":\"low_cost_hosted\",\"effort\":\"medium\",\"max_output_tokens\":1,\"messages\":[{\"role\":\"user\",\"content\":\"x\"}]}"
    );
    assert_eq!(small.body_bytes, written.len() as u64);
    let longer = "frontier".len() + 8_192 + "256".len();
    let shorter = "low_cost_hosted".len() + 1 + "1".len();
    assert_eq!(large.body_bytes, (written.len() + longer - shorter) as u64);
    assert_eq!(
        small.amount,
        Prices::release().hold(Tier::LowCostHosted, small.body_bytes, 1)
    );
    assert_eq!(sim.built(Tier::OnDevice, 1, 1), None);
    assert_eq!(sim.outcome.faults, 1);
    assert_eq!(sim.built(Tier::Frontier, 1, 0), None);
    assert_eq!(sim.outcome.faults, 2);
}

#[test]
fn a_step_is_written_in_the_line_form() {
    let step = Step {
        case: "fan_out",
        account: 3,
        step: 17,
        hold: 5,
        mv: Move::Reserve { amount: 41_250 },
        request: Some(Request {
            tier: Tier::Frontier,
            body_bytes: 2_000,
            max_output_tokens: 128,
            amount: 41_250,
        }),
        answer: Err(SpendCause::OverCap),
        cap_minor: Some(40),
        spent: 380_000,
        held: 0,
    };
    assert_eq!(
        step.line(),
        "{\"case\":\"fan_out\",\"account\":3,\"step\":17,\"hold\":5,\"move\":\"reserve\",\
         \"arg\":41250,\"tier\":\"frontier\",\"body_bytes\":2000,\"max_output_tokens\":128,\
         \"answer\":\"over_cap\",\"cap\":40,\"spent\":380000,\"held\":0}"
    );
    let settle = Step {
        mv: Move::Settle { cost: 9 },
        request: None,
        answer: Ok(()),
        cap_minor: None,
        ..step
    };
    assert_eq!(
        settle.line(),
        "{\"case\":\"fan_out\",\"account\":3,\"step\":17,\"hold\":5,\"move\":\"settle\",\
         \"arg\":9,\"tier\":null,\"body_bytes\":null,\"max_output_tokens\":null,\
         \"answer\":\"ok\",\"cap\":null,\"spent\":380000,\"held\":0}"
    );
    let dispatch = Step {
        mv: Move::Dispatch,
        ..settle
    };
    assert!(dispatch
        .line()
        .contains("\"move\":\"dispatch\",\"arg\":null,"));
    let outcome = simulate(1);
    let export = outcome.export();
    assert_eq!(export.lines().count(), outcome.steps.len());
    assert!(export.ends_with("}\n") && !export.contains('\r'));
}

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

#[test]
fn the_command_reads_its_arguments() {
    let call = |list: &[&str]| {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = command(&words(list), &mut out, &mut err);
        (
            code,
            String::from_utf8(out).expect("utf-8"),
            String::from_utf8(err).expect("utf-8"),
        )
    };
    let refused = |list: &[&str], detail: &str| {
        assert_eq!(
            call(list),
            (2, String::new(), format!("REFUSED usage: {detail}\n")),
            "{list:?}"
        );
    };
    refused(&[], "--seed is required");
    refused(&["--seed"], "--seed has no value");
    refused(&["--seed", "0"], "--seed takes a whole number from 1");
    refused(&["--seed", "01"], "--seed takes a whole number from 1");
    refused(&["--seed", "+1"], "--seed takes a whole number from 1");
    refused(&["--seed", ""], "--seed takes a whole number from 1");
    refused(
        &["--seed", "18446744073709551616"],
        "--seed takes a whole number from 1",
    );
    refused(&["--seed", "1", "--seed", "2"], "--seed twice");
    refused(
        &["--export", "a", "--export", "b", "--seed", "1"],
        "--export twice",
    );
    refused(
        &["--seed", "1", "--rounds", "2"],
        "unknown argument --rounds",
    );
    refused(
        &["--seed", "1", "--Rounds", "2"],
        "unknown argument of another form",
    );
    refused(&["--seed", "1", "x"], "unknown argument of another form");
    let (code, out, err) = call(&["--seed", "1"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out, format!("{}\n", simulate(1).lines().join("\n")));
    let (code, out, _) = call(&["--seed", "18446744073709551615"]);
    assert_eq!(code, u8::from(!simulate(u64::MAX).passed()));
    assert_eq!(out.lines().count(), 7);
}

#[test]
fn an_admission_that_lets_every_task_in_shows_above_the_cap_in_fan_out() {
    let mut sim = Sim::new(1, Prices::release(), Some(2000), past_the_cap);
    sim.fit = |_, _, _| Fit::Fits;
    let outcome = run(sim);
    let fan = counts_of(&outcome, "fan_out");
    assert_eq!(fan.admitted, 240);
    assert!(fan.above_cap > 0, "{fan:?}");
    assert!(!outcome.admit_sum_ok);
    assert_eq!(recount(&outcome), moved(&outcome.cases));
    assert!(outcome.lines()[6].ends_with(" admit_sum=miss result=fail"));
}

#[test]
fn the_admission_sum_holds_the_running_estimates() {
    let mut sim = Sim::new(1, Prices::release(), Some(2000), step::step);
    let room = Room {
        cap_minor: 1,
        spent: 0,
        held: 0,
        period_end_ms: 0,
    };
    let task = Task::new(
        vec![Request {
            tier: Tier::LowCostHosted,
            body_bytes: 1,
            max_output_tokens: 1,
            amount: 6_000,
        }],
        false,
    );
    let mut running = Vec::new();
    assert!(sim.admit_by(|_, _, _| Fit::Fits, &room, &task, &mut running));
    assert!(sim.outcome.admit_sum_ok);
    assert!(sim.admit_by(|_, _, _| Fit::Fits, &room, &task, &mut running));
    assert!(!sim.outcome.admit_sum_ok);
    assert_eq!(running, [6_000, 6_000]);
}

#[test]
fn stopped_mid_run_is_an_admitted_task_refused_over_cap_after_its_first_request() {
    let mut sim = Sim::new(1, Prices::release(), Some(2000), step::step);
    let admitted = Task::new(Vec::new(), true);
    let plain = Task::new(Vec::new(), false);
    sim.refused(&admitted, 0, SpendCause::OverCap);
    sim.refused(&plain, 1, SpendCause::OverCap);
    sim.refused(&admitted, 1, SpendCause::NoCap);
    assert_eq!(sim.counts.stopped_mid_run, 0);
    sim.refused(&admitted, 1, SpendCause::OverCap);
    sim.refused(&admitted, 2, SpendCause::OverCap);
    assert_eq!(sim.counts.stopped_mid_run, 2);
}

#[test]
fn every_answer_and_move_is_written_by_its_word() {
    let base = Step {
        case: "near_cap",
        account: 1,
        step: 1,
        hold: 1,
        mv: Move::Dispatch,
        request: None,
        answer: Ok(()),
        cap_minor: Some(1),
        spent: 0,
        held: 0,
    };
    for cause in SpendCause::ALL {
        let line = Step {
            answer: Err(cause),
            ..base
        }
        .line();
        let word = format!(",\"answer\":\"{}\",", cause.as_str());
        assert!(line.contains(&word), "{line}");
    }
    for mv in [
        Move::Reserve { amount: 1 },
        Move::Dispatch,
        Move::Void,
        Move::Settle { cost: 1 },
        Move::Expire,
    ] {
        let line = Step { mv, ..base }.line();
        let word = format!(",\"move\":\"{}\",", mv.name());
        assert!(line.contains(&word), "{line}");
    }
}

fn failing(seed: u64) -> Outcome {
    Outcome {
        admit_sum_ok: false,
        ..simulate(seed)
    }
}

#[test]
fn the_command_prints_the_run_of_its_seed_and_exits_by_the_result() {
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(command(&words(&["--seed", "2"]), &mut out, &mut err), 0);
    assert_eq!(
        String::from_utf8(out).expect("utf-8"),
        format!("{}\n", simulate(2).lines().join("\n"))
    );
    let mut out = Vec::new();
    assert_eq!(
        command_on(&words(&["--seed", "3"]), &mut out, &mut err, failing),
        1
    );
    assert_eq!(
        String::from_utf8(out).expect("utf-8"),
        format!("{}\n", failing(3).lines().join("\n"))
    );
    assert!(err.is_empty());
}
