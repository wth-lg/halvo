use std::collections::BTreeSet;
use std::num::NonZeroU32;

use halvo_learn::{
    build_list, candidate_move, counted, use_move, verify_run, CandidateFacts, CandidateMove,
    EditCause,
};
use halvo_schema::executable::body::{Finish, PutLog};
use halvo_schema::executable::names::LogEvent;
use halvo_schema::executable::{ExecutableStep, RunnerReply};
use halvo_schema::learning::record::{
    EditId, EditState, LearnEdit, LearnRun, LearnedTarget, RunStatus, Verdict,
};

use crate::agent;
use crate::convert;
use crate::facts::{Line, RunFacts};
use crate::handlers::{LearnHandlers, Refused};
use crate::host::LearnHost;
use crate::moves::{moved, withdrawals};
use crate::rows::{self, Edits};
use crate::serve::{fits, with_target};
use crate::sources;

fn lines_of(facts: &RunFacts, n: NonZeroU32) -> impl Iterator<Item = (usize, &Line)> {
    facts
        .lines
        .iter()
        .map(|(_, line)| line)
        .enumerate()
        .filter(move |(_, line)| line.step == n)
}

fn counted_cause(line: &Line) -> Option<EditCause> {
    match line.event {
        LogEvent::StepFailed => counted(line.cause.and_then(convert::log_cause)),
        _ => None,
    }
}

/// The counted cause of the first counted `step_failed` line of the step (contracts.md L3.5).
fn first_counted(facts: &RunFacts, n: NonZeroU32) -> Option<EditCause> {
    lines_of(facts, n).find_map(|(_, line)| counted_cause(line))
}

/// The log holds a `recovered` line of the step with no `step_failed` line of the step after it.
fn recovered(facts: &RunFacts, n: NonZeroU32) -> bool {
    let last = lines_of(facts, n)
        .filter(|(_, line)| line.event == LogEvent::Recovered)
        .map(|(at, _)| at)
        .last();
    last.is_some_and(|at| {
        !lines_of(facts, n).any(|(i, line)| i > at && line.event == LogEvent::StepFailed)
    })
}

impl LearnHandlers {
    pub(crate) fn log(
        &mut self,
        host: &mut dyn LearnHost,
        body: &[u8],
        b: &PutLog,
    ) -> Result<RunnerReply, Refused> {
        let reply = agent::runner(host, body)?;
        if let Some(facts) = self.facts.get_mut(&b.run).filter(|f| f.agent == b.agent) {
            for record in &b.records {
                let kept = matches!(
                    record.event,
                    LogEvent::StepOk | LogEvent::StepFailed | LogEvent::Recovered
                );
                if let (true, Some(step)) = (kept, record.step) {
                    let line = Line {
                        event: record.event,
                        step,
                        cause: record.cause,
                    };
                    let entry = (record.seq, line);
                    if !facts.lines.contains(&entry) {
                        facts.lines.push(entry);
                    }
                }
            }
            facts.lines.sort_by_key(|(seq, _)| *seq);
        }
        Ok(reply)
    }

    pub(crate) fn finish(
        &mut self,
        host: &mut dyn LearnHost,
        body: &[u8],
        b: &Finish,
    ) -> Result<RunnerReply, Refused> {
        let seen = self.facts.get(&b.run).filter(|f| f.agent == b.agent);
        let mut newest = None;
        let mut compiled = Vec::new();
        if let Some(facts) = seen {
            let held = !facts.candidates.is_empty();
            newest = self.newest_of(host, &b.agent, &b.run)?;
            if let (true, Some(number)) = (held, newest) {
                compiled = agent::compiled(host, &b.agent, &b.run, number)?;
            }
        }
        let reply = agent::runner(host, body)?;
        self.starts.remove(&b.run);
        let Some(facts) = self.facts.remove(&b.run) else {
            return Ok(reply);
        };
        if facts.agent != b.agent {
            return Ok(reply);
        }
        let runs = rows::runs(host.rows())?;
        let Some(started) = rows::newest_run(&runs, b.run.as_str()).cloned() else {
            return Ok(reply);
        };
        if started.status != RunStatus::Started || started.agent != b.agent {
            return Ok(reply);
        }
        let ended = Ended {
            facts: &facts,
            started: &started,
            newest,
            compiled: &compiled,
            verdict: verify_run(&halvo_learn::RunFacts {
                outcome: convert::outcome(b.record.outcome),
                unknown: b.record.unknown,
            }),
        };
        let stored = Edits::read(host.rows())?;
        let (edits, run) = ended.rows(&stored, || EditId::from_random(self.ids.bytes()))?;
        let mut records = Vec::new();
        for row in &edits {
            if !sources::kept(row, stored.newest(&row.edit), Some(&run)) {
                return Err(Refused::Source);
            }
            records.push(rows::edit_record(row)?);
        }
        records.push(rows::run_record(&run)?);
        host.commit(records)?;
        Ok(reply)
    }
}

struct Ended<'a> {
    facts: &'a RunFacts,
    started: &'a LearnRun,
    newest: Option<NonZeroU32>,
    compiled: &'a [ExecutableStep],
    verdict: Result<(), halvo_learn::RunCause>,
}

impl Ended<'_> {
    /// The rows of the commit of the run's `finish` (contracts.md L4.2): the withdrawals of lower edits first,
    /// then the use moves of the edits served to the run, then an edit for each candidate that
    /// `candidate_move` keeps, and the run's `learn_run` rev 2.
    fn rows(
        &self,
        stored: &Edits,
        mut mint: impl FnMut() -> EditId,
    ) -> Result<(Vec<LearnEdit>, LearnRun), Refused> {
        let facts = self.facts;
        let agent = &facts.agent;
        let day = self.started.day;
        let verified = self.verdict.is_ok();
        let mut edits = stored.clone();
        let mut out = Vec::new();
        if let Some(newest) = self.newest {
            out.extend(withdrawals(&mut edits, agent, newest)?);
            for (n, served) in &facts.served {
                let Some(edit) = served
                    .edit
                    .as_ref()
                    .and_then(|id| edits.newest(id))
                    .cloned()
                else {
                    continue;
                };
                if edit.base != newest {
                    continue;
                }
                let counted = first_counted(facts, *n);
                let step_ok = lines_of(facts, *n).any(|(_, l)| l.event == LogEvent::StepOk);
                let any_line = lines_of(facts, *n).next().is_some();
                let edit_move = use_move(counted.is_some(), verified, step_ok, any_line);
                if let Some(edit_move) = edit_move {
                    if let Some(next) =
                        moved(&edit, edit_move, Some((&self.started.run, day)), counted)?
                    {
                        edits.apply(next.clone());
                        out.push(next);
                    }
                }
            }
        }
        let mut kept = 0u32;
        for (n, candidate) in &facts.candidates {
            let (Some(newest), Some(step)) =
                (self.newest, self.compiled.iter().find(|s| s.n == *n))
            else {
                continue;
            };
            let base = edits.newest_active(agent, newest, *n).cloned();
            let base_list = base
                .as_ref()
                .map(|e| e.target.locators.clone())
                .unwrap_or_default();
            let candidate_facts = CandidateFacts {
                verified,
                recovered: recovered(facts, *n),
                pending: edits.provisional(agent, newest, *n).is_some(),
                current: facts.loaded == Some(newest),
                edits: edits.count(agent, newest, *n),
                locators: convert::learn_list(&candidate.locators),
                base_list: convert::learn_list(&base_list),
                barred: convert::learn_list(&edits.barred(agent, newest, *n)),
            };
            let CandidateMove::Keep(left) = candidate_move(&candidate_facts) else {
                continue;
            };
            let list = convert::schema_list(&build_list(&left, &convert::learn_list(&base_list)));
            if !with_target(step, &candidate.desc, &list).is_some_and(|s| fits(&s)) {
                continue;
            }
            let mut id = mint();
            while edits.contains(&id) {
                id = mint();
            }
            let row = LearnEdit {
                workflow: facts.workflow.clone(),
                agent: agent.clone(),
                edit: id,
                rev: NonZeroU32::MIN,
                base: newest,
                n: *n,
                state: EditState::Provisional,
                on: base.map(|e| e.edit),
                runs: vec![self.started.run.clone()],
                first: day,
                last: day,
                clean: 0,
                target: LearnedTarget {
                    desc: candidate.desc.clone(),
                    locators: list,
                },
            };
            edits.apply(row.clone());
            out.push(row);
            kept += 1;
        }
        Ok((out, self.run_row(kept)))
    }

    fn run_row(&self, kept: u32) -> LearnRun {
        let facts = self.facts;
        let assisted: BTreeSet<NonZeroU32> = facts
            .lines
            .iter()
            .map(|(_, line)| line)
            .filter(|line| line.event == LogEvent::Recovered)
            .map(|line| line.step)
            .collect();
        let failed_step = facts
            .lines
            .iter()
            .map(|(_, line)| line)
            .find(|line| counted_cause(line).is_some())
            .map(|line| line.step);
        let verdict = match self.verdict {
            Ok(()) => Verdict::Verified,
            Err(cause) => Verdict::Unverified(convert::run_cause(cause)),
        };
        LearnRun {
            status: RunStatus::Ended {
                verdict,
                base: facts.loaded,
                assisted: u32::try_from(assisted.len()).unwrap_or(u32::MAX),
                kept,
                failed_step,
            },
            day: self.started.day,
            ..self.started.clone()
        }
    }
}
