use std::num::NonZeroU32;

use halvo_schema::executable::names::{Cause, StepEffect, StepKind, Verb, YesNo};
use halvo_schema::executable::step::Target;
use halvo_schema::executable::{ActionClass, ExecutableStep};
use halvo_schema::SeedCategory;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybookCause {
    Class,
    Workbook,
}

/// The refusal of `check_playbook`: its cause at the step numbered `n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassDiffers {
    /// The number of the step that the cause names.
    pub n: NonZeroU32,
    cause: PlaybookCause,
}

impl ClassDiffers {
    /// `class` or `workbook`.
    pub fn cause(&self) -> Cause {
        match self.cause {
            PlaybookCause::Class => Cause::Class,
            PlaybookCause::Workbook => Cause::Workbook,
        }
    }
}

const WORKBOOK: &str = "workbook";

/// The class of a step of an agent of `category`, from the step's own keys (A4.3); the first row that fits gives
/// it. The stored `class` of the step is never read. A step that no row fits, which its parser refuses (an api
/// step without an api target, a file step whose verb is neither `read` nor `write`), is `credential`, the class
/// that is handed back and makes no effect.
pub fn class_of(category: SeedCategory, step: &ExecutableStep) -> ActionClass {
    match step.kind {
        StepKind::Web => web_class(category, step),
        StepKind::Api => match &step.target {
            Some(Target::Api(target)) => target.op.class(),
            _ => ActionClass::Credential,
        },
        StepKind::File => match step.verb {
            Verb::Read => ActionClass::Read,
            Verb::Write => ActionClass::LocalEdit,
            _ => ActionClass::Credential,
        },
    }
}

fn web_class(category: SeedCategory, step: &ExecutableStep) -> ActionClass {
    let credential_target =
        matches!(&step.target, Some(Target::Web(target)) if target.credential == YesNo::Yes);
    if step.verb == Verb::Handback || credential_target {
        return ActionClass::Credential;
    }
    match step.effect {
        StepEffect::Request => request_class(category),
        StepEffect::None => match step.verb {
            Verb::Navigate | Verb::Extract | Verb::Wait => ActionClass::Read,
            _ => ActionClass::Prepare,
        },
    }
}

fn request_class(category: SeedCategory) -> ActionClass {
    match category {
        SeedCategory::UtilityBillPayment | SeedCategory::TenantRent => ActionClass::Payment,
        SeedCategory::ShopOrder | SeedCategory::RideOrder => ActionClass::Purchase,
        SeedCategory::LeaseRenewalOrChange => ActionClass::Sign,
        SeedCategory::TenantApplicationReview
        | SeedCategory::Email
        | SeedCategory::SpreadsheetFill
        | SeedCategory::CalendarCleanup
        | SeedCategory::DailyChartCheck => ActionClass::Submit,
    }
}

/// `class_of` again for every step, in order: the refusal `class` with the number of the first step whose stored
/// class differs. Then the refusal `workbook` with the number of the first step that writes a file and whose `file`
/// is not the slot `workbook` (A9.4): a step writes a file when its class is not `read` and it is a file step or
/// its target is a file target, and a file step with no file target names no workbook.
pub fn check_playbook(
    category: SeedCategory,
    steps: &[ExecutableStep],
) -> Result<(), ClassDiffers> {
    if let Some(step) = steps
        .iter()
        .find(|step| class_of(category, step) != step.class)
    {
        return Err(ClassDiffers {
            n: step.n,
            cause: PlaybookCause::Class,
        });
    }
    match steps.iter().find(|step| writes_another_file(step)) {
        Some(step) => Err(ClassDiffers {
            n: step.n,
            cause: PlaybookCause::Workbook,
        }),
        None => Ok(()),
    }
}

fn writes_another_file(step: &ExecutableStep) -> bool {
    let file = match &step.target {
        Some(Target::File(target)) => Some(target.file.as_str()),
        _ => None,
    };
    let writes = step.class != ActionClass::Read && (step.kind == StepKind::File || file.is_some());
    writes && file != Some(WORKBOOK)
}
