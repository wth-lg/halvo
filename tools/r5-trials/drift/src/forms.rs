use crate::case::{Case, Control, Form, Kind};
use crate::model::{self, Arm, Tally};

pub type Rule = &'static str;

pub const KIND_SHOWS: Rule = "kind or shows does not fit the form";
pub const FLAKY_OUTSIDE: Rule = "flaky is not empty outside the form flaky";
pub const NO_CHANGE: Rule = "the intended control of one step does not change";
pub const OTHER_CHANGE: Rule = "the page changes other than the form says";
pub const NOT_NEW: Rule = "a change does not give the control a new test id and a new name alone";
pub const EARLY: Rule = "the change is before the third occurrence";
pub const SHORT_TAIL: Rule = "fewer than three occurrences follow the change";
pub const NO_SECOND: Rule = "the changed control does not change a second time";
pub const SECOND_CLOSE: Rule = "the second change is fewer than three occurrences after the first";
pub const SECOND_TAIL: Rule = "fewer than three occurrences follow the second change";
pub const ALTERNATES_TAIL: Rule = "fewer than four occurrences follow the change";
pub const NOT_IN_TURN: Rule = "the page does not show the new and the old form in turn";
pub const FLAKY_STEP: Rule =
    "flaky is not the changed step alone on the occurrence after the change and empty on every other";
pub const DECOY_TAIL: Rule = "fewer than shows + 2 occurrences follow the change";
pub const NO_DECOY: Rule =
    "no decoy of the intended control's role that passes the changed step alone appears on the change day";
pub const DECOY_NOT_NEW: Rule = "the decoy's name or test id is not new on the change day";
pub const DECOY_PASSES: Rule =
    "the decoy does not pass the changed step until the occurrence shows after the change and nothing from it on";
pub const DECOY_REPAIR: Rule = "repair does not name the decoy first on the change day";
pub const NO_RECURRENCE: Rule = "the intended control does not change again on a later occurrence";
pub const REUSED_SECOND: Rule =
    "the intended control does not change again on the occurrence shows after the change";
pub const NO_OTHER: Rule =
    "no other control of the intended control's role that passes nothing takes the change day's name on that occurrence";
pub const NO_THIRD: Rule =
    "the intended control does not change a third time on a later occurrence";
pub const CARRIES: Rule =
    "another control carries a test id or a role and name that the intended control has or had";
pub const FAULT_REPAIR: Rule =
    "repair names more than the intended control on a day other than the change day of decoy";
pub const BENIGN_HARM: Rule = "plain does not complete with harm 0";
pub const BENIGN_ASSISTED: Rule = "the assisted of plain is not drifts";
pub const BENIGN_NONE: Rule = "the assisted of none is below drifts + 2";
pub const DECOY_HARM: Rule = "the harm of plain is below 3";
pub const REUSED_HARM: Rule = "the harm of plain is below 2";

pub struct Plan {
    pub control: String,
    pub step: usize,
    pub change: usize,
    pub extra: Option<String>,
}

struct Diff<'p> {
    added: Vec<&'p Control>,
    removed: usize,
    changed: Vec<&'p str>,
}

impl Diff<'_> {
    fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed == 0 && self.changed.is_empty()
    }

    fn changes_only(&self, id: &str) -> bool {
        self.added.is_empty() && self.removed == 0 && self.changed == [id]
    }
}

fn diff<'p>(before: &[Control], after: &'p [Control]) -> Diff<'p> {
    Diff {
        added: after
            .iter()
            .filter(|now| before.iter().all(|was| was.id != now.id))
            .collect(),
        removed: before
            .iter()
            .filter(|was| after.iter().all(|now| now.id != was.id))
            .count(),
        changed: after
            .iter()
            .filter(|now| before.iter().any(|was| was.id == now.id && was != *now))
            .map(|now| now.id.as_str())
            .collect(),
    }
}

fn same(before: &[Control], after: &[Control]) -> bool {
    diff(before, after).is_empty()
}

fn find<'p>(page: &'p [Control], id: &str) -> Option<&'p Control> {
    page.iter().find(|control| control.id == id)
}

fn with(page: &[Control], form: &Control) -> Vec<Control> {
    page.iter()
        .map(|control| {
            if control.id == form.id {
                form.clone()
            } else {
                control.clone()
            }
        })
        .collect()
}

fn passes_differ_only(a: &Control, b: &Control) -> bool {
    a.id == b.id
        && a.role == b.role
        && a.name == b.name
        && a.test_id == b.test_id
        && a.passes != b.passes
}

fn new_change(case: &Case, at: usize, id: &str) -> bool {
    let occurrences = &case.occurrences;
    match (occurrences[at - 1].control(id), occurrences[at].control(id)) {
        (Some(before), Some(now)) => {
            before.role == now.role
                && before.passes == now.passes
                && occurrences[..at]
                    .iter()
                    .filter_map(|occurrence| occurrence.control(id))
                    .all(|seen| seen.test_id != now.test_id && seen.name != now.name)
        }
        _ => false,
    }
}

fn only_step(case: &Case, control: &str) -> Result<usize, Rule> {
    let mut steps = case
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| step.control == control)
        .map(|(index, _)| index);
    match (steps.next(), steps.next()) {
        (Some(index), None) => Ok(index),
        _ => Err(NO_CHANGE),
    }
}

fn recurrence(
    case: &Case,
    pages: &[&[Control]],
    from: usize,
    control: &str,
) -> Result<Option<usize>, Rule> {
    let mut found = None;
    for at in from..pages.len() {
        let changes = diff(pages[at - 1], pages[at]);
        if changes.is_empty() {
            continue;
        }
        if found.is_some() || !changes.changes_only(control) {
            return Err(OTHER_CHANGE);
        }
        if !new_change(case, at, control) {
            return Err(NOT_NEW);
        }
        found = Some(at);
    }
    Ok(found)
}

fn decoy(case: &Case, pages: &[&[Control]], plan: &Plan, decoy: &Control) -> Result<(), Rule> {
    let change = plan.change;
    let last = pages.len() - 1;
    let n = case.steps[plan.step].n;
    let intended = find(pages[change], &plan.control).ok_or(NO_CHANGE)?;
    if decoy.role != intended.role || decoy.passes != [n] {
        return Err(NO_DECOY);
    }
    let seen = pages[..change].iter().any(|page| {
        page.iter()
            .any(|control| control.name == decoy.name || control.test_id == decoy.test_id)
    });
    if seen {
        return Err(DECOY_NOT_NEW);
    }
    let shows = usize::try_from(case.shows).unwrap_or(usize::MAX);
    if last - change < shows.saturating_add(2) {
        return Err(DECOY_TAIL);
    }
    if case.occurrences[change].repair[plan.step] != [decoy.id.clone(), plan.control.clone()] {
        return Err(DECOY_REPAIR);
    }
    let quiet = change + shows;
    for at in change + 1..=quiet {
        let mut expected = pages[at - 1].to_vec();
        if at == quiet {
            if let Some(control) = expected.iter_mut().find(|control| control.id == decoy.id) {
                control.passes.clear();
            }
        }
        if same(&expected, pages[at]) {
            continue;
        }
        let changes = diff(&expected, pages[at]);
        let passes_only = changes.changes_only(&decoy.id)
            && match (find(&expected, &decoy.id), find(pages[at], &decoy.id)) {
                (Some(want), Some(got)) => passes_differ_only(want, got),
                _ => false,
            };
        return Err(if passes_only {
            DECOY_PASSES
        } else {
            OTHER_CHANGE
        });
    }
    recurrence(case, pages, quiet + 1, &plan.control)?.ok_or(NO_RECURRENCE)?;
    Ok(())
}

fn reused(case: &Case, pages: &[&[Control]], plan: &Plan) -> Result<String, Rule> {
    let change = plan.change;
    let last = pages.len() - 1;
    let quiet = change + usize::try_from(case.shows).unwrap_or(usize::MAX);
    if quiet > last {
        return Err(REUSED_SECOND);
    }
    for at in change + 1..quiet {
        if !same(pages[at - 1], pages[at]) {
            return Err(OTHER_CHANGE);
        }
    }
    let second = diff(pages[quiet - 1], pages[quiet]);
    if !second.changed.contains(&plan.control.as_str()) {
        return Err(REUSED_SECOND);
    }
    if !new_change(case, quiet, &plan.control) {
        return Err(NOT_NEW);
    }
    let named = find(pages[change], &plan.control).ok_or(NO_CHANGE)?;
    let other = match second.added[..] {
        [] => return Err(NO_OTHER),
        [other] => other,
        _ => return Err(OTHER_CHANGE),
    };
    if other.role != named.role || other.name != named.name || !other.passes.is_empty() {
        return Err(NO_OTHER);
    }
    if second.changed.len() != 1 || second.removed > 0 {
        return Err(OTHER_CHANGE);
    }
    recurrence(case, pages, quiet + 1, &plan.control)?.ok_or(NO_THIRD)?;
    Ok(other.id.clone())
}

fn structure(case: &Case) -> Result<Plan, Rule> {
    let occurrences = &case.occurrences;
    let pages: Vec<&[Control]> = occurrences
        .iter()
        .map(|occurrence| occurrence.page.as_slice())
        .collect();
    let last = pages.len() - 1;
    let events: Vec<usize> = (1..=last)
        .filter(|&at| !same(pages[at - 1], pages[at]))
        .collect();
    let &change = events.first().ok_or(NO_CHANGE)?;
    let first = diff(pages[change - 1], pages[change]);
    let control = match first.changed[..] {
        [] => return Err(NO_CHANGE),
        [control] => control.to_owned(),
        _ => return Err(OTHER_CHANGE),
    };
    let step = only_step(case, &control)?;
    let is_decoy = case.form == Form::Decoy;
    if is_decoy && first.added.is_empty() {
        return Err(NO_DECOY);
    }
    if first.removed > 0 || first.added.len() != usize::from(is_decoy) {
        return Err(OTHER_CHANGE);
    }
    if !new_change(case, change, &control) {
        return Err(NOT_NEW);
    }
    if change < 2 {
        return Err(EARLY);
    }
    let after = last - change;
    let mut plan = Plan {
        control,
        step,
        change,
        extra: None,
    };
    match case.form {
        Form::Renamed | Form::Flaky => {
            if after < 3 {
                return Err(SHORT_TAIL);
            }
            if events.len() > 1 {
                return Err(OTHER_CHANGE);
            }
            if case.form == Form::Flaky {
                let n = case.steps[step].n;
                let fits = occurrences.iter().enumerate().all(|(at, occurrence)| {
                    if at == change + 1 {
                        occurrence.flaky == [n]
                    } else {
                        occurrence.flaky.is_empty()
                    }
                });
                if !fits {
                    return Err(FLAKY_STEP);
                }
            }
        }
        Form::RenamedTwice => {
            let &second = events.get(1).ok_or(NO_SECOND)?;
            if !diff(pages[second - 1], pages[second]).changes_only(&plan.control) {
                return Err(OTHER_CHANGE);
            }
            if !new_change(case, second, &plan.control) {
                return Err(NOT_NEW);
            }
            if events.len() > 2 {
                return Err(OTHER_CHANGE);
            }
            if second - change < 3 {
                return Err(SECOND_CLOSE);
            }
            if last - second < 3 {
                return Err(SECOND_TAIL);
            }
        }
        Form::Alternates => {
            if after < 4 {
                return Err(ALTERNATES_TAIL);
            }
            let new = find(pages[change], &plan.control).ok_or(NO_CHANGE)?;
            let old = find(pages[change - 1], &plan.control).ok_or(NO_CHANGE)?;
            for at in change + 1..=last {
                let form = if (at - change) % 2 == 0 { new } else { old };
                if !same(&with(pages[at - 1], form), pages[at]) {
                    return Err(NOT_IN_TURN);
                }
            }
        }
        Form::Decoy => {
            let added = first.added[0];
            decoy(case, &pages, &plan, added)?;
            plan.extra = Some(added.id.clone());
        }
        Form::Reused => {
            plan.extra = Some(reused(case, &pages, &plan)?);
        }
    }
    Ok(plan)
}

fn carries(case: &Case, plan: &Plan) -> Result<(), Rule> {
    let occurrences = &case.occurrences;
    let at_change = occurrences[plan.change].control(&plan.control);
    for (at, occurrence) in occurrences.iter().enumerate() {
        for step in &case.steps {
            let history: Vec<&Control> = occurrences[..=at]
                .iter()
                .filter_map(|seen| seen.control(&step.control))
                .collect();
            for other in occurrence
                .page
                .iter()
                .filter(|other| other.id != step.control)
            {
                if history.iter().any(|seen| seen.test_id == other.test_id) {
                    return Err(CARRIES);
                }
                let excepted = case.form == Form::Reused
                    && plan.extra.as_deref() == Some(other.id.as_str())
                    && step.control == plan.control
                    && at_change
                        .is_some_and(|named| named.role == other.role && named.name == other.name);
                let shared = history
                    .iter()
                    .any(|seen| seen.role == other.role && seen.name == other.name);
                if shared && !excepted {
                    return Err(CARRIES);
                }
            }
        }
    }
    Ok(())
}

fn fault_repair(case: &Case, plan: &Plan) -> Result<(), Rule> {
    for (at, occurrence) in case.occurrences.iter().enumerate() {
        if case.form == Form::Decoy && at == plan.change {
            continue;
        }
        for (index, step) in case.steps.iter().enumerate() {
            if occurrence.repair[index] != [step.control.clone()] {
                return Err(FAULT_REPAIR);
            }
        }
    }
    Ok(())
}

pub fn paragraph(form: Form, drifts: u32, none: &Tally, plain: &Tally) -> Result<(), Rule> {
    match form {
        Form::Decoy if plain.harm < 3 => Err(DECOY_HARM),
        Form::Reused if plain.harm < 2 => Err(REUSED_HARM),
        Form::Decoy | Form::Reused => Ok(()),
        Form::Renamed | Form::RenamedTwice | Form::Alternates | Form::Flaky => {
            if plain.harm > 0 || plain.failed > 0 {
                Err(BENIGN_HARM)
            } else if plain.assisted != drifts {
                Err(BENIGN_ASSISTED)
            } else if none.assisted < drifts.saturating_add(2) {
                Err(BENIGN_NONE)
            } else {
                Ok(())
            }
        }
    }
}

fn rules(case: &Case) -> Result<(), Rule> {
    let benign = case.form.kind() == Kind::Benign;
    if case.kind != case.form.kind() || benign != (case.shows == 0) {
        return Err(KIND_SHOWS);
    }
    let flaky_outside = case.form != Form::Flaky
        && case
            .occurrences
            .iter()
            .any(|occurrence| !occurrence.flaky.is_empty());
    if flaky_outside {
        return Err(FLAKY_OUTSIDE);
    }
    let plan = structure(case)?;
    carries(case, &plan)?;
    if !benign {
        fault_repair(case, &plan)?;
    }
    paragraph(
        case.form,
        model::drifts(case),
        &model::run(case, Arm::None),
        &model::run(case, Arm::Plain),
    )
}

pub fn check(case: &Case) -> Result<(), String> {
    rules(case).map_err(|rule| format!("{}: {rule}", case.form.as_str()))
}
