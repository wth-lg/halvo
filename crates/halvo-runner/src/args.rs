use std::ffi::OsString;

use halvo_schema::executable::names::Mode;
use halvo_schema::executable::RunId;
use halvo_schema::{AgentId, WorkflowId};

/// The arguments of A6.1: `--run <run id> --agent <agent id> --workflow <workflow id> --mode compile|replay
/// --browser PATH [--model FILE] [--headed]`, each once, in any order. This build starts no browser and no model,
/// so it checks the form of `--browser`, `--model` and `--headed` and keeps none of them.
#[derive(Debug)]
pub(crate) struct Args {
    pub run: RunId,
    pub agent: AgentId,
    pub workflow: WorkflowId,
    pub mode: Mode,
}

/// The detail of a refusal with the cause `usage`.
pub(crate) type Usage = &'static str;

const KEYS: [&str; 6] = [
    "--run",
    "--agent",
    "--workflow",
    "--mode",
    "--browser",
    "--model",
];

const TWICE: [Usage; 6] = [
    "--run twice",
    "--agent twice",
    "--workflow twice",
    "--mode twice",
    "--browser twice",
    "--model twice",
];

fn text(value: &OsString, what: Usage) -> Result<&str, Usage> {
    value.to_str().ok_or(what)
}

/// Reads the arguments, or the detail of the refusal `usage`.
pub(crate) fn parse(args: &[OsString]) -> Result<Args, Usage> {
    let mut values: [Option<&OsString>; 6] = [None; 6];
    let mut headed = false;
    let mut at = args.iter();
    while let Some(key) = at.next() {
        if key == "--headed" {
            if headed {
                return Err("--headed twice");
            }
            headed = true;
            continue;
        }
        let key = text(key, "an argument that is not UTF-8")?;
        let index = KEYS
            .iter()
            .position(|k| *k == key)
            .ok_or("an unknown argument")?;
        let value = at.next().ok_or("a key with no value")?;
        if values[index].replace(value).is_some() {
            return Err(TWICE[index]);
        }
    }
    let [run, agent, workflow, mode, browser, _model] = values;
    let run = run.ok_or("no --run")?;
    let agent = agent.ok_or("no --agent")?;
    let workflow = workflow.ok_or("no --workflow")?;
    let mode = mode.ok_or("no --mode")?;
    browser.ok_or("no --browser")?;
    Ok(Args {
        run: RunId::parse(text(run, "--run")?).map_err(|_| "--run is no run id")?,
        agent: AgentId::parse(text(agent, "--agent")?).map_err(|_| "--agent is no agent id")?,
        workflow: WorkflowId::parse(text(workflow, "--workflow")?)
            .map_err(|_| "--workflow is no workflow id")?,
        mode: Mode::parse(text(mode, "--mode")?).map_err(|_| "--mode is not compile or replay")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    const RUN: &str = "0123456789abcdef0123456789abcdef";

    fn full() -> Vec<&'static str> {
        vec![
            "--run",
            RUN,
            "--agent",
            RUN,
            "--workflow",
            RUN,
            "--mode",
            "compile",
            "--browser",
            "b",
        ]
    }

    #[test]
    fn the_five_keys_in_any_order_and_the_two_optional_ones() {
        let parsed = parse(&args(&full())).expect("the five keys");
        assert_eq!(parsed.mode, Mode::Compile);
        assert_eq!(parsed.run.as_str(), RUN);
        let mut list = full();
        list.extend(["--headed", "--model", "m"]);
        assert!(parse(&args(&list)).is_ok(), "with the optional keys");
        let mut list = full();
        list.rotate_left(2);
        assert!(parse(&args(&list)).is_ok(), "another order");
        let mut list = full();
        list.reverse();
        assert_eq!(parse(&args(&list)).err(), Some("an unknown argument"));
    }

    #[test]
    fn each_refusal_names_its_detail() {
        let mut twice = full();
        twice.extend(["--browser", "c"]);
        assert_eq!(parse(&args(&twice)).err(), Some("--browser twice"));
        let mut twice = full();
        twice.extend(["--model", "m", "--model", "m"]);
        assert_eq!(parse(&args(&twice)).err(), Some("--model twice"));
        let mut empty = full();
        empty.push("--model");
        assert_eq!(parse(&args(&empty)).err(), Some("a key with no value"));
        let mut bad = full();
        bad[1] = "0123";
        assert_eq!(parse(&args(&bad)).err(), Some("--run is no run id"));
        assert_eq!(parse(&args(&full()[2..])).err(), Some("no --run"));
        assert_eq!(parse(&args(&full()[..8])).err(), Some("no --browser"));
        let mut headed = full();
        headed.extend(["--headed", "--headed"]);
        assert_eq!(parse(&args(&headed)).err(), Some("--headed twice"));
    }
}
