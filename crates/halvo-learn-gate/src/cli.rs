use crate::{delete, drift, model, privacy, routine, soak};

/// The closed list of refusal causes of the commands (contracts.md L10.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    Usage,
    Case,
    Store,
    Targets,
}

impl Cause {
    pub fn as_str(self) -> &'static str {
        match self {
            Cause::Usage => "usage",
            Cause::Case => "case",
            Cause::Store => "store",
            Cause::Targets => "targets",
        }
    }
}

/// A refusal of L0.3: the command did not do its work, and its one line goes to standard error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub cause: Cause,
    pub detail: String,
}

impl Refusal {
    pub fn new(cause: Cause, detail: impl Into<String>) -> Refusal {
        Refusal {
            cause,
            detail: detail.into(),
        }
    }

    pub fn usage(detail: impl Into<String>) -> Refusal {
        Refusal::new(Cause::Usage, detail)
    }

    /// `REFUSED <cause>: <detail>`.
    pub fn line(&self) -> String {
        format!("REFUSED {}: {}", self.cause.as_str(), self.detail)
    }
}

/// The lines that a command prints on standard output, and whether every check that it makes holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub lines: Vec<String>,
    pub ok: bool,
}

/// The store that a command asks for with `--store`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreName {
    Memory,
    Derived,
}

impl StoreName {
    pub fn parse(text: &str) -> Result<StoreName, Refusal> {
        match text {
            "memory" => Ok(StoreName::Memory),
            "derived" => Ok(StoreName::Derived),
            _ => Err(Refusal::usage("--store takes memory or derived")),
        }
    }

    /// The memory store, or the refusal `store` for the derived store, which this build does not hold (L10.1).
    pub fn memory_only(self) -> Result<(), Refusal> {
        match self {
            StoreName::Memory => Ok(()),
            StoreName::Derived => Err(Refusal::new(
                Cause::Store,
                "the derived store is not in this build",
            )),
        }
    }
}

/// The flags of a command after its name: each `--name value` pair or bare `--name`, in the order given.
pub struct Flags<'a> {
    rest: &'a [String],
    taken: Vec<bool>,
}

impl<'a> Flags<'a> {
    pub fn new(rest: &'a [String]) -> Flags<'a> {
        Flags {
            rest,
            taken: vec![false; rest.len()],
        }
    }

    fn position(&self, name: &str) -> Result<Option<usize>, Refusal> {
        let mut found = self.rest.iter().enumerate().filter(|(_, a)| *a == name);
        match (found.next(), found.next()) {
            (None, _) => Ok(None),
            (Some((at, _)), None) => Ok(Some(at)),
            (Some(_), Some(_)) => Err(Refusal::usage(format!("{name} is given twice"))),
        }
    }

    /// The value after `name`, or none when the flag is absent.
    pub fn value(&mut self, name: &str) -> Result<Option<&'a str>, Refusal> {
        let Some(at) = self.position(name)? else {
            return Ok(None);
        };
        let value = self
            .rest
            .get(at + 1)
            .filter(|v| !v.starts_with("--"))
            .ok_or_else(|| Refusal::usage(format!("{name} takes a value")))?;
        self.taken[at] = true;
        self.taken[at + 1] = true;
        Ok(Some(value.as_str()))
    }

    /// Whether the bare flag `name` is given.
    pub fn switch(&mut self, name: &str) -> Result<bool, Refusal> {
        let Some(at) = self.position(name)? else {
            return Ok(false);
        };
        self.taken[at] = true;
        Ok(true)
    }

    /// Refuses an argument that no flag took.
    pub fn done(self) -> Result<(), Refusal> {
        match self.taken.iter().position(|taken| !taken) {
            None => Ok(()),
            Some(_) => Err(Refusal::usage("an argument that the command does not take")),
        }
    }
}

/// Runs the command that the arguments name.
pub fn run(args: &[String]) -> Result<Report, Refusal> {
    let (command, rest) = args
        .split_first()
        .ok_or_else(|| Refusal::usage("no command; commands: drift, delete"))?;
    match command.as_str() {
        "drift" => drift::run(rest),
        "delete" => delete::run(rest),
        "privacy" => privacy::run(rest),
        "routine" => routine::run(rest),
        "soak" => soak::run(rest),
        "model" => model::run(rest),
        _ => Err(Refusal::usage(
            "an unknown command; commands: drift, delete",
        )),
    }
}
