mod support;

use halvo_schema::executable::body::RunnerCall;
use halvo_schema::executable::names::LogEvent;
use halvo_schema::executable::text::V1;
use halvo_schema::executable::{AgentCheckpoint, Cause, CheckpointState, FromRunner, Outcome};
use halvo_schema::Category;
use halvo_sheet::Sha256;

use support::{is_op, store_error, Act, FillFiles, Seen, Supervisor, LONG};

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
