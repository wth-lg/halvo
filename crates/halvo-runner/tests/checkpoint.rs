mod support;

use halvo_schema::executable::body::RunnerCall;
use halvo_schema::executable::names::LogEvent;
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{AgentCheckpoint, Cause, CheckpointState, FromRunner, Outcome};
use halvo_schema::Category;
use halvo_sheet::Sha256;

use support::{
    is_op, person_changes, serve_loads, set_slot, store_error, stored, Act, FillFiles, Seen,
    Supervisor, LONG,
};

fn fill(n: u8, files: &FillFiles) -> Supervisor {
    Supervisor::new(n, Category::SpreadsheetFill, files.slots())
}

fn checkpoint_name(sup: &Supervisor) -> String {
    format!("formulas.halvo-checkpoint-{}.xlsx", sup.run.as_str())
}

fn is_checkpoint(frame: &FromRunner, state: CheckpointState) -> bool {
    matches!(frame, FromRunner::Store(s) if matches!(s.request.body(), RunnerCall::Checkpoint(b) if b.record.state == state))
}

fn prepared_copies(files: &FillFiles) -> usize {
    files
        .names()
        .iter()
        .filter(|n| n.contains(".halvo-prepared-"))
        .count()
}

fn stopped(seen: &Seen) -> Option<(LogEvent, Option<Cause>)> {
    seen.logs().last().map(|l| (l.event, l.cause))
}

/// The ids, the paths and the name of the copy that every row of the run's checkpoint of the fill workbook holds.
fn assert_names_the_checkpoint(row: &AgentCheckpoint, sup: &Supervisor, files: &FillFiles) {
    assert_eq!(row.v, V1);
    assert_eq!(
        (&row.workflow, &row.agent, &row.run),
        (&sup.workflow, &sup.agent, &sup.run)
    );
    assert_eq!(
        row.folder.joined(),
        files.dir.to_string_lossy(),
        "the folder that prepare was given"
    );
    assert_eq!(
        row.workbook.joined(),
        files.workbook.to_string_lossy(),
        "the workbook's path as given to prepare"
    );
    assert_eq!(row.copy.joined(), checkpoint_name(sup), "the copy's name");
    assert_eq!(row.left, None);
}

/// A7.1, A7.4, A11.3, CP-96 (b), CP-98 (4), CP-135 (3) (i): a fill run sends the row `planned` after the load that
/// precedes the edit and before `prepare`, while no checkpoint and no prepared copy stand beside the workbook, and
/// the row `made` with the SHA-256, the length and the name of the copy that `prepare` made, before the effect's
/// `propose`; the runner removes no checkpoint, so the copy stands after the run with that digest.
#[test]
fn a_fill_run_sends_the_planned_row_before_prepare_and_the_made_row_before_the_propose() {
    let files = FillFiles::new("checkpoint-rows", 1);
    let original = files.workbook_bytes();
    let names_before = files.names();
    let sup = fill(171, &files);
    let copy = files.dir.join(checkpoint_name(&sup));
    let mut at_planned = None;
    let mut at_made = None;
    let seen = sup.serve(
        |frame, before| {
            let effects_before = before.iter().filter(|f| is_op(f, "effect")).count();
            if is_checkpoint(frame, CheckpointState::Planned) {
                at_planned = Some((files.names(), effects_before));
            }
            if is_checkpoint(frame, CheckpointState::Made) {
                at_made = Some((
                    std::fs::read(&copy).expect("the checkpoint at the made row"),
                    prepared_copies(&files),
                    effects_before,
                ));
            }
            Act::Answer
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
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
        "the load before the edit, the planned row, the made row, then the propose"
    );
    let (names, effects) = at_planned.expect("a planned row");
    assert_eq!(
        (names, effects),
        (names_before.clone(), 0),
        "no checkpoint, no prepared copy and no effect row at the planned row"
    );
    let (bytes, prepared, effects) = at_made.expect("a made row");
    assert_eq!(
        bytes, original,
        "the copy that prepare made is the workbook"
    );
    assert_eq!(
        (prepared, effects),
        (1, 0),
        "the prepared copy stands and no effect row came before the made row"
    );

    let rows = seen.checkpoints();
    assert_eq!(rows.len(), 2, "two rows");
    let (planned, made) = (&rows[0], &rows[1]);
    assert_names_the_checkpoint(planned, &sup, &files);
    assert_names_the_checkpoint(made, &sup, &files);
    assert_eq!(
        (
            planned.seq.get(),
            planned.state,
            &planned.sha256,
            planned.bytes
        ),
        (1, CheckpointState::Planned, &None, None)
    );
    let sha = made.sha256.as_ref().expect("the made row's digest");
    assert_eq!(
        (made.seq.get(), made.state),
        (2, CheckpointState::Made),
        "the made row"
    );
    assert_eq!(
        sha.as_str(),
        Sha256::of(&bytes).hex(),
        "the SHA-256 of the copy"
    );
    assert_eq!(
        made.bytes,
        Some(u32::try_from(bytes.len()).expect("a length")),
        "the length of the copy"
    );
    assert!(made.at >= planned.at, "the made row's instant is later");
    for row in &rows {
        assert_eq!(
            i64::from(row.day),
            row.at.0.div_euclid(86_400 * 1_000_000_000),
            "the day of the row's instant"
        );
    }

    let stands = std::fs::read(&copy).expect("the checkpoint stands after the run");
    assert_eq!(
        Sha256::of(&stands).hex(),
        sha.as_str(),
        "the copy that stands has the made row's digest"
    );
    assert_ne!(files.workbook_bytes(), original, "the workbook is filled");
    assert_eq!(
        prepared_copies(&files),
        0,
        "no prepared copy: {:?}",
        files.names()
    );
}

/// CP-98 (4): a `planned` row that the store does not take ends the run before `prepare`: no checkpoint, no
/// prepared copy and no effect row, and the workbook byte for byte.
#[test]
fn a_planned_row_that_the_store_refuses_ends_the_run_before_prepare() {
    let files = FillFiles::new("checkpoint-planned-refused", 1);
    let original = files.workbook_bytes();
    let names_before = files.names();
    let sup = fill(172, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_checkpoint(frame, CheckpointState::Planned) {
                return Act::With(store_error(frame, "store"));
            }
            Act::Answer
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::StepFailed)))
    );
    assert_eq!(seen.checkpoints().len(), 1, "the planned row alone");
    assert!(seen.states().is_empty(), "no effect row");
    assert_eq!(
        files.names(),
        names_before,
        "no checkpoint and no prepared copy"
    );
    assert_eq!(files.workbook_bytes(), original);
}

/// CP-98 (4): a `made` row that the store does not take ends the run before the `propose`: no effect row, the
/// prepared copy gone, the workbook byte for byte, and the checkpoint, which the stored `planned` row names, stays.
#[test]
fn a_made_row_that_the_store_refuses_ends_the_run_before_the_propose() {
    let files = FillFiles::new("checkpoint-made-refused", 1);
    let original = files.workbook_bytes();
    let sup = fill(173, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_checkpoint(frame, CheckpointState::Made) {
                return Act::With(store_error(frame, "store"));
            }
            Act::Answer
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::StepFailed)))
    );
    assert_eq!(seen.checkpoints().len(), 2, "the planned and the made row");
    assert!(seen.states().is_empty(), "no effect row");
    assert_eq!(files.workbook_bytes(), original);
    assert_eq!(
        std::fs::read(files.dir.join(checkpoint_name(&sup))).expect("the checkpoint stays"),
        original
    );
    assert_eq!(
        prepared_copies(&files),
        0,
        "no prepared copy: {:?}",
        files.names()
    );
}

/// A7.1: the store takes a `checkpoint` body with `{"stored":1}`; a `made` row answered `{"stored":0}` was not
/// stored, and the run ends `failed` before the `propose`, with the prepared copy gone.
#[test]
fn a_made_row_answered_stored_0_ends_the_run_before_the_propose() {
    let files = FillFiles::new("checkpoint-made-stored-0", 1);
    let original = files.workbook_bytes();
    let sup = fill(178, &files);
    let seen = sup.serve(
        |frame, _| {
            if is_checkpoint(frame, CheckpointState::Made) {
                return Act::With(stored(frame, 0));
            }
            Act::Answer
        },
        LONG,
    );
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::StepFailed)))
    );
    assert!(seen.states().is_empty(), "no effect row");
    assert_eq!(files.workbook_bytes(), original);
    assert_eq!(
        prepared_copies(&files),
        0,
        "no prepared copy: {:?}",
        files.names()
    );
}

/// The states and the `seq` of each checkpoint row.
fn states(seen: &Seen) -> Vec<(CheckpointState, u32)> {
    seen.checkpoints()
        .iter()
        .map(|r| (r.state, r.seq.get()))
        .collect()
}

/// CP-139 (b), A11.3, A7.4: a later `prepare` of the run, for the new effect after a changed range, answers the
/// checkpoint error because the person changed the workbook between the two calls: the runner sends no second
/// `planned` row and no second `made` row, and the `made` row keeps the SHA-256 of the copy that stands.
#[test]
fn a_later_prepare_that_answers_the_checkpoint_error_sends_no_second_row() {
    let files = FillFiles::new("checkpoint-person", 2);
    let original = files.workbook_bytes();
    let sup = fill(175, &files);
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
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::StepFailed)))
    );
    assert_eq!(
        states(&seen),
        [(CheckpointState::Planned, 1), (CheckpointState::Made, 2)],
        "one planned and one made row"
    );
    let stands =
        std::fs::read(files.dir.join(checkpoint_name(&sup))).expect("the checkpoint stands");
    assert_eq!(
        stands, original,
        "the checkpoint as the first prepare made it"
    );
    assert_eq!(
        seen.checkpoints()[1].sha256.as_ref().map(|d| d.as_str()),
        Some(Sha256::of(&stands).hex().as_str()),
        "the made row holds the SHA-256 of the copy that stands"
    );
    assert_eq!(files.workbook_bytes(), persons, "the person's workbook");
}

/// A11.3, A7.4: a later `prepare` of the run that takes the run's checkpoint as it stands, for the new effect
/// after a changed range, sends no second row: one checkpoint of one run and one workbook has one `planned` and
/// one `made` row, and the run completes.
#[test]
fn a_later_prepare_that_takes_the_checkpoint_as_it_stands_sends_no_second_row() {
    let files = FillFiles::new("checkpoint-rebind", 2);
    let original = files.workbook_bytes();
    let sup = fill(176, &files);
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
    assert_eq!(
        seen.effects().len(),
        5,
        "a proposed and ended effect, then a proposed, begun and committed one"
    );
    assert_eq!(
        states(&seen),
        [(CheckpointState::Planned, 1), (CheckpointState::Made, 2)],
        "one planned and one made row"
    );
    let stands =
        std::fs::read(files.dir.join(checkpoint_name(&sup))).expect("the checkpoint stands");
    assert_eq!(stands, original);
    assert_eq!(
        seen.checkpoints()[1].sha256.as_ref().map(|d| d.as_str()),
        Some(Sha256::of(&stands).hex().as_str())
    );
}

/// A7.4: one checkpoint is one run and one workbook path. A slot `workbook` that names another file of the folder
/// at the load before the `begin` gives a new effect with its own `prepare` of that file, and that file's checkpoint
/// gets its own `planned` and `made` rows; both copies stand with the digests of their rows.
#[test]
fn a_prepare_of_another_workbook_in_the_run_sends_that_checkpoints_rows() {
    let files = FillFiles::new("checkpoint-other", 2);
    let original = files.workbook_bytes();
    let other = files.dir.join("other.xlsx");
    std::fs::copy(&files.workbook, &other).expect("a second workbook");
    let other_text = other.to_string_lossy().into_owned();
    let sup = fill(177, &files);
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                set_slot(l, "workbook", &other_text);
            }
        },
        |_| {},
    );
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    assert_eq!(
        states(&seen),
        [
            (CheckpointState::Planned, 1),
            (CheckpointState::Made, 2),
            (CheckpointState::Planned, 1),
            (CheckpointState::Made, 2)
        ]
    );
    let rows = seen.checkpoints();
    for row in &rows[..2] {
        assert_names_the_checkpoint(row, &sup, &files);
    }
    let other_copy = format!("other.halvo-checkpoint-{}.xlsx", sup.run.as_str());
    for row in &rows[2..] {
        assert_eq!(
            (row.workbook.joined(), row.copy.joined()),
            (other_text.clone(), other_copy.clone()),
            "the other workbook's checkpoint"
        );
    }
    for (row, copy) in [(&rows[1], checkpoint_name(&sup)), (&rows[3], other_copy)] {
        let stands = std::fs::read(files.dir.join(&copy)).expect("the checkpoint stands");
        assert_eq!(stands, original, "{copy}");
        assert_eq!(
            row.sha256.as_ref().map(|d| d.as_str()),
            Some(Sha256::of(&stands).hex().as_str()),
            "{copy}"
        );
    }
    assert_eq!(
        files.workbook_bytes(),
        original,
        "the first workbook as it was"
    );
    assert_ne!(
        std::fs::read(&other).expect("read"),
        original,
        "the other is filled"
    );
}

/// A11.3, A7.4, CP-139 (b): the person removes the run's checkpoint and changes the workbook between the run's
/// two `prepare` calls of one workbook. The later `prepare` would make the checkpoint again from the changed
/// workbook, under a `made` row that holds the first copy's SHA-256; the runner checks first that the checkpoint
/// stands with its `made` row's length and SHA-256, and fails the step with `step_failed` before `prepare`: no copy
/// stands that a row names with another digest, nothing is written, and no second row is sent.
#[test]
fn a_later_prepare_after_the_person_removed_the_checkpoint_makes_no_copy() {
    let files = FillFiles::new("checkpoint-removed", 2);
    let sup = fill(179, &files);
    let copy = files.dir.join(checkpoint_name(&sup));
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
                std::fs::remove_file(&copy).expect("the person removes the checkpoint");
                persons = Some(person_changes(&files));
            }
        },
    );
    let persons = persons.expect("the person changed the workbook");
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::StepFailed)))
    );
    assert_eq!(
        states(&seen),
        [(CheckpointState::Planned, 1), (CheckpointState::Made, 2)]
    );
    assert!(
        !copy.exists(),
        "no checkpoint made again: {:?}",
        files.names()
    );
    assert_eq!(prepared_copies(&files), 0, "{:?}", files.names());
    assert_eq!(files.workbook_bytes(), persons, "the person's workbook");
}

/// A11.3, A7.4: the person writes the changed workbook's bytes over the run's checkpoint between the run's two
/// `prepare` calls. The later `prepare` would take that checkpoint as equal to the workbook and the edit would go
/// on under a `made` row whose SHA-256 is not the copy's; the runner fails the step with `step_failed` before
/// `prepare`, and the workbook and the person's file stay as the person left them.
#[test]
fn a_later_prepare_after_the_person_changed_the_checkpoint_writes_nothing() {
    let files = FillFiles::new("checkpoint-overwritten", 2);
    let sup = fill(180, &files);
    let copy = files.dir.join(checkpoint_name(&sup));
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
                let changed = person_changes(&files);
                std::fs::write(&copy, &changed).expect("the person writes over the checkpoint");
                persons = Some(changed);
            }
        },
    );
    let persons = persons.expect("the person changed the workbook");
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::StepFailed)))
    );
    assert_eq!(
        states(&seen),
        [(CheckpointState::Planned, 1), (CheckpointState::Made, 2)]
    );
    assert_eq!(files.workbook_bytes(), persons, "the person's workbook");
    assert_eq!(
        std::fs::read(&copy).expect("the person's checkpoint"),
        persons
    );
    assert_eq!(prepared_copies(&files), 0, "{:?}", files.names());
}

/// A11.3, A7.4: the slot `workbook` names the same file under another spelling (`<dir>/./formulas.xlsx`) from the
/// load before the `begin`, and the person removes the run's checkpoint and changes the workbook there. The new
/// spelling is a new path of the rows, but its checkpoint is the same file; the runner prepares only while every
/// `made` copy of the run stands as its row says, so the step fails with `step_failed` before `prepare`, and no
/// checkpoint is made again under the first spelling's `made` row.
#[test]
fn a_later_prepare_under_another_spelling_after_the_checkpoint_was_removed_makes_no_copy() {
    let files = FillFiles::new("checkpoint-respelled", 2);
    let sup = fill(181, &files);
    let copy = files.dir.join(checkpoint_name(&sup));
    let respelled = files
        .dir
        .join(".")
        .join("formulas.xlsx")
        .to_string_lossy()
        .into_owned();
    let mut persons = None;
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                set_slot(l, "workbook", &respelled);
            }
        },
        |n| {
            if n == 3 {
                std::fs::remove_file(&copy).expect("the person removes the checkpoint");
                persons = Some(person_changes(&files));
            }
        },
    );
    let persons = persons.expect("the person changed the workbook");
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::StepFailed)))
    );
    assert_eq!(
        states(&seen),
        [(CheckpointState::Planned, 1), (CheckpointState::Made, 2)]
    );
    assert!(
        !copy.exists(),
        "no checkpoint made again: {:?}",
        files.names()
    );
    assert_eq!(prepared_copies(&files), 0, "{:?}", files.names());
    assert_eq!(files.workbook_bytes(), persons, "the person's workbook");
}

/// A11.3, A7.4: the person writes the same bytes over the workbook and over the run's checkpoint between the run's
/// two `prepare` calls of one workbook, bytes of the checkpoint's own length that are no readable workbook. The
/// checkpoint no longer has its `made` row's SHA-256, so the step fails with `step_failed` before `prepare`, which
/// would take that checkpoint as equal to the workbook; both files stay as the person left them.
#[test]
fn a_later_prepare_after_the_checkpoint_changed_at_its_own_length_writes_nothing() {
    let files = FillFiles::new("checkpoint-same-length", 2);
    let sup = fill(182, &files);
    let copy = files.dir.join(checkpoint_name(&sup));
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
                let mut bytes = std::fs::read(&copy).expect("the checkpoint");
                let last = bytes.len() - 1;
                bytes[last] ^= 0xff;
                std::fs::write(&copy, &bytes).expect("the person writes over the checkpoint");
                std::fs::write(&files.workbook, &bytes).expect("and over the workbook");
                persons = Some(bytes);
            }
        },
    );
    let persons = persons.expect("the person wrote both files");
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::StepFailed))),
        "the step fails before prepare, which would refuse the workbook with workbook"
    );
    assert_eq!(
        states(&seen),
        [(CheckpointState::Planned, 1), (CheckpointState::Made, 2)]
    );
    assert_eq!(files.workbook_bytes(), persons);
    assert_eq!(std::fs::read(&copy).expect("the checkpoint"), persons);
}

/// A11.3, A7.4: a playbook with two write steps, the second on another workbook of the folder. The first edit is
/// committed, so its workbook is no longer its checkpoint; the check before the second `prepare` reads the first
/// checkpoint, which stands as its `made` row says, and the run completes with two checkpoints, each named by its
/// two rows with the digest of the copy that stands.
#[test]
fn a_second_write_step_on_another_workbook_after_a_committed_edit_completes() {
    use halvo_schema::executable::names::Runtime;
    use halvo_schema::executable::ExecutableStep;
    let files = FillFiles::new("checkpoint-two-steps", 2);
    let original = files.workbook_bytes();
    let other = files.dir.join("other.xlsx");
    std::fs::copy(&files.workbook, &other).expect("a second workbook");
    let other_text = other.to_string_lossy().into_owned();
    let mut sup = fill(183, &files);
    sup.config.runtime = Runtime::Web;
    let read = r#"{"v":1,"n":1,"kind":"file","verb":"read","target":{"file":"source","sheet":null,"range":null},"args":[{"name":"as","from":"const","value":"rows","key":null}],"pre":[],"post":[],"effect":"none","class":"read"}"#;
    let write = |n: u8| {
        format!(
            r#"{{"v":1,"n":{n},"kind":"file","verb":"write","target":{{"file":"workbook","sheet":"sheet","range":"range"}},"args":[{{"name":"values","from":"extract","value":null,"key":"rows"}}],"pre":[],"post":[],"effect":"none","class":"local_edit"}}"#
        )
    };
    sup.steps = vec![
        ExecutableStep::parse(read.as_bytes()).expect("the read step"),
        ExecutableStep::parse(write(2).as_bytes()).expect("the first write step"),
        ExecutableStep::parse(write(3).as_bytes()).expect("the second write step"),
    ];
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 4 {
                set_slot(l, "workbook", &other_text);
            }
        },
        |_| {},
    );
    assert_eq!(seen.done(), Some(Outcome::Completed), "{}", seen.stderr);
    assert_eq!(
        states(&seen),
        [
            (CheckpointState::Planned, 1),
            (CheckpointState::Made, 2),
            (CheckpointState::Planned, 1),
            (CheckpointState::Made, 2)
        ]
    );
    let rows = seen.checkpoints();
    let other_copy = format!("other.halvo-checkpoint-{}.xlsx", sup.run.as_str());
    for (row, copy) in [(&rows[1], checkpoint_name(&sup)), (&rows[3], other_copy)] {
        let stands = std::fs::read(files.dir.join(&copy)).expect("the checkpoint stands");
        assert_eq!(stands, original, "{copy}");
        assert_eq!(
            row.sha256.as_ref().map(|d| d.as_str()),
            Some(Sha256::of(&stands).hex().as_str()),
            "{copy}"
        );
    }
    assert_ne!(
        files.workbook_bytes(),
        original,
        "the first workbook is filled"
    );
    assert_ne!(
        std::fs::read(&other).expect("read"),
        original,
        "the other is filled"
    );
}

/// A11.3, CP-207 K1: between the run's two `prepare` calls of one workbook the person removes the checkpoint and puts,
/// at its name, a link to a named pipe that no process writes. The runner opens a made copy without following a
/// link, so it reads nothing through the link and does not wait on the pipe: the run ends `failed` with
/// `step_failed` within the bound, as a removed checkpoint ends it.
#[cfg(unix)]
#[test]
fn a_link_to_a_named_pipe_at_the_checkpoints_name_fails_the_step_at_once() {
    let files = FillFiles::new("checkpoint-pipe-link", 2);
    let sup = fill(184, &files);
    let copy = files.dir.join(checkpoint_name(&sup));
    let pipe = files.dir.join("pipe");
    let started = std::time::Instant::now();
    let seen = serve_loads(
        &sup,
        |n, l| {
            if n >= 3 {
                set_slot(l, "range", "B3:D7");
            }
        },
        |n| {
            if n == 3 {
                let made = std::process::Command::new("mkfifo")
                    .arg(&pipe)
                    .status()
                    .expect("start mkfifo");
                assert!(made.success(), "mkfifo made the pipe");
                std::fs::remove_file(&copy).expect("the person removes the checkpoint");
                std::os::unix::fs::symlink(&pipe, &copy).expect("a link at the checkpoint's name");
            }
        },
    );
    let took = started.elapsed();
    assert_eq!(seen.done(), Some(Outcome::Failed), "{}", seen.stderr);
    assert_eq!(
        stopped(&seen),
        Some((LogEvent::Stopped, Some(Cause::StepFailed)))
    );
    assert_eq!(
        states(&seen),
        [(CheckpointState::Planned, 1), (CheckpointState::Made, 2)]
    );
    assert!(
        took < std::time::Duration::from_secs(20),
        "the run ended within 20 s: {took:?}"
    );
}

/// A7.1, CP-207 K2 and its Addition 1: a fill run whose `planned` row the store answers with a count other than 1
/// (`{"stored":0}` or `{"stored":2}`) ends `failed` before `prepare`: the planned row alone is sent, no file is added
/// beside the workbook and no effect row is sent.
#[test]
fn a_planned_row_answered_a_count_other_than_1_ends_the_run_before_prepare() {
    for count in [0, 2] {
        let files = FillFiles::new("checkpoint-planned-count", 1);
        let original = files.workbook_bytes();
        let names_before = files.names();
        let sup = fill(185, &files);
        let seen = sup.serve(
            |frame, _| {
                if is_checkpoint(frame, CheckpointState::Planned) {
                    return Act::With(stored(frame, count));
                }
                Act::Answer
            },
            LONG,
        );
        assert_eq!(
            seen.done(),
            Some(Outcome::Failed),
            "count {count}: {}",
            seen.stderr
        );
        assert_eq!(
            stopped(&seen),
            Some((LogEvent::Stopped, Some(Cause::StepFailed))),
            "count {count}"
        );
        assert_eq!(
            states(&seen),
            [(CheckpointState::Planned, 1)],
            "count {count}: the planned row alone"
        );
        assert!(seen.states().is_empty(), "count {count}: no effect row");
        assert_eq!(files.names(), names_before, "count {count}: no file added");
        assert_eq!(files.workbook_bytes(), original, "count {count}");
    }
}

/// A7.1, A7.5, CP-207 K3: a store that has the run's end refuses every runner body of the run with `effect`
/// (CP-105 (b)). A `planned` row answered so ends the run `failed` with the cause `effect` before `prepare`, with
/// nothing added beside the workbook; a `made` row answered so ends it with `effect` before the `propose`, with the
/// prepared copy gone and the checkpoint that the stored planned row names left as `prepare` made it.
#[test]
fn a_checkpoint_row_answered_effect_ends_the_run_with_the_cause_effect() {
    for state in [CheckpointState::Planned, CheckpointState::Made] {
        let files = FillFiles::new("checkpoint-row-effect", 1);
        let original = files.workbook_bytes();
        let names_before = files.names();
        let sup = fill(186, &files);
        let seen = sup.serve(
            |frame, _| {
                if is_checkpoint(frame, state) {
                    return Act::With(store_error(frame, "effect"));
                }
                Act::Answer
            },
            LONG,
        );
        assert_eq!(
            seen.done(),
            Some(Outcome::Failed),
            "{state:?}: {}",
            seen.stderr
        );
        assert_eq!(
            stopped(&seen),
            Some((LogEvent::Stopped, Some(Cause::Effect))),
            "{state:?}"
        );
        assert!(seen.states().is_empty(), "{state:?}: no effect row");
        assert_eq!(files.workbook_bytes(), original, "{state:?}");
        assert_eq!(prepared_copies(&files), 0, "{state:?}: {:?}", files.names());
        if state == CheckpointState::Planned {
            assert_eq!(files.names(), names_before, "nothing beside the workbook");
        } else {
            assert_eq!(
                std::fs::read(files.dir.join(checkpoint_name(&sup))).expect("the checkpoint stays"),
                original
            );
        }
    }
}
