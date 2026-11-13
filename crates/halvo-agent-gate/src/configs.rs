use halvo_policy::{validate, ConfigCause};
use halvo_schema::executable::body::CreateAgent;
use halvo_schema::executable::{AgentConfig, CoordinatorCall, CoordinatorReply};
use halvo_schema::{AgentId, ErrorName, SeedCategory, Workflow, WorkflowId, WorkflowState};

use crate::host::{minted, GateHost};
use crate::STORE;

/// The state of a workflow as the gate host reads it back after `create_agent`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateRead {
    /// No workflow of the id is stored.
    None,
    Proposed,
    Approved,
    Active,
    Paused,
    Dismissed,
}

impl StateRead {
    fn of(workflow: Option<&Workflow>) -> StateRead {
        match workflow {
            None => StateRead::None,
            Some(Workflow::Dismissed(_)) => StateRead::Dismissed,
            Some(Workflow::Full(w)) => match w.state {
                WorkflowState::Proposed => StateRead::Proposed,
                WorkflowState::Approved => StateRead::Approved,
                WorkflowState::Active => StateRead::Active,
                WorkflowState::Paused => StateRead::Paused,
            },
        }
    }

    /// The name that the line prints after `state=`.
    pub fn as_str(self) -> &'static str {
        match self {
            StateRead::None => "none",
            StateRead::Proposed => "proposed",
            StateRead::Approved => "approved",
            StateRead::Active => "active",
            StateRead::Paused => "paused",
            StateRead::Dismissed => "dismissed",
        }
    }
}

/// What a line prints after `result=`: the answer of `validate` for the agent's newest stored configuration, or,
/// where none is stored, the handler's refusal of `create_agent`, or `none` where the handler answered and stored
/// nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigResult {
    Valid,
    Invalid(ConfigCause),
    Refused(ErrorName),
    Missing,
}

impl ConfigResult {
    /// The result of a line from the agent's newest stored configuration and the handler's answer to
    /// `create_agent`: the answer of `validate` where a configuration is stored, else the handler's refusal, else
    /// `none`.
    pub fn of(
        newest: Option<&AgentConfig>,
        answer: &Result<CoordinatorReply, ErrorName>,
    ) -> ConfigResult {
        match (newest, answer) {
            (Some(config), _) => match validate(config) {
                Ok(()) => ConfigResult::Valid,
                Err(cause) => ConfigResult::Invalid(cause),
            },
            (None, Err(error)) => ConfigResult::Refused(*error),
            (None, Ok(_)) => ConfigResult::Missing,
        }
    }

    /// The name that the line prints after `result=`.
    pub fn as_str(self) -> &'static str {
        match self {
            ConfigResult::Valid => "valid",
            ConfigResult::Invalid(cause) => cause.as_str(),
            ConfigResult::Refused(error) => error.as_str(),
            ConfigResult::Missing => "none",
        }
    }
}

/// One line of `agent-gate configs` (A14.5), as read back from the store after the agent's `create_agent`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigLine {
    pub category: SeedCategory,
    /// A configuration of the agent is stored.
    pub stored: bool,
    pub state: StateRead,
    /// The stored workflow's agent is the agent of the line.
    pub agent_set: bool,
    pub result: ConfigResult,
}

impl ConfigLine {
    /// `config category=<category id> stored=<yes|no> state=<state> agent=<set|none> result=<valid|cause>`.
    pub fn render(&self) -> String {
        format!(
            "config category={} stored={} state={} agent={} result={}",
            self.category.as_str(),
            yes_no(self.stored),
            self.state.as_str(),
            if self.agent_set { "set" } else { "none" },
            self.result.as_str()
        )
    }

    /// Whether the line is `stored=yes state=approved agent=set result=valid`.
    pub fn pass(&self) -> bool {
        self.stored
            && self.state == StateRead::Approved
            && self.agent_set
            && self.result == ConfigResult::Valid
    }
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

/// The lines of `agent-gate configs`, one per seed category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configs {
    lines: Vec<ConfigLine>,
}

impl Configs {
    /// A report of the lines, in their order.
    pub fn from_lines(lines: Vec<ConfigLine>) -> Configs {
        Configs { lines }
    }

    /// The lines in order.
    pub fn lines(&self) -> &[ConfigLine] {
        &self.lines
    }

    /// The count of lines that pass: `stored=yes state=approved agent=set result=valid` (plan.md P3's gate line), so
    /// that the count line never counts a line that fails.
    pub fn valid(&self) -> usize {
        self.lines.iter().filter(|l| l.pass()).count()
    }

    /// Whether the report holds one line for each seed category, in the order of A1.1, and every line passes.
    pub fn pass(&self) -> bool {
        self.lines.len() == SeedCategory::ALL.len()
            && self
                .lines
                .iter()
                .zip(SeedCategory::ALL)
                .all(|(line, category)| line.category == *category && line.pass())
    }

    /// Every line, then `configs seeds=<n> valid=<n> store=<store>`, each ending with a line feed.
    pub fn render(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            out.push_str(&line.render());
            out.push('\n');
        }
        out.push_str(&format!(
            "configs seeds={} valid={} store={STORE}\n",
            self.lines.len(),
            self.valid()
        ));
        out
    }
}

/// Sends `create_agent` for the stored workflow with a fresh agent id, as the interface run's coordinator sends it,
/// and reads back what the store holds: whether a configuration of the agent is stored, the workflow's state and
/// agent, and the answer of `validate` for the agent's newest configuration.
pub fn config_line(
    host: &mut GateHost,
    workflow: &WorkflowId,
    category: SeedCategory,
) -> ConfigLine {
    let agent = AgentId::from_random(minted());
    let answer = host.send(&CoordinatorCall::CreateAgent(CreateAgent {
        workflow: workflow.clone(),
        agent: agent.clone(),
    }));
    let newest = host
        .configs()
        .into_iter()
        .filter(|c| c.agent == agent)
        .max_by_key(|c| c.rev);
    let stored_workflow = host.workflow(workflow);
    let agent_set =
        matches!(&stored_workflow, Some(Workflow::Full(w)) if w.agent.as_ref() == Some(&agent));
    let result = ConfigResult::of(newest.as_ref(), &answer);
    ConfigLine {
        category,
        stored: newest.is_some(),
        state: StateRead::of(stored_workflow.as_ref()),
        agent_set,
        result,
    }
}

/// `agent-gate configs`: one approved workflow per seed category, in the order of A1.1, each with its line.
pub fn configs(host: &mut GateHost) -> Configs {
    let lines = SeedCategory::ALL
        .iter()
        .map(|&category| {
            let workflow = host.put_seed_workflow(category);
            config_line(host, &workflow, category)
        })
        .collect();
    Configs { lines }
}
