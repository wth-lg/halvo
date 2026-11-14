use std::collections::BTreeSet;

use halvo_learn_store::LearnHandlers;
use halvo_policy::class_of;
use halvo_schema::executable::names::LocatorBy;
use halvo_schema::executable::step::Target;
use halvo_schema::executable::{CoordinatorReply, ExecutableStep, RunnerReply};
use halvo_schema::learning::record::LearnEdit;
use halvo_schema::{RowRead, SeedCategory, Workflow};
use serde_json::{json, Value};

use crate::case::{Case, Occurrence, Verb};
use crate::forms::{drifts, Counts};
use crate::host::GateHost;
use crate::page::{control, play_step, Control, Locator, Role};
use crate::store::{Family, MemoryStore};

/// The category of the workflow that the gate host makes for a case.
pub const CATEGORY: &str = "utility_bill_payment";

/// An arm of the gate (contracts.md L10.4): `none` links no `LearnHandlers` and puts the store back to its state
/// after the compile before every occurrence, `plain` links none and puts nothing back, `admit` links them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arm {
    None,
    Plain,
    Admit,
}

impl Arm {
    pub const ALL: [Arm; 3] = [Arm::None, Arm::Plain, Arm::Admit];

    pub fn as_str(self) -> &'static str {
        match self {
            Arm::None => "none",
            Arm::Plain => "plain",
            Arm::Admit => "admit",
        }
    }

    pub fn parse(text: &str) -> Option<Arm> {
        Arm::ALL.into_iter().find(|a| a.as_str() == text)
    }
}

/// The integers of one case in one arm (L10.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub counts: Counts,
    pub reverted_good: u32,
    pub edit_rows: u32,
    pub expired_runs: u32,
}

pub fn id(prefix: char, n: u32) -> String {
    format!("{prefix}{n:031x}")
}

/// A workflow of the category `CATEGORY` with the state `approved` and no agent, as a test proposer returns it.
pub fn workflow_record(workflow: &str) -> Workflow {
    serde_json::from_value(json!({
        "v": 1, "id": workflow, "state": "approved", "category": CATEGORY,
        "site": {"key": "northwind-power.test", "source": "address_field"},
        "app": "com.example.browser", "trigger": "schedule",
        "recurrence": {"count": 3, "period": 31, "first": 20705, "last": 20767},
        "confidence": 87, "agent": null,
    }))
    .expect("the workflow record of the gate host is of its form")
}

fn locator_json(locator: &Locator) -> Value {
    match locator {
        Locator::TestId(test_id) => json!({"by": "test_id", "value": test_id, "name": null}),
        Locator::Role(role, name) => json!({"by": "role", "value": role.as_str(), "name": name}),
    }
}

fn desc(control: &Control) -> String {
    format!("{} \"{}\"", control.role.as_str(), control.name)
}

/// The compiled step of step `n` aimed at a control of the first page (L10.3): kind `web`, effect `none`, the two
/// locators of L9.2, the description `<role> "<name>"`, for `fill` one arg from the slot `s<n>`, for `extract` the
/// args `as` and `parse`, and the class that `class_of` gives.
pub fn compiled_step(n: u32, verb: Verb, intended: &Control) -> Result<Value, String> {
    let (verb, args) = match verb {
        Verb::Click => ("click", json!([])),
        Verb::Fill => (
            "fill",
            json!([{"name": "text", "from": "slot", "value": null, "key": format!("s{n}")}]),
        ),
        Verb::Extract => (
            "extract",
            json!([
                {"name": "as", "from": "const", "value": format!("e{n}"), "key": null},
                {"name": "parse", "from": "const", "value": "text", "key": null},
            ]),
        ),
    };
    let locators: Vec<Value> = crate::page::compiled_locators(intended)
        .iter()
        .map(locator_json)
        .collect();
    let mut step = json!({
        "v": 1, "n": n, "kind": "web", "verb": verb,
        "target": {"desc": desc(intended), "frame": [], "credential": "no", "locators": locators},
        "args": args, "pre": [], "post": [], "effect": "none", "class": "read",
    });
    let parsed = ExecutableStep::parse(step.to_string().as_bytes())
        .map_err(|e| format!("the compiled step {n} is refused: {e:?}"))?;
    let class = class_of(SeedCategory::UtilityBillPayment, &parsed);
    step["class"] = serde_json::to_value(class).map_err(|_| "a class with no name".to_owned())?;
    Ok(step)
}

fn page_locators(step: &ExecutableStep) -> Vec<Locator> {
    let Some(Target::Web(target)) = &step.target else {
        return Vec::new();
    };
    target
        .locators
        .iter()
        .filter_map(|l| match l.by {
            LocatorBy::TestId => Some(Locator::TestId(l.value.as_str().to_owned())),
            LocatorBy::Role => Some(Locator::Role(
                Role::read(l.value.as_str())?,
                l.name.as_ref()?.as_str().to_owned(),
            )),
            _ => None,
        })
        .collect()
}

/// The served step with the locator `role` of the repaired control put first, moved there when the list holds
/// it, and that control's description (L9.2, L10.3).
pub(crate) fn recovered_step(served: &ExecutableStep, repaired: &Control) -> Result<Value, String> {
    let mut step =
        serde_json::to_value(served).map_err(|_| "a served step with no JSON".to_owned())?;
    let new = locator_json(&Locator::role_of(repaired));
    let mut locators = vec![new.clone()];
    if let Some(list) = step["target"]["locators"].as_array() {
        locators.extend(list.iter().filter(|l| **l != new).cloned());
    }
    step["target"]["locators"] = Value::Array(locators);
    step["target"]["desc"] = Value::String(desc(repaired));
    Ok(step)
}

/// The edits of the family `learn` in the state `reverted`, as (edit id, first locator of its list, step number).
fn reverted(rows: &dyn RowRead) -> Result<Vec<(String, Option<Locator>, u32)>, String> {
    let mut out = Vec::new();
    let mut failed = None;
    rows.for_each(
        "learn_edit",
        &mut |row| match LearnEdit::parse(row.as_bytes()) {
            Ok(_) => {
                let value: Value = serde_json::from_str(row).unwrap_or(Value::Null);
                if value["state"] == "reverted" {
                    let first = value["target"]["locators"].get(0).and_then(|l| {
                        let by = l["by"].as_str()?;
                        let v = l["value"].as_str()?.to_owned();
                        match by {
                            "test_id" => Some(Locator::TestId(v)),
                            "role" => Some(Locator::Role(
                                Role::read(&v)?,
                                l["name"].as_str()?.to_owned(),
                            )),
                            _ => None,
                        }
                    });
                    let n = value["n"]
                        .as_u64()
                        .and_then(|n| u32::try_from(n).ok())
                        .unwrap_or(0);
                    let edit = value["edit"].as_str().unwrap_or_default().to_owned();
                    out.push((edit, first, n));
                }
            }
            Err(_) => failed = Some(row.to_owned()),
        },
    );
    match failed {
        Some(_) => Err("a stored learn_edit row that its parser refuses".into()),
        None => Ok(out),
    }
}

/// One case played in one arm on a store of its own: the gate host, the handlers of the arm and the case's ids.
pub struct Play<'c> {
    case: &'c Case,
    arm: Arm,
    pub host: GateHost,
    handlers: LearnHandlers,
    after_compile: Option<MemoryStore>,
    workflow: String,
    agent: String,
    runs: u32,
}

impl<'c> Play<'c> {
    /// A play of the case under the workflow, agent and runs numbered `workflow`, on the given store.
    pub fn new(case: &'c Case, arm: Arm, workflow: u32, store: MemoryStore) -> Play<'c> {
        Play {
            case,
            arm,
            host: GateHost::new(store, case.occurrences[0].day),
            handlers: LearnHandlers::new(),
            after_compile: None,
            workflow: id('3', workflow),
            agent: id('4', workflow),
            runs: 0,
        }
        .numbered_from(workflow)
    }

    fn numbered_from(mut self, workflow: u32) -> Play<'c> {
        self.runs = workflow.saturating_sub(1).saturating_mul(1_000);
        self
    }

    pub fn workflow(&self) -> &str {
        &self.workflow
    }

    /// The `load_steps` reply that a new replay run of the agent gets, as the steps that it serves; the run is
    /// left open.
    pub fn served_now(&mut self) -> Result<Vec<ExecutableStep>, String> {
        let (workflow, agent) = (self.workflow.clone(), self.agent.clone());
        let run = self.next_run();
        self.coordinator(json!({"op": "start_run", "workflow": workflow, "agent": agent, "run": run, "mode": "replay"}))?;
        let RunnerReply::Load(load) =
            self.runner(json!({"op": "load", "agent": agent, "run": run}))?
        else {
            return Err("load: another reply".into());
        };
        let number = load.playbook.ok_or("load: no playbook")?.number.get();
        match self.runner(
            json!({"op": "load_steps", "agent": agent, "run": run, "number": number, "from": 1}),
        )? {
            RunnerReply::Steps(steps) => Ok(steps.steps),
            _ => Err("load_steps: another reply".into()),
        }
    }

    fn coordinator(&mut self, body: Value) -> Result<(), String> {
        let bytes = body.to_string();
        let answer = match self.arm {
            Arm::Admit => self
                .handlers
                .coordinator(&mut self.host, bytes.as_bytes())
                .map(|_| ())
                .map_err(|e| format!("{e:?}")),
            Arm::None | Arm::Plain => {
                let now = self.host.now();
                halvo_agent_store::coordinator(&mut self.host.store, now, bytes.as_bytes())
                    .map(|_: CoordinatorReply| ())
                    .map_err(|e| format!("{e:?}"))
            }
        };
        answer.map_err(|e| format!("{}: {e}", body["op"]))
    }

    fn runner(&mut self, body: Value) -> Result<RunnerReply, String> {
        let bytes = body.to_string();
        let answer = match self.arm {
            Arm::Admit => self
                .handlers
                .runner(&mut self.host, bytes.as_bytes())
                .map_err(|e| format!("{e:?}")),
            Arm::None | Arm::Plain => {
                let now = self.host.now();
                halvo_agent_store::runner(&mut self.host.store, now, bytes.as_bytes())
                    .map_err(|e| format!("{e:?}"))
            }
        };
        answer.map_err(|e| format!("{}: {e}", body["op"]))
    }

    fn next_run(&mut self) -> String {
        self.runs += 1;
        id('5', self.runs)
    }

    fn record(&self, run: &str, mode: &str, outcome: &str, playbook: u32, steps: usize) -> Value {
        json!({
            "v": 1, "workflow": self.workflow, "agent": self.agent, "run": run, "day": self.host.day,
            "mode": mode, "outcome": outcome, "playbook": playbook, "steps": steps, "prompts": 0,
            "handbacks": 0, "confirmations": 0, "effects": 0, "unknown": 0, "model_calls": 0,
            "tokens_in": 0, "tokens_out": 0, "wall_s": 1, "model": null,
            "before_start": null, "cut": null, "no_key": null, "not_to_account": null,
            "incomplete": null, "reply_to_other": null, "more_pages": null, "held_recipient": null,
            "organized": null, "other_task": null, "held_earlier": null, "earlier_page": null,
        })
    }

    /// The workflow, the agent and the compile of the case from the page of its first occurrence (L10.3). It
    /// stores the workflow, then sends, in order: `create_agent`, a `start_run` of the mode `compile`, a `load`,
    /// the `put_playbook` of the compiled playbook under number 1, `finish`, and `set_state` to `active`. That the
    /// stub sends these "as the runner does" is shown by code for the `load` alone (the product's runner loads
    /// straight after its hello in every run); the runner's compile path is not built.
    pub fn compile(&mut self) -> Result<(), String> {
        let first = &self.case.occurrences[0].page;
        let mut steps = Vec::new();
        for step in &self.case.steps {
            let intended =
                control(first, &step.control).ok_or("the first page lacks a step's control")?;
            steps.push(compiled_step(step.n, step.verb, intended)?);
        }
        self.host
            .store
            .put_workflow(workflow_record(&self.workflow));
        let (workflow, agent) = (self.workflow.clone(), self.agent.clone());
        self.coordinator(json!({"op": "create_agent", "workflow": workflow, "agent": agent}))?;
        let run = self.next_run();
        self.coordinator(json!({"op": "start_run", "workflow": workflow, "agent": agent, "run": run, "mode": "compile"}))?;
        self.runner(json!({"op": "load", "agent": agent, "run": run}))?;
        self.runner(
            json!({"op": "put_playbook", "agent": agent, "run": run, "number": 1, "steps": steps}),
        )?;
        let record = self.record(&run, "compile", "completed", 1, self.case.steps.len());
        self.runner(json!({"op": "finish", "agent": agent, "run": run, "record": record}))?;
        self.coordinator(
            json!({"op": "set_state", "workflow": workflow, "agent": agent, "state": "active"}),
        )?;
        if self.arm == Arm::None {
            self.after_compile = Some(self.host.store.clone());
        }
        Ok(())
    }

    /// Closes and opens the host and the store on the clock's day: the handlers lose the facts of every run, and
    /// the open expires bounded rows. Answers the `learn_run` rows that the open removed.
    pub fn restart(&mut self) -> u32 {
        self.handlers = LearnHandlers::new();
        let expired = self.host.store.open(self.host.day);
        u32::try_from(expired.of("learn_run")).unwrap_or(u32::MAX)
    }

    /// Plays one occurrence as one replay run of the stub runner (L10.3): `start_run`, `load`, `load_steps`, the
    /// steps against the page with the case's repairer, one `log`, at most one `put_playbook` and `finish`. The
    /// `put_playbook` names the number that the run loaded plus 1, in every arm, and holds the playbook's steps from
    /// step 1, the recovered ones with the repaired control's locator first. Answers whether the repairer was asked, whether a step acted on a control that is not its intended
    /// control, whether the run completed, and the reverts of the run whose first locator resolves on the page to
    /// the step's intended control or to nothing.
    pub fn occurrence(
        &mut self,
        occurrence: &Occurrence,
    ) -> Result<(bool, bool, bool, u32), String> {
        if let (Arm::None, Some(store)) = (self.arm, &self.after_compile) {
            self.host.store = store.clone();
        }
        let before: BTreeSet<String> = reverted(self.host.store.rows(Family::Learn))?
            .into_iter()
            .map(|(edit, _, _)| edit)
            .collect();
        let (workflow, agent) = (self.workflow.clone(), self.agent.clone());
        let run = self.next_run();
        self.coordinator(json!({"op": "start_run", "workflow": workflow, "agent": agent, "run": run, "mode": "replay"}))?;
        let RunnerReply::Load(load) =
            self.runner(json!({"op": "load", "agent": agent, "run": run}))?
        else {
            return Err("load: another reply".into());
        };
        let number = load.playbook.ok_or("load: no playbook")?.number.get();
        let reply = self.runner(
            json!({"op": "load_steps", "agent": agent, "run": run, "number": number, "from": 1}),
        )?;
        let RunnerReply::Steps(served) = reply else {
            return Err("load_steps: another reply".into());
        };
        if served.steps.len() != self.case.steps.len() {
            return Err("load_steps: not every step of the case".into());
        }
        let mut lines: Vec<(&str, u32, Option<&str>)> = Vec::new();
        let mut put = Vec::new();
        let (mut asked, mut wrong, mut completed, mut recovered) = (false, false, true, false);
        for (index, step) in self.case.steps.iter().enumerate() {
            let served_step = &served.steps[index];
            let flaky = occurrence.flaky.contains(&step.n);
            let play = play_step(
                &occurrence.page,
                step.n,
                &step.control,
                flaky,
                &page_locators(served_step),
                &occurrence.repair[index],
            );
            if play.timeout {
                lines.push(("step_failed", step.n, Some("timeout")));
            }
            if play.failed {
                lines.push(("step_failed", step.n, Some("step_failed")));
            }
            asked |= play.failed;
            wrong |= play.wrong;
            match &play.recovered {
                Some(repaired) => {
                    lines.push(("recovered", step.n, None));
                    put.push(recovered_step(served_step, repaired)?);
                    recovered = true;
                }
                None => {
                    put.push(serde_json::to_value(served_step).map_err(|_| "a step with no JSON")?)
                }
            }
            if play.done() {
                lines.push(("step_ok", step.n, None));
            } else {
                completed = false;
                break;
            }
        }
        let records: Vec<Value> = (1u32..)
            .zip(&lines)
            .map(|(seq, (event, step, cause))| {
                json!({
                    "v": 1, "workflow": workflow, "agent": agent, "run": run, "seq": seq,
                    "day": self.host.day, "at": self.host.now().at.0, "event": event, "step": step,
                    "cause": cause, "model": null, "ms": null, "tokens_in": null, "tokens_out": null,
                })
            })
            .collect();
        self.runner(json!({"op": "log", "agent": agent, "run": run, "records": records}))?;
        if recovered {
            self.runner(json!({"op": "put_playbook", "agent": agent, "run": run, "number": number + 1, "steps": put}))?;
        }
        let outcome = if completed { "completed" } else { "failed" };
        let record = self.record(&run, "replay", outcome, number, lines.len());
        self.runner(json!({"op": "finish", "agent": agent, "run": run, "record": record}))?;
        let mut good = 0;
        for (edit, first, n) in reverted(self.host.store.rows(Family::Learn))? {
            if before.contains(&edit) {
                continue;
            }
            let intended = self
                .case
                .steps
                .iter()
                .find(|s| s.n == n)
                .map(|s| s.control.as_str());
            let resolved = first.as_ref().and_then(|l| l.resolve(&occurrence.page));
            if resolved.is_none_or(|c| Some(c.id.as_str()) == intended) {
                good += 1;
            }
        }
        Ok((asked, wrong, completed, good))
    }
}

/// Plays every occurrence of a case in an arm on a store of its own, with every occurrence read as `restart`
/// `yes` when `restart_all` holds (L10.2, L10.4).
pub fn play_case(case: &Case, arm: Arm, restart_all: bool) -> Result<Tally, String> {
    let mut play = Play::new(case, arm, 1, MemoryStore::new());
    play_all(&mut play, restart_all)
}

/// Plays every occurrence of the play's case, compiling before the first.
pub fn play_all(play: &mut Play<'_>, restart_all: bool) -> Result<Tally, String> {
    let case = play.case;
    let mut tally = Tally::default();
    tally.counts.occurrences = u32::try_from(case.occurrences.len()).unwrap_or(u32::MAX);
    tally.counts.drifts = drifts(case);
    for (at, occurrence) in case.occurrences.iter().enumerate() {
        play.host.day = occurrence.day;
        if at == 0 {
            tally.expired_runs += play.restart();
            play.compile()?;
        } else if occurrence.restart || restart_all {
            tally.expired_runs += play.restart();
        }
        let (asked, wrong, completed, good) = play.occurrence(occurrence)?;
        tally.counts.assisted += u32::from(asked);
        tally.counts.harm += u32::from(wrong);
        tally.counts.failed += u32::from(!completed);
        tally.reverted_good += good;
    }
    tally.edit_rows =
        u32::try_from(play.host.store.rows(Family::Learn).count("learn_edit")).unwrap_or(u32::MAX);
    Ok(tally)
}
