mod support;

use std::time::Duration;

use halvo_schema::executable::frame::Cancel;
use halvo_schema::executable::names::{Autonomy, FieldName, HandbackReason, LogEvent};
use halvo_schema::executable::{
    ActionClass, Cause, EffectState, ExecutableStep, FromRunner, Outcome, ToRunner,
};
use halvo_schema::Category;

use support::{is_effect, is_op, range_texts, store_error, Act, FillFiles, Supervisor, LONG};

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
                "store", "store", "store", "done"
            ],
            "seed {seed}"
        );
        assert_eq!(
            seen.ops(),
            ["load", "log", "load", "effect", "effect", "effect", "log", "finish"],
            "seed {seed}: load, the read's line, load before the propose, three rows, the write's line"
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

#[test]
fn a_read_that_fails_is_handed_back_and_its_wait_is_no_wall_time() {
    let files = FillFiles::new("read-fails", 1);
    std::fs::write(&files.source, "a,b\r\n").expect("a source with a carriage return");
    let before = files.workbook_bytes();
    let sup = fill(43, &files);
    let seen = sup.serve(
        |frame, _| {
            if matches!(frame, FromRunner::Handback(_)) {
                Act::After(
                    Duration::from_millis(1100),
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
    assert_eq!(seen.handbacks(), [HandbackReason::StepFailed]);
    assert_eq!(seen.done(), Some(Outcome::HandedBack));
    let finish = seen.finish().expect("finish");
    assert_eq!((finish.handbacks, finish.prompts, finish.wall_s), (1, 1, 0));
    let events: Vec<(LogEvent, Option<Cause>)> =
        seen.logs().iter().map(|l| (l.event, l.cause)).collect();
    assert_eq!(
        events,
        [
            (LogEvent::StepFailed, Some(Cause::StepFailed)),
            (LogEvent::Handback, None),
            (LogEvent::Stopped, Some(Cause::StepFailed))
        ]
    );
    assert!(seen.effects().is_empty());
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

#[test]
fn a_workbook_changed_under_the_paused_edit_is_not_replaced_and_its_effect_is_handed_back_unknown()
{
    let files = FillFiles::new("changed-under", 1);
    let sup = fill(50, &files);
    let workbook = files.workbook.clone();
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Releasing) {
                let mut bytes = std::fs::read(&workbook).expect("read");
                bytes.push(0);
                std::fs::write(&workbook, &bytes).expect("change the workbook");
            }
            Act::Answer
        },
        LONG,
    );
    assert_eq!(
        seen.states(),
        [EffectState::Cleared, EffectState::Releasing]
    );
    assert_eq!(seen.handbacks(), [HandbackReason::EffectUnknown]);
    assert_eq!(seen.done(), Some(Outcome::EffectUnknown));
    let stopped = seen.logs().into_iter().last().expect("a line");
    assert_eq!(
        (stopped.event, stopped.cause),
        (LogEvent::Stopped, Some(Cause::Unknown)),
        "the stopped line of a run that ends effect_unknown names unknown"
    );
    let finish = seen.finish().expect("finish");
    assert_eq!(
        (finish.effects, finish.unknown, finish.handbacks),
        (1, 1, 1)
    );
    let tags = seen.tags();
    let handback = tags
        .iter()
        .position(|t| *t == "handback")
        .expect("handback");
    let finish_at = seen
        .frames
        .iter()
        .position(|f| is_op(f, "finish"))
        .expect("finish");
    assert!(handback < finish_at, "the handback comes before the finish");
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
    assert_eq!(
        seen.handbacks(),
        [HandbackReason::StepFailed],
        "no browser in this build"
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
        [EffectState::Cleared, EffectState::Releasing]
    );
    assert_eq!(
        files.workbook_bytes(),
        before,
        "the paused edit did not leave"
    );
    assert_eq!(seen.handbacks(), [HandbackReason::EffectUnknown]);
    assert_eq!(seen.done(), Some(Outcome::EffectUnknown));
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
    assert_eq!(
        seen.handbacks(),
        [HandbackReason::StepFailed],
        "the read fails"
    );
    assert_eq!(seen.done(), Some(Outcome::HandedBack));
    assert_eq!(files.workbook_bytes(), before);
    assert_eq!(files.names(), ["formulas.xlsx", "source.csv"]);
}

#[test]
fn a_workbook_slot_that_holds_a_relative_path_is_refused() {
    let files = FillFiles::new("relative", 1);
    let before = files.workbook_bytes();
    let mut slots = files.slots();
    slots[0] = support::slot("workbook", "formulas.xlsx");
    let sup = Supervisor::new(58, Category::SpreadsheetFill, slots);
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(seen.handbacks(), [HandbackReason::StepFailed]);
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
    assert_eq!(seen.handbacks(), [HandbackReason::StepFailed]);
    assert!(seen.effects().is_empty());
    assert_eq!(files.workbook_bytes(), before);
}
