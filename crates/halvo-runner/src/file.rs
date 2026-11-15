use std::ffi::OsStr;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use halvo_control::paths::DataDir;
use halvo_policy::{admit, decide, digest, Checkpoint, Decision, EffectMove, Facts, LIMITS};
use halvo_schema::executable::body::{LoadReply, PutCheckpoint, Stored};
use halvo_schema::executable::names::{Autonomy, FieldName, Permission, Source};
use halvo_schema::executable::step::FileTarget;
use halvo_schema::executable::text::{Name, OwnValue, V1};
use halvo_schema::executable::{
    ActionClass, AgentCheckpoint, AgentConfig, Bound, BoundField, Cause, CheckpointState, Digest,
    EffectState, ExecutableStep, Outcome, RunnerCall, RunnerReply,
};
use halvo_sheet::{Folder, Locks, Range, Sha256, Value};

use crate::clock::{day, now};
use crate::run::{cause_of, Failure, Runner, Stop};

/// The cause of A7.4 that an error of the sheet executor gives: `workbook` and `sheet` are its own, every other
/// error is `step_failed` (A11.3).
fn sheet_cause(error: &halvo_sheet::Error) -> Cause {
    match error.cause() {
        halvo_sheet::Cause::Workbook => Cause::Workbook,
        halvo_sheet::Cause::Sheet => Cause::Sheet,
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

/// The absolute path that a slot holds.
fn slot_path(loaded: &LoadReply, name: &Name) -> Result<PathBuf, Failure> {
    let path = PathBuf::from(slot_text(loaded, Some(name))?);
    if !path.is_absolute() {
        return Err(Failure::Step(Cause::StepFailed));
    }
    Ok(path)
}

/// How long a file step waits for the lock of the workbook's directory.
const LOCK_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

/// The folder of the permission `user_files`: the directory `dir` that holds the workbook of the slot `workbook`,
/// with the locks of `agents/locks/` under the data directory of `--data-dir`, or without the argument the app's
/// default (`DataDir::for_current_user`). When the data directory cannot be found or its `agents/locks/` cannot be
/// opened, the file step answers `step_failed` and touches no file of the folder.
fn folder_at(dir: &Path, data_dir: Option<&DataDir>) -> Result<Folder, Failure> {
    let data = match data_dir {
        Some(data) => data.clone(),
        None => DataDir::for_current_user().map_err(|_| Failure::Step(Cause::StepFailed))?,
    };
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
    let workbook = slot_path(loaded, slot)?;
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
    let workbook = slot_path(loaded, &target.file).ok()?;
    let range = slot_text(loaded, target.range.as_ref()).ok()?;
    digest(&edit_bound(&workbook, &range)?).ok()
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
/// SHA-256 and the length of the copy that it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Taken {
    Planned,
    Made(Sha256, u64),
}

/// Whether the regular file at `full`, opened without following a link, has the length `len` and the SHA-256 `sha`.
fn stands(full: &Path, sha: Sha256, len: u64) -> bool {
    halvo_checkpoint::open_regular(full)
        .ok()
        .and_then(|file| Sha256::of_reader(file, len).ok())
        == Some((sha, len))
}

/// Where the run's read step read: the slot of its source, the source and the folder.
#[derive(Debug)]
pub(crate) struct ReadUnder {
    slot: Name,
    source: String,
    folder: PathBuf,
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
            day: day(at),
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
                    Some((sha, len)) => Taken::Made(sha, len),
                };
                self.checkpoints.insert(path, taken);
                Ok(())
            }
            Ok(_) => Err(Stop::End(Outcome::Failed, Some(Cause::StepFailed)).into()),
            Err(error) => Err(Stop::End(Outcome::Failed, Some(cause_of(error))).into()),
        }
    }

    /// A `file` `read`: the rows of the source file of the target's slot, kept under the name of the arg `as`.
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
        let source = slot_path(loaded, &target.file)?;
        let rows = folder
            .read_source(&source)
            .map_err(|e| Failure::Step(sheet_cause(&e)))?;
        self.kept.insert(name.as_str().to_string(), rows);
        self.read_under = Some(ReadUnder {
            slot: target.file.clone(),
            source: source.to_string_lossy().into_owned(),
            folder: dir,
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
    /// `LIMITS.rebinds` times for the step, and then the run ends `failed` with `changed`; after such an end the
    /// prepared copy is gone and the run's checkpoint stays as `prepare` made it, until the library removes it for the
    /// stored end (A11.3); the runner removes no checkpoint. After the load before the edit and before `prepare` the
    /// runner stores the checkpoint's row `planned`, and once `prepare` has made the copy its row `made` with the
    /// SHA-256, the length and the name that `prepare` gives, before the effect's `propose` (A7.1, CP-98 (4)); a
    /// later `prepare` of the run on the same path sends neither row again, whether it takes the run's checkpoint as
    /// it stands or answers the checkpoint error, so the `made` row keeps the SHA-256 of the copy that stands
    /// (CP-139 (b)). Before such a later `prepare` the checkpoint must stand as a regular file with the length and
    /// the SHA-256 of its `made` row, and the copy that `prepare` then gives must have them too; otherwise the step
    /// fails with `step_failed`, so that no checkpoint is made again under a row that names another digest.
    /// The load before the
    /// `begin` is also held against the sheet of the `prepare`, a changed sheet being met as another digest is
    /// (CP-159 G2), and against the source of the read step, held as a text, so that another spelling of the same
    /// file ends the run too: another source ends the effect and then the run `failed` with `changed`, after the
    /// permission and the decision (CP-159 G3). A cancel or a passed
    /// `wall` limit before the `propose` or the `begin` ends the run there, and a cancel read under the lock after
    /// the answered `begin` lets nothing reach the workbook. A close that the runner's reader has returned before
    /// the `replace` lets nothing reach the workbook. A propose or begin row, or the replace after an answered
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
        loop {
            let (workbook, dir) = workbook_of(&loaded, &target.file)?;
            if let Some(read) = &self.read_under {
                slot_path(&loaded, &read.slot)?;
                let source = slot_text(&loaded, Some(&read.slot))?;
                if source != read.source || dir != read.folder {
                    return Err(Stop::End(Outcome::Failed, Some(Cause::Changed)).into());
                }
            }
            let sheet = slot_text(&loaded, target.sheet.as_ref())?;
            let range_text = slot_text(&loaded, target.range.as_ref())?;
            let range = Range::parse(&range_text).map_err(|e| Failure::Step(sheet_cause(&e)))?;
            let folder = folder_at(&dir, self.args.data_dir.as_ref())?;
            let taken = self
                .checkpoints
                .get(workbook.to_string_lossy().as_ref())
                .copied();
            let name = halvo_checkpoint::checkpoint_name(&workbook, &run)
                .ok_or(Failure::Step(Cause::StepFailed))?;
            match taken {
                None => self.checkpoint_row(&dir, &workbook, &name, None)?,
                Some(Taken::Made(sha, len)) if !stands(&dir.join(&name), sha, len) => {
                    return Err(Failure::Step(Cause::StepFailed));
                }
                Some(_) => {}
            }
            let prepared = folder
                .prepare(&workbook, &run, &sheet, range, &rows)
                .map_err(|e| Failure::Step(sheet_cause(&e)))?;
            let copy = (prepared.checkpoint_sha256(), prepared.checkpoint_bytes());
            match taken {
                Some(Taken::Made(sha, len)) if (sha, len) != copy => {
                    return Err(Failure::Step(Cause::StepFailed));
                }
                Some(Taken::Made(..)) => {}
                _ => {
                    self.checkpoint_row(&dir, &workbook, prepared.checkpoint_name(), Some(copy))?
                }
            }
            let bound =
                edit_bound(&workbook, &range_text).ok_or(Failure::Step(Cause::StepFailed))?;
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
            let at = self.propose(decided, &bound)?;
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
                self.check()?;
                match self.link.settle(|| folder.replace(&prepared))? {
                    Some(Ok(())) => {}
                    Some(Err(_)) => {
                        return Err(
                            Stop::End(Outcome::EffectUnknown, Some(Cause::StepFailed)).into()
                        )
                    }
                    None => return Err(Stop::End(Outcome::Declined, Some(Cause::Declined)).into()),
                }
                self.advance(at, EffectMove::Commit)?;
                return Ok(());
            }
            drop(prepared);
            self.advance(at, EffectMove::End)?;
            if !held {
                return Err(Failure::Step(Cause::StepFailed));
            }
            match decision {
                Decision::Run => {}
                Decision::Refuse => return Err(refused(&again.config).into()),
                _ => return Err(Stop::End(Outcome::Failed, Some(Cause::Effect)).into()),
            }
            if !same_source {
                return Err(Stop::End(Outcome::Failed, Some(Cause::Changed)).into());
            }
            if renewed >= LIMITS.rebinds {
                return Err(Stop::End(Outcome::Failed, Some(Cause::Changed)).into());
            }
            renewed += 1;
            loaded = again;
        }
    }
}
