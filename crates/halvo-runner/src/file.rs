use std::ffi::OsStr;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use halvo_control::paths::DataDir;
use halvo_policy::{
    admit, decide, digest, Checkpoint, Decided, Decision, EffectMove, Facts, LIMITS,
};
use halvo_schema::executable::body::{LoadReply, PutCheckpoint, Stored};
use halvo_schema::executable::names::{Autonomy, FieldName, Permission, Source};
use halvo_schema::executable::step::FileTarget;
use halvo_schema::executable::text::{Name, OwnValue, V1};
use halvo_schema::executable::{
    ActionClass, AgentCheckpoint, AgentConfig, Bound, BoundField, Cause, CheckpointState, Digest,
    EffectState, ExecutableStep, Outcome, RunnerCall, RunnerReply,
};
use halvo_sheet::{Folder, Letters, Locks, Prepared, Range, Sha256, Value};

use halvo_schema::executable::day::day_of;

use crate::clock::now;
use crate::run::{cause_of, Failure, Runner, Stop};

/// The cause of A7.4 that an error of the sheet executor gives, by the executor's own map (A11.3, r3 U11.3):
/// `Error::cause` for a call on the workbook or its folder.
fn sheet_cause(error: &halvo_sheet::Error) -> Cause {
    cause_of_sheet(error.cause())
}

/// The cause of A7.4 that an error of `read_source`, the one call on the data file, gives (r3 U11.3).
fn source_cause(error: &halvo_sheet::Error) -> Cause {
    cause_of_sheet(error.source_cause())
}

fn cause_of_sheet(cause: halvo_sheet::Cause) -> Cause {
    match cause {
        halvo_sheet::Cause::Workbook => Cause::Workbook,
        halvo_sheet::Cause::Sheet => Cause::Sheet,
        halvo_sheet::Cause::NoWorkbook => Cause::NoWorkbook,
        halvo_sheet::Cause::NoSheet => Cause::NoSheet,
        halvo_sheet::Cause::TwoSheets => Cause::TwoSheets,
        halvo_sheet::Cause::NoRange => Cause::NoRange,
        halvo_sheet::Cause::DataShape => Cause::DataShape,
        halvo_sheet::Cause::NoDataFile => Cause::NoDataFile,
        halvo_sheet::Cause::DataOutside => Cause::DataOutside,
        halvo_sheet::Cause::DataForm => Cause::DataForm,
        halvo_sheet::Cause::DataEncoding => Cause::DataEncoding,
        halvo_sheet::Cause::DataNarrow => Cause::DataNarrow,
        halvo_sheet::Cause::ReadOnly => Cause::ReadOnly,
        halvo_sheet::Cause::NoAccess => Cause::NoAccess,
        halvo_sheet::Cause::NotReplaced => Cause::NotReplaced,
        halvo_sheet::Cause::StepFailed => Cause::StepFailed,
    }
}

fn slot_text(loaded: &LoadReply, name: Option<&Name>) -> Result<String, Failure> {
    let name = name.ok_or(Failure::Step(Cause::StepFailed))?;
    loaded
        .bindings
        .as_ref()
        .and_then(|b| b.slots.iter().find(|s| s.name == *name))
        .map(|s| s.value.joined())
        .ok_or(Failure::Step(Cause::StepFailed))
}

/// The absolute path that a slot holds; a path that is no absolute path fails the step with `relative`, the cause of
/// the slot's file (r3 U11.3).
fn slot_path(loaded: &LoadReply, name: &Name, relative: Cause) -> Result<PathBuf, Failure> {
    let path = PathBuf::from(slot_text(loaded, Some(name))?);
    if !path.is_absolute() {
        return Err(Failure::Step(relative));
    }
    Ok(path)
}

/// How many cells of a written fill are texts of a number's shape (`halvo_sheet::number_texts`), in how many
/// columns, and the first of those columns of the workbook, A being 1 (r3 U11.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Texts {
    pub(crate) cells: u32,
    pub(crate) columns: u32,
    pub(crate) first: u32,
}

/// The `Texts` of `rows` written into `range`; none where no cell counts.
pub(crate) fn texts_of(rows: &[Vec<Value>], range: &Range) -> Option<Texts> {
    let by_column = halvo_sheet::number_texts(rows);
    let at = by_column.iter().position(|&n| n > 0)?;
    let cells = by_column.iter().fold(0u32, |sum, &n| sum.saturating_add(n));
    let columns = u32::try_from(by_column.iter().filter(|&&n| n > 0).count()).unwrap_or(u32::MAX);
    let first = range
        .first_col()
        .saturating_add(u32::try_from(at).unwrap_or(u32::MAX));
    Some(Texts {
        cells,
        columns,
        first,
    })
}

/// How long a file step waits for the lock of the workbook's directory.
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The folder of the permission `user_files`: the directory `dir` that holds the workbook of the slot `workbook`,
/// with the locks of `agents/locks/` under the data directory of `--data-dir`, which the supervisor library passes
/// (A6.1, U4.7). A runner started without `--data-dir` has no directory to lock under: its file step answers
/// `step_failed` before it opens a lock, as it does when `agents/locks/` cannot be opened, and touches no file of the
/// folder.
fn folder_at(dir: &Path, data_dir: Option<&DataDir>) -> Result<Folder, Failure> {
    let data = data_dir.ok_or(Failure::Step(Cause::StepFailed))?;
    let locks =
        Locks::open(data.path(), LOCK_WAIT).map_err(|_| Failure::Step(Cause::StepFailed))?;
    Folder::new(dir, locks).map_err(|e| Failure::Step(sheet_cause(&e)))
}

/// The slot of the one workbook that a write step may change (A9.4, CP-178 (7)).
const WORKBOOK: &str = "workbook";

/// The workbook of the slot `slot` and its directory, under the permission `user_files` of `loaded`: a write step
/// names the slot `workbook` in `target.file`, the one slot that `digest_of` reads too; a read step takes the
/// folder of the slot `workbook` (A9.4).
fn workbook_of(loaded: &LoadReply, slot: &Name) -> Result<(PathBuf, PathBuf), Failure> {
    if !loaded.config.permissions.contains(&Permission::UserFiles) {
        return Err(Failure::Step(Cause::StepFailed));
    }
    let workbook = slot_path(loaded, slot, Cause::NoWorkbook)?;
    let dir = workbook.parent().unwrap_or(Path::new("")).to_path_buf();
    Ok((workbook, dir))
}

fn slot_field(name: FieldName, value: &str) -> BoundField {
    BoundField {
        name,
        from: Source::Slot,
        origin: None,
        value: OwnValue::new(value),
    }
}

/// The bound object of a file edit (A9.3): the file and the range, both from their slots.
fn edit_bound(workbook: &Path, range: &str) -> Option<Bound> {
    Bound::new(
        ActionClass::LocalEdit,
        vec![
            slot_field(FieldName::File, &workbook.to_string_lossy()),
            slot_field(FieldName::Range, range),
        ],
    )
    .ok()
}

/// The digest of the bound object that `loaded` gives for the edit of `target`; none where a slot is missing or
/// the object cannot be built.
fn digest_of(loaded: &LoadReply, target: &FileTarget) -> Option<Digest> {
    let workbook = slot_path(loaded, &target.file, Cause::NoWorkbook).ok()?;
    let range = slot_text(loaded, target.range.as_ref()).ok()?;
    digest(&edit_bound(&workbook, &range)?).ok()
}

/// Whether an error of `replace` says that the call left the workbook as it was, which the executor's doc of
/// `replace` gives for each error that it answers: `NotReplaced`, `CopyLeft`, and `Path` with `Outside`, which
/// touches no file (A11.3).
fn left_as_it_was(error: &halvo_sheet::Error) -> bool {
    matches!(
        error,
        halvo_sheet::Error::NotReplaced(_)
            | halvo_sheet::Error::CopyLeft(_)
            | halvo_sheet::Error::Path(halvo_sheet::PathRefusal::Outside)
    )
}

/// The end of a file edit that `decide` refuses: `refused`, with the cause `autonomy` under `observe` and else
/// `checkpoint`.
fn refused(config: &AgentConfig) -> Stop {
    let cause = if config.autonomy == Autonomy::Observe {
        Cause::Autonomy
    } else {
        Cause::Checkpoint
    };
    Stop::End(Outcome::Refused, Some(cause))
}

/// The rows of one checkpoint of the run that the store took: the `planned` row alone, or the `made` row with the
/// path of the copy that it names, the copy's SHA-256 and its length.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Taken {
    Planned,
    Made {
        copy: PathBuf,
        sha: Sha256,
        len: u64,
    },
}

impl Taken {
    /// Whether the checkpoint is as its rows say: a `planned` row alone says nothing of a copy, and a `made` row's
    /// copy is a regular file, opened without following a link, with the row's length and SHA-256.
    fn stands(&self) -> bool {
        match self {
            Taken::Planned => true,
            Taken::Made { copy, sha, len } => {
                halvo_checkpoint::open_regular(copy)
                    .ok()
                    .and_then(|file| Sha256::of_reader(file, *len).ok())
                    == Some((*sha, *len))
            }
        }
    }
}

/// Where the run's read step read: the slot of its source, the source and the folder.
#[derive(Debug)]
pub(crate) struct ReadUnder {
    slot: Name,
    source: String,
    folder: PathBuf,
    /// Whether `read_data` read the file's letters by the computer's code page (r3 U11.10).
    guessed: bool,
}

impl Runner {
    /// Stores one row of the run's checkpoint of `workbook`, which `prepare` is given in the folder `dir`, under the
    /// copy's name `copy` (A7.1, A7.4, CP-98 (4)): without `made`, the row `planned` with `seq` 1 and no digest; with
    /// the SHA-256 and the length of the copy that `prepare` made, the row `made` with `seq` 2. A row that the store
    /// takes becomes the newest of that path in `checkpoints`; a row that the store does not take with
    /// `{"stored":1}` ends the run `failed`.
    fn checkpoint_row(
        &mut self,
        dir: &Path,
        workbook: &Path,
        copy: &OsStr,
        made: Option<(Sha256, u64)>,
    ) -> Result<(), Failure> {
        let (seq, state, sha256, bytes) = match made {
            None => (NonZeroU32::MIN, CheckpointState::Planned, None, None),
            Some((sha, len)) => {
                let sha =
                    Digest::parse(&sha.hex()).map_err(|_| Failure::Step(Cause::StepFailed))?;
                let len = u32::try_from(len).map_err(|_| Failure::Step(Cause::StepFailed))?;
                let second = NonZeroU32::MIN.saturating_add(1);
                (second, CheckpointState::Made, Some(sha), Some(len))
            }
        };
        let path = workbook.to_string_lossy().into_owned();
        let at = now();
        let record = AgentCheckpoint {
            v: V1,
            workflow: self.args.workflow.clone(),
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            seq,
            day: day_of(at, self.args.utc_offset_s),
            at,
            folder: OwnValue::new(&dir.to_string_lossy()),
            workbook: OwnValue::new(&path),
            copy: OwnValue::new(&copy.to_string_lossy()),
            sha256,
            bytes,
            state,
            left: None,
        };
        let call = RunnerCall::Checkpoint(PutCheckpoint {
            agent: self.args.agent.clone(),
            run: self.args.run.clone(),
            record,
        });
        match self.store(call)? {
            Ok(RunnerReply::Stored(Stored { stored: 1 })) => {
                let taken = match made {
                    None => Taken::Planned,
                    Some((sha, len)) => Taken::Made {
                        copy: dir.join(copy),
                        sha,
                        len,
                    },
                };
                self.checkpoints.insert(path, taken);
                Ok(())
            }
            Ok(_) => Err(Stop::End(Outcome::Failed, Some(Cause::StepFailed)).into()),
            Err(error) => Err(Stop::End(Outcome::Failed, Some(cause_of(error))).into()),
        }
    }

    /// A `file` `read`: the rows of the source file of the target's slot, kept under the name of the arg `as`, and
    /// whether its letters were read by the computer's code page, which a write that `replace` then answers well
    /// sets as the run's `letters_guessed`, before its `committed` row (r3 U11.10; the coordinator's way (b)).
    pub(crate) fn file_read(
        &mut self,
        step: &ExecutableStep,
        target: &FileTarget,
        loaded: &LoadReply,
    ) -> Result<(), Failure> {
        let name = step
            .args
            .first()
            .and_then(|a| a.value.as_ref())
            .ok_or(Failure::Step(Cause::StepFailed))?;
        let slot = Name::parse(WORKBOOK).map_err(|_| Failure::Step(Cause::StepFailed))?;
        let (_, dir) = workbook_of(loaded, &slot)?;
        let folder = folder_at(&dir, self.args.data_dir.as_ref())?;
        let source = slot_path(loaded, &target.file, Cause::NoDataFile)?;
        let data = folder
            .read_data(&source)
            .map_err(|e| Failure::Step(source_cause(&e)))?;
        self.kept.insert(name.as_str().to_string(), data.rows);
        self.read_under = Some(ReadUnder {
            slot: target.file.clone(),
            source: source.to_string_lossy().into_owned(),
            folder: dir,
            guessed: matches!(data.letters, Letters::Page(_)),
        });
        Ok(())
    }

    /// A `file` `write`, one file edit (A5.2, A11.3, CP-135 (3)). A step whose `file` is not the slot `workbook`
    /// fails with the cause `workbook` before any load, row or file is written (A9.4, CP-178 (7)), and the run ends
    /// `failed` with that cause. One load before `prepare` gives `decide`, the
    /// permission `user_files`, the workbook, the sheet, the range, the folder, the origins of `admit` and the
    /// limits, and `prepare`, the bound object and `replace` use that load; another source or folder than the
    /// read step's ends the run `failed` with `changed`. A second load before `begin` gives the configuration and
    /// the bindings: `begin`, which carries the digest of the object built again from that load, is sent only while
    /// `user_files` holds, `decide` gives `run` and that digest is the row's. Otherwise the prepared copy goes and
    /// the effect is ended: without the permission the step fails as at the first load; on `refuse` the run ends
    /// `refused`; on another digest a new effect is proposed from that load with its own `prepare`, at most
    /// `LIMITS.rebinds` times for the step, and then the run ends `failed` with `changed`. The effect that a load
    /// before the `begin` stops so is ended with the cause `reproposed` in the next turn, after that turn's `prepare`,
    /// `admit`, the read of a `cancel` and the wall bound, right before the new `propose`; an exit before that
    /// `propose` and the end at `LIMITS.rebinds` hold `ended` (CP-181). After such an end the
    /// prepared copy is gone and the run's checkpoint stays as `prepare` made it, until the library removes it for the
    /// stored end (A11.3); the runner removes no checkpoint. After the load before the edit and before `prepare` the
    /// runner stores the checkpoint's row `planned`, and once `prepare` has made the copy its row `made` with the
    /// SHA-256, the length and the name that `prepare` gives, before the effect's `propose` (A7.1, CP-98 (4)); a
    /// later `prepare` of the run on the same path sends neither row again, whether it takes the run's checkpoint as
    /// it stands or answers the checkpoint error, so the `made` row keeps the SHA-256 of the copy that stands
    /// (CP-139 (b)). Before every `prepare`, each checkpoint of the run whose `made` row the store took must stand as
    /// a regular file with that row's length and SHA-256, and the copy that a later `prepare` of the same path gives
    /// must have them too; otherwise the step fails with `step_failed`, so that no checkpoint that the person removed
    /// or changed is made again under a row that names another digest, also under another spelling of its path.
    /// The check reads the copies outside the lock that `prepare` takes: when the person removes the checkpoint and
    /// changes the workbook between that check and `prepare`'s own lock, `prepare` makes a copy of the changed
    /// workbook, which stays under the `made` row with the old digest; the check after `prepare` ends the run
    /// `failed` with `step_failed`, and the library leaves that copy as `changed`.
    /// The load before the
    /// `begin` is also held against the sheet of the `prepare`, a changed sheet being met as another digest is
    /// (CP-159 G2), and against the source of the read step, held as a text, so that another spelling of the same
    /// file ends the run too: another source ends the effect and then the run `failed` with `changed`, after the
    /// permission and the decision (CP-159 G3). A cancel or a passed
    /// `wall` limit before the `propose` or the `begin` ends the run there, and a cancel read under the lock after
    /// the answered `begin` lets nothing reach the workbook: the effect takes the move `not_made` with the cause
    /// `not_replaced`, and the run ends `declined`. When `replace` answers that it left the workbook as it was, the
    /// effect takes the same move and the step fails with the cause of that error; another error of `replace`
    /// leaves the effect `releasing` (A5.2, CP-135 (4), CP-139 (c)). A close that the runner's reader has returned
    /// before the `replace` lets nothing reach the workbook. A propose or begin row, or the replace after an answered
    /// begin row, can go while the system has not yet given the end of file to the reader's read; the reader marks
    /// the end as the first thing after that read returns, and nothing of an effect follows the mark; the
    /// supervisor drops what it reads after its close. The prepared copy goes when its value is dropped, and stays
    /// beside the workbook after the two exits that drop no value: the panic hook's exit and the reader's exit
    /// `LINK_GRACE` after a close.
    pub(crate) fn file_write(
        &mut self,
        step: &ExecutableStep,
        target: &FileTarget,
    ) -> Result<(), Failure> {
        if target.file.as_str() != WORKBOOK {
            return Err(Failure::Step(Cause::Workbook));
        }
        let key = step
            .args
            .first()
            .and_then(|a| a.key.as_ref())
            .ok_or(Failure::Step(Cause::StepFailed))?;
        let rows: Vec<Vec<Value>> = self
            .kept
            .get(key.as_str())
            .cloned()
            .ok_or(Failure::Step(Cause::StepFailed))?;
        let run = halvo_sheet::RunId::parse(self.args.run.as_str())
            .ok_or(Failure::Step(Cause::StepFailed))?;
        let mut loaded = self.load()?;
        let mut renewed = 0;
        let mut stopped = None;
        loop {
            let turn = self.edit_turn(step, target, &loaded, &rows, &run);
            let Turn {
                folder,
                prepared,
                bound,
                decided,
                sheet,
            } = match stopped {
                Some(ended) => self.or_ended(ended, turn)?,
                None => turn?,
            };
            let at = match stopped {
                Some(ended) => self.propose_again(ended, decided, &bound)?,
                None => self.propose(decided, &bound)?,
            };
            match self.effects[at].state {
                EffectState::Cleared => {}
                EffectState::Refused => return Err(refused(&loaded.config).into()),
                _ => return Err(Stop::End(Outcome::Failed, Some(Cause::Effect)).into()),
            }
            let again = self.load()?;
            self.check()?;
            self.within_wall()?;
            let held = again.config.permissions.contains(&Permission::UserFiles);
            let decision = decide(&again.config, step.class, Checkpoint::Yes).decision();
            let rebuilt = digest_of(&again, target);
            let same_sheet = slot_text(&again, target.sheet.as_ref()).ok() == Some(sheet.clone());
            let same_source = self.read_under.as_ref().is_none_or(|read| {
                slot_text(&again, Some(&read.slot)).ok().as_ref() == Some(&read.source)
            });
            if held
                && decision == Decision::Run
                && rebuilt.as_ref() == Some(self.effects[at].digest())
                && same_sheet
                && same_source
            {
                let digest = rebuilt.ok_or(Failure::Step(Cause::StepFailed))?;
                self.advance(at, EffectMove::Begin { digest })?;
                let not_replaced = EffectMove::NotMade {
                    cause: Cause::NotReplaced,
                };
                match self.link.settle(|| folder.replace(&prepared))? {
                    Some(Ok(())) => {
                        if self.read_under.as_ref().is_some_and(|read| read.guessed) {
                            self.letters_guessed = true;
                        }
                        self.number_texts = slot_text(&loaded, target.range.as_ref())
                            .ok()
                            .and_then(|text| Range::parse(&text).ok())
                            .and_then(|range| texts_of(&rows, &range));
                    }
                    Some(Err(error)) if left_as_it_was(&error) => {
                        self.advance(at, not_replaced)?;
                        return Err(Failure::Step(sheet_cause(&error)));
                    }
                    Some(Err(_)) => {
                        return Err(
                            Stop::End(Outcome::EffectUnknown, Some(Cause::StepFailed)).into()
                        )
                    }
                    None => {
                        self.advance(at, not_replaced)?;
                        return Err(Stop::End(Outcome::Declined, Some(Cause::Declined)).into());
                    }
                }
                self.advance(at, EffectMove::Commit)?;
                return Ok(());
            }
            drop(prepared);
            let end: Option<Failure> = if !held {
                Some(Failure::Step(Cause::StepFailed))
            } else if decision == Decision::Refuse {
                Some(refused(&again.config).into())
            } else if decision != Decision::Run {
                Some(Stop::End(Outcome::Failed, Some(Cause::Effect)).into())
            } else if !same_source || renewed >= LIMITS.rebinds {
                Some(Stop::End(Outcome::Failed, Some(Cause::Changed)).into())
            } else {
                None
            };
            if let Some(end) = end {
                self.advance(at, EffectMove::End)?;
                return Err(end);
            }
            renewed += 1;
            loaded = again;
            stopped = Some(at);
        }
    }

    /// One turn of a file edit up to its `propose`, under the load `loaded`: the workbook, the sheet and the range of
    /// that load held against the read step's source and folder, the run's checkpoint rows, `prepare`, the bound
    /// object, `admit` and `decide`, and then the read of a `cancel` and the wall bound. Every exit of a turn before
    /// its `propose` is here, so that `file_write` ends the effect that the load before the `begin` stopped with the
    /// cause `ended` on each of them, and with `reproposed` only once all of them have passed (CP-181).
    fn edit_turn(
        &mut self,
        step: &ExecutableStep,
        target: &FileTarget,
        loaded: &LoadReply,
        rows: &[Vec<Value>],
        run: &halvo_sheet::RunId,
    ) -> Result<Turn, Failure> {
        let (workbook, dir) = workbook_of(loaded, &target.file)?;
        if let Some(read) = &self.read_under {
            slot_path(loaded, &read.slot, Cause::StepFailed)?;
            let source = slot_text(loaded, Some(&read.slot))?;
            if source != read.source || dir != read.folder {
                return Err(Stop::End(Outcome::Failed, Some(Cause::Changed)).into());
            }
        }
        let sheet = slot_text(loaded, target.sheet.as_ref())?;
        let range_text = slot_text(loaded, target.range.as_ref())?;
        let range = Range::parse(&range_text).map_err(|e| Failure::Step(sheet_cause(&e)))?;
        let folder = folder_at(&dir, self.args.data_dir.as_ref())?;
        if !self.checkpoints.values().all(Taken::stands) {
            return Err(Failure::Step(Cause::StepFailed));
        }
        let taken = self
            .checkpoints
            .get(workbook.to_string_lossy().as_ref())
            .cloned();
        if taken.is_none() {
            let planned = halvo_checkpoint::checkpoint_name(&workbook, run)
                .ok_or(Failure::Step(Cause::StepFailed))?;
            self.checkpoint_row(&dir, &workbook, &planned, None)?;
        }
        let prepared = folder
            .prepare(&workbook, run, &sheet, range, rows)
            .map_err(|e| Failure::Step(sheet_cause(&e)))?;
        let copy = (prepared.checkpoint_sha256(), prepared.checkpoint_bytes());
        match taken {
            Some(Taken::Made { sha, len, .. }) if (sha, len) != copy => {
                return Err(Failure::Step(Cause::StepFailed));
            }
            Some(Taken::Made { .. }) => {}
            _ => self.checkpoint_row(&dir, &workbook, prepared.checkpoint_name(), Some(copy))?,
        }
        let bound = edit_bound(&workbook, &range_text).ok_or(Failure::Step(Cause::StepFailed))?;
        let facts = Facts {
            origins: loaded
                .bindings
                .as_ref()
                .map(|b| b.origins.clone())
                .unwrap_or_default(),
            ..Facts::default()
        };
        admit(step.class, &bound, &facts)
            .map_err(|refused| Stop::End(Outcome::Failed, Some(refused.cause())))?;
        let decided = decide(&loaded.config, step.class, Checkpoint::Yes);
        self.check()?;
        self.within_wall()?;
        Ok(Turn {
            folder,
            prepared,
            bound,
            decided,
            sheet,
        })
    }
}

/// What one turn of a file edit made before its `propose`: the folder of the workbook, the prepared copy, the bound
/// object of the edit, the decision for its class and the sheet of its load.
struct Turn {
    folder: Folder,
    prepared: Prepared,
    bound: Bound,
    decided: Decided,
    sheet: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::io::Write;
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::Duration;

    use halvo_policy::{check_playbook, preset};
    use halvo_schema::executable::step::Target;
    use halvo_schema::{AgentId, Category, WorkflowId};

    use crate::link::Link;

    struct Out(Arc<Mutex<Vec<u8>>>);

    impl Write for Out {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("lock").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    const READ: &str = r#"{"v":1,"n":1,"kind":"file","verb":"read","target":{"file":"source","sheet":null,"range":null},"args":[{"name":"as","from":"const","value":"rows","key":null}],"pre":[],"post":[],"effect":"none","class":"read"}"#;

    const WRITE_DEST: &str = r#"{"v":1,"n":2,"kind":"file","verb":"write","target":{"file":"dest","sheet":"sheet","range":"range"},"args":[{"name":"values","from":"extract","value":null,"key":"rows"}],"pre":[],"post":[],"effect":"none","class":"local_edit"}"#;

    /// How long the link's input stays open at most while the step runs: a step that waits for a reply then meets
    /// the link's end and returns, so that the test fails and does not hang.
    const INPUT_OPEN: Duration = Duration::from_secs(5);

    /// A9.4, CP-178 (7), CP-218 N3: the file step itself refuses a write step whose `file` is the slot `dest`, a
    /// step that `check_playbook` refuses at the load and that here reaches the step without it, with the cause
    /// `workbook` and before any frame: no load, no checkpoint row and no effect row. The link's input is a pipe
    /// that stays open until the step returns, so that a frame written before the refusal reaches the output.
    #[test]
    fn the_file_step_refuses_a_write_to_a_slot_other_than_workbook_before_any_frame() {
        let read = ExecutableStep::parse(READ.as_bytes()).expect("the read step");
        let write = ExecutableStep::parse(WRITE_DEST.as_bytes()).expect("the write step");
        let workflow = WorkflowId::from_random([9; 16]);
        let agent = AgentId::from_random([8; 16]);
        let config =
            preset(Category::SpreadsheetFill, workflow.clone(), agent.clone()).expect("a preset");
        assert_eq!(
            check_playbook(config.category, &[read, write.clone()])
                .err()
                .map(|differs| differs.cause()),
            Some(Cause::Workbook),
            "a step that check_playbook refuses at the load"
        );
        let Some(Target::File(target)) = write.target.clone() else {
            panic!("a file target")
        };
        let out = Arc::new(Mutex::new(Vec::new()));
        let (input, writer) = std::io::pipe().expect("a pipe");
        let writer = Arc::new(Mutex::new(Some(writer)));
        let (returned, watched) = mpsc::channel::<()>();
        let held = Arc::clone(&writer);
        let watchdog = std::thread::spawn(move || {
            let _ = watched.recv_timeout(INPUT_OPEN);
            held.lock().expect("lock").take();
        });
        let link = Link::start(
            Box::new(input),
            Box::new(Out(Arc::clone(&out))),
            Box::new(|| {}),
        );
        let run = "0b".repeat(16);
        let words = [
            "--run",
            &run,
            "--agent",
            agent.as_str(),
            "--workflow",
            workflow.as_str(),
            "--mode",
            "replay",
            "--browser",
            "none",
            "--utc-offset",
            "0",
        ];
        let words: Vec<OsString> = words.iter().map(OsString::from).collect();
        let args = crate::args::parse(&words).expect("the arguments");
        let mut runner = Runner::new(args, link);
        runner
            .kept
            .insert("rows".to_string(), vec![vec![Value::Text("a".to_string())]]);
        let got = runner.file_write(&write, &target);
        let _ = returned.send(());
        watchdog.join().expect("the watchdog");
        assert!(
            matches!(got, Err(Failure::Step(Cause::Workbook))),
            "the step fails with workbook: {got:?}"
        );
        assert!(
            out.lock().expect("lock").is_empty(),
            "no frame: no load, no checkpoint row and no effect row"
        );
    }

    /// r3 U11.10, the coordinator's way (a) of Q4: a write that `replace` answered well keeps how
    /// many cells it wrote as texts of a number's shape, in how many columns, and the first of those
    /// columns of the WORKBOOK, by the range's first column: the rows of `C2:E4` with texts in their
    /// second and third column give 3 cells, 2 columns, column D (4). A write of no such text keeps
    /// none.
    #[test]
    fn a_written_fill_keeps_how_many_values_it_wrote_as_text_and_in_which_column() {
        let t = |s: &str| Value::Text(s.to_owned());
        let rows = vec![
            vec![t("Rent"), Value::Number(12.0), t("1,234")],
            vec![t("Power"), t("50%"), t("12.50-")],
            vec![t("Gas"), Value::Number(3.0), t("")],
        ];
        let range = Range::parse("C2:E4").expect("a range");
        let want = Texts {
            cells: 3,
            columns: 2,
            first: 4,
        };
        assert_eq!(texts_of(&rows, &range), Some(want));
        let plain = vec![vec![t("Rent"), Value::Number(12.0)]];
        let range = Range::parse("A1:B1").expect("a range");
        assert_eq!(texts_of(&plain, &range), None);
    }
}
