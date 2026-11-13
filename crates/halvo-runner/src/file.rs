use std::path::{Path, PathBuf};

use halvo_policy::{admit, decide, digest, Checkpoint, EffectMove, Facts};
use halvo_schema::executable::body::LoadReply;
use halvo_schema::executable::names::{Autonomy, FieldName, Permission, Source};
use halvo_schema::executable::step::FileTarget;
use halvo_schema::executable::text::{Name, OwnValue};
use halvo_schema::executable::{
    ActionClass, Bound, BoundField, Cause, EffectState, ExecutableStep, Outcome,
};
use halvo_sheet::{Folder, Range, Value};

use crate::run::{Failure, Runner, Stop};

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

/// The folder of the permission `user_files`: the directory that holds the workbook of the slot `workbook`.
fn folder(loaded: &LoadReply) -> Result<Folder, Failure> {
    if !loaded.config.permissions.contains(&Permission::UserFiles) {
        return Err(Failure::Step(Cause::StepFailed));
    }
    let workbook = slot_path(
        loaded,
        &Name::parse("workbook").map_err(|_| Failure::Step(Cause::StepFailed))?,
    )?;
    let dir = workbook.parent().unwrap_or(Path::new(""));
    Folder::new(dir).map_err(|e| Failure::Step(sheet_cause(&e)))
}

fn slot_field(name: FieldName, value: &str) -> BoundField {
    BoundField {
        name,
        from: Source::Slot,
        origin: None,
        value: OwnValue::new(value),
    }
}

impl Runner {
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
        let folder = folder(loaded)?;
        let source = slot_path(loaded, &target.file)?;
        let rows = folder
            .read_source(&source)
            .map_err(|e| Failure::Step(sheet_cause(&e)))?;
        self.kept.insert(name.as_str().to_string(), rows);
        Ok(())
    }

    /// A `file` `write`, one file edit (A5.2, A11.3): `prepare` makes the checkpoint and a checked copy; the effect
    /// is proposed with the `Decided` of the configuration as it stands then, and under its permission `user_files`;
    /// `begin` is stored before the `replace` and `commit` after it. A cancel or a passed `wall` limit before the
    /// `propose` or the `begin` ends the run there. A close that the runner's reader has returned before the
    /// `replace`, or a cancel, lets nothing reach the workbook. A propose or begin row, or the replace after an
    /// answered begin row, can go while the system has not yet given the end of file to the reader's read; the
    /// reader marks the end as the first thing after that read returns, and nothing of an effect follows the mark;
    /// the supervisor drops what it reads after its close. The prepared copy goes when its value is dropped, and
    /// stays beside the workbook after the two exits that drop no value: the panic hook's exit and the reader's exit
    /// `LINK_GRACE` after a close.
    pub(crate) fn file_write(
        &mut self,
        step: &ExecutableStep,
        target: &FileTarget,
        loaded: &LoadReply,
    ) -> Result<(), Failure> {
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
        let workbook = slot_path(loaded, &target.file)?;
        let sheet = slot_text(loaded, target.sheet.as_ref())?;
        let range_text = slot_text(loaded, target.range.as_ref())?;
        let range = Range::parse(&range_text).map_err(|e| Failure::Step(sheet_cause(&e)))?;
        let folder = folder(loaded)?;
        let run = halvo_sheet::RunId::parse(self.args.run.as_str())
            .map_err(|e| Failure::Step(sheet_cause(&e)))?;
        let prepared = folder
            .prepare(&workbook, &run, &sheet, range, &rows)
            .map_err(|e| Failure::Step(sheet_cause(&e)))?;
        let bound = Bound::new(
            ActionClass::LocalEdit,
            vec![
                slot_field(FieldName::File, &workbook.to_string_lossy()),
                slot_field(FieldName::Range, &range_text),
            ],
        )
        .map_err(|_| Failure::Step(Cause::StepFailed))?;
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
        let config = self.load()?.config;
        if !config.permissions.contains(&Permission::UserFiles) {
            return Err(Failure::Step(Cause::StepFailed));
        }
        self.check()?;
        self.within_wall(&loaded.config.limits)?;
        let at = self.propose(decide(&config, step.class, Checkpoint::Yes), &bound)?;
        match self.effects[at].state {
            EffectState::Cleared => {}
            EffectState::Refused => {
                let cause = if config.autonomy == Autonomy::Observe {
                    Cause::Autonomy
                } else {
                    Cause::Checkpoint
                };
                return Err(Stop::End(Outcome::Refused, Some(cause)).into());
            }
            _ => return Err(Stop::End(Outcome::Failed, Some(Cause::Effect)).into()),
        }
        self.check()?;
        self.within_wall(&loaded.config.limits)?;
        let again = digest(&bound).map_err(|_| Failure::Step(Cause::StepFailed))?;
        self.advance(at, EffectMove::Begin { digest: again })?;
        self.check()?;
        if self.link.settle(|| folder.replace(&prepared))?.is_err() {
            return Err(Stop::End(Outcome::EffectUnknown, Some(Cause::StepFailed)).into());
        }
        self.advance(at, EffectMove::Commit)?;
        Ok(())
    }
}
