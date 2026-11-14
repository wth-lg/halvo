mod support;

use std::time::Duration;

use halvo_schema::executable::names::{HandbackReason, LogEvent};
use halvo_schema::executable::{Cause, EffectState, ExecutableStep, FromRunner, Outcome, ToRunner};
use halvo_schema::Category;

use support::{
    is_effect, is_op, store_error, store_error_for, stored, Act, FillFiles, Supervisor, LONG,
};

fn fill(n: u8, files: &FillFiles) -> Supervisor {
    Supervisor::new(n, Category::SpreadsheetFill, files.slots())
}

/// D1, the review's R1 and A1, CP-135 (3): a permission `user_files` that the load before the edit's `prepare` no
/// longer holds: no `prepare`, so no checkpoint and no prepared copy, and no effect row.
#[test]
fn a_permission_withdrawn_at_the_load_before_the_prepare_makes_no_effect_row() {
    let files = FillFiles::new("gates-d1", 1);
    let before = files.workbook_bytes();
    let sup = fill(70, &files);
    let mut without = sup.config.clone();
    without.permissions.clear();
    let loads = std::cell::Cell::new(0);
    let seen = sup.serve(
        |frame, _| {
            if !is_op(frame, "load") {
                return Act::Answer;
            }
            loads.set(loads.get() + 1);
            if loads.get() < 2 {
                return Act::Answer;
            }
            let FromRunner::Store(s) = frame else {
                unreachable!()
            };
            let mut reply = sup.store_answer(s.request.body());
            if let halvo_schema::executable::RunnerReply::Load(l) = &mut reply {
                l.config = without.clone();
            }
            Act::With(ToRunner::StoreReply(
                halvo_schema::executable::frame::StoreReplyFrame {
                    reply: halvo_schema::executable::StoreReply {
                        id: Some(s.request.id().clone()),
                        outcome: Ok(reply),
                    },
                },
            ))
        },
        LONG,
    );
    assert_eq!(loads.get(), 2, "the edit asks its second load");
    assert!(seen.effects().is_empty(), "no propose row is sent");
    assert_eq!(seen.done(), Some(Outcome::Failed));
    let causes: Vec<(LogEvent, Option<Cause>)> =
        seen.logs().iter().map(|l| (l.event, l.cause)).collect();
    assert_eq!(
        causes[1..],
        [
            (LogEvent::StepFailed, Some(Cause::StepFailed)),
            (LogEvent::Stopped, Some(Cause::StepFailed))
        ],
        "as a run whose first load lacks the permission ends its write step"
    );
    assert_eq!(
        files.workbook_bytes(),
        before,
        "the workbook byte for byte as before"
    );
    assert_eq!(
        files.names(),
        ["formulas.xlsx".to_string(), "source.csv".to_string()],
        "no checkpoint and no prepared copy"
    );
}

/// D3, the review's B-3, the verifier's v19: a row of an effect counts as stored only on {"stored":1}.
#[test]
fn an_effect_row_answered_with_another_stored_count_ends_the_run_with_no_later_row() {
    for (state, count) in [
        (EffectState::Cleared, 0),
        (EffectState::Cleared, 2),
        (EffectState::Releasing, 0),
        (EffectState::Releasing, 2),
    ] {
        let what = format!("{state:?} answered with stored {count}");
        let files = FillFiles::new("gates-d3", 2);
        let before = files.workbook_bytes();
        let sup = fill(71, &files);
        let seen = sup.serve(
            |frame, _| {
                if is_effect(frame, state) {
                    Act::With(stored(frame, count))
                } else {
                    Act::Answer
                }
            },
            LONG,
        );
        let states = seen.states();
        assert_eq!(
            states.last(),
            Some(&state),
            "{what}: no later row of the effect"
        );
        assert_eq!(seen.done(), Some(Outcome::Failed), "{what}");
        assert_eq!(
            seen.logs().last().map(|l| (l.event, l.cause)),
            Some((LogEvent::Stopped, Some(Cause::Effect))),
            "{what}"
        );
        assert!(seen.handbacks().is_empty(), "{what}: no handback");
        assert_eq!(files.workbook_bytes(), before, "{what}: no replace");
    }
}

/// D4, the review's B-4, the verifier's v23: the wall check between the propose and the begin.
#[test]
fn a_wall_limit_passed_while_the_propose_waits_ends_the_run_before_the_begin() {
    let files = FillFiles::new("gates-d4", 3);
    let before = files.workbook_bytes();
    let mut sup = fill(72, &files);
    sup.config.limits.wall_s = 1;
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Cleared) {
                Act::After(Duration::from_millis(2100), Box::new(Act::Answer))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(seen.states(), [EffectState::Cleared], "no releasing row");
    assert_eq!(seen.done(), Some(Outcome::Limit));
    assert_eq!(
        seen.logs().last().map(|l| (l.event, l.cause)),
        Some((LogEvent::Stopped, Some(Cause::Wall)))
    );
    assert_eq!(files.workbook_bytes(), before);
}

/// D5, the review's B-5: a finish answered with {"stored":0}.
#[test]
fn a_finish_answered_with_stored_0_ends_with_the_status_1_and_finish_no() {
    let files = FillFiles::new("gates-d5", 1);
    let sup = fill(73, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_op(frame, "finish") {
                Act::With(stored(frame, 0))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Completed));
    assert_eq!(seen.code(), Some(1));
    assert_eq!(
        seen.stderr.lines().last(),
        Some("run outcome=completed steps=2 finish=no"),
        "{}",
        seen.stderr
    );
}

/// D6, the review's B-6, the verifier's v07: an error reply that names another request.
#[test]
fn an_error_reply_of_another_request_is_refused_with_not_the_answer() {
    let files = FillFiles::new("gates-d6", 1);
    let sup = fill(74, &files);
    let other = "f".repeat(32);
    let seen = sup.serve(
        |frame, _| {
            if is_op(frame, "log") {
                Act::With(store_error_for(&other, "store"))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(seen.code(), Some(2));
    assert!(
        seen.stderr.starts_with("REFUSED frame: not_the_answer"),
        "{}",
        seen.stderr
    );
    assert!(seen.effects().is_empty());
}

/// D7, the review's B-7, the verifier's v14: the store's refusal `class`.
#[test]
fn a_body_refused_with_class_ends_the_run_with_the_cause_class() {
    let files = FillFiles::new("gates-d7", 2);
    let sup = fill(75, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Cleared) {
                Act::With(store_error(frame, "class"))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Failed));
    assert_eq!(seen.finish().map(|f| f.outcome), Some(Outcome::Failed));
    assert_eq!(
        seen.logs().last().map(|l| (l.event, l.cause)),
        Some((LogEvent::Stopped, Some(Cause::Class))),
        "the stopped line names class"
    );
}

fn step(json: &str) -> ExecutableStep {
    ExecutableStep::parse(json.as_bytes()).expect("a step")
}

const CLICK: &str = r#"{"v":1,"n":N,"kind":"web","verb":"click","target":{"desc":"d","frame":[],"credential":"no","locators":[{"by":"text","value":"v","name":null}]},"args":[],"pre":[],"post":[],"effect":"none","class":"prepare"}"#;

/// D9 (b), the verifier's v06: a failed step of the class `prepare` is handed back with `step_failed`.
#[test]
fn a_failed_prepare_step_is_handed_back_with_step_failed() {
    let mut sup = Supervisor::new(76, Category::UtilityBillPayment, Vec::new());
    sup.steps = vec![step(&CLICK.replace(r#""n":N"#, r#""n":1"#))];
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(seen.handbacks(), [HandbackReason::StepFailed]);
    assert_eq!(seen.done(), Some(Outcome::HandedBack));
}

/// D9 (d), the verifier's v31: a page of steps beyond the playbook's count.
#[test]
fn a_page_with_more_steps_than_the_playbooks_count_is_refused_with_steps_count() {
    let mut sup = Supervisor::new(77, Category::UtilityBillPayment, Vec::new());
    sup.steps = (1..=2)
        .map(|n| step(&CLICK.replace(r#""n":N"#, &format!(r#""n":{n}"#))))
        .collect();
    let seen = sup.serve(
        |frame, _| {
            let FromRunner::Store(s) = frame else {
                return Act::Answer;
            };
            if s.request.body().op() != "load" {
                return Act::Answer;
            }
            let mut reply = sup.store_answer(s.request.body());
            if let halvo_schema::executable::RunnerReply::Load(l) = &mut reply {
                if let Some(p) = &mut l.playbook {
                    p.steps = std::num::NonZeroU32::MIN;
                }
            }
            Act::With(ToRunner::StoreReply(
                halvo_schema::executable::frame::StoreReplyFrame {
                    reply: halvo_schema::executable::StoreReply {
                        id: Some(s.request.id().clone()),
                        outcome: Ok(reply),
                    },
                },
            ))
        },
        LONG,
    );
    assert_eq!(seen.code(), Some(2));
    assert!(
        seen.stderr.starts_with("REFUSED frame: steps_count"),
        "{}",
        seen.stderr
    );
    assert!(!seen.tags().contains(&"progress"));
}
