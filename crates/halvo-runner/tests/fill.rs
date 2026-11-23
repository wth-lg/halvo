mod support;

use std::time::{Duration, Instant};

use halvo_schema::executable::day::day_of;
use halvo_schema::executable::frame::Cancel;
use halvo_schema::executable::names::{Autonomy, FieldName, HandbackReason, LogEvent};
use halvo_schema::executable::{
    ActionClass, AgentLog, Cause, EffectState, ExecutableStep, FromRunner, Outcome, RunnerCall,
    ToRunner,
};
use halvo_schema::Category;

use support::{
    is_effect, is_op, now_nanos, offset_of_another_day, offset_of_another_day_at, range_texts,
    store_error, Act, FillFiles, Seen, Supervisor, LONG,
};

fn fill(n: u8, files: &FillFiles) -> Supervisor {
    Supervisor::new(n, Category::SpreadsheetFill, files.slots())
}

fn checkpoint_name(sup: &Supervisor) -> String {
    format!("formulas.halvo-checkpoint-{}.xlsx", sup.run.as_str())
}

/// CP-134: the harness gives the runner `--data-dir` with a directory of the test's own, and a
/// file step locks under that directory: `agents/locks/` appears there with a lock file, and the
/// runner's line held the argument. A runner that took this account's default directory would
/// leave the test's directory without `agents/`.
#[test]
fn a_file_step_locks_under_the_data_directory_that_the_harness_gives() {
    let files = FillFiles::new("data-dir", 1);
    let sup = fill(61, &files);
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    let given = seen.data_dir.to_string_lossy().into_owned();
    assert!(
        seen.line
            .windows(2)
            .any(|pair| pair[0] == "--data-dir" && pair[1] == given),
        "the runner's line holds --data-dir and the test's directory"
    );
    let locks: Vec<String> = std::fs::read_dir(seen.data_dir.join("agents").join("locks"))
        .expect("agents/locks/ under the test's directory")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".lock"))
        .collect();
    println!("cp134 runner data_dir lock_files={}", locks.len());
    assert!(
        !locks.is_empty(),
        "a lock file under the test's data directory"
    );
}

/// U4.7, A6.1: a runner started with no `--data-dir` has no directory to lock under. Its first file step, the read of
/// step 1, ends with `step_failed` before it opens a lock, no effect is proposed, and the workbook's folder holds the
/// names that it held before. A failed read of a run of the runtime `file` ends the run `failed` with its cause and
/// no handback (r3 U11.3, the coordinator's log line 18746 (2)). Run it only at a head whose runner takes no default
/// data directory (see `Runner::start_without_data_dir`).
#[test]
fn a_file_step_with_no_data_dir_fails_with_step_failed_and_makes_no_file() {
    let files = FillFiles::new("no-data-dir", 1);
    let names = files.names();
    let before = files.workbook_bytes();
    let mut sup = fill(63, &files);
    sup.data_dir = false;
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert!(
        !seen.line.iter().any(|a| a == "--data-dir"),
        "{:?}",
        seen.line
    );
    let first = seen
        .logs()
        .first()
        .map(|l| (l.event, l.step.map(|s| s.get()), l.cause));
    assert_eq!(
        first,
        Some((LogEvent::StepFailed, Some(1), Some(Cause::StepFailed))),
        "{}",
        seen.stderr
    );
    assert!(seen.handbacks().is_empty(), "no browser runs: no handback");
    assert_eq!(seen.done(), Some(Outcome::Failed));
    assert!(seen.effects().is_empty());
    assert_eq!(files.names(), names);
    assert_eq!(files.workbook_bytes(), before);
}

/// CP-204 D3 and D7, CP-212 V2: every log line and every row of an effect carries day_of(at, the run's offset), and
/// the run's finish, which holds no instant, the day of an instant of the run; a row is held to differ from the UTC
/// day only where the two days of its instant differ, so a run across 00:00 UTC does not fail a correct runner.
#[test]
fn every_row_of_a_run_takes_the_day_of_its_instant_at_the_runs_offset() {
    let files = FillFiles::new("day-of", 1);
    let mut sup = fill(170, &files);
    let offset = offset_of_another_day();
    sup.utc_offset_s = offset;
    let before = halvo_schema::Instant(now_nanos());
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    let after = halvo_schema::Instant(now_nanos());
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    println!("cp204 offset={offset}");
    let logs = seen.logs();
    let effects = seen.effects();
    assert!(!logs.is_empty() && effects.len() == 3, "{:?}", seen.ops());
    for (what, day, at) in logs
        .iter()
        .map(|l| ("log", l.day, l.at))
        .chain(effects.iter().map(|e| ("effect", e.day, e.at)))
    {
        assert_eq!(day, day_of(at, offset), "{what} at {}", at.0);
        if day_of(at, offset) != day_of(at, 0) {
            assert_ne!(day, day_of(at, 0), "{what} at {}: the UTC day", at.0);
        }
    }
    let finish = seen.finish().expect("the finish").day;
    assert!(
        (day_of(before, offset)..=day_of(after, offset)).contains(&finish),
        "finish {finish} between {} and {}",
        day_of(before, offset),
        day_of(after, offset)
    );
    if day_of(before, offset) == day_of(after, offset)
        && day_of(before, 0) == day_of(after, 0)
        && day_of(before, offset) != day_of(before, 0)
    {
        assert_ne!(finish, day_of(before, 0), "finish: the UTC day");
    }
}

/// CP-212 V2: at instants drawn over a whole UTC day, the helper gives +50,400 s from 12:00 UTC on and -50,400 s
/// before, and that offset gives a local day that differs from the UTC day from the call until the end that its doc
/// names (14:00 UTC for -50,400 s, the next 00:00 UTC for +50,400 s) and no longer at that end.
#[test]
fn the_offset_helper_differs_from_the_utc_day_until_the_end_that_its_doc_names() {
    const SECOND: i64 = 1_000_000_000;
    const DAY: i64 = 86_400 * SECOND;
    let midnight = 20_404 * DAY;
    let mut draws: Vec<i64> = vec![
        0,
        1,
        43_199 * SECOND + 999_999_999,
        43_200 * SECOND,
        DAY - 1,
    ];
    let mut x: u64 = 0x2545_f491_4f6c_dd1d;
    for _ in 0..2_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        draws.push((x % DAY as u64) as i64);
    }
    for into_day in draws {
        let call = midnight + into_day;
        let offset = offset_of_another_day_at(call);
        assert_eq!(
            offset,
            if into_day >= 12 * 3_600 * SECOND {
                50_400
            } else {
                -50_400
            },
            "a call {into_day} ns into the day: +50,400 s from 12:00 UTC on, else -50,400 s"
        );
        let end = if offset > 0 {
            midnight + DAY
        } else {
            midnight + 14 * 3_600 * SECOND
        };
        for at in [call, end - 1] {
            let at = halvo_schema::Instant(at);
            assert_ne!(
                day_of(at, offset),
                day_of(at, 0),
                "a call {into_day} ns into the day, offset {offset}, at {}",
                at.0
            );
        }
        let end = halvo_schema::Instant(end);
        assert_eq!(
            day_of(end, offset),
            day_of(end, 0),
            "a call {into_day} ns into the day, offset {offset}: the end"
        );
    }
}

#[test]
fn the_fill_playbook_replays_on_seeds_1_to_3() {
    for seed in 1..=3u8 {
        let files = FillFiles::new("replay", seed);
        let sup = fill(30 + seed, &files);
        let seen = sup.serve(|_, _| Act::Answer, LONG);
        assert_eq!(
            seen.done(),
            Some(Outcome::Completed),
            "seed {seed}: {}",
            seen.stderr
        );
        assert_eq!(seen.code(), Some(0), "seed {seed}");
        assert_eq!(
            seen.tags(),
            [
                "hello", "store", "progress", "store", "progress", "store", "store", "store",
                "store", "store", "store", "store", "store", "store", "done"
            ],
            "seed {seed}"
        );
        assert_eq!(
            seen.ops(),
            [
                "load",
                "log",
                "load",
                "checkpoint",
                "checkpoint",
                "effect",
                "load",
                "effect",
                "effect",
                "log",
                "finish"
            ],
            "seed {seed}: load, the read's line, the load before the prepare and the propose, the checkpoint's \
             planned and made rows, the propose, the load before the begin, the begin and the commit, the write's \
             line"
        );
        let effects = seen.effects();
        assert_eq!(
            seen.states(),
            [
                EffectState::Cleared,
                EffectState::Releasing,
                EffectState::Committed
            ],
            "seed {seed}"
        );
        let first = &effects[0];
        let bound = halvo_schema::executable::Bound::new(
            ActionClass::LocalEdit,
            [
                (
                    FieldName::File,
                    files.workbook.to_string_lossy().into_owned(),
                ),
                (FieldName::Range, "B2:D6".to_string()),
            ]
            .into_iter()
            .map(|(name, value)| halvo_schema::executable::BoundField {
                name,
                from: halvo_schema::executable::names::Source::Slot,
                origin: None,
                value: halvo_schema::executable::text::OwnValue::new(&value),
            })
            .collect(),
        )
        .expect("the bound object of the seed's slots");
        assert_eq!(
            first.digest,
            halvo_policy::digest(&bound).expect("a digest"),
            "seed {seed}: the digest of the bound object built from the slots"
        );
        for (i, row) in effects.iter().enumerate() {
            assert_eq!(row.seq.get(), i as u32 + 1, "seed {seed}: seq");
            assert_eq!(
                (&row.effect, row.class, &row.digest, row.fields),
                (&first.effect, ActionClass::LocalEdit, &first.digest, 2),
                "seed {seed}: one effect"
            );
            assert_eq!((&row.approval, row.cause), (&None, None));
            let kept: Vec<(FieldName, String)> = row
                .kept
                .iter()
                .map(|k| (k.name, k.value.joined()))
                .collect();
            assert_eq!(
                kept,
                [
                    (
                        FieldName::File,
                        files.workbook.to_string_lossy().into_owned()
                    ),
                    (FieldName::Range, "B2:D6".to_string())
                ],
                "seed {seed}"
            );
        }
        let finish = seen.finish().expect("finish");
        assert_eq!(
            (
                finish.steps,
                finish.effects,
                finish.unknown,
                finish.prompts,
                finish.playbook
            ),
            (2, 1, 0, 0, None),
            "seed {seed}"
        );
        assert_eq!(
            range_texts(&files.dir, &files.workbook),
            files.rows,
            "seed {seed}"
        );
        assert_eq!(
            files.names(),
            [
                checkpoint_name(&sup),
                "formulas.xlsx".into(),
                "source.csv".into()
            ],
            "seed {seed}: the checkpoint stays and no prepared copy is left"
        );
        let progress: Vec<(u32, u32)> = seen
            .frames
            .iter()
            .filter_map(|f| match f {
                FromRunner::Progress(p) => Some((p.step.get(), p.of.get())),
                _ => None,
            })
            .collect();
        assert_eq!(progress, [(1, 2), (2, 2)]);
        let events: Vec<(LogEvent, Option<u32>)> = seen
            .logs()
            .iter()
            .map(|l| (l.event, l.step.map(|s| s.get())))
            .collect();
        assert_eq!(
            events,
            [(LogEvent::StepOk, Some(1)), (LogEvent::StepOk, Some(2))]
        );
    }
}

#[test]
fn a_step_limit_ends_the_run_limit_with_the_cause_steps() {
    let files = FillFiles::new("limit-steps", 1);
    let before = files.workbook_bytes();
    let mut sup = fill(41, &files);
    sup.config.limits.steps = 1;
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(seen.done(), Some(Outcome::Limit));
    let stopped = seen.logs().into_iter().last().expect("a line");
    assert_eq!(
        (stopped.event, stopped.cause),
        (LogEvent::Stopped, Some(Cause::Steps))
    );
    assert_eq!(seen.finish().map(|f| f.steps), Some(1));
    assert!(seen.effects().is_empty());
    assert_eq!(files.workbook_bytes(), before);
}

#[test]
fn a_wall_limit_ends_the_run_limit_with_the_cause_wall() {
    let files = FillFiles::new("limit-wall", 1);
    let mut sup = fill(42, &files);
    sup.config.limits.wall_s = 0;
    let seen = sup.serve(
        |frame, _| {
            if is_op(frame, "load") {
                Act::After(Duration::from_millis(1100), Box::new(Act::Answer))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Limit));
    let stopped = seen.logs().into_iter().last().expect("a line");
    assert_eq!(stopped.cause, Some(Cause::Wall));
    assert_eq!(seen.finish().map(|f| f.steps), Some(0));
}

/// The wait for a handback's reply, which comes after `PAUSE`, is no time of the run's `wall_s`: here the handback of
/// an effect that is not known, which the run opens at its end because every send of its begin met a store fault (a
/// failed read opens no handback in a run whose browser does not run, r3 U5.15e). The finish's `wall_s` is at most the
/// time from before the runner's start to the finish frame less `PAUSE`, in whole seconds, however slowly the run's
/// own steps go. A runner that counts the wait reports the whole seconds of its own time, which pass that bound
/// whenever its late start and the finish frame's way to the test take together at most `PAUSE` less 1 s, 2.5 s.
#[test]
fn a_handback_s_wait_is_no_wall_time() {
    const PAUSE: Duration = Duration::from_millis(3500);
    let files = FillFiles::new("handback-wait", 2);
    let before = files.workbook_bytes();
    let sup = fill(43, &files);
    let started = Instant::now();
    let mut finished = None;
    let seen = sup.serve(
        |frame, _| {
            if is_op(frame, "finish") {
                finished = Some(started.elapsed());
            }
            if is_effect(frame, EffectState::Releasing) {
                Act::With(store_error(frame, "internal"))
            } else if matches!(frame, FromRunner::Handback(_)) {
                Act::After(
                    PAUSE,
                    Box::new(Act::With(support::handback_reply(
                        halvo_schema::executable::names::HandbackOutcome::Done,
                        None,
                    ))),
                )
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(seen.handbacks(), [HandbackReason::EffectUnknown]);
    assert_eq!(seen.done(), Some(Outcome::EffectUnknown));
    let finish = seen.finish().expect("finish");
    assert_eq!(finish.handbacks, 1);
    let most = finished
        .expect("the finish frame came")
        .saturating_sub(PAUSE)
        .as_secs();
    assert!(
        u64::from(finish.wall_s) <= most,
        "the handback's wait is no wall time: wall_s {} and at most {most} s",
        finish.wall_s
    );
    assert_eq!(files.workbook_bytes(), before);
}

#[test]
fn a_workbook_that_the_executor_refuses_ends_the_run_failed_with_workbook() {
    let files = FillFiles::new("bad-workbook", 2);
    std::fs::write(&files.workbook, b"no zip package").expect("write");
    let sup = fill(44, &files);
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(seen.done(), Some(Outcome::Failed));
    assert!(
        seen.handbacks().is_empty(),
        "a failed effect step hands nothing back"
    );
    let causes: Vec<Option<Cause>> = seen.logs().iter().map(|l| l.cause).collect();
    assert_eq!(causes, [None, Some(Cause::Workbook), Some(Cause::Workbook)]);
    assert!(
        seen.effects().is_empty(),
        "no propose before a prepared copy"
    );
    assert_eq!(files.workbook_bytes(), b"no zip package");
}

#[test]
fn under_observe_the_edit_is_refused_and_the_workbook_is_untouched() {
    let files = FillFiles::new("observe", 3);
    let before = files.workbook_bytes();
    let mut sup = fill(45, &files);
    sup.config.autonomy = Autonomy::Observe;
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(seen.states(), [EffectState::Refused]);
    assert_eq!(seen.done(), Some(Outcome::Refused));
    assert_eq!(
        seen.logs().last().map(|l| l.cause),
        Some(Some(Cause::Autonomy))
    );
    assert_eq!(files.workbook_bytes(), before);
    assert_eq!(
        files.names(),
        [
            checkpoint_name(&sup),
            "formulas.xlsx".into(),
            "source.csv".into()
        ]
    );
}

#[test]
fn decide_reads_the_configuration_as_it_stands_at_the_propose() {
    let files = FillFiles::new("config-now", 1);
    let before = files.workbook_bytes();
    let sup = fill(46, &files);
    let mut observe = sup.config.clone();
    observe.autonomy = Autonomy::Observe;
    let loads = std::cell::Cell::new(0);
    let seen = sup.serve(
        |frame, _| {
            if is_op(frame, "load") {
                loads.set(loads.get() + 1);
                if loads.get() == 2 {
                    let FromRunner::Store(s) = frame else {
                        unreachable!()
                    };
                    let mut reply = sup.store_answer(s.request.body());
                    if let halvo_schema::executable::RunnerReply::Load(l) = &mut reply {
                        l.config = observe.clone();
                    }
                    return Act::With(ToRunner::StoreReply(
                        halvo_schema::executable::frame::StoreReplyFrame {
                            reply: halvo_schema::executable::StoreReply {
                                id: Some(s.request.id().clone()),
                                outcome: Ok(reply),
                            },
                        },
                    ));
                }
            }
            Act::Answer
        },
        LONG,
    );
    assert_eq!(
        seen.states(),
        [EffectState::Refused],
        "the second load decides"
    );
    assert_eq!(seen.done(), Some(Outcome::Refused));
    assert_eq!(files.workbook_bytes(), before);
}

#[test]
fn a_run_that_ends_while_its_edit_is_paused_lets_no_byte_reach_the_workbook() {
    let files = FillFiles::new("paused-cancel", 1);
    let before = files.workbook_bytes();
    let sup = fill(47, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Cleared) {
                Act::Then(ToRunner::Cancel(Cancel {}), Box::new(Act::Answer))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(
        seen.states(),
        [EffectState::Cleared],
        "no begin after the cancel"
    );
    assert_eq!(seen.done(), Some(Outcome::Declined));
    assert_eq!(
        files.workbook_bytes(),
        before,
        "no byte of the edit reached the workbook"
    );
    assert_eq!(
        files.names(),
        [
            checkpoint_name(&sup),
            "formulas.xlsx".into(),
            "source.csv".into()
        ],
        "the prepared copy is gone"
    );
}

#[test]
fn a_begin_that_the_store_refuses_lets_no_byte_reach_the_workbook() {
    let files = FillFiles::new("paused-refused", 2);
    let before = files.workbook_bytes();
    let sup = fill(48, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Releasing) {
                Act::With(store_error(frame, "effect"))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(
        seen.states(),
        [EffectState::Cleared, EffectState::Releasing]
    );
    assert_eq!(seen.done(), Some(Outcome::Failed));
    assert_eq!(
        seen.logs().last().map(|l| l.cause),
        Some(Some(Cause::Effect))
    );
    assert_eq!(
        seen.finish().map(|f| f.unknown),
        Some(0),
        "the begin was not stored"
    );
    assert!(seen.handbacks().is_empty());
    assert_eq!(files.workbook_bytes(), before);
}

#[test]
fn a_begin_that_meets_a_store_fault_is_sent_again_with_its_request_id() {
    let files = FillFiles::new("begin-again", 3);
    let sup = fill(49, &files);
    let faults = std::cell::Cell::new(0);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Releasing) && faults.get() < 2 {
                faults.set(faults.get() + 1);
                Act::With(store_error(frame, "store"))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(
        seen.states(),
        [
            EffectState::Cleared,
            EffectState::Releasing,
            EffectState::Releasing,
            EffectState::Releasing,
            EffectState::Committed
        ]
    );
    let ids: Vec<String> = seen
        .frames
        .iter()
        .filter(|f| is_effect(f, EffectState::Releasing))
        .map(|f| match f {
            FromRunner::Store(s) => s.request.id().as_str().to_string(),
            _ => unreachable!(),
        })
        .collect();
    assert!(ids.windows(2).all(|w| w[0] == w[1]), "one request id");
    assert_eq!(seen.done(), Some(Outcome::Completed));
    assert_eq!(range_texts(&files.dir, &files.workbook), files.rows);
}

/// A5.2, A11.3: a workbook that changes under the paused edit makes `replace` answer that it left the workbook as
/// it was; the effect takes the move `not_made` with the cause `not_replaced`, no handback follows, and the step
/// fails, so that the run ends `failed` with `step_failed`.
#[test]
fn a_workbook_changed_under_the_paused_edit_is_not_replaced_and_its_effect_is_not_made() {
    let files = FillFiles::new("changed-under", 1);
    let sup = fill(50, &files);
    let workbook = files.workbook.clone();
    let mut changed = Vec::new();
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Releasing) {
                let mut bytes = std::fs::read(&workbook).expect("read");
                bytes.push(0);
                std::fs::write(&workbook, &bytes).expect("change the workbook");
                changed = bytes;
            }
            Act::Answer
        },
        LONG,
    );
    assert_eq!(files.workbook_bytes(), changed, "not replaced");
    let rows: Vec<(EffectState, Option<Cause>)> =
        seen.effects().iter().map(|e| (e.state, e.cause)).collect();
    assert_eq!(
        rows,
        [
            (EffectState::Cleared, None),
            (EffectState::Releasing, None),
            (EffectState::NotMade, Some(Cause::NotReplaced))
        ]
    );
    assert!(seen.handbacks().is_empty());
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    let lines: Vec<(LogEvent, Option<Cause>)> =
        seen.logs().iter().map(|l| (l.event, l.cause)).collect();
    assert_eq!(
        lines[lines.len() - 2..],
        [
            (LogEvent::StepFailed, Some(Cause::StepFailed)),
            (LogEvent::Stopped, Some(Cause::StepFailed))
        ]
    );
    let finish = seen.finish().expect("finish");
    assert_eq!(
        (finish.effects, finish.unknown, finish.handbacks),
        (1, 0, 0)
    );
}

fn step(json: &str) -> ExecutableStep {
    ExecutableStep::parse(json.as_bytes()).expect("a step")
}

#[test]
fn a_stored_playbook_is_loaded_and_one_whose_class_differs_is_refused_with_class() {
    let navigate = r#"{"v":1,"n":1,"kind":"web","verb":"navigate","target":null,"args":[{"name":"url","from":"slot","value":null,"key":"start"}],"pre":[],"post":[],"effect":"none","class":"CLASS"}"#;
    let mut sup = Supervisor::new(51, Category::UtilityBillPayment, Vec::new());
    sup.steps = vec![step(&navigate.replace("CLASS", "prepare"))];
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(seen.ops()[..2], ["load", "load_steps"]);
    assert_eq!(seen.done(), Some(Outcome::Failed));
    assert_eq!(
        seen.logs().last().map(|l| l.cause),
        Some(Some(Cause::Class))
    );
    assert!(
        !seen.tags().contains(&"progress"),
        "no step of a refused playbook runs"
    );
    assert_eq!(
        seen.finish().and_then(|f| f.playbook).map(|p| p.get()),
        Some(1)
    );

    let mut sup = Supervisor::new(52, Category::UtilityBillPayment, Vec::new());
    sup.steps = (1..=17)
        .map(|n| {
            step(
                &navigate
                    .replace("CLASS", "read")
                    .replace(r#""n":1"#, &format!(r#""n":{n}"#)),
            )
        })
        .collect();
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    let froms: Vec<u32> = seen
        .calls()
        .iter()
        .filter_map(|c| match c {
            halvo_schema::executable::RunnerCall::LoadSteps(b) => Some(b.from.get()),
            _ => None,
        })
        .collect();
    assert_eq!(froms, [1, 17], "two pages of at most 16 steps");
    assert!(
        seen.handbacks().is_empty(),
        "no browser in this build, so no handback (r3 U5.15e)"
    );
    assert_eq!(seen.done(), Some(Outcome::HandedBack));
}

#[test]
fn a_begin_whose_every_send_meets_a_store_fault_is_handed_back_unknown_and_leaves_nothing() {
    let files = FillFiles::new("begin-faults", 2);
    let before = files.workbook_bytes();
    let sup = fill(53, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Releasing) {
                Act::With(store_error(frame, "internal"))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(
        seen.states(),
        [
            EffectState::Cleared,
            EffectState::Releasing,
            EffectState::Releasing,
            EffectState::Releasing
        ],
        "three sends of one begin"
    );
    assert_eq!(
        seen.handbacks(),
        [HandbackReason::EffectUnknown],
        "the store may hold the begin, so the effect may end unknown"
    );
    assert_eq!(seen.done(), Some(Outcome::EffectUnknown));
    assert_eq!(seen.finish().map(|f| f.unknown), Some(1));
    assert_eq!(
        files.workbook_bytes(),
        before,
        "no replace without a stored begin"
    );
}

#[test]
fn a_finish_that_the_store_refuses_still_ends_with_done_and_the_status_1() {
    let files = FillFiles::new("finish-refused", 1);
    let sup = fill(54, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_op(frame, "finish") {
                Act::With(store_error(frame, "binding"))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Completed));
    assert_eq!(seen.code(), Some(1), "the run's record was not stored");
    assert!(seen.stderr.contains("finish=no"), "{}", seen.stderr);
}

/// The run's end row that a body carries: the `stopped` line of a `log` body.
fn end_row(call: &RunnerCall) -> Option<AgentLog> {
    match call {
        RunnerCall::Log(b) => b
            .records
            .iter()
            .find(|r| r.event == LogEvent::Stopped)
            .cloned(),
        _ => None,
    }
}

fn is_end_row(frame: &FromRunner) -> bool {
    matches!(frame, FromRunner::Store(s) if end_row(s.request.body()).is_some())
}

/// The request id and what `take` takes from the body of each `store` frame, in the order that the runner sent them.
fn sent<T>(seen: &Seen, take: impl Fn(&RunnerCall) -> Option<T>) -> Vec<(String, T)> {
    seen.frames
        .iter()
        .filter_map(|f| match f {
            FromRunner::Store(s) => {
                take(s.request.body()).map(|t| (s.request.id().as_str().to_string(), t))
            }
            _ => None,
        })
        .collect()
}

/// The frames from the first end row on, each as `support::named` names it.
fn from_the_end_row(seen: &Seen) -> Vec<String> {
    let first = seen.frames.iter().position(is_end_row).expect("an end row");
    seen.frames[first..].iter().map(support::named).collect()
}

/// A7.5, CP-178 (1): the end row of a run that ends with a cause, here `steps`, is sent again after a fault of the
/// store as the same request with the same record, `seq` and `at` among its keys, so that the store's handler of
/// `log`, which keeps a record only when no held row equals it, keeps the row once; then come `finish` and `done`.
#[test]
fn an_end_row_that_meets_a_store_fault_is_sent_again_and_stands_once() {
    let files = FillFiles::new("end-row-again", 1);
    let mut sup = fill(64, &files);
    sup.config.limits.steps = 1;
    let faults = std::cell::Cell::new(0);
    let seen = sup.serve(
        |frame, _| {
            if is_end_row(frame) && faults.get() < 1 {
                faults.set(faults.get() + 1);
                Act::With(store_error(frame, "store"))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    let rows = sent(&seen, end_row);
    assert_eq!(
        rows.len(),
        2,
        "frames with the end row: the first send and one repeat after the fault"
    );
    assert_eq!(rows[0], rows[1], "one request id and one record");
    let mut held: Vec<AgentLog> = Vec::new();
    for line in seen.logs() {
        if !held.contains(&line) {
            held.push(line);
        }
    }
    let kept: Vec<Option<Cause>> = held
        .iter()
        .filter(|l| l.event == LogEvent::Stopped)
        .map(|l| l.cause)
        .collect();
    assert_eq!(
        kept,
        [Some(Cause::Steps)],
        "the handler's rule keeps one row"
    );
    assert_eq!(
        from_the_end_row(&seen),
        ["store log", "store log", "store finish", "done"]
    );
    assert_eq!(seen.done(), Some(Outcome::Limit));
}

/// A7.5: after the third fault the runner sends the end row no more and goes on to its `finish` and `done`.
#[test]
fn an_end_row_whose_every_send_meets_a_fault_is_sent_three_times_and_the_run_finishes() {
    let files = FillFiles::new("end-row-faults", 1);
    let mut sup = fill(65, &files);
    sup.config.limits.steps = 1;
    let seen = sup.serve(
        |frame, _| {
            if is_end_row(frame) {
                Act::With(store_error(frame, "internal"))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    let rows = sent(&seen, end_row);
    assert_eq!(
        rows.len(),
        3,
        "frames with the end row: the first send and two repeats"
    );
    assert!(
        rows.windows(2).all(|w| w[0] == w[1]),
        "one request id and one record"
    );
    assert_eq!(
        from_the_end_row(&seen),
        [
            "store log",
            "store log",
            "store log",
            "store finish",
            "done"
        ]
    );
    assert_eq!(seen.done(), Some(Outcome::Limit));
    assert_eq!(seen.code(), Some(0), "the store took the run's record");
}

/// A7.5, CP-132 (e): a `finish` that meets a fault of the store is sent again as the same request with the same
/// record, which the store answers as the first; the run ends with `done` and the status 0.
#[test]
fn a_finish_that_meets_a_store_fault_is_sent_again_with_its_request_id() {
    let files = FillFiles::new("finish-again", 1);
    let sup = fill(66, &files);
    let faults = std::cell::Cell::new(0);
    let seen = sup.serve(
        |frame, _| {
            if is_op(frame, "finish") && faults.get() < 1 {
                faults.set(faults.get() + 1);
                Act::With(store_error(frame, "store"))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    let finishes = sent(&seen, |c| match c {
        RunnerCall::Finish(b) => Some(b.record.clone()),
        _ => None,
    });
    assert_eq!(
        finishes.len(),
        2,
        "finish frames: the first send and one repeat after the fault"
    );
    assert_eq!(finishes[0], finishes[1], "one request id and one record");
    assert_eq!(seen.done(), Some(Outcome::Completed));
    assert_eq!(
        seen.code(),
        Some(0),
        "the store took the run's record: {}",
        seen.stderr
    );
}

#[test]
fn a_propose_that_the_store_refuses_makes_no_effect_and_no_replace() {
    let files = FillFiles::new("propose-refused", 2);
    let before = files.workbook_bytes();
    let sup = fill(55, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Cleared) {
                Act::With(store_error(frame, "effect"))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(seen.states(), [EffectState::Cleared]);
    assert_eq!(seen.done(), Some(Outcome::Failed));
    let finish = seen.finish().expect("finish");
    assert_eq!(
        (finish.effects, finish.unknown),
        (0, 0),
        "the store holds no effect"
    );
    assert_eq!(files.workbook_bytes(), before);
}

#[test]
fn a_cancel_that_comes_with_the_begins_ack_lets_no_byte_reach_the_workbook() {
    let files = FillFiles::new("cancel-at-begin", 3);
    let before = files.workbook_bytes();
    let sup = fill(56, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Releasing) {
                Act::Then(ToRunner::Cancel(Cancel {}), Box::new(Act::Answer))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(
        seen.states(),
        [
            EffectState::Cleared,
            EffectState::Releasing,
            EffectState::NotMade
        ]
    );
    assert_eq!(
        files.workbook_bytes(),
        before,
        "the paused edit did not leave"
    );
    assert!(seen.handbacks().is_empty());
    assert_eq!(seen.done(), Some(Outcome::Declined));
    assert_eq!(
        files.names(),
        [
            checkpoint_name(&sup),
            "formulas.xlsx".into(),
            "source.csv".into()
        ]
    );
}

#[test]
fn an_agent_without_the_permission_user_files_touches_no_file() {
    let files = FillFiles::new("no-permission", 1);
    let before = files.workbook_bytes();
    let mut sup = fill(57, &files);
    sup.config.permissions.clear();
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert!(
        seen.handbacks().is_empty(),
        "the read fails, and no browser runs: no handback (r3 U5.15e)"
    );
    assert_eq!(
        seen.done(),
        Some(Outcome::Failed),
        "a file run's failed read ends failed (r3 U11.3)"
    );
    assert_eq!(files.workbook_bytes(), before);
    assert_eq!(files.names(), ["formulas.xlsx", "source.csv"]);
}

/// The step at which a file run failed and the cause of its end: the `step_failed` line's step and cause, and the
/// cause of the run's `stopped` line.
fn failed_at(seen: &Seen) -> Vec<(LogEvent, Option<u32>, Option<Cause>)> {
    seen.logs()
        .iter()
        .filter(|l| matches!(l.event, LogEvent::StepFailed | LogEvent::Stopped))
        .map(|l| (l.event, l.step.map(|s| s.get()), l.cause))
        .collect()
}

#[test]
fn a_workbook_slot_that_holds_a_relative_path_is_refused() {
    let files = FillFiles::new("relative", 1);
    let before = files.workbook_bytes();
    let mut slots = files.slots();
    slots[0] = support::slot("workbook", "formulas.xlsx");
    let sup = Supervisor::new(58, Category::SpreadsheetFill, slots);
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert!(
        seen.handbacks().is_empty(),
        "no browser runs: no handback (r3 U5.15e)"
    );
    assert_eq!(seen.done(), Some(Outcome::Failed), "r3 U11.3");
    assert_eq!(
        failed_at(&seen),
        [
            (LogEvent::StepFailed, Some(1), Some(Cause::NoWorkbook)),
            (LogEvent::Stopped, None, Some(Cause::NoWorkbook))
        ],
        "{}",
        seen.stderr
    );
    assert!(seen.effects().is_empty());
    assert_eq!(files.workbook_bytes(), before);
}

#[test]
fn a_page_of_steps_that_does_not_start_at_from_is_refused_with_frame() {
    let navigate = r#"{"v":1,"n":N,"kind":"web","verb":"navigate","target":null,"args":[{"name":"url","from":"slot","value":null,"key":"start"}],"pre":[],"post":[],"effect":"none","class":"read"}"#;
    let mut sup = Supervisor::new(59, Category::UtilityBillPayment, Vec::new());
    sup.steps = (1..=17)
        .map(|n| step(&navigate.replace(r#""n":N"#, &format!(r#""n":{n}"#))))
        .collect();
    let first_page = sup.steps[..16].to_vec();
    let seen = sup.serve(
        |frame, _| {
            let FromRunner::Store(s) = frame else {
                return Act::Answer;
            };
            match s.request.body() {
                halvo_schema::executable::RunnerCall::LoadSteps(b) if b.from.get() == 17 => {
                    Act::With(ToRunner::StoreReply(
                        halvo_schema::executable::frame::StoreReplyFrame {
                            reply: halvo_schema::executable::StoreReply {
                                id: Some(s.request.id().clone()),
                                outcome: Ok(halvo_schema::executable::RunnerReply::Steps(
                                    halvo_schema::executable::body::StepsReply {
                                        agent: b.agent.clone(),
                                        steps: vec![first_page[0].clone()],
                                    },
                                )),
                            },
                        },
                    ))
                }
                _ => Act::Answer,
            }
        },
        LONG,
    );
    assert_eq!(seen.code(), Some(2));
    assert!(
        seen.stderr.starts_with("REFUSED frame: "),
        "{}",
        seen.stderr
    );
    assert!(!seen.tags().contains(&"progress"));
}

#[test]
fn a_propose_whose_every_send_meets_a_fault_counts_as_an_effect_that_did_not_begin() {
    let files = FillFiles::new("propose-faults", 3);
    let before = files.workbook_bytes();
    let sup = fill(60, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Cleared) {
                Act::With(store_error(frame, "store"))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(
        seen.states(),
        [
            EffectState::Cleared,
            EffectState::Cleared,
            EffectState::Cleared
        ]
    );
    assert_eq!(seen.done(), Some(Outcome::Failed));
    let finish = seen.finish().expect("finish");
    assert_eq!(
        (finish.effects, finish.unknown),
        (1, 0),
        "the store may hold the cleared row"
    );
    assert!(
        seen.handbacks().is_empty(),
        "a cleared effect did not leave"
    );
    assert_eq!(files.workbook_bytes(), before);
}

#[test]
fn a_commit_whose_every_send_meets_a_fault_is_handed_back_unknown() {
    let files = FillFiles::new("commit-faults", 1);
    let sup = fill(61, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Committed) {
                Act::With(store_error(frame, "internal"))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(seen.handbacks(), [HandbackReason::EffectUnknown]);
    assert_eq!(seen.done(), Some(Outcome::EffectUnknown));
    assert_eq!(seen.finish().map(|f| (f.effects, f.unknown)), Some((1, 1)));
    assert_eq!(
        range_texts(&files.dir, &files.workbook),
        files.rows,
        "the edit left once"
    );
}

#[test]
fn a_source_slot_that_holds_a_relative_path_is_refused() {
    let files = FillFiles::new("relative-source", 2);
    let before = files.workbook_bytes();
    let mut slots = files.slots();
    slots[1] = support::slot("source", "source.csv");
    let sup = Supervisor::new(62, Category::SpreadsheetFill, slots);
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert!(
        seen.handbacks().is_empty(),
        "no browser runs: no handback (r3 U5.15e)"
    );
    assert_eq!(seen.done(), Some(Outcome::Failed), "r3 U11.3");
    assert_eq!(
        failed_at(&seen),
        [
            (LogEvent::StepFailed, Some(1), Some(Cause::NoDataFile)),
            (LogEvent::Stopped, None, Some(Cause::NoDataFile))
        ],
        "{}",
        seen.stderr
    );
    assert!(seen.effects().is_empty());
    assert_eq!(files.workbook_bytes(), before);
}

/// r3 U11.4: a file run whose data file, whose workbook or whose workbook's folder the system refuses for the
/// permission ends `failed` with `no_access`, each refusal made by a mode: a data file of mode 000 at the read
/// (step 1); a workbook of mode 200 at the prepare (step 2), past the check of `read_only`, which a mode without the
/// owner's write bit meets first; the workbook's folder inside a folder of mode 000 at the read (step 1), where the
/// folder's walk fails and the stat of the folder tells the refusal from a folder that is not there; the workbook's
/// folder of mode 300, searched and written but not read, at the prepare (step 2), where the lock's open of the
/// folder is the first call that the system refuses, before any file is written. Each mode is set back before the
/// asserts.
#[cfg(unix)]
#[test]
fn a_file_or_a_folder_that_the_system_refuses_for_the_permission_ends_the_run_with_no_access() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &std::path::Path, bits: u32| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits)).expect("chmod");
    };
    let no_access = |step: u32| {
        [
            (LogEvent::StepFailed, Some(step), Some(Cause::NoAccess)),
            (LogEvent::Stopped, None, Some(Cause::NoAccess)),
        ]
    };

    let files = FillFiles::new("no-access-source", 1);
    mode(&files.source, 0o000);
    let refused = std::fs::read(&files.source).is_err();
    let seen = fill(67, &files).serve(|_, _| Act::Answer, LONG);
    mode(&files.source, 0o600);
    assert!(refused, "the data file cannot be read");
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(failed_at(&seen), no_access(1), "{}", seen.stderr);
    assert!(seen.effects().is_empty());

    let files = FillFiles::new("no-access-workbook", 1);
    let before = files.workbook_bytes();
    mode(&files.workbook, 0o200);
    let refused = std::fs::File::open(&files.workbook).is_err();
    let seen = fill(68, &files).serve(|_, _| Act::Answer, LONG);
    mode(&files.workbook, 0o600);
    assert!(refused, "the workbook cannot be opened");
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(failed_at(&seen), no_access(2), "{}", seen.stderr);
    assert!(seen.effects().is_empty());
    assert_eq!(files.workbook_bytes(), before);
    assert_eq!(files.names(), ["formulas.xlsx", "source.csv"]);

    let files = FillFiles::new("no-access-folder", 1);
    let shut = files.dir.join("shut");
    let inner = shut.join("inner");
    std::fs::create_dir_all(&inner).expect("make the folders");
    let workbook = inner.join("formulas.xlsx");
    let source = inner.join("source.csv");
    std::fs::rename(&files.workbook, &workbook).expect("move the workbook");
    std::fs::rename(&files.source, &source).expect("move the data file");
    let mut slots = files.slots();
    slots[0] = support::slot("workbook", &workbook.to_string_lossy());
    slots[1] = support::slot("source", &source.to_string_lossy());
    mode(&shut, 0o000);
    let refused = std::fs::metadata(&inner).is_err();
    let sup = Supervisor::new(69, Category::SpreadsheetFill, slots);
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    mode(&shut, 0o700);
    assert!(refused, "the workbook's folder cannot be reached");
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(failed_at(&seen), no_access(1), "{}", seen.stderr);
    assert!(seen.effects().is_empty());

    let files = FillFiles::new("no-access-folder-read", 1);
    let before = files.workbook_bytes();
    mode(&files.dir, 0o300);
    let refused = std::fs::File::open(&files.dir).is_err();
    let seen = fill(70, &files).serve(|_, _| Act::Answer, LONG);
    mode(&files.dir, 0o700);
    assert!(refused, "the workbook's folder cannot be opened");
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(failed_at(&seen), no_access(2), "{}", seen.stderr);
    assert!(seen.effects().is_empty());
    assert_eq!(files.workbook_bytes(), before);
    assert_eq!(files.names(), ["formulas.xlsx", "source.csv"]);
}
