use crate::types::{AgentId, Trigger};

/// A start that waits in the queue: its agent and who asked for it (I5.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting {
    /// The agent.
    pub agent: AgentId,
    /// `person` or `schedule`.
    pub trigger: Trigger,
}

/// The queue of starts (I5.5): every start of the person stands before every start of the
/// scheduler, and each class in the order of arrival. A new life of the app starts with an
/// empty queue (I5.8).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Queue {
    entries: Vec<Waiting>,
}

impl Queue {
    /// Adds a start behind the starts of its class.
    pub fn push(&mut self, waiting: Waiting) {
        let at = match waiting.trigger {
            Trigger::Person => self
                .entries
                .iter()
                .take_while(|w| w.trigger == Trigger::Person)
                .count(),
            Trigger::Schedule => self.entries.len(),
        };
        self.entries.insert(at, waiting);
    }

    /// The start at the head.
    pub fn head(&self) -> Option<&Waiting> {
        self.entries.first()
    }

    /// Takes the start at the head.
    pub fn pop(&mut self) -> Option<Waiting> {
        if self.entries.is_empty() {
            None
        } else {
            Some(self.entries.remove(0))
        }
    }

    /// Takes the start of the agent out, wherever it stands; whether there was one.
    pub fn remove(&mut self, agent: &AgentId) -> bool {
        let before = self.entries.len();
        self.entries.retain(|w| &w.agent != agent);
        self.entries.len() != before
    }

    /// Whether a start of the agent waits.
    pub fn holds(&self, agent: &AgentId) -> bool {
        self.entries.iter().any(|w| &w.agent == agent)
    }

    /// The starts that wait.
    pub fn len(&self) -> u32 {
        u32::try_from(self.entries.len()).unwrap_or(u32::MAX)
    }

    /// Whether no start waits.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The starts, the head first.
    pub fn entries(&self) -> &[Waiting] {
        &self.entries
    }
}
