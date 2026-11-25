use std::num::NonZeroU32;

use halvo_schema::executable::names::{Cause, StepEffect, StepKind, Verb, YesNo};
use halvo_schema::executable::step::Target;
use halvo_schema::executable::{ActionClass, ExecutableStep};
use halvo_schema::SeedCategory;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybookCause {
    Class,
    Workbook,
    TwoPayingSteps,
}

/// The refusal of `check_playbook`: its cause at the step numbered `n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassDiffers {
    /// The number of the step that the cause names.
    pub n: NonZeroU32,
    cause: PlaybookCause,
}

impl ClassDiffers {
    /// `class`, `workbook` or `two_paying_steps`.
    pub fn cause(&self) -> Cause {
        match self.cause {
            PlaybookCause::Class => Cause::Class,
            PlaybookCause::Workbook => Cause::Workbook,
            PlaybookCause::TwoPayingSteps => Cause::TwoPayingSteps,
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
/// its target is a file target, and a file step with no file target names no workbook. Then the refusal
/// `two_paying_steps` with the number of the second step of the class `payment` or `purchase`: a playbook holds at
/// most one step that pays or orders, so that no run makes a second payment or order (r3 U5.10-S, the coordinator's
/// log line 20879).
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
    if let Some(step) = steps.iter().find(|step| writes_another_file(step)) {
        return Err(ClassDiffers {
            n: step.n,
            cause: PlaybookCause::Workbook,
        });
    }
    match steps
        .iter()
        .filter(|step| matches!(step.class, ActionClass::Payment | ActionClass::Purchase))
        .nth(1)
    {
        Some(step) => Err(ClassDiffers {
            n: step.n,
            cause: PlaybookCause::TwoPayingSteps,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A web step `n` that clicks a button with the effect `request`, stored as `class`: its bound fields from slots of
    /// their own names, its origin a const.
    fn request(n: u32, class: ActionClass) -> ExecutableStep {
        let args: Vec<String> = class
            .bound_fields()
            .iter()
            .map(|f| match f.as_str() {
                "origin" => {
                    r#"{"name":"origin","from":"const","value":"https://pay.example:443","key":null}"#
                        .to_owned()
                }
                name => format!(r#"{{"name":"{name}","from":"slot","value":null,"key":"{name}"}}"#),
            })
            .collect();
        let json = format!(
            r#"{{"v":1,"n":{n},"kind":"web","verb":"click","target":{{"desc":"button Pay","frame":[],"credential":"no","locators":[{{"by":"role","value":"button","name":"Pay"}}]}},"args":[{}],"pre":[],"post":[],"effect":"request","class":"{}"}}"#,
            args.join(","),
            class.as_str()
        );
        ExecutableStep::parse(json.as_bytes()).expect("a request step")
    }

    /// A web step `n` that opens the slot `start`, of the class `read`.
    fn navigate(n: u32) -> ExecutableStep {
        let json = format!(
            r#"{{"v":1,"n":{n},"kind":"web","verb":"navigate","target":null,"args":[{{"name":"url","from":"slot","value":null,"key":"start"}}],"pre":[],"post":[],"effect":"none","class":"read"}}"#
        );
        ExecutableStep::parse(json.as_bytes()).expect("a navigate step")
    }

    /// r3 U5.10-S (the coordinator's log line 20879): a playbook holds at most ONE step that pays or orders.
    /// `check_playbook` takes a playbook with one step of the class `payment` or `purchase`, and refuses one with a
    /// second, at that second step, with the cause `two_paying_steps`.
    #[test]
    fn u510s_check_playbook_refuses_a_second_step_that_pays_or_orders() {
        let mut got = Vec::new();
        for (category, class) in [
            (SeedCategory::UtilityBillPayment, ActionClass::Payment),
            (SeedCategory::ShopOrder, ActionClass::Purchase),
        ] {
            let one = [navigate(1), request(2, class)];
            let two = [
                navigate(1),
                request(2, class),
                navigate(3),
                request(4, class),
            ];
            let read = |steps: &[ExecutableStep]| {
                check_playbook(category, steps).map_err(|r| (r.n.get(), r.cause().as_str()))
            };
            got.push((class, read(&one), read(&two)));
        }
        assert_eq!(
            got,
            [ActionClass::Payment, ActionClass::Purchase].map(|class| (
                class,
                Ok(()),
                Err((4, "two_paying_steps"))
            )),
            "(class, one paying step, two paying steps)"
        );
    }
}
