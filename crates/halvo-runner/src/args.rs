use std::ffi::OsString;
use std::path::PathBuf;

use halvo_control::paths::{DataDir, PathError};
use halvo_schema::executable::day::UTC_OFFSET_S_MAX;
use halvo_schema::executable::names::Mode;
use halvo_schema::executable::RunId;
use halvo_schema::{AgentId, WorkflowId};

/// The arguments of A6.1: `--run <run id> --agent <agent id> --workflow <workflow id> --mode compile|replay
/// --browser PATH --utc-offset SECONDS [--model FILE] [--headed]`, each once, in any order, and `[--data-dir DIR]`,
/// Halvo's data directory, whose `agents/locks/` the file steps lock (the coordinator's ruling CP-134). The supervisor
/// library passes `--data-dir` with the directory that its caller resolved (U4.7); the argument stays optional here,
/// as A6.1 writes it, and a file step or a web step of a runner without it fails with `step_failed`. It keeps
/// `--browser` and `--headed`, which the start of the run's browser takes at its first web step (crate::web); this
/// build starts no model, so it checks the form of `--model` and keeps it not.
#[derive(Debug)]
pub(crate) struct Args {
    pub run: RunId,
    pub agent: AgentId,
    pub workflow: WorkflowId,
    pub mode: Mode,
    /// The data directory of `--data-dir`, read by halvo-control's `DataDir::from_flag`, its
    /// check of a data directory given as a flag; `None` without the argument, and every file
    /// step then fails with `step_failed` before it opens a lock.
    pub data_dir: Option<DataDir>,
    /// The run's offset of `--utc-offset`: whole seconds east of UTC, from -50,400 to 50,400, by which the runner
    /// stamps the day of every row (`day_of`, the coordinator's ruling CP-204).
    pub utc_offset_s: i32,
    /// The browser's program of `--browser`, as given; the executor's start refuses a relative path.
    pub browser: PathBuf,
    /// Whether `--headed` was given: the browser then runs with a window.
    pub headed: bool,
}

/// The detail of a refusal with the cause `usage`.
pub(crate) type Usage = &'static str;

const KEYS: [&str; 8] = [
    "--run",
    "--agent",
    "--workflow",
    "--mode",
    "--browser",
    "--model",
    "--data-dir",
    "--utc-offset",
];

const TWICE: [Usage; 8] = [
    "--run twice",
    "--agent twice",
    "--workflow twice",
    "--mode twice",
    "--browser twice",
    "--model twice",
    "--data-dir twice",
    "--utc-offset twice",
];

/// The detail of a `--utc-offset` that is no whole number from -50,400 to 50,400 in the decimal form that `i32`
/// writes, which has no sign before a positive number and no leading zero.
const OUT_OF_RANGE: Usage = "--utc-offset is out of range";

/// The offset of `--utc-offset`, or the detail of its refusal.
fn utc_offset(value: &OsString) -> Result<i32, Usage> {
    let text = text(value, OUT_OF_RANGE)?;
    text.parse::<i32>()
        .ok()
        .filter(|n| (-UTC_OFFSET_S_MAX..=UTC_OFFSET_S_MAX).contains(n) && n.to_string() == text)
        .ok_or(OUT_OF_RANGE)
}

/// The detail of a `--data-dir` that `DataDir::from_flag` refuses: not UTF-8, not absolute (an
/// empty value among them), or holding a parent step.
fn data_dir_refusal(error: PathError) -> Usage {
    match error {
        PathError::Relative => "--data-dir is not absolute",
        PathError::NotNormal => "--data-dir holds a parent step",
        _ => "--data-dir is not UTF-8",
    }
}

fn text(value: &OsString, what: Usage) -> Result<&str, Usage> {
    value.to_str().ok_or(what)
}

/// The flag that asks for the runner's version, as halvo and halvo-capture take it.
const VERSION_FLAG: &str = "--version";

/// Whether the arguments are the flag `--version` alone; beside another argument the flag is an unknown argument
/// of `parse`.
pub(crate) fn asks_version(args: &[OsString]) -> bool {
    matches!(args, [only] if only == VERSION_FLAG)
}

/// Reads the arguments, or the detail of the refusal `usage`.
pub(crate) fn parse(args: &[OsString]) -> Result<Args, Usage> {
    let mut values: [Option<&OsString>; 8] = [None; 8];
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
    let [run, agent, workflow, mode, browser, _model, data_dir, offset] = values;
    let run = run.ok_or("no --run")?;
    let agent = agent.ok_or("no --agent")?;
    let workflow = workflow.ok_or("no --workflow")?;
    let mode = mode.ok_or("no --mode")?;
    let browser = browser.ok_or("no --browser")?;
    let offset = offset.ok_or("no --utc-offset")?;
    Ok(Args {
        run: RunId::parse(text(run, "--run")?).map_err(|_| "--run is no run id")?,
        agent: AgentId::parse(text(agent, "--agent")?).map_err(|_| "--agent is no agent id")?,
        workflow: WorkflowId::parse(text(workflow, "--workflow")?)
            .map_err(|_| "--workflow is no workflow id")?,
        mode: Mode::parse(text(mode, "--mode")?).map_err(|_| "--mode is not compile or replay")?,
        data_dir: data_dir
            .map(|dir| DataDir::from_flag(dir).map_err(data_dir_refusal))
            .transpose()?,
        utc_offset_s: utc_offset(offset)?,
        browser: PathBuf::from(browser),
        headed,
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
            "--utc-offset",
            "0",
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
        assert_eq!(parse(&args(&full()[..10])).err(), Some("no --utc-offset"));
        let mut headed = full();
        headed.extend(["--headed", "--headed"]);
        assert_eq!(parse(&args(&headed)).err(), Some("--headed twice"));
    }

    /// CP-134: `--data-dir` is optional, read once, and refused as `DataDir::from_flag` refuses
    /// the daemon's flag: an empty value and a relative path are not absolute, and a parent step
    /// is refused.
    #[test]
    fn the_data_directory_is_optional_once_and_absolute() {
        assert!(parse(&args(&full()))
            .expect("no --data-dir")
            .data_dir
            .is_none());
        let dir = std::env::temp_dir();
        let dir = dir.to_str().expect("a UTF-8 directory");
        let mut given = full();
        given.extend(["--data-dir", dir]);
        let parsed = parse(&args(&given)).expect("an absolute --data-dir");
        assert_eq!(
            parsed.data_dir.as_ref().map(|d| d.path().to_path_buf()),
            Some(std::path::PathBuf::from(dir).components().collect())
        );
        let mut twice = given.clone();
        twice.extend(["--data-dir", dir]);
        assert_eq!(parse(&args(&twice)).err(), Some("--data-dir twice"));
        for (value, refusal) in [
            ("", "--data-dir is not absolute"),
            ("relative/data", "--data-dir is not absolute"),
        ] {
            let mut list = full();
            list.extend(["--data-dir", value]);
            assert_eq!(parse(&args(&list)).err(), Some(refusal), "{value:?}");
        }
        let parent = format!("{dir}/a/../b");
        let mut list = full();
        list.extend(["--data-dir", &parent]);
        assert_eq!(
            parse(&args(&list)).err(),
            Some("--data-dir holds a parent step")
        );
    }

    fn with_offset(value: &str) -> Result<i32, Usage> {
        let mut list: Vec<&str> = full();
        list[11] = value;
        parse(&args(&list)).map(|a| a.utc_offset_s)
    }

    /// CP-204 D1: the offset is a whole number of seconds east of UTC from -50,400 to 50,400, kept as given.
    #[test]
    fn an_offset_in_its_range_is_kept() {
        for n in [-50_400, -50_399, -3_600, -1, 0, 1, 19_800, 50_399, 50_400] {
            assert_eq!(with_offset(&n.to_string()), Ok(n), "{n}");
        }
    }

    /// CP-204 D4: a value outside the range, or not in the form that `i32` writes, is refused with one detail.
    #[test]
    fn an_offset_out_of_range_or_of_another_form_is_refused() {
        for value in [
            "50401",
            "-50401",
            "2147483647",
            "-2147483648",
            "2147483648",
            "1.5",
            "",
            "x",
            "+3600",
            "0x10",
            "007",
            "-0",
            " 1",
            "1 ",
            "3600s",
        ] {
            assert_eq!(
                with_offset(value),
                Err("--utc-offset is out of range"),
                "{value:?}"
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            let mut list = args(&full());
            list[11] = OsString::from_vec(vec![0x31, 0xff]);
            assert_eq!(parse(&list).err(), Some("--utc-offset is out of range"));
        }
    }
}
