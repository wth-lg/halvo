use crate::case::{Case, Control, Occurrence, Role, Step};

pub const K_CLEAN: u32 = 2;
pub const LOCATORS_MAX: usize = 4;
pub const EDITS_MAX: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Locator {
    TestId(String),
    Role(Role, String),
}

impl Locator {
    pub fn role_of(control: &Control) -> Locator {
        Locator::Role(control.role, control.name.clone())
    }

    fn finds(&self, control: &Control) -> bool {
        match self {
            Locator::TestId(test_id) => control.test_id == *test_id,
            Locator::Role(role, name) => control.role == *role && control.name == *name,
        }
    }
}

pub fn resolve<'p>(page: &'p [Control], locator: &Locator) -> Option<&'p Control> {
    let mut found = page.iter().filter(|control| locator.finds(control));
    match (found.next(), found.next()) {
        (Some(control), None) => Some(control),
        _ => None,
    }
}

pub fn compiled(page: &[Control], intended: &str) -> Vec<Locator> {
    page.iter()
        .find(|control| control.id == intended)
        .map(|control| {
            vec![
                Locator::TestId(control.test_id.clone()),
                Locator::role_of(control),
            ]
        })
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Yes,
    No,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arm {
    None,
    Plain,
    Admit(Sort),
}

impl Arm {
    pub const TRIAL: [Arm; 4] = [
        Arm::None,
        Arm::Plain,
        Arm::Admit(Sort::Yes),
        Arm::Admit(Sort::No),
    ];

    pub fn name(self) -> &'static str {
        match self {
            Arm::None => "none",
            Arm::Plain => "plain",
            Arm::Admit(_) => "admit",
        }
    }

    pub fn sort(self) -> &'static str {
        match self {
            Arm::None | Arm::Plain => "-",
            Arm::Admit(Sort::Yes) => "yes",
            Arm::Admit(Sort::No) => "no",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub occurrences: u32,
    pub drifts: u32,
    pub assisted: u32,
    pub failed: u32,
    pub harm: u32,
    pub reverted_good: u32,
}

fn whole(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

pub fn drifts(case: &Case) -> u32 {
    let occurrences = &case.occurrences;
    whole(
        (1..occurrences.len())
            .filter(|&at| {
                case.steps.iter().any(|step| {
                    let earlier: Vec<&Control> = occurrences[..at]
                        .iter()
                        .filter_map(|occurrence| occurrence.control(&step.control))
                        .collect();
                    occurrences[at].control(&step.control).is_some_and(|now| {
                        earlier.iter().all(|seen| seen.test_id != now.test_id)
                            && earlier
                                .iter()
                                .all(|seen| seen.role != now.role || seen.name != now.name)
                    })
                })
            })
            .count(),
    )
}

#[derive(Default)]
struct StepLog {
    timeout: bool,
    failed: bool,
    ok: bool,
    asked: bool,
    wrong: bool,
    recovered: Option<Locator>,
}

impl StepLog {
    fn done(&self) -> bool {
        self.ok || self.recovered.is_some()
    }
}

fn play(occurrence: &Occurrence, step: &Step, index: usize, served: &[Locator]) -> StepLog {
    let holds = |control: &Control| control.id == step.control || control.passes.contains(&step.n);
    let mut log = StepLog {
        timeout: occurrence.flaky.contains(&step.n),
        ..StepLog::default()
    };
    if let Some(control) = served
        .iter()
        .find_map(|locator| resolve(&occurrence.page, locator))
    {
        log.wrong = control.id != step.control;
        if holds(control) {
            log.ok = true;
            return log;
        }
    }
    log.failed = true;
    log.asked = true;
    for id in &occurrence.repair[index] {
        if let Some(control) = occurrence.control(id) {
            log.wrong |= control.id != step.control;
            if holds(control) {
                log.recovered = Some(Locator::role_of(control));
                return log;
            }
        }
    }
    log
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Provisional,
    Active,
    Reverted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Use {
    Ok,
    FailCounted,
    FailOther,
}

pub(crate) struct Edit {
    pub(crate) list: Vec<Locator>,
    pub(crate) on: Option<usize>,
    pub(crate) state: State,
    pub(crate) clean: u32,
}

#[derive(Default)]
pub(crate) struct Edits {
    pub(crate) edits: Vec<Edit>,
}

impl Edits {
    fn provisional(&self) -> Option<usize> {
        self.edits
            .iter()
            .position(|edit| edit.state == State::Provisional)
    }

    fn newest_active(&self) -> Option<usize> {
        self.edits
            .iter()
            .rposition(|edit| edit.state == State::Active)
    }

    fn in_force(&self) -> Option<usize> {
        self.provisional().or_else(|| self.newest_active())
    }

    fn brought(&self, index: usize) -> impl Iterator<Item = &Locator> {
        let edit = &self.edits[index];
        let on = edit.on.map(|base| &self.edits[base].list);
        edit.list
            .iter()
            .filter(move |locator| on.is_none_or(|on| !on.contains(locator)))
    }

    fn barred(&self) -> Vec<&Locator> {
        (0..self.edits.len())
            .filter(|&index| self.edits[index].state == State::Reverted)
            .flat_map(|index| self.brought(index))
            .collect()
    }

    pub(crate) fn admit(&self, served: &[Locator], proposal: &Locator) -> bool {
        !served.contains(proposal)
            && !self.barred().contains(&proposal)
            && self.edits.len() < EDITS_MAX
    }

    fn use_edit(&mut self, index: usize, outcome: Use, page: &[Control], intended: &str) -> u32 {
        let edit = &mut self.edits[index];
        match (edit.state, outcome) {
            (State::Provisional, Use::Ok) => {
                edit.clean += 1;
                if edit.clean == K_CLEAN {
                    edit.state = State::Active;
                }
                0
            }
            (State::Provisional, Use::FailCounted) => {
                edit.state = State::Reverted;
                let first = edit.list.first().and_then(|locator| resolve(page, locator));
                u32::from(first.is_none_or(|control| control.id == intended))
            }
            _ => 0,
        }
    }

    pub(crate) fn keep(&mut self, candidate: Locator) {
        if self.provisional().is_some() || self.edits.len() >= EDITS_MAX {
            return;
        }
        let base = self.newest_active();
        let base_list = base.map_or_else(Vec::new, |base| self.edits[base].list.clone());
        if base_list.contains(&candidate) || self.barred().contains(&&candidate) {
            return;
        }
        let mut list: Vec<Locator> = base_list
            .into_iter()
            .filter(|locator| *locator != candidate)
            .collect();
        list.insert(0, candidate);
        list.truncate(LOCATORS_MAX);
        self.edits.push(Edit {
            list,
            on: base,
            state: State::Provisional,
            clean: 0,
        });
    }
}

enum Learner {
    Compiled,
    Plain(Vec<Vec<Locator>>),
    Admit(Sort, Vec<Edits>),
}

impl Learner {
    fn served(&self, step: usize, compiled: &[Locator]) -> Vec<Locator> {
        match self {
            Learner::Compiled => compiled.to_vec(),
            Learner::Plain(lists) => lists[step].clone(),
            Learner::Admit(_, steps) => {
                let edits = &steps[step];
                let mut list = edits
                    .in_force()
                    .map_or_else(Vec::new, |index| edits.edits[index].list.clone());
                list.extend_from_slice(compiled);
                list
            }
        }
    }

    fn finish(
        &mut self,
        case: &Case,
        occurrence: &Occurrence,
        served: &[Vec<Locator>],
        logs: &[StepLog],
        verified: bool,
    ) -> u32 {
        match self {
            Learner::Compiled => 0,
            Learner::Plain(lists) => {
                for (list, log) in lists.iter_mut().zip(logs) {
                    if let Some(proposal) = &log.recovered {
                        list.retain(|locator| locator != proposal);
                        list.insert(0, proposal.clone());
                    }
                }
                0
            }
            Learner::Admit(sort, steps) => {
                let mut good = 0;
                for (index, log) in logs.iter().enumerate() {
                    let edits = &mut steps[index];
                    let candidate = log
                        .recovered
                        .clone()
                        .filter(|proposal| edits.admit(&served[index], proposal));
                    if let Some(edit) = edits.in_force() {
                        let counted = log.failed || (*sort == Sort::No && log.timeout);
                        let outcome = if counted {
                            Use::FailCounted
                        } else if verified && log.ok {
                            Use::Ok
                        } else {
                            Use::FailOther
                        };
                        good += edits.use_edit(
                            edit,
                            outcome,
                            &occurrence.page,
                            &case.steps[index].control,
                        );
                    }
                    if let (Some(candidate), true) = (candidate, verified) {
                        edits.keep(candidate);
                    }
                }
                good
            }
        }
    }
}

pub fn run(case: &Case, arm: Arm) -> Tally {
    let first = &case.occurrences[0].page;
    let compiled: Vec<Vec<Locator>> = case
        .steps
        .iter()
        .map(|step| compiled(first, &step.control))
        .collect();
    let mut learner = match arm {
        Arm::None => Learner::Compiled,
        Arm::Plain => Learner::Plain(compiled.clone()),
        Arm::Admit(sort) => {
            Learner::Admit(sort, case.steps.iter().map(|_| Edits::default()).collect())
        }
    };
    let mut tally = Tally {
        occurrences: whole(case.occurrences.len()),
        drifts: drifts(case),
        ..Tally::default()
    };
    for occurrence in &case.occurrences {
        let served: Vec<Vec<Locator>> = (0..case.steps.len())
            .map(|step| learner.served(step, &compiled[step]))
            .collect();
        let mut logs = Vec::new();
        for (index, step) in case.steps.iter().enumerate() {
            let log = play(occurrence, step, index, &served[index]);
            let done = log.done();
            logs.push(log);
            if !done {
                break;
            }
        }
        let completed = logs.len() == case.steps.len() && logs.iter().all(StepLog::done);
        tally.assisted += u32::from(logs.iter().any(|log| log.asked));
        tally.harm += u32::from(logs.iter().any(|log| log.wrong));
        tally.failed += u32::from(!completed);
        tally.reverted_good += learner.finish(case, occurrence, &served, &logs, completed);
    }
    tally
}
