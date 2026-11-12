use std::num::NonZeroU32;

use halvo_schema::executable::names::{Cause, StepEffect, StepKind, Verb, YesNo};
use halvo_schema::executable::step::Target;
use halvo_schema::executable::{ActionClass, ExecutableStep};
use halvo_schema::SeedCategory;

/// The refusal of `check_playbook`: the cause `class` at the step numbered `n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassDiffers {
    /// The number of the first step whose stored class differs from `class_of`.
    pub n: NonZeroU32,
}

impl ClassDiffers {
    /// `class`.
    pub fn cause(&self) -> Cause {
        Cause::Class
    }
}

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
/// class differs.
pub fn check_playbook(
    category: SeedCategory,
    steps: &[ExecutableStep],
) -> Result<(), ClassDiffers> {
    match steps
        .iter()
        .find(|step| class_of(category, step) != step.class)
    {
        Some(step) => Err(ClassDiffers { n: step.n }),
        None => Ok(()),
    }
}
