mod support;

use std::path::Path;

use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::frame::{Cancel, SecretReply, StoreReply, StoreReplyFrame};
use halvo_schema::executable::names::{
    Autonomy, FieldName, HandbackOutcome, HandbackReason, LogEvent, Runtime, Source,
};
use halvo_schema::executable::text::{OwnValue, SecretText};
use halvo_schema::executable::{
    ActionClass, AgentEffect, Bound, BoundField, Cause, EffectState, ExecutableStep, FromRunner,
    Outcome, RunnerReply, ToRunner, WebOrigin,
};
use halvo_schema::Category;

use support::{
    handback_reply, is_effect, is_op, scratch, slot, Act, FillFiles, Seen, Supervisor, LONG,
};

fn fill(n: u8, files: &FillFiles) -> Supervisor {
    Supervisor::new(n, Category::SpreadsheetFill, files.slots())
}

/// CP-135 (5), A7.4: the handler of `start_run` stores the row 1 of the run's log, so the runner's lines count
/// from 2.
#[test]
fn the_runners_log_lines_count_from_2() {
    let files = FillFiles::new("log-from-2", 1);
    let sup = fill(140, &files);
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    let seqs: Vec<u32> = seen.logs().iter().map(|l| l.seq.get()).collect();
    assert_eq!(seqs, [2, 3], "the read's line, then the write's line");
}

/// The load reply that the supervisor gives with `change` applied to its configuration and slots.
fn load_with(sup: &Supervisor, frame: &FromRunner, change: impl FnOnce(&mut LoadReply)) -> Act {
    let FromRunner::Store(s) = frame else {
        unreachable!("a load is a store frame")
    };
    let mut reply = sup.store_answer(s.request.body());
    if let RunnerReply::Load(l) = &mut reply {
        change(l);
    }
    Act::With(ToRunner::StoreReply(StoreReplyFrame {
        reply: StoreReply {
            id: Some(s.request.id().clone()),
            outcome: Ok(reply),
        },
    }))
}

/// A6.5, CP-135 (3) (iv): a run whose time is at the `wall_s` of its configuration ends with `limit` and `wall`;
/// whole seconds are not rounded up, so a limit of 0 ends the run at its first read.
#[test]
fn a_run_at_exactly_its_wall_s_ends_with_limit_wall() {
    let files = FillFiles::new("wall-exact", 1);
    let before = files.workbook_bytes();
    let mut sup = fill(141, &files);
    sup.config.limits.wall_s = 0;
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(seen.done(), Some(Outcome::Limit), "{}", seen.stderr);
    assert_eq!(
        seen.logs().last().map(|l| (l.event, l.cause)),
        Some((LogEvent::Stopped, Some(Cause::Wall)))
    );
    assert!(!seen.tags().contains(&"progress"), "no step began");
    assert_eq!(files.workbook_bytes(), before);
}

/// A6.5: the limit `wall` is read against the `wall_s` of the newest load; a load before the edit that lowers it
/// ends the run with `limit` and `wall` at the next read, before any effect row.
#[test]
fn a_wall_s_lowered_at_the_edits_load_ends_the_run_at_the_next_read() {
    let files = FillFiles::new("wall-lowered", 2);
    let before = files.workbook_bytes();
    let sup = fill(142, &files);
    let mut loads = 0;
    let seen = sup.serve(
        |frame, _| {
            if !is_op(frame, "load") {
                return Act::Answer;
            }
            loads += 1;
            if loads < 2 {
                return Act::Answer;
            }
            load_with(&sup, frame, |l| l.config.limits.wall_s = 0)
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Limit), "{}", seen.stderr);
    assert_eq!(
        seen.logs().last().map(|l| (l.event, l.cause)),
        Some((LogEvent::Stopped, Some(Cause::Wall)))
    );
    assert!(seen.effects().is_empty(), "no effect row");
    assert_eq!(files.workbook_bytes(), before);
}

/// Serves a fill run in which `change` may alter the reply to each load, counted from 1 (the run's first load), and
/// `at_load` runs before that load is answered.
fn serve_loads(
    sup: &Supervisor,
    mut change: impl FnMut(u32, &mut LoadReply),
    mut at_load: impl FnMut(u32),
) -> Seen {
    let mut loads = 0;
    sup.serve(
        |frame, _| {
            if !is_op(frame, "load") {
                return Act::Answer;
            }
            loads += 1;
            at_load(loads);
            load_with(sup, frame, |l| change(loads, l))
        },
        LONG,
    )
}

fn set_slot(l: &mut LoadReply, name: &str, value: &str) {
    let bindings = l.bindings.as_mut().expect("bindings");
    let at = bindings
        .slots
        .iter()
        .position(|s| s.name.as_str() == name)
        .expect("the slot");
    bindings.slots[at] = slot(name, value);
}

fn checkpoint_of(sup: &Supervisor, stem: &str) -> String {
    format!("{stem}.halvo-checkpoint-{}.xlsx", sup.run.as_str())
}

/// The bound object of a file edit of `file` and `range`, as the runner builds it from the slots.
fn edit_bound(file: &Path, range: &str) -> Bound {
    Bound::new(
        ActionClass::LocalEdit,
        [
            (FieldName::File, file.to_string_lossy().into_owned()),
            (FieldName::Range, range.to_string()),
        ]
        .into_iter()
        .map(|(name, value)| BoundField {
            name,
            from: Source::Slot,
            origin: None,
            value: OwnValue::new(&value),
        })
        .collect(),
    )
    .expect("a bound object")
}

fn kept(effect: &AgentEffect) -> Vec<(FieldName, String)> {
    effect
        .kept
        .iter()
        .map(|k| (k.name, k.value.joined()))
        .collect()
}

fn stopped(seen: &Seen) -> Option<(LogEvent, Option<Cause>)> {
    seen.logs().last().map(|l| (l.event, l.cause))
}

/// The review's R2, CP-135 (3) (i): the slot `workbook` names another file of the same folder at the load before
/// the edit than at the run's first load; the edit is prepared for the file of the edit's load, and no byte is
/// written beside the first file.
#[test]
fn a_workbook_slot_changed_after_the_first_load_edits_the_file_of_the_edits_load() {
    let files = FillFiles::new("two-loads-r2", 1);
    let other = files.dir.join("other.xlsx");
    std::fs::copy(&files.workbook, &other).expect("a second workbook");
    let before = files.workbook_bytes();
    let other_before = std::fs::read(&other).expect("read");
    let sup = fill(143, &files);
    let other_text = other.to_string_lossy().into_owned();
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 2 {
                set_slot(l, "workbook", &other_text);
            }
        },
        |_| {},
    );
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    assert_eq!(
        files.workbook_bytes(),
        before,
        "the first file is untouched"
    );
    assert!(
        !files.names().contains(&checkpoint_of(&sup, "formulas")),
        "no checkpoint beside the first file: {:?}",
        files.names()
    );
    assert_ne!(std::fs::read(&other).expect("read"), other_before);
    assert!(files.names().contains(&checkpoint_of(&sup, "other")));
    let effects = seen.effects();
    assert_eq!(
        kept(&effects[0]),
        [
            (FieldName::File, other_text.clone()),
            (FieldName::Range, "B2:D6".to_string())
        ]
    );
}

/// CP-135 (3) (ii), (iii), A11.3: the range changed between the `propose` and the `begin`: no `begin` row and no
/// `replace` for the first effect, which is ended with the cause `ended`; a new effect is proposed from that load,
/// with its own `prepare`, and goes on when the next load gives the same values.
#[test]
fn a_range_changed_between_the_propose_and_the_begin_ends_the_effect_and_proposes_again() {
    let files = FillFiles::new("two-loads-range", 2);
    let sup = fill(144, &files);
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                set_slot(l, "range", "B3:D7");
            }
        },
        |_| {},
    );
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    let effects = seen.effects();
    assert_eq!(
        seen.states(),
        [
            EffectState::Cleared,
            EffectState::Rejected,
            EffectState::Cleared,
            EffectState::Releasing,
            EffectState::Committed
        ]
    );
    assert_eq!(effects[1].cause, Some(Cause::Ended), "the ended row");
    assert_eq!(effects[0].effect, effects[1].effect);
    assert_ne!(effects[1].effect, effects[2].effect, "a new effect");
    assert_eq!(
        effects[2].digest,
        halvo_policy::digest(&edit_bound(&files.workbook, "B3:D7")).expect("a digest")
    );
    assert_eq!(effects[3].digest, effects[2].digest);
}

/// CP-135 (3) (iii), A5.2: at most `LIMITS.rebinds` (2) new effects for one step; a range that changes at every
/// load before the `begin` ends the run `failed` with the cause `changed` after the third effect is ended, with
/// no `begin`, the workbook byte for byte and no prepared copy left.
#[test]
fn a_range_that_changes_at_every_load_ends_the_run_failed_changed_after_two_new_effects() {
    let files = FillFiles::new("two-loads-limit", 3);
    let before = files.workbook_bytes();
    let sup = fill(145, &files);
    let ranges = ["B2:D6", "B2:D6", "B3:D7", "B4:D8", "B5:D9", "B6:D10"];
    let seen = serve_loads(
        &sup,
        |n, l| set_slot(l, "range", ranges[(n as usize - 1).min(ranges.len() - 1)]),
        |_| {},
    );
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::Changed)))
    );
    assert_eq!(
        seen.states(),
        [
            EffectState::Cleared,
            EffectState::Rejected,
            EffectState::Cleared,
            EffectState::Rejected,
            EffectState::Cleared,
            EffectState::Rejected
        ]
    );
    assert_eq!(files.workbook_bytes(), before);
    assert_eq!(
        files.names(),
        [
            checkpoint_of(&sup, "formulas"),
            "formulas.xlsx".to_string(),
            "source.csv".to_string()
        ],
        "no prepared copy is left"
    );
}

/// CP-135 (3) (ii), CP-110 D1: the permission `user_files` withdrawn between the `propose` and the `begin`: no
/// `begin`, the effect ended, the workbook byte for byte, the prepared copy gone, and the run ends as a run whose
/// first load lacks the permission.
#[test]
fn user_files_withdrawn_between_the_propose_and_the_begin_ends_the_effect() {
    let files = FillFiles::new("two-loads-perm", 1);
    let before = files.workbook_bytes();
    let sup = fill(146, &files);
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                l.config.permissions.clear();
            }
        },
        |_| {},
    );
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::StepFailed)))
    );
    assert_eq!(seen.states(), [EffectState::Cleared, EffectState::Rejected]);
    assert_eq!(seen.effects()[1].cause, Some(Cause::Ended));
    assert_eq!(files.workbook_bytes(), before);
    assert_eq!(
        files.names(),
        [
            checkpoint_of(&sup, "formulas"),
            "formulas.xlsx".to_string(),
            "source.csv".to_string()
        ],
        "the prepared copy is gone"
    );
}

/// CP-135 (3) (ii), A5.1: the autonomy set to `observe` between the `propose` and the `begin`: `decide` on the
/// later load gives `refuse`, the effect is ended and the run ends `refused` with the cause `autonomy`.
#[test]
fn autonomy_observe_between_the_propose_and_the_begin_ends_the_run_refused() {
    let files = FillFiles::new("two-loads-observe", 2);
    let before = files.workbook_bytes();
    let sup = fill(147, &files);
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                l.config.autonomy = Autonomy::Observe;
            }
        },
        |_| {},
    );
    assert_eq!(seen.done(), Some(Outcome::Refused), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::Autonomy)))
    );
    assert_eq!(seen.states(), [EffectState::Cleared, EffectState::Rejected]);
    assert_eq!(files.workbook_bytes(), before);
}

/// CP-135 (3) (iv), A6.5: `wall_s` lowered between the load before the `prepare` and the load before the `begin`
/// ends the run with `limit` and `wall` at the next read, with no `begin`.
#[test]
fn a_wall_s_lowered_between_the_two_loads_ends_the_run_before_the_begin() {
    let files = FillFiles::new("two-loads-wall", 3);
    let before = files.workbook_bytes();
    let sup = fill(148, &files);
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                l.config.limits.wall_s = 0;
            }
        },
        |_| {},
    );
    assert_eq!(seen.done(), Some(Outcome::Limit), "{}", seen.stderr);
    assert_eq!(stopped(&seen), Some((LogEvent::Stopped, Some(Cause::Wall))));
    assert_eq!(seen.states(), [EffectState::Cleared], "no begin");
    assert_eq!(files.workbook_bytes(), before);
}

/// CP-135 (3), A11.3: the `begin` row carries the digest of the bound object built again from the load before the
/// `begin`, here a range that both loads of the edit give and the run's first load does not.
#[test]
fn the_begin_row_carries_the_digest_of_the_object_built_from_the_second_load() {
    let files = FillFiles::new("two-loads-digest", 1);
    let sup = fill(149, &files);
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 2 {
                set_slot(l, "range", "B3:D7");
            }
        },
        |_| {},
    );
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    let begin = seen
        .effects()
        .into_iter()
        .find(|e| e.state == EffectState::Releasing)
        .expect("a begin row");
    assert_eq!(
        begin.digest,
        halvo_policy::digest(&edit_bound(&files.workbook, "B3:D7")).expect("a digest")
    );
}

/// CP-135 (3) (i), (v), A9.4: a load before the edit that gives another source or another folder than the one
/// under which the read step read ends the run `failed` with the cause `changed`, and nothing is written.
#[test]
fn another_source_or_folder_than_the_read_steps_ends_the_run_failed_changed() {
    for what in ["source", "folder"] {
        let files = FillFiles::new("two-loads-moved", 2);
        let before = files.workbook_bytes();
        let names = files.names();
        let sub = files.dir.join("sub");
        std::fs::create_dir_all(&sub).expect("a second folder");
        let moved = if what == "source" {
            let path = files.dir.join("other.csv");
            std::fs::copy(&files.source, &path).expect("a second source");
            ("source", path)
        } else {
            let path = sub.join("formulas.xlsx");
            std::fs::copy(&files.workbook, &path).expect("a second workbook");
            ("workbook", path)
        };
        let moved_before = std::fs::read(&moved.1).expect("read");
        let sup = fill(150, &files);
        let text = moved.1.to_string_lossy().into_owned();
        let seen = serve_loads(
            &sup,
            |n, l| {
                if n >= 2 {
                    set_slot(l, moved.0, &text);
                }
            },
            |_| {},
        );
        assert_eq!(
            seen.done(),
            Some(Outcome::Failed),
            "{what}: {}",
            seen.stderr
        );
        assert_eq!(
            stopped(&seen),
            Some((LogEvent::Stopped, Some(Cause::Changed))),
            "{what}"
        );
        assert!(seen.effects().is_empty(), "{what}: no effect row");
        assert_eq!(files.workbook_bytes(), before, "{what}");
        assert_eq!(
            std::fs::read(&moved.1).expect("read"),
            moved_before,
            "{what}"
        );
        let mut now = files.names();
        now.retain(|n| n != "sub" && n != "other.csv");
        assert_eq!(now, names, "{what}: nothing written beside the workbook");
        assert_eq!(
            std::fs::read_dir(&sub).expect("list").count(),
            usize::from(what == "folder"),
            "{what}: nothing written in the second folder"
        );
    }
}

/// CP-139 (b), A11.3: a later `prepare` of the run, for the new effect after a changed range, finds the run's
/// checkpoint not equal to the workbook, because the person changed the workbook between the two calls: the step
/// ends with `step_failed`, nothing is written, the workbook stays the person's and the checkpoint stays.
#[test]
fn a_later_prepare_that_finds_the_checkpoint_not_the_workbook_ends_the_step_failed() {
    let files = FillFiles::new("two-loads-person", 2);
    let original = files.workbook_bytes();
    let sup = fill(151, &files);
    let mut persons = None;
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                set_slot(l, "range", "B3:D7");
            }
        },
        |n| {
            if n == 3 {
                persons = Some(person_changes(&files));
            }
        },
    );
    let persons = persons.expect("the person changed the workbook");
    assert_ne!(persons, original);
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::StepFailed)))
    );
    assert_eq!(
        seen.states(),
        [EffectState::Cleared, EffectState::Rejected],
        "no second propose"
    );
    assert_eq!(files.workbook_bytes(), persons, "the person's workbook");
    assert_eq!(
        std::fs::read(files.dir.join(checkpoint_of(&sup, "formulas"))).expect("the checkpoint"),
        original,
        "the checkpoint as it was made"
    );
    assert!(
        !files.names().iter().any(|n| n.contains(".halvo-prepared-")),
        "no prepared copy: {:?}",
        files.names()
    );
}

/// The person's own change of the workbook: one cell outside the fill range, written through the sheet executor
/// under a data directory and a run of the test's own; the workbook's bytes after it.
fn person_changes(files: &FillFiles) -> Vec<u8> {
    let data = scratch("person-data");
    let locks = halvo_sheet::Locks::open(&data, LONG).expect("locks");
    let folder = halvo_sheet::Folder::new(&files.dir, locks).expect("a folder");
    let run = halvo_sheet::RunId::parse(&"5e".repeat(16)).expect("a run id");
    let range = halvo_sheet::Range::parse("F9:F9").expect("a range");
    let values = vec![vec![halvo_sheet::Value::Text(
        "the person's note".to_string(),
    )]];
    let prepared = folder
        .prepare(&files.workbook, &run, "Ledger", range, &values)
        .expect("prepare");
    folder.replace(&prepared).expect("replace");
    files.workbook_bytes()
}

fn api_agent(n: u8) -> Supervisor {
    let mut sup = Supervisor::new(n, Category::Email, Vec::new());
    sup.origins = vec![WebOrigin::parse("http://mail.agent.test:8080").expect("an origin")];
    sup
}

fn cancel() -> ToRunner {
    ToRunner::Cancel(Cancel {})
}

fn stored_token() -> SecretReply {
    SecretReply::Stored(SecretText::parse("Zq7-halvo-marker-token").expect("a token"))
}

/// CP-135 (4), A6.2, the review's N-4: a cancel that comes while the runner waits for the result of an
/// `account_link` handback is read when the result comes, and no `secret` frame follows; the run ends `declined`.
#[test]
fn a_cancel_during_the_wait_for_an_account_link_result_is_read_when_it_comes() {
    let sup = api_agent(152);
    let seen = sup.serve(
        |frame, _| match frame {
            FromRunner::Secret(_) => Act::With(ToRunner::SecretReply(SecretReply::None)),
            FromRunner::Handback(_) => Act::Then(
                cancel(),
                Box::new(Act::With(handback_reply(
                    HandbackOutcome::Done,
                    Some("fixture_token"),
                ))),
            ),
            _ => Act::Answer,
        },
        LONG,
    );
    let after: Vec<&str> = seen
        .tags()
        .into_iter()
        .skip_while(|t| *t != "handback")
        .collect();
    assert!(
        !after.contains(&"secret"),
        "no secret frame after the handback: {after:?}"
    );
    assert_eq!(seen.handbacks(), [HandbackReason::AccountLink]);
    assert_eq!(seen.done(), Some(Outcome::Declined), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::Declined)))
    );
}

/// CP-135 (4), A6.2: a cancel that comes while the runner waits for a `secret_reply` is read when the reply
/// comes, and the run ends `declined` before any call.
#[test]
fn a_cancel_during_the_wait_for_a_secret_reply_is_read_when_it_comes() {
    let sup = api_agent(153);
    let seen = sup.serve(
        |frame, _| match frame {
            FromRunner::Secret(_) => Act::Then(
                cancel(),
                Box::new(Act::With(ToRunner::SecretReply(stored_token()))),
            ),
            _ => Act::Answer,
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Declined), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::Declined)))
    );
    assert!(seen.handbacks().is_empty());
}

/// CP-135 (4), A6.5: a cancel that the runner holds once it has taken the answer to the edit's `begin`, and before
/// its `replace`, leaves the workbook byte for byte, and the run ends `effect_unknown` after the handback
/// `effect_unknown`, with the effect `releasing` at the `finish`, which the store's handler of the run's end makes
/// `unknown`. The supervisor writes the cancel before the answer, so that the runner holds it when it takes the
/// answer; a cancel that comes after the act is reading 1's limit, and the lock's own read of a held cancel is the
/// link's test `a_cancel_that_the_link_holds_lets_no_effect_act`.
#[test]
fn a_cancel_after_the_answer_to_begin_leaves_the_workbook_and_ends_effect_unknown() {
    let files = FillFiles::new("cancel-after-begin", 1);
    let before = files.workbook_bytes();
    let sup = fill(154, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Releasing) {
                let reply = sup.answer(frame).expect("the begin's answer");
                Act::Then(cancel(), Box::new(Act::With(reply)))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(files.workbook_bytes(), before, "no replace");
    assert_eq!(
        seen.states(),
        [EffectState::Cleared, EffectState::Releasing],
        "no commit"
    );
    assert_eq!(seen.handbacks(), [HandbackReason::EffectUnknown]);
    assert_eq!(seen.done(), Some(Outcome::EffectUnknown), "{}", seen.stderr);
    assert_eq!(seen.finish().map(|f| f.unknown), Some(1));
}

/// A6.5, CP-135 (4): a cancel before any `begin`, here after the answer to the edit's `propose`, ends the run
/// `declined` with no `begin` row.
#[test]
fn a_cancel_before_any_begin_ends_the_run_declined() {
    let files = FillFiles::new("cancel-before-begin", 2);
    let before = files.workbook_bytes();
    let sup = fill(155, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_effect(frame, EffectState::Cleared) {
                let reply = sup.answer(frame).expect("the propose's answer");
                Act::Then(cancel(), Box::new(Act::With(reply)))
            } else {
                Act::Answer
            }
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Declined), "{}", seen.stderr);
    assert_eq!(seen.states(), [EffectState::Cleared]);
    assert_eq!(files.workbook_bytes(), before);
}

/// CP-142 (3): the confirm path's request maker of the suite is test code alone; the binary still fails a `web`
/// step with `step_failed` and an `api` call with the cause `adapter`, with no effect row and no confirmation.
#[test]
fn the_binary_still_fails_a_web_step_and_an_api_call_and_asks_no_confirmation() {
    let click = r#"{"v":1,"n":1,"kind":"web","verb":"click","target":{"desc":"d","frame":[],"credential":"no","locators":[{"by":"text","value":"v","name":null}]},"args":[],"pre":[],"post":[],"effect":"none","class":"prepare"}"#;
    let mut web = Supervisor::new(156, Category::UtilityBillPayment, Vec::new());
    web.steps = vec![ExecutableStep::parse(click.as_bytes()).expect("a step")];
    let seen = web.serve(|_, _| Act::Answer, LONG);
    let failed: Vec<(LogEvent, Option<Cause>)> = seen
        .logs()
        .iter()
        .map(|l| (l.event, l.cause))
        .filter(|(e, _)| *e == LogEvent::StepFailed)
        .collect();
    assert_eq!(failed, [(LogEvent::StepFailed, Some(Cause::StepFailed))]);
    assert!(seen.effects().is_empty() && !seen.tags().contains(&"confirm"));

    let api = api_agent(157);
    let seen = api.serve(
        |frame, _| match frame {
            FromRunner::Secret(_) => Act::With(ToRunner::SecretReply(stored_token())),
            _ => Act::Answer,
        },
        LONG,
    );
    let failed: Vec<Option<Cause>> = seen
        .logs()
        .iter()
        .filter(|l| l.event == LogEvent::StepFailed)
        .map(|l| l.cause)
        .collect();
    assert_eq!(failed, [Some(Cause::Adapter)]);
    assert!(seen.effects().is_empty() && !seen.tags().contains(&"confirm"));
}

/// The cells of `range` of the sheet `sheet` of `workbook`, each written as text, read under a data directory of
/// the test's own.
fn sheet_texts(dir: &Path, workbook: &Path, sheet: &str, range: &str) -> Vec<Vec<String>> {
    let data = scratch("read-sheet-data");
    let locks = halvo_sheet::Locks::open(&data, LONG).expect("locks");
    let folder = halvo_sheet::Folder::new(dir, locks).expect("a folder");
    folder
        .read(
            workbook,
            sheet,
            halvo_sheet::Range::parse(range).expect("a range"),
        )
        .expect("read the range")
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|cell| match cell {
                    halvo_sheet::Cell::Text(t) => t,
                    halvo_sheet::Cell::Number(n) => format!("{n:.2}"),
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .collect()
}

/// The ruling CP-159 G2: the slot `sheet` names another sheet at the load before the `begin` than at the load
/// before the `prepare`. No cell of the first sheet is written; the first effect is ended, and a new effect with its
/// own `prepare` and rows writes the second sheet.
#[test]
fn a_sheet_changed_at_the_load_before_the_begin_is_written_by_a_new_effect() {
    let files = FillFiles::new("sheet-changed", 1);
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/r3-trials/sheet/corpus/two-sheets.xlsx");
    std::fs::copy(corpus, &files.workbook).expect("a workbook of two sheets");
    let input_before = sheet_texts(&files.dir, &files.workbook, "Input", "B2:D6");
    let mut sup = fill(160, &files);
    let at = sup
        .slots
        .iter()
        .position(|s| s.name.as_str() == "sheet")
        .expect("the slot sheet");
    sup.slots[at] = slot("sheet", "Input");
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                set_slot(l, "sheet", "Summary");
            }
        },
        |_| {},
    );
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    let effects = seen.effects();
    assert_eq!(
        seen.states(),
        [
            EffectState::Cleared,
            EffectState::Rejected,
            EffectState::Cleared,
            EffectState::Releasing,
            EffectState::Committed
        ]
    );
    assert_eq!(
        effects[1].cause,
        Some(Cause::Ended),
        "the first effect's ended row"
    );
    assert_ne!(effects[1].effect, effects[2].effect, "a new effect");
    assert_eq!(
        sheet_texts(&files.dir, &files.workbook, "Input", "B2:D6"),
        input_before,
        "no cell of the first sheet is written"
    );
    assert_eq!(
        sheet_texts(&files.dir, &files.workbook, "Summary", "B2:D6"),
        files.rows,
        "the second sheet holds the rows"
    );
}

/// The ruling CP-159 G3: the slot `source` names another file at the load before the `begin` than the one under
/// which the read step read. No `begin` and no `replace`; the effect is ended with its `ended` row, and the run ends
/// `failed` with `changed`; the workbook is byte for byte as it was and no prepared copy is left. The checkpoint
/// that the `prepare` made stays (A11.3: the runner removes no checkpoint).
#[test]
fn a_source_changed_at_the_load_before_the_begin_ends_the_effect_and_the_run_changed() {
    let files = FillFiles::new("source-changed", 2);
    let before = files.workbook_bytes();
    let other = files.dir.join("other.csv");
    std::fs::copy(&files.source, &other).expect("a second source");
    let sup = fill(161, &files);
    let other_text = other.to_string_lossy().into_owned();
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                set_slot(l, "source", &other_text);
            }
        },
        |_| {},
    );
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::Changed)))
    );
    assert_eq!(
        seen.states(),
        [EffectState::Cleared, EffectState::Rejected],
        "no begin; the effect's ended row"
    );
    assert_eq!(seen.effects()[1].cause, Some(Cause::Ended));
    assert_eq!(files.workbook_bytes(), before, "the workbook byte for byte");
    assert_eq!(
        files.names(),
        [
            checkpoint_of(&sup, "formulas"),
            "formulas.xlsx".to_string(),
            "other.csv".to_string(),
            "source.csv".to_string()
        ],
        "no prepared copy is left"
    );
}

/// CP-166 E4 (a), the verifier's v5: no permission and another source at the load before the `begin` end as no
/// permission alone does, after the effect's `ended` row: no permission comes first.
#[test]
fn no_permission_and_another_source_end_as_no_permission_alone() {
    let files = FillFiles::new("order-perm-source", 1);
    let before = files.workbook_bytes();
    let other = files.dir.join("other.csv");
    std::fs::copy(&files.source, &other).expect("a second source");
    let sup = fill(162, &files);
    let other_text = other.to_string_lossy().into_owned();
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                l.config.permissions.clear();
                set_slot(l, "source", &other_text);
            }
        },
        |_| {},
    );
    assert_eq!(seen.states(), [EffectState::Cleared, EffectState::Rejected]);
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::StepFailed)))
    );
    assert_eq!(files.workbook_bytes(), before);
}

/// CP-166 E4 (a), the verifier's v6: `observe` and another source at the load before the `begin` end `refused`
/// with `autonomy`: the decision comes before another source.
#[test]
fn observe_and_another_source_end_refused_with_autonomy() {
    let files = FillFiles::new("order-observe-source", 2);
    let before = files.workbook_bytes();
    let other = files.dir.join("other.csv");
    std::fs::copy(&files.source, &other).expect("a second source");
    let sup = fill(163, &files);
    let other_text = other.to_string_lossy().into_owned();
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                l.config.autonomy = Autonomy::Observe;
                set_slot(l, "source", &other_text);
            }
        },
        |_| {},
    );
    assert_eq!(seen.states(), [EffectState::Cleared, EffectState::Rejected]);
    assert_eq!(seen.done(), Some(Outcome::Refused), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::Autonomy)))
    );
    assert_eq!(files.workbook_bytes(), before);
}

/// CP-166 E4 (b), the verifier's v8: the slot of the source holds a text that is no absolute path at the load
/// before the `begin`; that is another source, and the run ends `failed` with `changed`, not with `step_failed`.
#[test]
fn a_source_that_is_no_absolute_path_at_the_load_before_the_begin_ends_changed() {
    let files = FillFiles::new("source-relative", 3);
    let before = files.workbook_bytes();
    let sup = fill(164, &files);
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                set_slot(l, "source", "source.csv");
            }
        },
        |_| {},
    );
    assert_eq!(seen.states(), [EffectState::Cleared, EffectState::Rejected]);
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::Changed)))
    );
    assert_eq!(files.workbook_bytes(), before);
}

/// CP-166 E4 (c), the review's item 3: the read step's source is held as a text, so the same file under another
/// spelling at the load before the `begin` is another source and the run ends `failed` with `changed`.
#[test]
fn the_same_source_under_another_spelling_ends_changed() {
    let files = FillFiles::new("source-spelling", 1);
    let before = files.workbook_bytes();
    let sup = fill(165, &files);
    let spelled = files.dir.join(".").join("source.csv");
    let spelled_text = spelled.to_string_lossy().into_owned();
    assert_ne!(
        spelled_text,
        files.source.to_string_lossy(),
        "another spelling"
    );
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                set_slot(l, "source", &spelled_text);
            }
        },
        |_| {},
    );
    assert_eq!(seen.states(), [EffectState::Cleared, EffectState::Rejected]);
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::Changed)))
    );
    assert_eq!(files.workbook_bytes(), before);
}

/// Serves a fill run whose slot `source` holds `text` from the second load on, the load before the `prepare`; the
/// run and whether the workbook stayed byte for byte as it was.
fn source_at_the_edits_load(tag: &str, n: u8, text: impl Fn(&FillFiles) -> String) -> (Seen, bool) {
    let files = FillFiles::new(tag, 1);
    let before = files.workbook_bytes();
    let sup = fill(n, &files);
    let value = text(&files);
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 2 {
                set_slot(l, "source", &value);
            }
        },
        |_| {},
    );
    let same = files.workbook_bytes() == before;
    (seen, same)
}

/// CP-174 F3, the verifier's case w5: the source at the load before the `prepare` is held as a text, so the same
/// file spelled with `/./` from that load on ends the run `failed` with `changed`, with no effect row and the
/// workbook as it was.
#[test]
fn the_same_source_under_another_spelling_at_the_load_before_the_prepare_ends_changed() {
    let (seen, same) = source_at_the_edits_load("source-spelling-early", 167, |f| {
        f.dir
            .join(".")
            .join("source.csv")
            .to_string_lossy()
            .into_owned()
    });
    assert!(same, "the workbook byte for byte as it was");
    assert!(seen.states().is_empty(), "no effect row");
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::Changed)))
    );
}

/// CP-174 F3, the verifier's case w6: a source that is no absolute path at the load before the `prepare` is a
/// failed step: the run ends `failed` with `step_failed`, with no effect row and the workbook as it was.
#[test]
fn a_relative_source_at_the_load_before_the_prepare_ends_step_failed() {
    let (seen, same) =
        source_at_the_edits_load("source-relative-early", 168, |_| "source.csv".to_string());
    assert!(same, "the workbook byte for byte as it was");
    assert!(seen.states().is_empty(), "no effect row");
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::StepFailed)))
    );
}

/// CP-166 E4 (d), the review's item 4: a stored write step whose `file` names the slot `dest`, while the slot
/// `workbook` names another file. The edit is prepared, bound and replaced in the file that the step names, at both
/// loads, and no byte beside the other file is written.
#[test]
fn a_write_step_whose_file_names_another_slot_edits_that_file_alone() {
    let files = FillFiles::new("dest-slot", 2);
    let before = files.workbook_bytes();
    let dest = files.dir.join("other.xlsx");
    std::fs::copy(&files.workbook, &dest).expect("a second workbook");
    let dest_before = std::fs::read(&dest).expect("read");
    let mut sup = fill(166, &files);
    sup.config.runtime = Runtime::Web;
    let dest_text = dest.to_string_lossy().into_owned();
    sup.slots.push(slot("dest", &dest_text));
    let read = r#"{"v":1,"n":1,"kind":"file","verb":"read","target":{"file":"source","sheet":null,"range":null},"args":[{"name":"as","from":"const","value":"rows","key":null}],"pre":[],"post":[],"effect":"none","class":"read"}"#;
    let write = r#"{"v":1,"n":2,"kind":"file","verb":"write","target":{"file":"dest","sheet":"sheet","range":"range"},"args":[{"name":"values","from":"extract","value":null,"key":"rows"}],"pre":[],"post":[],"effect":"none","class":"local_edit"}"#;
    sup.steps = vec![
        ExecutableStep::parse(read.as_bytes()).expect("the read step"),
        ExecutableStep::parse(write.as_bytes()).expect("the write step"),
    ];
    let seen = sup.serve(|_, _| Act::Answer, LONG);
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    assert_eq!(
        kept(&seen.effects()[0])[0],
        (FieldName::File, dest_text.clone())
    );
    assert_eq!(
        files.workbook_bytes(),
        before,
        "the slot workbook's file is untouched"
    );
    assert!(
        !files.names().contains(&checkpoint_of(&sup, "formulas")),
        "nothing beside the file that the step does not name: {:?}",
        files.names()
    );
    assert_ne!(
        std::fs::read(&dest).expect("read"),
        dest_before,
        "the step's file is edited"
    );
}
