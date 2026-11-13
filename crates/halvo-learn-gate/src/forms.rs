use crate::case::{Case, Form, Kind, Occurrence};
use crate::page::{compiled_locators, control, play_step, Control, Locator};

/// The integers that one arm gives on one case (contracts.md L7.4, L10.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub occurrences: u32,
    pub drifts: u32,
    pub assisted: u32,
    pub failed: u32,
    pub harm: u32,
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The occurrences after the first on which, for some step, the intended control has a test id that it had on no
/// earlier occurrence and a role with a name that it had together on no earlier occurrence (L7.4).
pub fn drifts(case: &Case) -> u32 {
    let o = &case.occurrences;
    count(
        (1..o.len())
            .filter(|&at| {
                case.steps.iter().any(|step| {
                    let Some(now) = control(&o[at].page, &step.control) else {
                        return false;
                    };
                    let mut before = o[..at]
                        .iter()
                        .filter_map(|e| control(&e.page, &step.control));
                    before.all(|b| {
                        b.test_id != now.test_id && (b.role, &b.name) != (now.role, &now.name)
                    })
                })
            })
            .count(),
    )
}

/// The reader's own arms (L9.3): `none` serves the compiled locators every time, `plain` puts the locator of
/// every recovered step first in its list, moving it there when the list holds it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReaderArm {
    None,
    Plain,
}

pub fn play(case: &Case, arm: ReaderArm) -> Counts {
    let first = &case.occurrences[0].page;
    let mut lists: Vec<Vec<Locator>> = case
        .steps
        .iter()
        .map(|s| {
            control(first, &s.control)
                .map(|c| compiled_locators(c).to_vec())
                .unwrap_or_default()
        })
        .collect();
    let mut counts = Counts {
        occurrences: count(case.occurrences.len()),
        drifts: drifts(case),
        ..Counts::default()
    };
    for occurrence in &case.occurrences {
        let mut asked = false;
        let mut wrong = false;
        let mut completed = true;
        let mut learned = Vec::new();
        for (index, step) in case.steps.iter().enumerate() {
            let flaky = occurrence.flaky.contains(&step.n);
            let p = play_step(
                &occurrence.page,
                step.n,
                &step.control,
                flaky,
                &lists[index],
                &occurrence.repair[index],
            );
            asked |= p.failed;
            wrong |= p.wrong;
            if let Some(c) = &p.recovered {
                learned.push((index, Locator::role_of(c)));
            }
            if !p.done() {
                completed = false;
                break;
            }
        }
        counts.assisted += u32::from(asked);
        counts.harm += u32::from(wrong);
        counts.failed += u32::from(!completed);
        if arm == ReaderArm::Plain {
            for (index, locator) in learned {
                lists[index].retain(|l| *l != locator);
                lists[index].insert(0, locator);
            }
        }
    }
    counts
}

fn same_control(a: &Control, b: &Control) -> bool {
    a == b
}

/// Two pages are the same when they hold the same controls by `id` with the same role, name, test id and
/// `passes`, in any order (L9.3).
fn same_page(a: &[Control], b: &[Control]) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|x| control(b, &x.id).is_some_and(|y| same_control(x, y)))
}

/// What changed from one page to the next: the ids added, the count removed, the ids whose control differs.
struct Change {
    added: Vec<Control>,
    removed: usize,
    changed: Vec<String>,
}

fn change(before: &[Control], after: &[Control]) -> Change {
    Change {
        added: after
            .iter()
            .filter(|c| control(before, &c.id).is_none())
            .cloned()
            .collect(),
        removed: before
            .iter()
            .filter(|c| control(after, &c.id).is_none())
            .count(),
        changed: after
            .iter()
            .filter(|c| control(before, &c.id).is_some_and(|b| b != *c))
            .map(|c| c.id.clone())
            .collect(),
    }
}

impl Change {
    fn only(&self, id: &str) -> bool {
        self.added.is_empty()
            && self.removed == 0
            && self.changed.len() == 1
            && self.changed[0] == id
    }
}

/// A change of control `id` at occurrence `at`: the control keeps its role and `passes` and takes a test id and a
/// name that it held on no earlier occurrence.
fn fresh(o: &[Occurrence], at: usize, id: &str) -> bool {
    let (Some(was), Some(now)) = (control(&o[at - 1].page, id), control(&o[at].page, id)) else {
        return false;
    };
    was.role == now.role
        && was.passes == now.passes
        && o[..at]
            .iter()
            .filter_map(|e| control(&e.page, id))
            .all(|e| e.test_id != now.test_id && e.name != now.name)
}

/// The one occurrence from `from` on at which the page changes, which must change control `id` alone and anew,
/// or none when the page does not change.
fn one_more_change(o: &[Occurrence], from: usize, id: &str) -> Result<Option<usize>, &'static str> {
    let mut found = None;
    for at in from..o.len() {
        if same_page(&o[at - 1].page, &o[at].page) {
            continue;
        }
        if found.is_some() || !change(&o[at - 1].page, &o[at].page).only(id) {
            return Err("the page changes other than the form says");
        }
        if !fresh(o, at, id) {
            return Err("a change does not give the control a new test id and a new name");
        }
        found = Some(at);
    }
    Ok(found)
}

struct Shape {
    changed: String,
    step: usize,
    at: usize,
    other: Option<String>,
}

fn shape(case: &Case) -> Result<Shape, &'static str> {
    let o = &case.occurrences;
    let last = o.len() - 1;
    let events: Vec<usize> = (1..o.len())
        .filter(|&at| !same_page(&o[at - 1].page, &o[at].page))
        .collect();
    let at = *events
        .first()
        .ok_or("the intended control of one step does not change")?;
    let first = change(&o[at - 1].page, &o[at].page);
    if first.changed.len() != 1 {
        return Err("the page changes other than the form says");
    }
    let changed = first.changed[0].clone();
    let naming: Vec<usize> = (0..case.steps.len())
        .filter(|&i| case.steps[i].control == changed)
        .collect();
    if naming.len() != 1 {
        return Err("the page changes other than the form says");
    }
    let step = naming[0];
    let decoy_form = case.form == Form::Decoy;
    if decoy_form && first.added.is_empty() {
        return Err("no decoy appears on the change day");
    }
    if first.removed != 0 || first.added.len() != usize::from(decoy_form) {
        return Err("the page changes other than the form says");
    }
    if !fresh(o, at, &changed) {
        return Err("a change does not give the control a new test id and a new name");
    }
    if at < 2 {
        return Err("the change is before the third occurrence");
    }
    let mut shape = Shape {
        changed,
        step,
        at,
        other: None,
    };
    match case.form {
        Form::Renamed | Form::Flaky => {
            if last - at < 3 {
                return Err("fewer than three occurrences follow the change");
            }
            if events.len() != 1 {
                return Err("the page changes other than the form says");
            }
            if case.form == Form::Flaky {
                let n = case.steps[step].n;
                let fits = (0..o.len()).all(|i| match i == at + 1 {
                    true => o[i].flaky == [n],
                    false => o[i].flaky.is_empty(),
                });
                if !fits {
                    return Err(
                        "flaky is not the changed step alone on the occurrence after the change",
                    );
                }
            }
        }
        Form::RenamedTwice => {
            let second = *events
                .get(1)
                .ok_or("the changed control does not change a second time")?;
            if !change(&o[second - 1].page, &o[second].page).only(&shape.changed) {
                return Err("the page changes other than the form says");
            }
            if !fresh(o, second, &shape.changed) {
                return Err("a change does not give the control a new test id and a new name");
            }
            if events.len() != 2 {
                return Err("the page changes other than the form says");
            }
            if second - at < 3 || last - second < 3 {
                return Err("the second change is too close to the first or to the end");
            }
        }
        Form::Alternates => {
            if last - at < 4 {
                return Err("fewer than four occurrences follow the change");
            }
            let new = control(&o[at].page, &shape.changed)
                .ok_or("no changed control")?
                .clone();
            let old = control(&o[at - 1].page, &shape.changed)
                .ok_or("no changed control")?
                .clone();
            for i in at + 1..=last {
                let form = if (i - at) % 2 == 0 { &new } else { &old };
                let want: Vec<Control> = o[i - 1]
                    .page
                    .iter()
                    .map(|c| {
                        if c.id == form.id {
                            form.clone()
                        } else {
                            c.clone()
                        }
                    })
                    .collect();
                if !same_page(&want, &o[i].page) {
                    return Err("the page does not show the new and the old form in turn");
                }
            }
        }
        Form::Decoy => {
            let decoy = first.added[0].clone();
            decoy_rules(case, &shape, &decoy)?;
            shape.other = Some(decoy.id);
        }
        Form::Reused => {
            shape.other = Some(reused_rules(case, &shape)?);
        }
    }
    Ok(shape)
}

fn decoy_rules(case: &Case, shape: &Shape, decoy: &Control) -> Result<(), &'static str> {
    let o = &case.occurrences;
    let at = shape.at;
    let n = case.steps[shape.step].n;
    let intended = control(&o[at].page, &shape.changed).ok_or("no changed control")?;
    if decoy.role != intended.role || decoy.passes != [n] {
        return Err("no decoy of the intended control's role that passes the changed step alone");
    }
    let seen = o[..at]
        .iter()
        .flat_map(|e| &e.page)
        .any(|c| c.name == decoy.name || c.test_id == decoy.test_id);
    if seen {
        return Err("the decoy's name or test id is not new on the change day");
    }
    let shows = case.shows as usize;
    if o.len() - 1 - at < shows + 2 {
        return Err("fewer than shows + 2 occurrences follow the change");
    }
    if o[at].repair[shape.step] != [decoy.id.clone(), shape.changed.clone()] {
        return Err("repair does not name the decoy first on the change day");
    }
    let quiet = at + shows;
    for i in at + 1..=quiet {
        let mut want = o[i - 1].page.clone();
        if i == quiet {
            for c in want.iter_mut().filter(|c| c.id == decoy.id) {
                c.passes.clear();
            }
        }
        if !same_page(&want, &o[i].page) {
            return Err("the decoy does not pass the changed step until the occurrence shows after the change");
        }
    }
    one_more_change(o, quiet + 1, &shape.changed)?
        .ok_or("the intended control does not change again on a later occurrence")?;
    Ok(())
}

fn reused_rules(case: &Case, shape: &Shape) -> Result<String, &'static str> {
    let o = &case.occurrences;
    let at = shape.at;
    let quiet = at + case.shows as usize;
    if quiet > o.len() - 1 {
        return Err(
            "the intended control does not change again on the occurrence shows after the change",
        );
    }
    for i in at + 1..quiet {
        if !same_page(&o[i - 1].page, &o[i].page) {
            return Err("the page changes other than the form says");
        }
    }
    let second = change(&o[quiet - 1].page, &o[quiet].page);
    if !second.changed.contains(&shape.changed) {
        return Err(
            "the intended control does not change again on the occurrence shows after the change",
        );
    }
    if !fresh(o, quiet, &shape.changed) {
        return Err("a change does not give the control a new test id and a new name");
    }
    let named = control(&o[at].page, &shape.changed).ok_or("no changed control")?;
    let other = match second.added.as_slice() {
        [other] => other,
        [] => return Err("no other control takes the change day's name"),
        _ => return Err("the page changes other than the form says"),
    };
    if other.role != named.role || other.name != named.name || !other.passes.is_empty() {
        return Err("no other control takes the change day's name");
    }
    if second.changed.len() != 1 || second.removed != 0 {
        return Err("the page changes other than the form says");
    }
    one_more_change(o, quiet + 1, &shape.changed)?
        .ok_or("the intended control does not change a third time on a later occurrence")?;
    Ok(other.id.clone())
}

/// No control but a step's intended one carries a test id, or a role with a name, that the intended control has
/// or had; the other control of `reused` may carry the change day's name of the changed control.
fn carries(case: &Case, shape: &Shape) -> Result<(), &'static str> {
    let o = &case.occurrences;
    let named_then = control(&o[shape.at].page, &shape.changed);
    for at in 0..o.len() {
        for step in &case.steps {
            let history: Vec<&Control> = o[..=at]
                .iter()
                .filter_map(|e| control(&e.page, &step.control))
                .collect();
            for other in o[at].page.iter().filter(|c| c.id != step.control) {
                if history.iter().any(|h| h.test_id == other.test_id) {
                    return Err(
                        "another control carries a test id that the intended control has or had",
                    );
                }
                let shares = history
                    .iter()
                    .any(|h| h.role == other.role && h.name == other.name);
                let allowed = case.form == Form::Reused
                    && shape.other.as_deref() == Some(other.id.as_str())
                    && step.control == shape.changed
                    && named_then.is_some_and(|c| c.role == other.role && c.name == other.name);
                if shares && !allowed {
                    return Err("another control carries a role and name that the intended control has or had");
                }
            }
        }
    }
    Ok(())
}

pub fn check(case: &Case) -> Result<(), &'static str> {
    if case.kind != case.form.kind() || (case.kind == Kind::Benign) != (case.shows == 0) {
        return Err("kind or shows does not fit the form");
    }
    if case.form != Form::Flaky && case.occurrences.iter().any(|o| !o.flaky.is_empty()) {
        return Err("flaky is not empty outside the form flaky");
    }
    let shape = shape(case)?;
    carries(case, &shape)?;
    if let (Form::Reused, Some(other)) = (case.form, &shape.other) {
        let held: Vec<&str> = case
            .occurrences
            .iter()
            .filter_map(|e| control(&e.page, other))
            .map(|c| c.test_id.as_str())
            .collect();
        let shared = case
            .occurrences
            .iter()
            .flat_map(|e| &e.page)
            .any(|c| c.id != *other && held.contains(&c.test_id.as_str()));
        if shared {
            return Err("another control holds or held the test id of the other control");
        }
    }
    for (at, occurrence) in case.occurrences.iter().enumerate() {
        for (i, step) in case.steps.iter().enumerate() {
            let excepted = case.form == Form::Decoy && at == shape.at && i == shape.step;
            if !excepted && occurrence.repair[i] != [step.control.clone()] {
                return Err(
                    "repair names more than the intended control outside the decoy's change day",
                );
            }
        }
    }
    let drifts = drifts(case);
    let none = play(case, ReaderArm::None);
    let plain = play(case, ReaderArm::Plain);
    match case.form {
        Form::Decoy if plain.harm < 3 => Err("the harm of plain is below 3"),
        Form::Reused if plain.harm < 2 => Err("the harm of plain is below 2"),
        Form::Decoy | Form::Reused => Ok(()),
        _ if plain.harm != 0 || plain.failed != 0 => Err("plain does not complete with harm 0"),
        _ if plain.assisted != drifts => Err("the assisted of plain is not drifts"),
        _ if none.assisted < drifts + 2 => Err("the assisted of none is below drifts + 2"),
        _ => Ok(()),
    }
}
